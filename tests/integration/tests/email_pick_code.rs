//! The pick code by email when Ask Hava can't text the homeowner, and the opted-out "2 YES" pick,
//! on a fresh database (TD-PICK-13; PICK-029, PICK-030, PICK-013, D-050, D-1612 to D-1614). Fake
//! numbers (555-01xx), example.test addresses, IDs, codes and words only.
//!
//! A request is opened with its consent text and a job walks to Quotes ready with two quotes.
//! The Verify adapter records each send's destination and channel and passes only [`CODE`].
//! TD-TXT-23 and TD-INT-36 (TXT-018's quote-page channel choice, INT-041's approved card and
//! TXT-040's page lines) have no case here: they wait on `privacy`, `messaging`, `intake` and
//! `quotes` work this task does not own (D-1612).

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::rules::SendRefused;
use askhava_picks::api::{
    CodeCheck, CodeRoute, CodeSent, ConversationEmail, EmailFrom, OptedOutYes, Texting,
    check_email_code, code_route, opted_out_yes, record_terms_line, send_email_code,
    terms_line_sent, waiting_quote,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PHONE: &str = "+19285550152";
const EMAIL: &str = "pat.sample@example.test";
const CODE: &str = "123456";
const DAY: &str = "2026-11-02T17:00:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x47), &key(0x57), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-095".to_owned())
}

fn conversation_email() -> ConversationEmail {
    let address = SecretString::from(EMAIL.to_owned());
    ConversationEmail::new(address, EmailFrom::Conversation).expect("conversation address")
}

/// A Verify stand-in: records each send's destination and channel and passes only [`CODE`].
#[derive(Clone, Default)]
struct FakeVerify {
    sent: Arc<Mutex<Vec<(String, CodeChannel)>>>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sent
            .lock()
            .expect("lock")
            .push((to.to_owned(), channel));
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, to: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(to == EMAIL && code == CODE)
    }
}

/// A homeowner texts in and her plumbing job reaches Quotes ready with two confirmed quotes;
/// returns her contact and the job.
async fn ready(db: &mut PgConnection) -> (Uuid, Uuid) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: ts(DAY),
    };
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    for n in 1..=2_i64 {
        quote(db, job, n).await;
    }
    (her, job)
}

/// Business `n` quotes on `job`, confirmed, arriving `n` minutes in (fake wording).
async fn quote(db: &mut PgConnection, job: Uuid, n: i64) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0x0950 + u128::from(n.unsigned_abs())))
    .bind((ts(DAY) + SignedDuration::from_mins(n)).to_sqlx())
    .execute(db)
    .await
    .expect("quote");
}

/// Every queued text, whoever it is to: an opted-out YES must add none (CON-013).
async fn outbox(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messaging.outbox")
        .fetch_one(db)
        .await
        .expect("outbox")
}

/// The job's stored picks.
async fn picks(db: &mut PgConnection, job: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM picks.pick WHERE job_id = $1")
        .bind(job)
        .fetch_one(db)
        .await
        .expect("count")
}

/// TD-PICK-13 (PICK-029, D-050): with texting open the code goes by text; opted out or texting
/// off, only to an address from her own conversation; a quote-page address never gets a code.
#[test]
fn td_pick_13_the_code_goes_by_email_only_to_a_conversation_address() {
    assert_eq!(code_route(Texting::Open, None), CodeRoute::Text);
    for texting in [Texting::OptedOut, Texting::Off] {
        let from = Some(EmailFrom::Conversation);
        assert_eq!(code_route(texting, from), CodeRoute::Email);
        let typed = Some(EmailFrom::QuotePage);
        assert_eq!(code_route(texting, typed), CodeRoute::Nowhere);
        assert_eq!(code_route(texting, None), CodeRoute::Nowhere);
    }
    let typed = SecretString::from(EMAIL.to_owned());
    assert!(ConversationEmail::new(typed, EmailFrom::QuotePage).is_none());
    assert!(!format!("{:?}", conversation_email()).contains("example"));
}

