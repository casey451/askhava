//! The under-13 stop's purge of the request's job photos and kept job words (TD-INT-44, T650:
//! INT-038, PRIV-008; D-4294). Fake IDs, fake keys, fake phones, fake object names and fake words
//! only.
//!
//! "I'm 11" deletes, in the stop's own transaction, every photo row of the request's jobs (each
//! sealed object's delete queued) and every kept job description and card of those jobs; the
//! consent record stays; another person's request keeps its photos and words; a failing purge
//! rolls the whole stop back.

use askhava_intake::api::rules::{AgeStop, Inbound, Settings};
use askhava_intake::api::{
    JobStart, Opened, RawReply, Reply, TextIn, begin_job, receive_raw_reply,
};
use askhava_privacy::api::{NewContact, PHOTO_DELETE_KIND, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const LEAK: &str = "my kitchen sink is leaking under the cabinet";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x65), &key(0x66), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

/// The reply to `words` from `contact` at `minute`, through the route's entry.
async fn reply(db: &mut Db, contact: Uuid, words: &str, minute: i64) -> Result<Reply, String> {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(contact.as_u128() ^ (0x650_0000 + minute.unsigned_abs() as u128)),
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
        .map_err(|e| e.to_string())
}

/// A fake contact with a request, its job, two photo rows and a kept description and card.
async fn seeded(db: &mut Db, phone: &str, tag: &str) -> (Uuid, Uuid, Uuid) {
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    let who = store_contact(db, &vault(), &new).await.expect("contact");
    let Ok(Reply::Handled(first)) = reply(db, who, LEAK, 0).await else {
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
    for sql in [
        "INSERT INTO privacy.photo (id, job_id, key_version, storage_key, stored_at) VALUES \
         (gen_random_uuid(), $1, 1, 'photos/fake-' || $2 || '-1', now()), \
         (gen_random_uuid(), $1, 1, 'photos/fake-' || $2 || '-2', now())",
        "INSERT INTO quotes.job_description (request_id, job_id, message_id, words, recorded_at) \
         VALUES ($3, $1, gen_random_uuid(), 'fake scrubbed words ' || $2, now())",
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
         photo_ids, category, candidates, emergency, recorded_at) VALUES ($3, $1, \
         md5($2), 'fake card ' || $2, 'Lake Havasu City', 'house', '{}', 'plumbing_repair', \
         '{leak_repair}', false, now())",
    ] {
        sqlx::query(sql)
            .bind(job)
            .bind(tag)
            .bind(request)
            .execute(&mut **db)
            .await
            .expect("seed");
    }
    (who, request, job)
}

/// (photo rows, queued object deletes, descriptions, cards) of `job`, whose objects carry `tag`.
async fn kept(db: &mut Db, job: Uuid, tag: &str) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM privacy.photo WHERE job_id = $1),
           (SELECT count(*) FROM core.task WHERE kind = $3
            AND payload->>'name' LIKE 'photos/fake-' || $2 || '-%'),
           (SELECT count(*) FROM quotes.job_description WHERE job_id = $1),
           (SELECT count(*) FROM quotes.shown_card WHERE job_id = $1)",
    )
    .bind(job)
    .bind(tag)
    .bind(PHOTO_DELETE_KIND)
    .fetch_one(&mut **db)
    .await
    .expect("counts")
}

async fn closed_and_consents(db: &mut Db, request: Uuid, who: Uuid) -> (bool, i64) {
    sqlx::query_as(
        "SELECT (SELECT closed_at IS NOT NULL FROM intake.conversation WHERE id = $1),
           (SELECT count(*) FROM intake.consent_record r JOIN intake.homeowner h
            ON h.id = r.homeowner_id WHERE h.contact_id = $2)",
    )
    .bind(request)
    .bind(who)
    .fetch_one(&mut **db)
    .await
    .expect("request")
}

/// TD-INT-44 (INT-038, PRIV-008, D-4294): "I'm 11" deletes her request's job photo rows (each
/// object's delete queued) and its kept words and card; the consent record stays; another
/// person's request keeps its photos and words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_44_under_13_deletes_photos_and_job_words(mut db: Db) {
    let (who, request, job) = seeded(&mut db, "(928) 555-0165", "child").await;
    let (_, _, other) = seeded(&mut db, "(928) 555-0166", "other").await;
    assert_eq!(kept(&mut db, job, "child").await, (2, 0, 1, 1));
    let got = reply(&mut db, who, "I'm 11", 10).await;
    assert!(matches!(got, Ok(Reply::UnderAge(AgeStop::Under13, _))));
    assert_eq!(kept(&mut db, job, "child").await, (0, 2, 0, 0));
    assert_eq!(closed_and_consents(&mut db, request, who).await, (true, 1));
    assert_eq!(kept(&mut db, other, "other").await, (2, 0, 1, 1));
}

/// TD-INT-44 (INT-038, PRIV-008): "I'm 15" closes the request and keeps its photos and words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_44_under_18_keeps_photos_and_job_words(mut db: Db) {
    let (who, request, job) = seeded(&mut db, "(928) 555-0167", "teen").await;
    let got = reply(&mut db, who, "I'm 15", 10).await;
    assert!(matches!(got, Ok(Reply::UnderAge(AgeStop::Under18, _))));
    assert_eq!(kept(&mut db, job, "teen").await, (2, 0, 1, 1));
    assert!(closed_and_consents(&mut db, request, who).await.0);
}

/// TD-INT-44 (INT-038, D-4294): a job-word delete that fails rolls the whole stop back: the
/// request stays open and its photos, queued deletes, words and card are as before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_44_a_failing_purge_keeps_nothing(mut db: Db) {
    let (who, request, job) = seeded(&mut db, "(928) 555-0168", "kept").await;
    for sql in [
        "CREATE FUNCTION public.t650_refuse() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'fake card delete failure'; END $$",
        "CREATE TRIGGER t650_refuse BEFORE DELETE ON quotes.shown_card FOR EACH ROW \
         EXECUTE FUNCTION public.t650_refuse()",
    ] {
        sqlx::query(sql).execute(&mut *db).await.expect("refuse");
    }
    assert!(reply(&mut db, who, "I'm 11", 10).await.is_err());
    assert_eq!(kept(&mut db, job, "kept").await, (2, 0, 1, 1));
    assert!(!closed_and_consents(&mut db, request, who).await.0);
}
