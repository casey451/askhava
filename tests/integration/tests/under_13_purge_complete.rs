//! The under-13 purge also deletes her kept problem words and queues her provider body deletes at
//! once (T839: INT-038, PRIV-008, PRIV-027; D-5639). Fake IDs, fake keys, fake 555 phones, fake
//! SIDs and fake words only.
//!
//! A day-3 check-in PROBLEM reply's words are kept as a `privacy.message_body` row under her job,
//! and a help request she opened without a job keeps its words under its own ID. "I'm 12" then
//! deletes both rows and queues one `messaging.body_delete` task, due at the purge time, for each
//! inbound SID of hers (the PROBLEM reply's and the "I'm 12" text's) and each outbound SID sent to
//! her; the consent record stays and another person's words and SIDs are untouched.

use askhava_intake::api::rules::{AgeStop, Inbound, Settings};
use askhava_intake::api::{
    JobStart, Opened, RawReply, Reply, TextIn, begin_job, receive_raw_reply,
};
use askhava_messaging::api::BODY_DELETE_KIND;
use askhava_ops::api::{Asker, HelpLimits, HelpOpened, open_help_request};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const LEAK: &str = "my kitchen sink is leaking under the cabinet";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x83), &key(0x39), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

/// A fake Twilio message SID numbered `n`.
fn sid(n: u128) -> String {
    format!("SM{n:032x}")
}

/// The inbox ID of `contact`'s text at `minute`.
fn inbox_of(contact: Uuid, minute: i64) -> Uuid {
    Uuid::from_u128(contact.as_u128() ^ (0x839_0000 + minute.unsigned_abs() as u128))
}

/// Records `contact`'s inbound text at `minute` as the webhook does: its `messaging.inbox` row
/// under SID `n` and its `messaging.inbox_contact` row.
async fn arrived(db: &mut Db, contact: Uuid, minute: i64, n: u128) {
    let id = inbox_of(contact, minute);
    for sql in [
        "INSERT INTO messaging.inbox (id, message_sid, received_at) VALUES ($1, $2, $3)",
        "INSERT INTO messaging.inbox_contact (inbox_id, contact_id, received_at) \
         VALUES ($1, $4, $3)",
    ] {
        sqlx::query(sql)
            .bind(id)
            .bind(sid(n))
            .bind(at(minute).to_sqlx())
            .bind(contact)
            .execute(&mut **db)
            .await
            .expect("inbound");
    }
}

