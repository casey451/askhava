//! T652: which pushed alerts start safe mode (TD-OPS-46; OPS-001, OPS-004, OPS-023; D-075,
//! D-942, D-1719, D-4310).
//!
//! Safe mode starts on an open, pushed alert that is critical or High of one of
//! `ops::api::SAFE_MODE_KINDS`; the vault-read, call-back, delivery and email-limit alerts never
//! start it. The push record is T204's, so these tests insert it directly (D-3844). The timed
//! cases plant pushes in 2030, so the database-clock fallback stays off and only the check's own
//! clock decides; the never-start cases plant pushes in early 2026, so the fallback would see
//! them too. Fake data only: made-up kinds and IDs.

use askhava_messaging::api::CALL_BACK_ALERT;
use askhava_ops::api::{
    DELIVERY_ALERT, EMAIL_LIMIT_ALERT, FAILED_TEXT_CALL_BACK_ALERT, HEALTH_ALERT, LEDGER_ALERT,
    NUMBER_SUSPENDED_ALERT, NewAlert, PUSH_AT_ONCE, SAFE_MODE_KINDS, Severity, VENDOR_ALERT,
    check_safe_mode, raise_alert, safe_mode_on,
};
use askhava_payments::api::{MISMATCH_ALERT, charges_paused};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

const VAULT_SPIKE: &str = "privacy.vault_read_spike";
const VAULT_UNREGISTERED: &str = "privacy.vault_read_unregistered";

/// The kinds OPS-001 leaves out, each raised High by its own module (D-4310).
const NEVER: [&str; 5] = [
    VAULT_SPIKE,
    VAULT_UNREGISTERED,
    FAILED_TEXT_CALL_BACK_ALERT,
    DELIVERY_ALERT,
    EMAIL_LIMIT_ALERT,
];

/// `iso` plus `secs` seconds.
fn plus(iso: &str, secs: i64) -> Timestamp {
    let start: Timestamp = iso.parse().expect("instant");
    start + SignedDuration::from_secs(secs)
}

/// Raises a `kind` alert of `severity` and, when `pushed` is given, its push row (D-3844).
async fn plant(
    pool: &PgPool,
    kind: &'static str,
    severity: Severity,
    pushed: Option<&str>,
) -> Uuid {
    let mut conn = pool.acquire().await.expect("conn");
    let alert = NewAlert {
        kind,
        severity,
        subject: None,
    };
    let id = raise_alert(&mut conn, &alert).await.expect("alert");
    if let Some(at) = pushed {
        sqlx::query(
            "INSERT INTO ops.alert_push (alert_id, pushed_at) VALUES ($1, $2::timestamptz)",
        )
        .bind(id)
        .bind(at)
        .execute(pool)
        .await
        .expect("push");
    }
    id
}

/// Acknowledges alert `id` (fake acknowledgement by Casey).
async fn resolve(pool: &PgPool, id: Uuid) {
    sqlx::query("UPDATE ops.alert SET resolved_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("resolve");
}

async fn check(pool: &PgPool, at: Timestamp) -> bool {
    let mut conn = pool.acquire().await.expect("conn");
    check_safe_mode(&mut conn, at).await.expect("check")
}

/// `safe_mode_on` and `charges_paused`, read by the database's clock.
async fn reads(pool: &PgPool) -> (bool, bool) {
    let mut conn = pool.acquire().await.expect("conn");
    let on = safe_mode_on(&mut conn).await.expect("on");
    let paused = charges_paused(&mut conn).await.expect("paused");
    (on, paused)
}

/// TD-OPS-46 (OPS-001, D-4310): each `SAFE_MODE_KINDS` entry is its raiser's constant, and none
/// of the left-out kinds is in it.
#[test]
fn td_ops_46_the_kinds_are_the_raisers_constants() {
    assert_eq!(
        SAFE_MODE_KINDS,
        [
            VENDOR_ALERT,
            HEALTH_ALERT,
            LEDGER_ALERT,
            NUMBER_SUSPENDED_ALERT
        ]
    );
    assert_eq!(LEDGER_ALERT, MISMATCH_ALERT);
    assert_eq!(FAILED_TEXT_CALL_BACK_ALERT, CALL_BACK_ALERT);
    assert!(PUSH_AT_ONCE.contains(&VAULT_SPIKE));
    assert!(PUSH_AT_ONCE.contains(&VAULT_UNREGISTERED));
    assert!(NEVER.iter().all(|k| !SAFE_MODE_KINDS.contains(k)));
}

/// TD-OPS-46 (OPS-001, OPS-004, D-4310): High vault-read, call-back, delivery and email-limit
/// alerts pushed 25 hours before the check leave safe mode off, by either clock.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_46_left_out_high_alerts_never_start_it(pool: PgPool) {
    let pushed = "2026-01-01T00:00:00Z";
    for kind in NEVER {
        plant(&pool, kind, Severity::High, Some(pushed)).await;
    }
    assert!(!check(&pool, plus(pushed, 25 * 3_600)).await);
    assert_eq!(
        reads(&pool).await,
        (false, false),
        "nor by the database clock"
    );
}

/// TD-OPS-46 (OPS-001, D-075, D-1719): each listed kind, and a critical alert of another kind,
/// starts safe mode 24:00:00 after its push and not at 23:59:59; resolving it ends safe mode.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_46_each_listed_kind_and_a_critical_alert_start_it(pool: PgPool) {
    let cases = SAFE_MODE_KINDS
        .map(|k| (k, Severity::High))
        .into_iter()
        .chain([("ops.critical_fake_emergency", Severity::Critical)]);
    for (day, (kind, severity)) in (0_i64..).zip(cases) {
        let pushed = plus("2030-03-04T17:00:00Z", day * 3 * 86_400).to_string();
        let id = plant(&pool, kind, severity, Some(&pushed)).await;
        assert!(
            !check(&pool, plus(&pushed, 86_399)).await,
            "{kind} at 23:59:59"
        );
        assert!(
            check(&pool, plus(&pushed, 86_400)).await,
            "{kind} at 24:00:00"
        );
        resolve(&pool, id).await;
        assert!(
            !check(&pool, plus(&pushed, 86_460)).await,
            "{kind} resolved"
        );
    }
}

/// TD-OPS-46 (OPS-001, D-942): a listed alert that never pushed never starts safe mode.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_46_an_unpushed_listed_alert_never_starts_it(pool: PgPool) {
    plant(&pool, LEDGER_ALERT, Severity::High, None).await;
    assert!(!check(&pool, plus("2030-03-04T17:00:00Z", 10 * 86_400)).await);
    assert_eq!(reads(&pool).await, (false, false));
}

/// TD-OPS-46 (OPS-023, D-4310): with a listed alert and a vault spike both open, resolving the
/// listed one ends safe mode.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_46_an_open_vault_spike_does_not_hold_safe_mode(pool: PgPool) {
    let pushed = "2030-03-04T17:00:00Z";
    let ledger = plant(&pool, LEDGER_ALERT, Severity::High, Some(pushed)).await;
    plant(&pool, VAULT_SPIKE, Severity::High, Some(pushed)).await;
    assert!(check(&pool, plus(pushed, 86_400)).await);
    resolve(&pool, ledger).await;
    assert!(
        !check(&pool, plus(pushed, 86_460)).await,
        "the spike holds nothing"
    );
    assert_eq!(reads(&pool).await, (false, false));
}
