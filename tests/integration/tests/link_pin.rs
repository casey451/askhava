//! TD-CALL-18 (CALL-011, CALL-003; D-1696, D-2091, D-2247): the "Reply LINK <PIN>" notice that
//! `request_link` queues names the line's stored `calls.number_pool.pin`, the PIN the voice route
//! accepts, never the line ID modulo 10,000; a line with no stored PIN (opened before migration
//! 0151) queues no notice. Fake numbers (555-01xx and the 500-555 test range), fake names, fake
//! CallSids and IDs only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::create;
use askhava_calls::api::{
    Attestation, Connected, IncomingCall, JobPin, VoiceCall, VoiceKeys, VoiceReply, assign_number,
    request_link, stored_pin, voice_turn,
};
use askhava_core::api::Settings;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const NUMBER: &str = "+15005550100";
const HER_PHONE: &str = "+19285550181";
const BLOCKED: &str = "+19285550187";
/// 10:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn ts(at: &str) -> Timestamp {
    at.parse().expect("instant")
}

/// A pool of one fake number, so every line is on it.
fn one_number_pool() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", NUMBER),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

/// Her fake contact: name, phone and address.
async fn her(db: &mut PgConnection) -> Uuid {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER_PHONE),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// The line ID modulo 10,000 as four digits: D-1684's retired ID-derived PIN.
fn id_digits(line: Uuid) -> String {
    format!("{:04}", line.as_u128() % 10_000)
}

/// A line opened by `assign_number` for her and business `b` whose stored PIN differs from its
/// ID-derived digits, so a notice built from the ID cannot pass; with that PIN.
async fn line_unlike_its_id(db: &mut PgConnection, (h, b): (Uuid, Uuid)) -> (Uuid, JobPin) {
    loop {
        let job = Connected {
            job: Uuid::new_v4(),
            homeowner: h,
            business: b,
        };
        let line = assign_number(db, &one_number_pool(), job, ts(DAY))
            .await
            .expect("assign")
            .id();
        let pin = stored_pin(db, line).await.expect("read").expect("pin");
        if pin.digits() != id_digits(line) {
            return (line, pin);
        }
    }
}

/// The `values.job` of every LINK notice queued for `line`.
async fn notice_jobs(db: &mut PgConnection, line: Uuid) -> Vec<Option<String>> {
    sqlx::query_scalar(
        "SELECT payload->'values'->>'job' FROM core.task \
         WHERE kind = 'messaging.business_notice' AND payload->>'line' = $1",
    )
    .bind(line.to_string())
    .fetch_all(db)
    .await
    .expect("notices")
}

/// One voice turn from the blocked phone on the pool number entering `entered`.
async fn enter(db: &mut PgConnection, entered: JobPin) -> VoiceReply {
    let (id_secret, pool) = (
        SecretString::from("test-only-id-secret-not-a-real-key-0000".to_owned()),
        one_number_pool(),
    );
    let keys = VoiceKeys {
        id_secret: &id_secret,
        version: 1,
        pool: pool.numbers(),
    };
    let caller = SecretString::from(BLOCKED.to_owned());
    let call = VoiceCall {
        call: IncomingCall {
            pool_number: NUMBER,
            caller: &caller,
        },
        call_sid: &format!("CA{:032}", 18),
        attestation: Attestation::A,
        try_no: 1,
        entered: Some(entered),
    };
    voice_turn(db, &vault(), &keys, &call, ts(DAY))
        .await
        .expect("turn")
}

/// TD-CALL-18 (CALL-011, CALL-003, D-2247): the LINK notice's `job` is the line's stored PIN,
/// never its ID modulo 10,000, and a voice turn entering those digits connects that line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_18_the_link_notice_names_the_stored_pin_that_connects(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let business = create(&mut db, false, ts(DAY)).await.expect("business");
    let h = her(&mut db).await;
    let (line, pin) = line_unlike_its_id(&mut db, (h, business)).await;
    let blocked = SecretString::from(BLOCKED.to_owned());
    assert_eq!(
        request_link(&mut db, line, &blocked, ts(DAY)).await,
        Ok(true)
    );
    let jobs = notice_jobs(&mut db, line).await;
    assert_eq!(jobs, vec![Some(pin.digits())]);
    assert_ne!(jobs, vec![Some(id_digits(line))]);
    let named = JobPin::parse(jobs[0].as_deref().expect("job")).expect("four digits");
    let reply = enter(&mut db, named).await;
    assert!(
        matches!(reply, VoiceReply::Dial(_)),
        "the named PIN connects"
    );
    let started: i64 =
        sqlx::query_scalar("SELECT count(*) FROM calls.call_start WHERE line_id = $1")
            .bind(line)
            .fetch_one(&mut *db)
            .await
            .expect("starts");
    assert_eq!(started, 1, "the call connected that line");
}

/// TD-CALL-18 (CALL-011, D-2247): a line with no stored PIN (one opened before migration 0151)
/// queues no LINK notice and answers `false`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_18_a_line_with_no_stored_pin_queues_no_notice(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let business = create(&mut db, false, ts(DAY)).await.expect("business");
    let h = her(&mut db).await;
    let (line, _) = line_unlike_its_id(&mut db, (h, business)).await;
    sqlx::query("UPDATE calls.number_pool SET pin = NULL WHERE id = $1")
        .bind(line)
        .execute(&mut *db)
        .await
        .expect("a pre-0151 line");
    assert_eq!(stored_pin(&mut db, line).await, Ok(None));
    let blocked = SecretString::from(BLOCKED.to_owned());
    assert_eq!(
        request_link(&mut db, line, &blocked, ts(DAY)).await,
        Ok(false)
    );
    assert!(notice_jobs(&mut db, line).await.is_empty());
}
