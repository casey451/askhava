//! D-4052 B23 (CALL-003, CALL-017, D-231, D-1755): a full number pool raises one open
//! `calls.pool_exhausted` alert however many calls it refuses, so a busy evening leaves Casey one
//! alert to acknowledge, not a pile. On PostgreSQL; fake numbers (the 500-555 test range) and
//! fake IDs only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{CallsError, Connected, POOL_ALERT, assign_number};
use askhava_core::api::Settings;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const NUMBER: &str = "+15005550100";

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

/// A new job asking for a line at 10:00 Arizona.
async fn refused_call(db: &mut PgConnection, pool: &MaskedPool) -> Result<Uuid, CallsError> {
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner: Uuid::new_v4(),
        business: Uuid::new_v4(),
    };
    let at: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    Ok(assign_number(db, pool, job, at).await?.id())
}

async fn open_pool_alerts(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL")
        .bind(POOL_ALERT)
        .fetch_one(db)
        .await
        .expect("count")
}

/// D-4052 B23, CALL-017, D-231: with every PIN of the only number held, two refused calls each
/// get `NoFreePin` and leave exactly one open pool alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn two_refused_calls_on_a_full_pool_raise_one_open_alert(db: PgPool) {
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
    let pool = one_number_pool();
    assert_eq!(open_pool_alerts(&mut db).await, 0);
    for _ in 0..2 {
        let refused = refused_call(&mut db, &pool).await;
        assert_eq!(refused, Err(CallsError::NoFreePin));
    }
    assert_eq!(open_pool_alerts(&mut db).await, 1);
}
