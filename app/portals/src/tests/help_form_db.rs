//! The help form's and Get help's database paths (T200; TD-PRIV-16, TD-BIZ-23, CON-035, SEC-014,
//! D-4293, D-4372 to D-4374, D-4376): the request each opens, what it leaves untouched, the
//! opt-out it records and the limits that refuse a post. The page halves are `help::tests`. Every
//! read goes through the owning module's API (TD-SEC-10: no other schema named here). Fake data.

use std::net::IpAddr;
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_core::api::{Claimed, WorkerId, claim};
use askhava_intake::api::{Admission, receive_help_email};
use askhava_messaging::api::{
    DailyLimit, EmailTo, HELP_NOTICE_KIND, MailboxKind, RevokeChannel, send_email, suppress,
};
use askhava_ops::api::{Severity, digest_alerts, export_audit, open_help_requests};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::business_auth::BusinessPrincipal;
use crate::help::{
    GetHelpForm, GetHelpInput, HELP_FORM_ALL, HELP_FORM_PER_IP, HelpFormScreen, Topic,
    admit_help_form, get_help_answer, help_form_page, send_get_help, send_help_form, site_email,
};

const PHONE: &str = "+19285550142";

fn at() -> Timestamp {
    "2026-11-10T17:15:00Z".parse().expect("time")
}

fn vault() -> Vault {
    let key = |k: &str| SecretString::from(format!("1:{k}"));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (seal, index) = ("UlJS".repeat(10) + "UlI=", "YmJi".repeat(10) + "YmI=");
    Vault::new(&key(&seal), &key(&index), &url).expect("vault")
}

/// Every queued task, claimed through `core`'s queue.
async fn tasks(pool: &PgPool) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = at() + SignedDuration::from_mins(1);
    claim(pool, owner, later, SignedDuration::from_mins(5), 100)
        .await
        .expect("claim")
}

/// Open help requests and audit rows (a delete or copy request, received or done, is an audit
/// row, `data_download`): what a delete or copy ask may and may not leave.
async fn left_behind(db: &mut PgConnection) -> (usize, usize) {
    let requests = open_help_requests(db).await.expect("requests").len();
    let audit = export_audit(db, 0, 100).await.expect("audit").len();
    (requests, audit)
}

/// TD-PRIV-16: "please delete my data" through the help form opens exactly one help request
/// and deletes, copies and audits nothing; the only task is its owner notification.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_16_a_help_form_delete_ask_acts_on_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let said = ("Please delete my data.", None);
    let screen = send_help_form(&mut db, &vault(), said, at()).await;
    assert_eq!(screen, Ok(HelpFormScreen::Sent));
    assert_eq!(left_behind(&mut db).await, (1, 0));
    let kinds: Vec<String> = tasks(&pool).await.into_iter().map(|t| t.kind).collect();
    assert_eq!(kinds, [HELP_NOTICE_KIND]);
    let page = help_form_page(HelpFormScreen::Sent).expect("page");
    assert!(page.contains("href=\"/h/sign-in\""));
}

/// TD-PRIV-16: the same ask by email to the help address opens one request and acts on nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_16_a_help_email_delete_ask_acts_on_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let email = site_email("Please delete my data.");
    let opened = receive_help_email(&mut db, &email, at()).await;
    assert!(opened.expect("help").is_some());
    assert_eq!(left_behind(&mut db).await, (1, 0));
}

/// The fake email provider: keeps the plain part of each email it took.
#[derive(Default)]
struct FakeMail(Mutex<Vec<String>>);

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        self.0.lock().expect("lock").push(e.text.to_owned());
        Ok("re_fake".to_owned())
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn get_help_form() -> GetHelpForm {
    let input = GetHelpInput {
        topic: "emails".to_owned(),
        job: "1042".to_owned(),
        message: "No card emails since Monday.".to_owned(),
    };
    GetHelpForm::parse(&input).expect("form")
}

