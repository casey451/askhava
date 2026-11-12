//! The sign-up consent box, the welcome request at activation and the welcome and phone-stopped
//! emails, against PostgreSQL and a fake email provider (TD-CON-29: CON-019, CON-020, BIZ-016,
//! PICK-049; T268, D-1715, D-1788, D-3184 to D-3188). Every address, number and ID is fake.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, Charge, Email, EmailAdapter, IdempotencyKey, PaymentIntent, PaymentsAdapter,
};
use askhava_businesses::api::{
    AccountDirectory, AccountMailer, Acting, ConsentChannel, Flag, PHONE_STOPPED_KIND, Roles,
    SenderLink, SignUpForm, TeamError, WELCOME_KIND, add_member, approve, consent_box,
    may_get_cards, phone_stopped, record_consent, record_match, run_phone_stopped, run_welcome,
    welcome_card_line,
};
use askhava_core::api::{Claimed, HandlerError, SystemClock};
use askhava_licensing::api::ClassMap;
use askhava_messaging::api::{DailyLimit, MailboxKind};
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    CONSENT_MISSING, Entered, PageLanguage, Parts, Screen, SignupConsent, SignupRefusal,
    TEXT_PHONE_MISSING, fee_numbers, refusal_problem, signup_channels, signup_page, submit_signup,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use ConsentChannel::{Email as ByEmail, Text};

const OWNER_ADDRESS: &str = "owner@example.test";
const MEMBER_ADDRESS: &str = "member@example.test";
const OWNER_MAILBOX: Uuid = Uuid::from_u128(0xe1);
const MEMBER_MAILBOX: Uuid = Uuid::from_u128(0xe2);
const FULL_NUMBER: &str = "+15555550142";
const CATS: &str = "categories:
  - {id: plumbing_repair, enabled: true, launch_open: true, job_types: [leak repair]}
";

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
        Err(AdapterError::InvalidId)
    }
    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::InvalidId)
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::InvalidId)
    }
}

/// A sender directory that accepts every fake number (the real one is `sender_directory_sync.rs`'s).
struct AnyPhone;

impl SenderLink for AnyPhone {
    async fn link(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
        _: &SecretString,
    ) -> Result<(), TeamError> {
        Ok(())
    }
}

/// One email the fake provider took: its provider key, recipient, subject and plain text.
#[derive(Clone)]
struct Took {
    key: String,
    to: String,
    subject: String,
    text: String,
}

/// The fake provider: refuses while `down`, else records each email it took.
#[derive(Default)]
struct FakeMail {
    down: Mutex<bool>,
    sent: Mutex<Vec<Took>>,
}

