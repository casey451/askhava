//! The hourly vault-read check and the daily database-volume check through the worker
//! (TD-SEC-26, TD-OPS-09; SEC-047, SEC-048, OPS-052; T525, D-3880, D-4044, D-4280).
//!
//! TD-SEC-26 (SEC-047): a planted burst at 4x the 14-day baseline raises one pushed High alert when
//! the worker runs `server::watch_checks`' hourly task for its hour, even when that run starts late,
//! past the next hour's end (D-4435). The unregistered-purpose case is `tests/integration/tests/vault.rs`'s (it alerts at
//! the read), and SEC-048's 50 failed sign-ins are `tests/security/tests/cookies_csrf.rs`'s, which
//! run against `portals`' security events unchanged.
//!
//! TD-OPS-09 (OPS-052): `ops::api::volume_level` at 69, 70, 84 and 85% of the volume; one case
//! through the worker (a database at about 90% raises one High `ops.volume_high`, listed in the next
//! 3 p.m. push, starting no safe mode); a Low `ops.volume_low` stored `normal`, once while it lasts;
//! dev raises nothing; test without `ASKHAVA_DB_VOLUME_BYTES` refuses to register. Fake data only.

use std::sync::Arc;

use askhava_core::api::{Clock, Lease, ManualClock, Outcome, Registry, Settings, WorkerId, claim};
use askhava_ops::api::{
    VOLUME_HIGH_ALERT, VOLUME_LOW_ALERT, VolumeLevel, check_safe_mode, check_volume,
    daytime_push_due, push_slot, safe_mode_on, volume_level,
};
use askhava_privacy::api::BASELINE_HOURS;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;

/// The two checks, compiled in place (the integration crate does not depend on `server`).
#[path = r"../../../app/server/src/watch_checks.rs"]
mod watch_checks;

/// Monday, November 2, 2026, 10:00 in Arizona.
const START: &str = "2026-11-02T17:00:00Z";
/// A fake reader-purpose count name.
const ACTION: &str = "vault_read.calls.bridge_call";
/// A fake test-environment allow-list.
const TEST_LIST: &str =
    "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
/// The next UTC midnight after [`START`], when the next volume check is due.
const TOMORROW: &str = "2026-11-03T00:00:00Z";

fn at(text: &str) -> Timestamp {
    text.parse().expect("time")
}

/// A registry running only the two checks on a manual clock at [`START`].
fn setup(volume: Option<u64>) -> (Arc<ManualClock>, Registry) {
    let clock = Arc::new(ManualClock::new(at(START)));
    let mut registry = Registry::new();
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    watch_checks::register_checks(&mut registry, volume, &dyn_clock);
    (clock, registry)
}

/// Claims and runs every task due on `clock`; each must finish.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, clock.now(), Lease::WORKER.length, 50)
        .await
        .expect("claim");
    assert!(!tasks.is_empty(), "a check was due");
    for task in tasks {
        let outcome = registry.run(pool, clock, owner, task, Lease::WORKER).await;
        assert!(matches!(outcome, Ok(Outcome::Done)), "{outcome:?}");
    }
}

/// Alerts of `kind` with `severity`.
async fn alerts(pool: &PgPool, kind: &str, severity: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = $2")
        .bind(kind)
        .bind(severity)
        .fetch_one(pool)
        .await
        .expect("alerts")
}

/// Queued runs of `kind` due at `run_at`.
async fn queued(pool: &PgPool, kind: &str, run_at: Timestamp) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1 AND run_at = $2")
        .bind(kind)
        .bind(run_at.to_sqlx())
        .fetch_one(pool)
        .await
        .expect("queued")
}

/// This database's size in bytes.
async fn size(pool: &PgPool) -> u64 {
    let size: i64 = sqlx::query_scalar("SELECT pg_database_size(current_database())")
        .fetch_one(pool)
        .await
        .expect("size");
    u64::try_from(size).expect("size")
}

