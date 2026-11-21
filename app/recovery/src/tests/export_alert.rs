//! OPS-013, PRIV-029, D-5081: a failed or timed-out nightly export raises one High
//! `recovery.export_failed` alert per Arizona day, about no record (so it names no path, key or
//! size) and never a safe-mode kind; a stored export raises none. Fake data only; the fake
//! `pg_dump` is written through a waited-on child shell, never in-process (D-4864).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use askhava_adapters::api::LocalStorage;
use askhava_core::api::{Clock, HandlerError, ManualClock, SystemClock};
use askhava_ops::api::{DigestAlert, SAFE_MODE_KINDS, alert_subject, digest_alerts};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;

use crate::backup::{EXPORT_DEADLINE, EXPORT_FAILED_ALERT, run_export_on};
use crate::models::ExportSettings;

/// A fresh scratch directory for this test process.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("t781-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes an executable script through a child shell this process waits on (D-4864).
fn write_script(path: &Path, text: &str) {
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", r#"printf %s "$2" > "$1" && chmod 755 "$1""#, "sh"])
        .arg(path)
        .arg(text)
        .status()
        .unwrap();
    assert!(status.success(), "writing {} failed", path.display());
}

/// A search path holding only a fake PostgreSQL 16 `pg_dump` whose dump runs `body`.
fn fake_dump(name: &str, body: &str) -> OsString {
    let dir = scratch(name);
    let version = "if [ \"$1\" = --version ]; then echo 'pg_dump (PostgreSQL) 16.4'; exit 0; fi";
    write_script(
        &dir.join("pg_dump"),
        &format!("#!/bin/sh\n{version}\n{body}\n"),
    );
    dir.into_os_string()
}

/// One nightly run (task `task`) at `clock`'s time with the fake dump on `search`, stored under
/// a scratch folder.
async fn run(
    pool: &PgPool,
    (search, clock): (&OsString, &dyn Clock),
    (task, deadline): (i64, Duration),
) -> Result<(), HandlerError> {
    let folder = Path::new(search).join("store");
    std::fs::create_dir_all(&folder).unwrap();
    let store = LocalStorage::new(&format!("file://{}", folder.display())).unwrap();
    let export = Ok(ExportSettings {
        recipient: age::x25519::Identity::generate().to_public(),
        backup_url: SecretString::from("postgres://fake_user:fake-pw@localhost/fake_db"),
    });
    let secret = SecretString::from("fake-id-secret-for-tests-only-000000");
    let parts = (&store, &secret, clock);
    run_export_on(pool, parts, &export, (task, deadline), search.clone()).await
}

/// Every `recovery.export_failed` alert raised in the last two days.
async fn alerts(pool: &PgPool) -> Vec<DigestAlert> {
    let mut conn = pool.acquire().await.unwrap();
    let now = Timestamp::now();
    let (from, to) = (
        now - SignedDuration::from_hours(48),
        now + SignedDuration::from_hours(1),
    );
    digest_alerts(&mut conn, &[EXPORT_FAILED_ALERT], from, to)
        .await
        .unwrap()
}

/// The `failure` of every export row, oldest first.
async fn export_failures(pool: &PgPool) -> Vec<Option<String>> {
    sqlx::query_scalar!(
        "SELECT failure FROM recovery.backup_check WHERE kind = 'export' ORDER BY started_at, id"
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_dump_past_its_deadline_is_one_timeout_row_and_one_high_alert(pool: PgPool) {
    let slow = fake_dump("slow", "exec sleep 30");
    let late = run(&pool, (&slow, &SystemClock), (1, Duration::from_secs(1))).await;
    assert_eq!(late, Err(HandlerError("timeout")));
    assert_eq!(export_failures(&pool).await, [Some("timeout".to_owned())]);
    let raised = alerts(&pool).await;
    assert_eq!(raised.len(), 1, "one alert");
    assert_eq!(
        raised[0].subject, None,
        "it names no record, path, key or size"
    );
    let mut conn = pool.acquire().await.unwrap();
    let alert = alert_subject(&mut conn, raised[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(alert.severity, "high");
    assert!(!SAFE_MODE_KINDS.contains(&EXPORT_FAILED_ALERT));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_second_failure_the_same_day_is_quiet_and_the_next_day_alerts_again(pool: PgPool) {
    let broken = fake_dump("broken", "exit 1");
    let clock = ManualClock::new("2026-03-02T10:00:00Z".parse::<Timestamp>().unwrap());
    let night = |task| run(&pool, (&broken, &clock), (task, EXPORT_DEADLINE));
    assert!(night(1).await.is_err(), "3 a.m. in Arizona fails");
    clock.advance(SignedDuration::from_hours(20));
    assert!(
        night(2).await.is_err(),
        "11 p.m. the same Arizona day fails"
    );
    assert_eq!(export_failures(&pool).await.len(), 2);
    let raised = alerts(&pool).await;
    assert_eq!(raised.len(), 1, "one alert the same Arizona day");
    clock.advance(SignedDuration::from_hours(1));
    assert!(
        night(3).await.is_err(),
        "midnight, the next Arizona day, fails"
    );
    assert_eq!(
        alerts(&pool).await.len(),
        2,
        "the next day's failure alerts once"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_stored_export_raises_no_alert(pool: PgPool) {
    let good = fake_dump("good", "printf PGDMP-fake");
    run(&pool, (&good, &SystemClock), (1, EXPORT_DEADLINE))
        .await
        .unwrap();
    assert_eq!(export_failures(&pool).await, [None]);
    assert!(
        alerts(&pool).await.is_empty(),
        "a good export raises nothing"
    );
}