impl EmailAdapter for FakeMail {
    async fn send_email(
        &self,
        key: &IdempotencyKey,
        e: &Email<'_>,
    ) -> Result<String, AdapterError> {
        if *self.down.lock().expect("lock") {
            return Err(AdapterError::Vendor {
                status: 500,
                code: None,
            });
        }
        let mut sent = self.sent.lock().expect("lock");
        sent.push(Took {
            key: key.as_str().to_owned(),
            to: e.to.to_owned(),
            subject: e.subject.to_owned(),
            text: e.text.to_owned(),
        });
        Ok(format!("re_{:08}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// The composition's directory: the owner's and a member's mailbox each have their own fake
/// address (any other has none); the stopped phone's "last 4" is whatever `digits` holds, so a
/// directory that hands back the whole number can be tried.
struct FakeDirectory {
    digits: &'static str,
}

impl AccountDirectory for FakeDirectory {
    async fn address(
        &self,
        _: &mut PgConnection,
        mailbox: Uuid,
    ) -> Result<Option<SecretString>, HandlerError> {
        let found = match mailbox {
            OWNER_MAILBOX => Some(OWNER_ADDRESS),
            MEMBER_MAILBOX => Some(MEMBER_ADDRESS),
            _ => None,
        };
        Ok(found.map(|a| SecretString::from(a.to_owned())))
    }
    async fn last4(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
    ) -> Result<Option<String>, HandlerError> {
        Ok(Some(self.digits.to_owned()))
    }
}

type Mailer = Arc<AccountMailer<FakeMail, FakeDirectory>>;

fn mailer() -> Mailer {
    mailer_with("0142")
}

fn mailer_with(digits: &'static str) -> Mailer {
    Arc::new(AccountMailer {
        secret: SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned()),
        postal: (
            "PO Box 100, Faketown, AZ 00000".to_owned(),
            MailboxKind::PoBox,
        ),
        provider: FakeMail::default(),
        limit: DailyLimit::new(1000, 80).expect("limit"),
        directory: FakeDirectory { digits },
        clock: Arc::new(SystemClock),
    })
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

fn form() -> SignUpForm {
    SignUpForm {
        categories: vec!["plumbing_repair".to_owned()],
        mailbox_id: Some(OWNER_MAILBOX),
        heard_from: None,
        g_code: None,
    }
}

async fn count(db: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect("count")
}

async fn tasks(db: &mut PgConnection, kind: &str) -> i64 {
    let q = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1").bind(kind);
    q.fetch_one(db).await.expect("tasks")
}

async fn signup(
    pool: &PgPool,
    consent: SignupConsent<'_, AnyPhone>,
) -> Result<Uuid, SignupRefusal> {
    let map = ClassMap::load("commercial_only: []\nclasses: []\n", CATS).expect("map");
    let secret = SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned());
    let up = submit_signup(pool, (&FakeStripe, &secret, &map), &form(), consent, at(0)).await;
    up.map(|u| u.business)
}

fn email_only(ticked: bool) -> SignupConsent<'static, AnyPhone> {
    SignupConsent {
        ticked,
        channels: &[ByEmail],
        text_phone: None,
    }
}

fn claimed(kind: &str, payload: serde_json::Value) -> Claimed {
    Claimed {
        id: 1,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    }
}

/// TD-CON-29 (CON-019, PICK-049): the form shows the box for the chosen channels, never ticked,
/// and a post without the tick comes back saying why by the box.
#[test]
fn td_con_29_the_form_shows_the_box_unticked_for_the_chosen_channels() {
    let parts = |texting_on| Parts {
        texting_on,
        language: PageLanguage::En,
        csrf: "fake-csrf",
        fees: fee_numbers(ConfirmSettings::default(), WindowRules::default()),
    };
    let cats = [("plumbing_repair".to_owned(), "Open now")];
    let page = signup_page(Screen::Form(&cats), &parts(false)).expect("form");
    assert!(page.contains(&consent_box(&[ByEmail]).wording));
    assert!(
        page.contains("<input type=\"checkbox\" id=\"consent\" name=\"consent\" value=\"yes\">")
    );
    assert!(
        !page.contains("job-card texts"),
        "no text wording without the text choice"
    );
    let chose = Entered {
        text_chosen: true,
        ..Entered::default()
    };
    let text = signup_page(Screen::FormAgain(&cats, &chose), &parts(true)).expect("form");
    assert!(text.contains(&consent_box(&[ByEmail, Text]).wording));
    assert_eq!(
        signup_channels(true, false),
        vec![ByEmail],
        "texting off: email only"
    );
    let missing = Entered {
        problem: Some(("consent", CONSENT_MISSING)),
        ..Entered::default()
    };
    let again = signup_page(Screen::FormAgain(&cats, &missing), &parts(false)).expect("form");
    assert!(again.contains("<a href=\"#consent\">Tick the box to agree to get notices."));
    assert!(again.contains("id=\"consent-error\" class=\"error\">Error: Tick the box"));
    assert!(again.contains("aria-invalid=\"true\" aria-describedby=\"consent-error\""));
    assert!(
        !again.contains("checked"),
        "never ticked, also after an error"
    );
}

/// TD-CON-29 (CON-019, A11Y-030): each refusal that stored nothing names its own fix by the box:
/// no tick asks for the tick, a ticked text box with no phone asks for the phone, never the tick.
#[test]
fn td_con_29_each_refusal_says_its_own_fix() {
    assert_eq!(
        refusal_problem(&SignupRefusal::NoConsent),
        Some(("consent", CONSENT_MISSING))
    );
    assert_eq!(
        refusal_problem(&SignupRefusal::NoTextPhone),
        Some(("consent", TEXT_PHONE_MISSING))
    );
    assert_ne!(CONSENT_MISSING, TEXT_PHONE_MISSING);
    assert!(!TEXT_PHONE_MISSING.contains("Tick"));
    let failed = SignupRefusal::Team(TeamError::Database);
    assert_eq!(refusal_problem(&failed), None, "not a form problem");
    let parts = Parts {
        texting_on: true,
        language: PageLanguage::En,
        csrf: "fake-csrf",
        fees: fee_numbers(ConfirmSettings::default(), WindowRules::default()),
    };
    let cats = [("plumbing_repair".to_owned(), "Open now")];
    let shown = Entered {
        problem: refusal_problem(&SignupRefusal::NoTextPhone),
        text_chosen: true,
        ..Entered::default()
    };
    let page = signup_page(Screen::FormAgain(&cats, &shown), &parts).expect("form");
    assert!(page.contains(&format!("class=\"error\">Error: {TEXT_PHONE_MISSING}")));
    assert!(!page.contains(CONSENT_MISSING));
}

/// TD-CON-29 (CON-019): without the tick, or a text box with no phone, nothing is stored; with
/// it, one consent row per channel beside the owner, and a repeat of the rows adds none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_signup_stores_consent_only_with_the_tick(pool: PgPool) {
    let refused = signup(&pool, email_only(false)).await;
    assert!(matches!(refused, Err(SignupRefusal::NoConsent)));
    let no_phone = SignupConsent::<AnyPhone> {
        ticked: true,
        channels: &[ByEmail, Text],
        text_phone: None,
    };
    assert!(matches!(
        signup(&pool, no_phone).await,
        Err(SignupRefusal::NoTextPhone)
    ));
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(
        count(&mut db, "SELECT count(*) FROM businesses.business").await,
        0
    );
    assert_eq!(
        count(&mut db, "SELECT count(*) FROM businesses.esign_consent").await,
        0
    );
    let number = SecretString::from("+15555550142".to_owned());
    let both = SignupConsent {
        ticked: true,
        channels: &[ByEmail, Text],
        text_phone: Some((&AnyPhone, &number)),
    };
    let b = signup(&pool, both).await.expect("signed up");
    let rows: Vec<(String, bool, String)> = sqlx::query_as(
        "SELECT channel, line_id IS NOT NULL, wording_version FROM businesses.esign_consent \
         WHERE business_id = $1 ORDER BY channel",
    )
    .bind(b)
    .fetch_all(&mut *db)
    .await
    .expect("rows");
    let v = || "con019-v1".to_owned();
    assert_eq!(
        rows,
        vec![
            ("email".to_owned(), false, v()),
            ("text".to_owned(), true, v())
        ]
    );
    let owners = "SELECT count(*) FROM businesses.team_member WHERE is_owner";
    assert_eq!(count(&mut db, owners).await, 1);
    repeat_adds_no_consent_row(&mut db, b).await;
}

/// Records the same owner's email and text consent again for the same phone: no new row.
async fn repeat_adds_no_consent_row(db: &mut PgConnection, b: Uuid) {
    let (owner, line): (Uuid, Uuid) = sqlx::query_as(
        "SELECT member_id, line_id FROM businesses.esign_consent \
         WHERE business_id = $1 AND channel = 'text'",
    )
    .bind(b)
    .fetch_one(&mut *db)
    .await
    .expect("text row");
    for _ in 0..2 {
        record_consent(db, (b, owner, Some(line)), &[ByEmail, Text], true, at(5))
            .await
            .expect("repeat");
    }
    let all = "SELECT count(*) FROM businesses.esign_consent";
    assert_eq!(count(db, all).await, 2, "a repeat adds no consent row");
}

async fn signed_up_with_code(pool: &PgPool, db: &mut PgConnection) -> Uuid {
    let b = signup(pool, email_only(true)).await.expect("signed up");
    sqlx::query("UPDATE businesses.signup SET code_done = true WHERE business_id = $1")
        .bind(b)
        .execute(db)
        .await
        .expect("code done");
    b
}

/// TD-CON-29 (CON-020, BIZ-016): activation by code and match queues one welcome; a provider
/// failure never marks it; it goes once, by email to the owner, with the contact card, and only
/// then may cards go; repeats change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_activation_sends_one_welcome(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signed_up_with_code(&pool, &mut db).await;
    let s = record_match(&mut db, b, Some(true), None, at(1))
        .await
        .expect("match");
    assert_eq!(s.code(), "active");
    record_match(&mut db, b, Some(true), None, at(2))
        .await
        .expect("again");
    assert_eq!(tasks(&mut db, WELCOME_KIND).await, 1);
    let (m, task) = (mailer(), claimed(WELCOME_KIND, json!({"business": b})));
    *m.provider.down.lock().expect("lock") = true;
    let failed = run_welcome(pool.clone(), Some(Arc::clone(&m)), task.clone()).await;
    assert!(failed.is_err());
    assert!(
        !may_get_cards(&mut db, b).await.expect("gate"),
        "never marked on a failure"
    );
    *m.provider.down.lock().expect("lock") = false;
    for _ in 0..2 {
        run_welcome(pool.clone(), Some(Arc::clone(&m)), task.clone())
            .await
            .expect("sent");
    }
    assert!(may_get_cards(&mut db, b).await.expect("gate"));
    let sent = m.provider.sent.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1, "one welcome");
    assert_eq!(sent[0].to, OWNER_ADDRESS);
    assert!(sent[0].text.contains(&welcome_card_line()));
    let texts = "SELECT count(*) FROM messaging.outbox";
    assert_eq!(count(&mut db, texts).await, 0, "never a text");
    let none = run_welcome::<FakeMail, FakeDirectory>(pool.clone(), None, task).await;
    assert!(none.is_err(), "no sender: tried again, never marked");
}