/// TD-BIZ-23, OPS-036: "emails not arriving" about job 1042 notifies the owner once with the
/// business number and the job, and queues an email whose body starts with the topic's answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_23_get_help_notifies_and_answers(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let who = BusinessPrincipal {
        member: Uuid::from_u128(0xb0),
        business: Uuid::from_u128(0x5ca1_ab1e_0000_4000_8000_0000_0000_00b1),
        owner: true,
        roles: (true, true),
    };
    let key = SecretString::from("fake-key-for-tests".to_owned());
    let postal = ("PO Box 1234, Faketown, AZ 00000", MailboxKind::PoBox);
    let sent = send_get_help(&mut db, (&key, postal), &who, &get_help_form(), at()).await;
    let sent = sent.expect("sent");
    let notices = tasks(&pool).await;
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].kind, HELP_NOTICE_KIND);
    let number = format!("B-{}", &who.business.simple().to_string()[..8]);
    assert_eq!(notices[0].payload["number"], number.as_str());
    let message = notices[0].payload["message"].as_str().expect("message");
    assert!(
        message.contains("Job 1042.") && message.contains("since Monday"),
        "{message}"
    );
    assert_eq!(sent.answer, get_help_answer(Topic::Emails).expect("answer"));
    let (mail, limit) = (
        FakeMail::default(),
        DailyLimit::new(100, 90).expect("limit"),
    );
    let to = EmailTo::Business("fake.owner@example.com");
    send_email(&mut db, (&key, &mail, limit), sent.email, to, at())
        .await
        .expect("email");
    let plain = mail.0.lock().expect("lock").clone();
    assert_eq!(plain.len(), 1);
    assert!(plain[0].starts_with(&sent.answer[0]) && sent.answer[0].contains("spam"));
}

/// CON-035, D-4293: an opt-out with a number lists it; a new, an already listed and a bad number
/// get the same screen, and the number is in no queued task. The `help_form` channel is stored in
/// `messaging.optout_event`, which no `messaging` API reads back, so it is not checked here.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_035_a_help_form_opt_out_lists_the_number_alone(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let vault = vault();
    let mut screens = Vec::new();
    for phone in [PHONE, PHONE, "12"] {
        let said = ("No more texts please.", Some(phone));
        let screen = send_help_form(&mut db, &vault, said, at()).await;
        screens.push(screen.expect("sent"));
    }
    assert_eq!(screens, [HelpFormScreen::OptedOut; 3]);
    let how = (RevokeChannel::HelpForm, "word:stop");
    let again = suppress(&mut db, &vault, PHONE, how, at()).await;
    assert_eq!(again, Ok(false), "the form listed the number");
    let other = suppress(&mut db, &vault, "+19285550143", how, at()).await;
    assert_eq!(other, Ok(true), "an unlisted number is newly listed");
    let queued: Vec<String> = tasks(&pool)
        .await
        .iter()
        .map(|t| t.payload.to_string())
        .collect();
    assert!(
        queued.iter().any(|p| p.contains("No more texts")),
        "{queued:?}"
    );
    assert!(
        queued
            .iter()
            .all(|p| !p.contains("5550142") && !p.contains("555-0142"))
    );
}

fn v6(net: u32, host: u16) -> IpAddr {
    format!("2001:db8:{:x}:{:x}::{host:x}", net >> 16, net & 0xffff)
        .parse()
        .expect("ip")
}

/// SEC-014, D-4374: the 6th post from one IPv6 /64 within an hour is refused; another /64 is not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_014_the_sixth_post_from_one_64_is_refused(pool: PgPool) {
    for host in 1..=5 {
        let admitted = admit_help_form(&pool, v6(1, host), at()).await;
        assert_eq!(admitted, Ok(Admission::Admitted), "post {host}");
    }
    let sixth = admit_help_form(&pool, v6(1, 6), at()).await;
    assert_eq!(sixth, Ok(Admission::Refused(HELP_FORM_PER_IP)));
    let other = admit_help_form(&pool, v6(2, 1), at()).await;
    assert_eq!(other, Ok(Admission::Admitted));
}

/// SEC-014, SEC-015, D-4374: past 200 posts in an Arizona day every post is refused, and one
/// High alert is raised, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_014_past_200_a_day_one_high_alert(pool: PgPool) {
    for net in 1..=200 {
        let admitted = admit_help_form(&pool, v6(net, 1), at()).await;
        assert_eq!(admitted, Ok(Admission::Admitted), "post {net}");
    }
    for net in 201..=203 {
        let refused = admit_help_form(&pool, v6(net, 1), at()).await;
        assert_eq!(refused, Ok(Admission::Refused(HELP_FORM_ALL)), "post {net}");
    }
    assert_eq!(HELP_FORM_ALL.alert, Severity::High);
    let mut db = pool.acquire().await.expect("conn");
    let day = SignedDuration::from_hours(24);
    let (from, to) = (Timestamp::now() - day, Timestamp::now() + day);
    let alerts = digest_alerts(&mut db, &[HELP_FORM_ALL.scope], from, to).await;
    assert_eq!(alerts.expect("alerts").len(), 1);
}