/// Settings for `env` with `extra` pairs; fake values only.
fn settings(env: &str, extra: &[(&str, &str)]) -> Settings {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", env),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
    ];
    pairs.extend_from_slice(extra);
    Settings::from_pairs(pairs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())))
        .expect("settings")
}

/// TD-OPS-09 (OPS-052): 69% raises nothing, 70% Low, 84% Low, 85% High, on a small and a
/// realistic volume; a byte under 70% is still nothing.
#[test]
fn td_ops_09_the_volume_boundary_is_70_and_85_percent() {
    for volume in [100_u64, 50 * 1024 * 1024 * 1024] {
        let pct = |p: u64| volume / 100 * p;
        assert_eq!(volume_level(pct(69), volume), VolumeLevel::Fine);
        assert_eq!(volume_level(pct(70), volume), VolumeLevel::Low);
        assert_eq!(volume_level(pct(84), volume), VolumeLevel::Low);
        assert_eq!(volume_level(pct(85), volume), VolumeLevel::High);
        assert_eq!(volume_level(pct(70) - 1, volume), VolumeLevel::Fine);
    }
    assert_eq!(volume_level(1, 0), VolumeLevel::High);
}

/// TD-OPS-09 (OPS-052): through the worker, a database at about 90% of its volume raises one High
/// `ops.volume_high` (no Low), queues tomorrow's run, is listed in the 3 p.m. daytime push and
/// starts no safe mode, even unacknowledged a day later.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_a_volume_at_90_percent_raises_one_high_alert_through_the_worker(pool: PgPool) {
    let volume = size(&pool).await * 10 / 9;
    let (clock, registry) = setup(Some(volume));
    let mut conn = pool.acquire().await.expect("conn");
    watch_checks::schedule_volume_check(&mut conn, at(START))
        .await
        .expect("seed");
    watch_checks::schedule_volume_check(&mut conn, at(START))
        .await
        .expect("seed again");
    run_due(&pool, &registry, &clock).await;
    assert_eq!(alerts(&pool, VOLUME_HIGH_ALERT, "high").await, 1);
    assert_eq!(alerts(&pool, VOLUME_LOW_ALERT, "normal").await, 0);
    let tomorrow = queued(&pool, watch_checks::VOLUME_CHECK_KIND, at(TOMORROW));
    assert_eq!(tomorrow.await, 1);
    let raised: jiff_sqlx::Timestamp =
        sqlx::query_scalar("SELECT raised_at FROM ops.alert WHERE kind = $1")
            .bind(VOLUME_HIGH_ALERT)
            .fetch_one(&pool)
            .await
            .expect("raised");
    let raised = raised.to_jiff();
    // The latest 9 a.m. or 3 p.m. slot 18 hours on is the first slot at or after the raise.
    let slot = push_slot(raised + SignedDuration::from_hours(18));
    let push = daytime_push_due(&mut conn, slot + SignedDuration::from_mins(5))
        .await
        .expect("push")
        .expect("the next daytime push is due");
    assert!(push.alerts.iter().any(|a| a.kind == VOLUME_HIGH_ALERT));
    let later = raised + SignedDuration::from_hours(30);
    assert!(!check_safe_mode(&mut conn, later).await.expect("check"));
    assert!(!safe_mode_on(&mut conn).await.expect("on"));
}

/// TD-OPS-09 (OPS-052): at about 75% the check raises one Low alert stored `normal`, and a second
/// day at the same level leaves that one open alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_a_volume_at_75_percent_raises_one_low_alert(pool: PgPool) {
    let volume = size(&pool).await * 4 / 3;
    let mut conn = pool.acquire().await.expect("conn");
    for day in ["first", "second"] {
        let level = check_volume(&mut conn, volume).await.expect(day);
        assert_eq!(level, VolumeLevel::Low);
    }
    assert_eq!(alerts(&pool, VOLUME_LOW_ALERT, "normal").await, 1);
    assert_eq!(alerts(&pool, VOLUME_HIGH_ALERT, "high").await, 0);
}

