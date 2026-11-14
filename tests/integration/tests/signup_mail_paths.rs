//! Sign-up's mail paths (BIZ-040, SEC-004, SEC-018, SEC-026; T468, D-3743).
//!
//! `portals::api::send_sign_in_link` stores and hands over one sign-in link for a current member
//! only and never past `LINKS_PER_LIFE`; `businesses::api::signup_mailbox` reads the sign-up's
//! mailbox; `messaging::api::MailboxCodeSender` emails the code while its stored row holds only
//! `CODE_MARK`, and a repeat of one send is one email. Every address, ID and code is fake.

use std::sync::{Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, Charge, Email, EmailAdapter, IdempotencyKey, PaymentIntent, PaymentsAdapter,
};
use askhava_businesses::api::{SignUpForm, add_owner, create, sign_up, signup_mailbox};
use askhava_core::api::brand;
use askhava_licensing::api::ClassMap;
use askhava_messaging::api::{
    CODE_MARK, DailyLimit, MAILBOX_CODE_TEMPLATE, MailboxCodeSender, MailboxKind,
};
use askhava_portals::api::{
    LINKS_PER_LIFE, PortalsError, SignInAddress, SignInMail, send_sign_in_link,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_BOX: u128 = 0xc1;
const CODE: &str = "246813";
const ADDRESS: &str = "owner@x.example";
const CATS: &str = "categories:
  - {id: plumbing_repair, enabled: true, launch_open: true, job_types: [leak repair]}
";

fn now() -> Timestamp {
    "2026-11-07T17:00:00Z".parse().expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned())
}

/// The links "emailed", with the address each went to.
#[derive(Default)]
struct RecordingLinks(Mutex<Vec<(SignInAddress, String)>>);

impl RecordingLinks {
    fn sent(&self) -> Vec<(SignInAddress, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SignInMail for RecordingLinks {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        _: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(Vec::new())
    }
    async fn send_link(
        &self,
        _: &mut PgConnection,
        to: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let mut sent = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push((to, link.expose_secret().to_owned()));
        Ok(())
    }
}

/// A business whose owner signs in at [`OWNER_BOX`].
async fn owned(db: &mut PgConnection) -> SignInAddress {
    let business = create(db, false, now()).await.expect("business");
    let mailbox = Uuid::from_u128(OWNER_BOX);
    add_owner(db, business, mailbox, now())
        .await
        .expect("owner");
    SignInAddress { business, mailbox }
}

async fn stored_links(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM portals.login_link")
        .fetch_one(db)
        .await
        .expect("links")
}

/// SEC-004, BIZ-040: one stored link on the brand's site goes to the owner's own address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_link_is_stored_and_sent_to_the_owner(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let owner = owned(&mut db).await;
    let mail = RecordingLinks::default();
    send_sign_in_link(&mut db, &mail, owner, now())
        .await
        .expect("sent");
    let sent = mail.sent();
    assert_eq!(sent.len(), 1, "one link");
    assert_eq!(sent[0].0, owner, "to the owner's own sign-in address");
    let site = format!("https://{}/b/sign-in/", brand().domain());
    let token = sent[0]
        .1
        .strip_prefix(&site)
        .expect("a sign-in link on the brand's site");
    assert_eq!(token.len(), 64, "a bare token, no next job");
    assert_eq!(stored_links(&mut db).await, 1, "one stored link");
}

/// SEC-004: a mailbox that is not a current member of the business gets nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_link_for_a_mailbox_that_is_not_a_member(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let owner = owned(&mut db).await;
    let stranger = SignInAddress {
        business: owner.business,
        mailbox: Uuid::from_u128(0xc9),
    };
    let mail = RecordingLinks::default();
    send_sign_in_link(&mut db, &mail, stranger, now())
        .await
        .expect("nothing to do");
    assert!(mail.sent().is_empty(), "nothing sent");
    assert_eq!(stored_links(&mut db).await, 0, "nothing stored");
}