/// TD-CON-29 (BIZ-016, CON-020): Casey's approval with the code and the match activates and
/// queues one welcome; a needs-review sign-up queues none before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_approval_queues_one_welcome(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signed_up_with_code(&pool, &mut db).await;
    let flag = Some(Flag::NameMismatch);
    let s = record_match(&mut db, b, Some(true), flag, at(1))
        .await
        .expect("flag");
    assert_eq!(s.code(), "needs_review");
    assert_eq!(tasks(&mut db, WELCOME_KIND).await, 0);
    let s = approve(&mut db, b, (Uuid::from_u128(0xca5e), at(2)))
        .await
        .expect("approve");
    assert_eq!(s.code(), "active");
    assert_eq!(tasks(&mut db, WELCOME_KIND).await, 1);
}

/// TD-CON-29 (BIZ-017): a business phone's STOP queues one email to the owner's account address
/// naming only the last 4 digits; repeats change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_phone_stop_emails_the_owner_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signup(&pool, email_only(true)).await.expect("signed up");
    let inbox = Uuid::from_u128(0x1b0);
    for _ in 0..2 {
        phone_stopped(&mut db, b, inbox, at(1))
            .await
            .expect("stopped");
    }
    assert_eq!(tasks(&mut db, PHONE_STOPPED_KIND).await, 1);
    let m = mailer();
    let task = claimed(PHONE_STOPPED_KIND, json!({"business": b, "inbox": inbox}));
    for _ in 0..2 {
        run_phone_stopped(pool.clone(), Some(Arc::clone(&m)), task.clone())
            .await
            .expect("sent");
    }
    let sent = m.provider.sent.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1, "one email");
    assert_eq!(sent[0].to, OWNER_ADDRESS);
    assert!(sent[0].text.contains("the phone ending 0142"));
    assert!(!sent[0].text.contains("5550142"), "only the last 4 digits");
}

