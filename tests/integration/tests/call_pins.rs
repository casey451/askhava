//! TD-CALL-13 (CALL-003, CALL-004, CALL-025; D-1684, D-1685, D-1755, D-2091 to D-2095): stored,
//! random PINs unique among a number's open lines, and the stored per-day PIN-try count, on
//! PostgreSQL. Fake numbers (555-01xx and the 500-555 test range), fake CallSids and IDs only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{
    Attestation, CallsError, Connected, IncomingCall, JobPin, PIN_ALERT, POOL_ALERT, VoiceCall,
    VoiceKeys, VoiceReply, assign_number, job_pin, stored_pin, voice_turn,
};
use askhava_core::api::Settings;
use askhava_privacy::api::Vault;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const NUMBER: &str = "+15005550100";
const STRANGER: &str = "+19285550163";
/// 10:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";
/// 00:00 Arizona the next day.
const NEXT_DAY: &str = "2026-11-03T07:00:00Z";

fn ts(at: &str) -> Timestamp {
    at.parse().expect("instant")
}

/// A pool of one fake number, so every line shares it.
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
    Vault::new(&key(0x71), &key(0x72), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-0000".to_owned())
}

async fn new_line(db: &mut PgConnection, pool: &MaskedPool) -> Result<Uuid, CallsError> {
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner: Uuid::new_v4(),
        business: Uuid::new_v4(),
    };
    Ok(assign_number(db, pool, job, ts(DAY)).await?.id())
}

async fn count(db: &mut PgConnection, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(db)
        .await
        .expect("count")
}

/// A PIN none of `pins` is.
fn other_than(pins: &[JobPin]) -> JobPin {
    (0..10_000_u16)
        .map(|n| format!("{n:04}"))
        .filter_map(|p| JobPin::parse(&p))
        .find(|p| !pins.contains(p))
        .expect("a free PIN")
}

/// One wrong-PIN turn from `caller` on call `sid` at `at`.
async fn wrong_try(
    db: &mut PgConnection,
    (pool, wrong): (&MaskedPool, JobPin),
    (sid, caller): (&str, &str),
    at: Timestamp,
) -> VoiceReply {
    let caller = SecretString::from(caller.to_owned());
    let keys = VoiceKeys {
        id_secret: &secret(),
        version: 1,
        pool: pool.numbers(),
    };
    let call = VoiceCall {
        call: IncomingCall {
            pool_number: NUMBER,
            caller: &caller,
        },
        call_sid: sid,
        attestation: Attestation::A,
        try_no: 1,
        entered: Some(wrong),
    };
    voice_turn(db, &vault(), &keys, &call, at)
        .await
        .expect("turn")
}

fn sid(n: u32) -> String {
    format!("CA{n:032}")
}

async fn tries(db: &mut PgConnection) -> Vec<i16> {
    sqlx::query_scalar("SELECT tries FROM calls.pin_try ORDER BY az_day")
        .fetch_all(db)
        .await
        .expect("tries")
}

/// TD-CALL-13 (CALL-003, CALL-004, D-1755): 500 lines on one pool number get stored PINs unique
/// among its open lines, and a known line ID does not give its PIN (no arithmetic relation).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_13_five_hundred_lines_get_unique_stored_pins(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = one_number_pool();
    let (mut pins, mut derived) = (Vec::new(), 0);
    for _ in 0..500 {
        let line = new_line(&mut db, &pool).await.expect("line");
        let pin = stored_pin(&mut db, line).await.expect("read").expect("pin");
        derived += usize::from(pin == job_pin(line));
        pins.push(pin.digits());
    }
    let mut unique = pins.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 500);
    assert!(
        derived < 5,
        "{derived} of 500 PINs equal the ID's last digits"
    );
}