/// OPS-052, D-4280: in dev the daily check registers with no volume and raises nothing (it still
/// queues tomorrow's run); in test a missing `ASKHAVA_DB_VOLUME_BYTES` refuses to register, naming
/// the key; in test with it, both checks register.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_052_dev_raises_nothing_and_test_needs_the_volume(pool: PgPool) {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(at(START)));
    let mut registry = Registry::new();
    let missing = watch_checks::register(&mut registry, &settings("test", &[]), &clock);
    let error = missing.expect_err("missing volume in test");
    assert!(error.to_string().contains("ASKHAVA_DB_VOLUME_BYTES"));
    let with = settings("test", &[("ASKHAVA_DB_VOLUME_BYTES", "1000")]);
    watch_checks::register(&mut registry, &with, &clock).expect("test with volume");
    let mut dev = Registry::new();
    let tiny = settings("dev", &[("ASKHAVA_DB_VOLUME_BYTES", "1")]);
    watch_checks::register(&mut dev, &tiny, &clock).expect("dev");
    let manual = ManualClock::new(at(START));
    let mut conn = pool.acquire().await.expect("conn");
    watch_checks::schedule_volume_check(&mut conn, at(START))
        .await
        .expect("seed");
    run_due(&pool, &dev, &manual).await;
    assert_eq!(alerts(&pool, VOLUME_HIGH_ALERT, "high").await, 0);
    let tomorrow = queued(&pool, watch_checks::VOLUME_CHECK_KIND, at(TOMORROW));
    assert_eq!(tomorrow.await, 1);
}

/// Plants `reads` of [`ACTION`] in `hour` and `per_hour` in each of the 336 hours before it.
async fn plant(pool: &PgPool, hour: Timestamp, per_hour: i32, reads: i32) {
    sqlx::query(
        "INSERT INTO privacy.read_count (hour, action, reads) SELECT $1 - make_interval(hours => h), $2, $3 \
         FROM generate_series(1, $4) AS h",
    )
    .bind(hour.to_sqlx())
    .bind(ACTION)
    .bind(per_hour)
    .bind(BASELINE_HOURS as i32)
    .execute(pool)
    .await
    .expect("baseline");
    sqlx::query("INSERT INTO privacy.read_count (hour, action, reads) VALUES ($1, $2, $3)")
        .bind(hour.to_sqlx())
        .bind(ACTION)
        .bind(reads)
        .execute(pool)
        .await
        .expect("burst");
}

/// TD-SEC-26 (SEC-047, D-4435): the 10:05 seed checks the 9:00 hour; 8 reads at 10:00 against a
/// baseline of 2 an hour (4x) raise one pushed High alert when the run due at 11:00 starts late at
/// 12:00:05, crossing an hour boundary; the 11:00 hour's run follows at once, and a repeat of the
/// 10:00 hour raises nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_26_a_4x_read_burst_alerts_once_even_when_the_hourly_check_runs_late(pool: PgPool) {
    let (clock, registry) = setup(None);
    plant(&pool, at(START), 2, 8).await;
    let mut conn = pool.acquire().await.expect("conn");
    let hour = |m: i64| at(START) + SignedDuration::from_mins(m);
    watch_checks::schedule_read_check(&mut conn, hour(5))
        .await
        .expect("seed");
    clock.set(hour(5));
    run_due(&pool, &registry, &clock).await;
    assert_eq!(alerts(&pool, "privacy.vault_read_spike", "high").await, 0);
    assert_eq!(
        queued(&pool, watch_checks::READ_CHECK_KIND, hour(60)).await,
        1
    );
    clock.set(hour(120) + SignedDuration::from_secs(5));
    run_due(&pool, &registry, &clock).await;
    assert_eq!(alerts(&pool, "privacy.vault_read_spike", "high").await, 1);
    run_due(&pool, &registry, &clock).await;
    assert_eq!(
        queued(&pool, watch_checks::READ_CHECK_KIND, hour(180)).await,
        1
    );
    let ten = at(START).as_second() / 3600;
    watch_checks::run_read_check(&pool, ten)
        .await
        .expect("again");
    assert_eq!(alerts(&pool, "privacy.vault_read_spike", "high").await, 1);
}