/// TD-CON-29 (CON-020, SEC-018): a crash after the provider took the welcome but before it was
/// marked sent leaves `sent_at` empty; the retry finds the same outbox row and provider key and
/// sends nothing more, then marks it. A random key per run would send a second welcome.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_a_retry_after_a_crash_sends_no_second_welcome(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signed_up_with_code(&pool, &mut db).await;
    record_match(&mut db, b, Some(true), None, at(1))
        .await
        .expect("match");
    let (m, task) = (mailer(), claimed(WELCOME_KIND, json!({"business": b})));
    run_welcome(pool.clone(), Some(Arc::clone(&m)), task.clone())
        .await
        .expect("sent");
    sqlx::query("UPDATE businesses.welcome SET sent_at = NULL WHERE business_id = $1")
        .bind(b)
        .execute(&mut *db)
        .await
        .expect("crash before the mark");
    assert!(!may_get_cards(&mut db, b).await.expect("gate"));
    run_welcome(pool.clone(), Some(Arc::clone(&m)), task)
        .await
        .expect("retried");
    assert!(may_get_cards(&mut db, b).await.expect("gate"), "marked now");
    let sent = m.provider.sent.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1, "the provider took one welcome");
    let rows = "SELECT count(*) FROM messaging.email_outbox";
    assert_eq!(count(&mut db, rows).await, 1, "one outbox row");
}