/// TD-CALL-13 (CALL-003, CALL-017, D-1755): when every PIN of the number is held by an open line,
/// the bounded retries are used up, the pool alert is raised and no line is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_13_a_used_up_retry_raises_the_pool_alert(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    sqlx::query(
        "INSERT INTO calls.number_pool (job_id, homeowner_id, business_id, pool_number, shared, \
         pin, assigned_at) SELECT gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), $1, \
         true, lpad(n::text, 4, '0'), now() FROM generate_series(0, 9999) AS n",
    )
    .bind(NUMBER)
    .execute(&mut *db)
    .await
    .expect("every PIN held");
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = $1";
    let before = count(&mut db, alerts, POOL_ALERT).await;
    let refused = new_line(&mut db, &one_number_pool()).await;
    assert_eq!(refused, Err(CallsError::NoFreePin));
    assert_eq!(count(&mut db, alerts, POOL_ALERT).await, before + 1);
    let lines = "SELECT count(*) FROM calls.number_pool WHERE pool_number = $1";
    assert_eq!(count(&mut db, lines, NUMBER).await, 10_000);
}

/// TD-CALL-13 (CALL-025, D-1685): 12 wrong tries from one number on separate connections at once
/// never pass 6 that day and raise one alert; the same CallSid and digits again are not another
/// try; the count holds after a restart and starts again at Arizona midnight.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_13_twelve_wrong_tries_at_once_never_pass_six(db: PgPool) {
    let pool = one_number_pool();
    let mut conn = db.acquire().await.expect("connection");
    let line = new_line(&mut conn, &pool).await.expect("line");
    let pin = stored_pin(&mut conn, line)
        .await
        .expect("read")
        .expect("pin");
    let wrong = other_than(&[pin]);
    drop(conn);
    let runs = (0..12).map(|n| {
        let (db, pool) = (db.clone(), pool.clone());
        tokio::spawn(async move {
            let mut conn = db.acquire().await.expect("connection");
            let reply = wrong_try(&mut conn, (&pool, wrong), (&sid(n), STRANGER), ts(DAY)).await;
            matches!(reply, VoiceReply::Dial(_))
        })
    });
    let tasks: Vec<_> = runs.collect();
    for task in tasks {
        assert!(!task.await.expect("task"), "a wrong PIN connected");
    }
    let mut conn = db.acquire().await.expect("connection");
    assert_eq!(tries(&mut conn).await, [6]);
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind = $1";
    assert_eq!(count(&mut conn, alerts, PIN_ALERT).await, 1);
    drop(conn);
    let mut restarted = db.acquire().await.expect("a new connection");
    let late = wrong_try(
        &mut restarted,
        (&pool, wrong),
        (&sid(99), STRANGER),
        ts(DAY),
    )
    .await;
    assert!(matches!(late, VoiceReply::HangUp));
    let next = wrong_try(
        &mut restarted,
        (&pool, wrong),
        (&sid(98), STRANGER),
        ts(NEXT_DAY),
    )
    .await;
    assert!(matches!(next, VoiceReply::AskPin));
    assert_eq!(tries(&mut restarted).await, [6, 1]);
}

/// TD-CALL-13 (CALL-025, SEC-017): a repeated callback with the same CallSid and digits is not
/// another try; the right stored PIN connects nothing it should not and counts nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_13_the_same_callsid_and_digits_are_not_another_try(db: PgPool) {
    let mut db = db.acquire().await.expect("connection");
    let pool = one_number_pool();
    let line = new_line(&mut db, &pool).await.expect("line");
    let pin = stored_pin(&mut db, line).await.expect("read").expect("pin");
    let wrong = other_than(&[pin]);
    for _ in 0..3 {
        let reply = wrong_try(&mut db, (&pool, wrong), (&sid(1), STRANGER), ts(DAY)).await;
        assert!(matches!(reply, VoiceReply::AskPin));
    }
    assert_eq!(tries(&mut db).await, [1]);
    let calls = "SELECT count(*) FROM calls.pin_call WHERE call_sid = $1";
    assert_eq!(count(&mut db, calls, &sid(1)).await, 1);
    let digest = "SELECT count(*) FROM calls.pin_try WHERE encode(caller_digest, 'escape') LIKE $1";
    assert_eq!(count(&mut db, digest, "%5550163%").await, 0);
}