/// D-1725, D-1837: no more links once `LINKS_PER_LIFE` went in `LINK_LIFE`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_link_past_the_cap(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let owner = owned(&mut db).await;
    let mail = RecordingLinks::default();
    for minute in 0..=LINKS_PER_LIFE {
        let at = now() + SignedDuration::from_mins(minute);
        send_sign_in_link(&mut db, &mail, owner, at)
            .await
            .expect("sent or capped");
    }
    let cap = usize::try_from(LINKS_PER_LIFE).expect("cap");
    assert_eq!(mail.sent().len(), cap, "the cap holds");
    assert_eq!(stored_links(&mut db).await, LINKS_PER_LIFE);
}

/// A fake Stripe that makes customers only.
struct FakeStripe;

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_Fake1".to_owned())
    }
    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }
    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// BIZ-040: the mailbox `sign_up` stored is the one answered; an unknown business is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn signup_mailbox_answers_the_stored_mailbox(pool: PgPool) {
    let map = ClassMap::load("commercial_only: []\nclasses: []\n", CATS).expect("catalog");
    let form = SignUpForm {
        categories: vec!["plumbing_repair".to_owned()],
        mailbox_id: Some(Uuid::from_u128(0xe7)),
        heard_from: None,
        g_code: None,
    };
    let up = sign_up(&pool, (&FakeStripe, &secret()), &map, &form, now()).await;
    let business = up.expect("signed up").business;
    let mut db = pool.acquire().await.expect("db");
    let mailbox = signup_mailbox(&mut db, business).await.expect("mailbox");
    assert_eq!(mailbox, Uuid::from_u128(0xe7));
    let unknown = signup_mailbox(&mut db, Uuid::from_u128(0xdead)).await;
    assert!(unknown.is_err(), "no sign-up, no mailbox");
}

/// The emails handed over: (to, text, html); `refuse` takes nothing.
#[derive(Default)]
struct RecordingMail {
    refuse: bool,
    sent: Mutex<Vec<(String, String, String)>>,
}

impl EmailAdapter for &RecordingMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        if self.refuse {
            return Err(AdapterError::NotSent);
        }
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push((e.to.to_owned(), e.text.to_owned(), e.html.to_owned()));
        Ok(format!("re_fake{}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn code_sender(mail: &RecordingMail) -> MailboxCodeSender<&RecordingMail> {
    let postal = ("PO Box 1234, Faketown, AZ 00000", MailboxKind::PoBox);
    let limit = DailyLimit::new(100, 90).expect("limit");
    MailboxCodeSender::new(secret(), mail, postal, limit).expect("sender")
}

async fn stored_rows(db: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT concat(subject, html, plain) FROM messaging.email_outbox WHERE template_id = $1",
    )
    .bind(MAILBOX_CODE_TEMPLATE)
    .fetch_all(db)
    .await
    .expect("rows")
}

/// BIZ-040, SEC-018: one email holding the code; a repeat of the same send is nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_code_goes_once_to_the_address(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let mail = RecordingMail::default();
    let sender = code_sender(&mail);
    let (address, code) = (SecretString::from(ADDRESS), SecretString::from(CODE));
    let to = (Uuid::from_u128(0xe7), &address);
    for _ in 0..2 {
        sender.send(&mut db, to, &code, now()).await.expect("sent");
    }
    let sent = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(sent.len(), 1, "a repeat sends nothing more");
    let (to, text, html) = &sent[0];
    assert_eq!(to, ADDRESS);
    assert!(
        text.contains(CODE) && html.contains(CODE),
        "the code is in both parts"
    );
    assert!(!text.contains(CODE_MARK) && !html.contains(CODE_MARK));
    assert_eq!(stored_rows(&mut db).await.len(), 1, "one row");
}

/// SEC-026, PRIV-016: a refused send leaves a row holding `CODE_MARK`, never the code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refused_send_stores_only_the_mark(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let mail = RecordingMail {
        refuse: true,
        ..RecordingMail::default()
    };
    let sender = code_sender(&mail);
    let (address, code) = (SecretString::from(ADDRESS), SecretString::from(CODE));
    let sent = sender
        .send(&mut db, (Uuid::from_u128(0xe7), &address), &code, now())
        .await;
    assert!(sent.is_err(), "the provider refused");
    let rows = stored_rows(&mut db).await;
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].contains(CODE_MARK) && !rows[0].contains(CODE),
        "only the mark"
    );
}