/// TD-CON-29 (CON-020, D-964): with another member on the account, the welcome goes to the
/// owner's account address only, never the member's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_the_welcome_goes_to_the_owner_not_a_member(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signed_up_with_code(&pool, &mut db).await;
    let owner: Uuid = sqlx::query_scalar(
        "SELECT id FROM businesses.team_member WHERE business_id = $1 AND is_owner",
    )
    .bind(b)
    .fetch_one(&mut *db)
    .await
    .expect("owner");
    let by = Acting {
        business: b,
        member: owner,
    };
    let roles = Roles::new(true, true).expect("roles");
    add_member(&mut db, by, MEMBER_MAILBOX, roles, at(1))
        .await
        .expect("member");
    record_match(&mut db, b, Some(true), None, at(2))
        .await
        .expect("match");
    let m = mailer();
    run_welcome(
        pool.clone(),
        Some(Arc::clone(&m)),
        claimed(WELCOME_KIND, json!({"business": b})),
    )
    .await
    .expect("sent");
    let sent = m.provider.sent.lock().expect("lock").clone();
    let to: Vec<&str> = sent.iter().map(|t| t.to.as_str()).collect();
    assert_eq!(to, vec![OWNER_ADDRESS], "the owner alone");
}

/// TD-CON-29 (CON-020, D-3188): the activation and the welcome request commit together: when
/// the welcome request fails, the business stays pending with no status move recorded; once it
/// works, the same checks activate and queue one welcome.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_a_failed_welcome_request_rolls_back_the_activation(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signed_up_with_code(&pool, &mut db).await;
    for sql in [
        "CREATE FUNCTION pg_temp.no_welcome() RETURNS trigger LANGUAGE plpgsql AS \
         $$BEGIN RAISE EXCEPTION 'injected welcome failure'; END$$",
        "CREATE TRIGGER no_welcome BEFORE INSERT ON businesses.welcome \
         FOR EACH ROW EXECUTE FUNCTION pg_temp.no_welcome()",
    ] {
        sqlx::query(sql).execute(&mut *db).await.expect("inject");
    }
    let failed = record_match(&mut db, b, Some(true), None, at(1)).await;
    assert!(failed.is_err(), "the welcome request failed");
    let status: String = sqlx::query_scalar("SELECT status FROM businesses.business WHERE id = $1")
        .bind(b)
        .fetch_one(&mut *db)
        .await
        .expect("status");
    assert_eq!(status, "pending", "the activation rolled back with it");
    assert_eq!(tasks(&mut db, WELCOME_KIND).await, 0);
    sqlx::query("DROP TRIGGER no_welcome ON businesses.welcome")
        .execute(&mut *db)
        .await
        .expect("heal");
    let s = record_match(&mut db, b, Some(true), None, at(2))
        .await
        .expect("match");
    assert_eq!(s.code(), "active");
    assert_eq!(tasks(&mut db, WELCOME_KIND).await, 1);
}

/// TD-CON-29 (BIZ-017, PRIV-001): a directory that hands back the whole number gets no email
/// out at all, so the full number never reaches a subject, a body or the outbox.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_29_a_full_number_never_reaches_the_stop_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = signup(&pool, email_only(true)).await.expect("signed up");
    let inbox = Uuid::from_u128(0x1b1);
    phone_stopped(&mut db, b, inbox, at(1))
        .await
        .expect("stopped");
    let task = claimed(PHONE_STOPPED_KIND, json!({"business": b, "inbox": inbox}));
    let leaky = mailer_with(FULL_NUMBER);
    let refused = run_phone_stopped(pool.clone(), Some(Arc::clone(&leaky)), task.clone()).await;
    assert!(refused.is_err(), "only 4 digits are sent");
    assert!(leaky.provider.sent.lock().expect("lock").is_empty());
    let rows = "SELECT count(*) FROM messaging.email_outbox";
    assert_eq!(count(&mut db, rows).await, 0, "nothing stored");
    let m = mailer();
    run_phone_stopped(pool.clone(), Some(Arc::clone(&m)), task)
        .await
        .expect("sent");
    let sent = m.provider.sent.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1);
    for words in [&sent[0].subject, &sent[0].text, &sent[0].key] {
        assert!(!words.contains("5550142"), "never more than 4 digits");
    }
    assert!(sent[0].subject.ends_with("ending 0142 are off"));
}