/// TD-PICK-13 (PICK-029, PICK-013, PICK-055): the emailed code goes through Verify's email channel
/// with the texted code's limits: a wrong try counts, the right one passes, five codes a day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_13_the_emailed_code_keeps_the_pick_code_limits(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (her, job) = ready(&mut db).await;
    let verify = FakeVerify::default();
    let to = conversation_email();
    let keys = (&verify, &secret());
    let mut at = ts(DAY);
    let sent = send_email_code(&mut db, keys, (job, her), &to, (at, 500)).await;
    assert_eq!(sent.expect("send"), CodeSent::Sent);
    let first = verify.sent.lock().expect("lock")[0].clone();
    assert_eq!(first, (EMAIL.to_owned(), CodeChannel::Email));
    let wrong = check_email_code(&mut db, &verify, (job, her), &to, ("654321", at)).await;
    assert_eq!(wrong.expect("check"), CodeCheck::Wrong { left: 4 });
    let right = check_email_code(&mut db, &verify, (job, her), &to, ("123 456", at)).await;
    assert_eq!(right.expect("check"), CodeCheck::Accepted);
    for _ in 0..4 {
        at += SignedDuration::from_mins(11);
        let sent = send_email_code(&mut db, keys, (job, her), &to, (at, 500)).await;
        assert_eq!(sent.expect("send"), CodeSent::Sent);
    }
    let sixth = send_email_code(&mut db, keys, (job, her), &to, (at, 500)).await;
    assert_eq!(
        sixth.expect("send"),
        CodeSent::Refused(SendRefused::NumberCap)
    );
    assert_eq!(verify.sent.lock().expect("lock").len(), 5);
}

/// TD-PICK-13 (PICK-013): five wrong emailed codes start the 15-minute cooldown, during which no
/// new code goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_13_five_wrong_emailed_codes_cool_down(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (her, job) = ready(&mut db).await;
    let verify = FakeVerify::default();
    let to = conversation_email();
    let at = ts(DAY);
    let sent = send_email_code(&mut db, (&verify, &secret()), (job, her), &to, (at, 500)).await;
    assert_eq!(sent.expect("send"), CodeSent::Sent);
    for _ in 0..5 {
        let tried = check_email_code(&mut db, &verify, (job, her), &to, ("000000", at)).await;
        assert!(!matches!(tried.expect("check"), CodeCheck::Accepted));
    }
    let again = send_email_code(&mut db, (&verify, &secret()), (job, her), &to, (at, 500)).await;
    assert!(matches!(
        again.expect("send"),
        CodeSent::Refused(SendRefused::CoolingDown(_))
    ));
    assert_eq!(verify.sent.lock().expect("lock").len(), 1);
}

/// TD-PICK-13 (PICK-030, D-050, D-478): "2 YES" from an opted-out phone queues no text; before the
/// Terms line it picks nothing, once the Terms line was sent it picks quote 2.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_13_two_yes_from_an_opted_out_phone_still_picks(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, job) = ready(&mut db).await;
    let before = outbox(&mut db).await;
    let by = (Actor::System, ts(DAY));
    assert!(!terms_line_sent(&mut db, job).await.expect("terms"));
    let first = opted_out_yes(&mut db, "2 YES", &[(job, false)], by).await;
    assert_eq!(first.expect("yes"), Some(OptedOutYes::TermsFirst(job)));
    assert_eq!(picks(&mut db, job).await, 0);
    record_terms_line(
        &mut db,
        (job, Uuid::new_v4()),
        "fake:en:000000000000",
        ts(DAY),
    )
    .await
    .expect("terms line");
    let second = opted_out_yes(&mut db, "2 yes", &[(job, false)], by).await;
    assert!(matches!(second.expect("yes"), Some(OptedOutYes::Picked(_))));
    assert_eq!(picks(&mut db, job).await, 1);
    assert_eq!(waiting_quote(&mut db, job).await.expect("waiting"), Some(2));
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status.code(), "awaiting_confirm");
    assert_eq!(outbox(&mut db).await, before);
    let repeat = opted_out_yes(&mut db, "2 YES", &[(job, false)], by).await;
    assert!(matches!(repeat.expect("yes"), Some(OptedOutYes::Same(_))));
    let none = opted_out_yes(&mut db, "thanks", &[(job, false)], by).await;
    assert_eq!(none.expect("words"), None);
    let paused = opted_out_yes(&mut db, "1 YES", &[(job, true)], by).await;
    assert_eq!(paused.expect("yes"), Some(OptedOutYes::Nothing));
}