/// The reply to `words` from `contact` at `minute` (SID `n`), through the route's entry.
async fn reply(db: &mut Db, contact: Uuid, (words, minute, n): (&str, i64, u128)) -> Reply {
    arrived(db, contact, minute, n).await;
    let msg = TextIn {
        inbox_id: inbox_of(contact, minute),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    };
    let raw = SecretString::from(words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    receive_raw_reply(db, &secret, (&msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

/// Keeps fake scrubbed words for inbox row `inbox` under `key` (a job or a help request ID).
async fn keep(db: &mut Db, inbox: Uuid, key: Uuid) {
    sqlx::query(
        "INSERT INTO privacy.message_body (inbox_id, job_id, words, safety, created_at) \
         VALUES ($1, $2, 'fake words: the faucet still drips', false, now())",
    )
    .bind(inbox)
    .bind(key)
    .execute(&mut **db)
    .await
    .expect("kept words");
}

/// Marks `contact`'s oldest queued text sent under SID `n`.
async fn sent(db: &mut Db, contact: Uuid, n: u128) {
    sqlx::query(
        "UPDATE messaging.outbox SET status = 'sent', sent_at = now(), provider_sid = $2, \
         body = NULL WHERE id = (SELECT id FROM messaging.outbox WHERE recipient_id = $1 \
         ORDER BY created_at, id LIMIT 1)",
    )
    .bind(contact)
    .bind(sid(n))
    .execute(&mut **db)
    .await
    .expect("sent");
}

/// A fake contact with phone `phone`, a request and its plumbing job: (contact, request, job).
async fn seeded(db: &mut Db, phone: &str, base: u128) -> (Uuid, Uuid, Uuid) {
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    let who = store_contact(db, &vault(), &new).await.expect("contact");
    let Reply::Handled(first) = reply(db, who, (LEAK, 0, base)).await else {
        panic!("a first text opens a request");
    };
    let request = first.conversation.expect("request");
    let start = JobStart {
        conversation: request,
        words: LEAK,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    sent(db, who, base + 1).await;
    (who, request, job)
}

/// A day-3 PROBLEM reply from `who` on `job` (SID `n`), its words kept under the job.
async fn problem(db: &mut Db, who: Uuid, job: Uuid, n: u128) {
    let minute = 3 * 24 * 60;
    arrived(db, who, minute, n).await;
    keep(db, inbox_of(who, minute), job).await;
}

/// A help request `who` opened without a job, its words kept under its own ID.
async fn help(db: &mut Db, who: Uuid) -> Uuid {
    let key = (
        Uuid::new_v4(),
        at(5),
        at(5) + SignedDuration::from_hours(24),
    );
    let opened = open_help_request(db, Asker::Homeowner(who), key, HelpLimits::default())
        .await
        .expect("help request");
    let HelpOpened::Opened { request, .. } = opened else {
        panic!("a new help request");
    };
    keep(db, key.0, request).await;
    request
}

async fn words_kept(db: &mut Db, keys: &[Uuid]) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM privacy.message_body WHERE job_id = ANY($1)")
        .bind(keys)
        .fetch_one(&mut **db)
        .await
        .expect("words")
}

/// The SIDs with a body delete queued due at `due`, sorted.
async fn deletes_due(db: &mut Db, due: Timestamp) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT payload->>'sid' FROM core.task WHERE kind = $1 AND run_at = $2 \
         AND dedupe_key = $1 || ':' || (payload->>'sid') ORDER BY 1",
    )
    .bind(BODY_DELETE_KIND)
    .bind(due.to_sqlx())
    .fetch_all(&mut **db)
    .await
    .expect("deletes")
}

async fn consents(db: &mut Db, who: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM intake.consent_record r JOIN intake.homeowner h \
         ON h.id = r.homeowner_id WHERE h.contact_id = $1",
    )
    .bind(who)
    .fetch_one(&mut **db)
    .await
    .expect("consents")
}

/// INT-038, PRIV-008, PRIV-027 (D-5639): "I'm 12" after a day-3 PROBLEM reply deletes her kept
/// words under the job and under a help request with no job, queues a body delete due at once for
/// every inbound and outbound SID of hers, and keeps the consent record; another person's words
/// and SIDs stay as they were.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn under_13_purge_deletes_kept_words_and_queues_body_deletes_now(mut db: Db) {
    let (who, _, job) = seeded(&mut db, "(928) 555-0183", 0x8390).await;
    let (other, _, other_job) = seeded(&mut db, "(928) 555-0184", 0x8490).await;
    problem(&mut db, who, job, 0x8392).await;
    problem(&mut db, other, other_job, 0x8492).await;
    let asked = help(&mut db, who).await;
    assert_eq!(words_kept(&mut db, &[job, asked]).await, 2);
    let purge_minute = 3 * 24 * 60 + 10;
    let got = reply(&mut db, who, ("I'm 12", purge_minute, 0x8393)).await;
    assert!(matches!(got, Reply::UnderAge(AgeStop::Under13, _)));
    assert_eq!(words_kept(&mut db, &[job, asked]).await, 0);
    let mut hers: Vec<String> = [0x8390, 0x8391, 0x8392, 0x8393].map(sid).into();
    hers.sort();
    assert_eq!(deletes_due(&mut db, at(purge_minute)).await, hers);
    assert_eq!(consents(&mut db, who).await, 1);
    assert_eq!(words_kept(&mut db, &[other_job]).await, 1);
}
