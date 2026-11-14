//! The global daily Verify cap under concurrent code sends (TD-PICK-56; PICK-055, D-480,
//! D-2335). Fake numbers and IDs only; Verify is a fake adapter.
//!
//! With the day's global count at the threshold less one, 40 sends to 40 different contacts at
//! once accept at most one, the once-a-day High alert is raised once, and no advisory lock is
//! held while Verify is called.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_picks::api::rules::{CodeVia, SendAsk, SendRefused};
use askhava_picks::api::{CodeSent, CodeTo, VERIFY_CAP_ALERT, send_code};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const PHONE: &str = "+19285550123";
const THRESHOLD: i64 = 3;
const RACERS: u128 = 40;

/// A Verify stand-in: counts its sends and, on each, records how many advisory locks any session
/// of this test's database holds at that moment.
struct FakeVerify {
    pool: PgPool,
    sends: AtomicU32,
    locks_seen: AtomicU32,
}

impl FakeVerify {
    fn new(pool: &PgPool) -> Self {
        Self {
            pool: pool.clone(),
            sends: AtomicU32::new(0),
            locks_seen: AtomicU32::new(0),
        }
    }
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let held: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND granted \
             AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
        )
        .fetch_one(&self.pool)
        .await
        .expect("locks");
        self.locks_seen
            .fetch_add(u32::try_from(held).expect("small"), Ordering::SeqCst);
        self.sends.fetch_add(1, Ordering::SeqCst);
        Ok("VEfake0000".to_owned())
    }

    async fn check_code(&self, _to: &str, _code: &str) -> Result<bool, AdapterError> {
        Ok(false)
    }
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

/// Noon Arizona time on a fixed day, plus `seconds`.
fn noon(seconds: i64) -> Timestamp {
    let t: Timestamp = "2026-11-03T12:00:00-07:00".parse().expect("time");
    t + SignedDuration::from_secs(seconds)
}

fn text() -> SendAsk {
    SendAsk {
        via: CodeVia::Text,
        own_session: false,
        local_hour: 12,
    }
}

/// One text code for job `job` to contact `contact`, under [`THRESHOLD`].
async fn send(pool: &PgPool, v: &FakeVerify, (job, contact): (u128, u128), at: i64) -> CodeSent {
    let phone = SecretString::from(PHONE.to_owned());
    let to = CodeTo {
        job: Uuid::from_u128(job),
        contact: Uuid::from_u128(contact),
        to: &phone,
    };
    let mut db = pool.acquire().await.expect("connection");
    send_code(&mut db, (v, &secret()), &to, text(), (noon(at), THRESHOLD))
        .await
        .expect("send")
}

/// Fills the day to the threshold less one, one send per contact.
async fn fill_to_one_below(pool: &PgPool, v: &FakeVerify) {
    for n in 0..u128::try_from(THRESHOLD - 1).expect("small") {
        let at = i64::try_from(n).expect("small");
        assert_eq!(send(pool, v, (1 + n, 100 + n), at).await, CodeSent::Sent);
    }
}

async fn high_alerts(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high'")
        .bind(VERIFY_CAP_ALERT)
        .fetch_one(pool)
        .await
        .expect("alerts")
}

/// TD-PICK-56 (PICK-055, D-2335): at the threshold less one, 40 concurrent sends to 40 different
/// contacts accept at most one; the rest are refused by the global cap; one High alert is raised.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_56_concurrent_sends_at_threshold_less_one_accept_at_most_one(pool: PgPool) {
    let v = Arc::new(FakeVerify::new(&pool));
    fill_to_one_below(&pool, &v).await;
    let mut racers = tokio::task::JoinSet::new();
    for n in 0..RACERS {
        let (pool, v) = (pool.clone(), Arc::clone(&v));
        racers.spawn(async move { send(&pool, &v, (500 + n, 900 + n), 60).await });
    }
    let (mut sent, mut capped) = (0, 0);
    while let Some(done) = racers.join_next().await {
        match done.expect("join") {
            CodeSent::Sent => sent += 1,
            CodeSent::Refused(SendRefused::GlobalCap) => capped += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(sent <= 1, "{sent} sends passed the global cap");
    assert_eq!(sent + capped, RACERS);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM picks.code_attempt")
        .fetch_one(&pool)
        .await
        .expect("attempts");
    assert!(attempts <= THRESHOLD);
    assert_eq!(
        v.sends.load(Ordering::SeqCst),
        u32::try_from(attempts).expect("small")
    );
    assert_eq!(high_alerts(&pool).await, 1);
}

/// TD-PICK-56 (PICK-055, D-2335): the contact's and the day's locks are released at the commit,
/// so no advisory lock is held while Verify is called.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_56_no_lock_is_held_across_the_verify_call(pool: PgPool) {
    let v = FakeVerify::new(&pool);
    fill_to_one_below(&pool, &v).await;
    assert_eq!(send(&pool, &v, (40, 400), 30).await, CodeSent::Sent);
    assert_eq!(v.sends.load(Ordering::SeqCst), 3);
    assert_eq!(v.locks_seen.load(Ordering::SeqCst), 0);
    assert_eq!(high_alerts(&pool).await, 0);
}
