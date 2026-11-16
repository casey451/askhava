//! T105: the delivery-rate and coverage alerts and the delivery alert's automatic acknowledgement
//! (TD-OPS-01, TD-OPS-06, TD-LIC-21; OPS-002, OPS-004, OPS-006, OPS-023, OPS-025).
//!
//! Delivery counts reach `ops` through `messaging`'s status callback, coverage counts through
//! `businesses`' status changes and edits, and request counts through `jobs`' hourly count. Fake
//! data only: made-up SIDs, codes and IDs. Safe mode and the pause switches are T487's.

use askhava_adapters::api::{Delivery, VerifiedWebhook, WebhookVendor, verify_webhook};
use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, apply, create, pause_self, set_job_types, set_towns,
};
use askhava_jobs::api::{record_job_facts, report_request_counts};
use askhava_messaging::api::receive_status;
use askhava_ops::api::{
    Actor, Audience, COVERAGE_ALERT, DELIVERY_ALERT, DeliveryCounts, DeliveryOutcome, NewAlert,
    Severity, raise_alert, record_delivery_counts, record_request_counts,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};
use std::collections::BTreeSet;

const URL: &str = "https://hooks.example.test/twilio";
const TOKEN: &str = "fake_twilio_auth_token";
const TOWN: &str = "fake_town";
const TRADE: &str = "plumbing_repair";

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-10-30T16:00:00Z".parse().expect("instant");
    start + SignedDuration::from_mins(minute)
}

fn counts(failed_of_50: u32, failed_of_200: u32) -> DeliveryCounts {
    DeliveryCounts::new(Audience::Homeowner, failed_of_50, failed_of_200).expect("counts")
}

async fn report(pool: &PgPool, c: DeliveryCounts, minute: i64) -> DeliveryOutcome {
    let mut db = pool.acquire().await.expect("conn");
    record_delivery_counts(&mut db, c, at(minute))
        .await
        .expect("report")
}

async fn scalar(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// TD-OPS-01 (OPS-004, D-134, D-163): 6 failures in the last 50 (88%) alert, 5 (90%) don't; 11
/// in the last 200 (94.5%) alert, 10 (95%) don't; 3 in 50 at a normal rate never alert.
#[test]
fn td_ops_01_delivery_rate_lines() {
    assert!(counts(6, 6).low());
    assert!(!counts(5, 5).low());
    assert!(counts(0, 11).low());
    assert!(!counts(0, 10).low());
    assert!(!counts(3, 3).low());
    assert!(!counts(3, 10).low());
    assert_eq!(DELIVERY_ALERT, "ops.delivery_low");
    assert!(counts(0, 6).recovered());
    assert!(!counts(0, 7).recovered());
    assert!(!counts(6, 6).recovered());
    assert!(DeliveryCounts::new(Audience::Business, 51, 51).is_none());
    assert!(DeliveryCounts::new(Audience::Business, 6, 5).is_none());
}

/// TD-OPS-06 (OPS-023, D-163): a delivery alert stays open at 96% of 200, is acknowledged
/// automatically at 97% (logged and listed for Casey), and a later drop alerts again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_06_recovery_to_97_acknowledges(pool: PgPool) {
    let DeliveryOutcome::Raised(alert) = report(&pool, counts(6, 6), 0).await else {
        panic!("no alert");
    };
    let severity: String = sqlx::query_scalar("SELECT severity FROM ops.alert WHERE id = $1")
        .bind(alert)
        .fetch_one(&pool)
        .await
        .expect("severity");
    assert_eq!(severity, "high");
    assert_eq!(
        report(&pool, counts(0, 8), 1).await,
        DeliveryOutcome::Nothing
    );
    let done = report(&pool, counts(0, 6), 2).await;
    assert_eq!(done, DeliveryOutcome::Acknowledged(alert));
    let by = "SELECT count(*) FROM ops.alert_ack WHERE acknowledged_by = 'system'";
    assert_eq!(scalar(&pool, by).await, 1);
    let logged = "SELECT count(*) FROM ops.audit_log WHERE action = 'ops.alert_acknowledged'";
    assert_eq!(scalar(&pool, logged).await, 1);
    assert_eq!(
        report(&pool, counts(0, 6), 3).await,
        DeliveryOutcome::Nothing
    );
    assert!(matches!(
        report(&pool, counts(0, 11), 4).await,
        DeliveryOutcome::Raised(_)
    ));
}

/// TD-OPS-06, TD-OPS-01 (OPS-025, D-163): at most one delivery alert is open; after Casey
/// acknowledges one, a second drop fires nothing until the line was back at 97% of 200.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_06_a_second_drop_waits_for_97_in_between(pool: PgPool) {
    let DeliveryOutcome::Raised(alert) = report(&pool, counts(6, 6), 0).await else {
        panic!("no alert");
    };
    let other = DeliveryCounts::new(Audience::Business, 9, 9).expect("counts");
    assert_eq!(report(&pool, other, 1).await, DeliveryOutcome::Nothing);
    assert_eq!(
        report(&pool, counts(7, 12), 2).await,
        DeliveryOutcome::Nothing
    );
    let open = "SELECT count(*) FROM ops.alert WHERE kind = 'ops.delivery_low'";
    assert_eq!(scalar(&pool, open).await, 1);
    let mut db = pool.acquire().await.expect("conn");
    let ack = "UPDATE ops.alert SET resolved_at = '2026-10-30T16:03:00Z' WHERE id = $1";
    sqlx::query(ack)
        .bind(alert)
        .execute(&mut *db)
        .await
        .expect("casey");
    assert_eq!(report(&pool, counts(6, 6), 4).await, DeliveryOutcome::Held);
    assert_eq!(
        report(&pool, counts(0, 7), 5).await,
        DeliveryOutcome::Nothing
    );
    assert_eq!(report(&pool, counts(0, 11), 5).await, DeliveryOutcome::Held);
    assert_eq!(
        report(&pool, counts(0, 6), 6).await,
        DeliveryOutcome::Nothing
    );
    assert!(matches!(
        report(&pool, counts(6, 6), 7).await,
        DeliveryOutcome::Raised(_)
    ));
}

/// TD-OPS-06 (OPS-023, D-134): a ledger alert is never acknowledged by delivery recovery or
/// elapsed time; its clean-nightly-check acknowledgement is TD-OPS-08's (D-220).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_06_a_ledger_alert_is_never_acknowledged_here(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let ledger = NewAlert {
        kind: "payments.ledger_difference",
        severity: Severity::High,
        subject: None,
    };
    let id = raise_alert(&mut db, &ledger).await.expect("ledger");
    report(&pool, counts(6, 6), 0).await;
    report(&pool, counts(0, 0), 60 * 24 * 3).await;
    let open: bool = sqlx::query_scalar("SELECT resolved_at IS NULL FROM ops.alert WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("ledger row");
    assert!(open);
}

/// A Twilio status callback for `sid`, signed with the fake token and verified.
fn status(sid: &str, value: &str) -> VerifiedWebhook {
    let fields = [("MessageSid", sid), ("MessageStatus", value)];
    let body: Vec<String> = fields.iter().map(|(n, v)| format!("{n}={v}")).collect();
    let body = body.join("&");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(URL.as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let sig = STANDARD.encode(mac.finalize().into_bytes());
    let headers = [("X-Twilio-Signature", sig.as_str())];
    let delivery = Delivery {
        url: URL,
        body: body.as_bytes(),
        headers: &headers,
    };
    let token = SecretString::from(TOKEN);
    verify_webhook(WebhookVendor::Twilio, &token, delivery, 0).expect("signed")
}

/// Sends fake homeowner texts `range` through the status callback, text `n` failing when
/// `fails(n)`; returns the open delivery alerts after each callback.
async fn texts(
    pool: &PgPool,
    range: std::ops::Range<u32>,
    fails: impl Fn(u32) -> bool,
) -> Vec<i64> {
    let mut db = pool.acquire().await.expect("conn");
    let mut open = Vec::new();
    for n in range {
        let sid = format!("SM{n:032}");
        sqlx::query(
            "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, \
             recipient_kind, subject_ids, status, provider_sid, sent_at) \
             VALUES (lpad(to_hex($1), 64, '0'), 'fake.text', gen_random_uuid(), 'homeowner', \
             '{}', 'sent', $2, now())",
        )
        .bind(i64::from(n))
        .bind(&sid)
        .execute(&mut *db)
        .await
        .expect("outbox");
        let value = if fails(n) { "undelivered" } else { "delivered" };
        receive_status(&mut db, &status(&sid, value), at(i64::from(n)))
            .await
            .expect("callback");
        let sql = "SELECT count(*) FROM ops.alert WHERE kind = 'ops.delivery_low' \
                   AND resolved_at IS NULL";
        open.push(scalar(pool, sql).await);
    }
    open
}

/// TD-OPS-01 (OPS-004, D-163): through `messaging`'s callback, 3 failures in 50 at a normal
/// 1.5% rate never alert, and a sixth failure in the last 50 does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_01_callbacks_alert_on_the_sixth_failure_in_50(pool: PgPool) {
    let normal = texts(&pool, 0..200, |n| n % 67 == 5).await;
    assert!(normal.iter().all(|open| *open == 0));
    let more = texts(&pool, 200..206, |_| true).await;
    assert_eq!(more, [0, 0, 0, 0, 0, 1]);
}

/// A business made active at `minute`, serving [`TOWN`] and the job types `trades` name.
async fn active(db: &mut PgConnection, trades: &[&str], minute: i64) -> Uuid {
    let b = create(db, false, at(minute)).await.expect("business");
    set_towns(db, b, &[TOWN], Actor::System)
        .await
        .expect("towns");
    let ticked: Vec<JobTypeChoice> = trades
        .iter()
        .map(|t| JobTypeChoice::new(t, "Fake job type"))
        .collect();
    let allowed: BTreeSet<JobTypeChoice> = ticked.iter().cloned().collect();
    set_job_types(db, b, &ticked, &allowed, Actor::System)
        .await
        .expect("job types");
    let done = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let to = apply(db, b, Event::Checked(done), Actor::System, at(minute)).await;
    assert_eq!(to.expect("activate").code(), "active");
    b
}

async fn coverage_alerts(pool: &PgPool, scope: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert a JOIN ops.coverage c ON c.id = a.subject_id \
         WHERE a.kind = $1 AND c.scope = $2 AND a.resolved_at IS NULL",
    )
    .bind(COVERAGE_ALERT)
    .bind(scope)
    .fetch_one(pool)
    .await
    .expect("alerts")
}

/// TD-OPS-01 (OPS-006, D-459, D-482): a town under 2 active businesses alerts; a trade under 2
/// with no request this month is stored for the digest only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_01_a_town_under_2_alerts_and_a_quiet_trade_waits(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    active(&mut db, &[TRADE], 0).await;
    assert_eq!(coverage_alerts(&pool, "town").await, 1);
    assert_eq!(coverage_alerts(&pool, "trade").await, 0);
    let stored = "SELECT active::bigint FROM ops.coverage WHERE scope = 'trade'";
    assert_eq!(scalar(&pool, stored).await, 1);
    let second = active(&mut db, &[TRADE], 1).await;
    let town = "SELECT active::bigint FROM ops.coverage WHERE scope = 'town'";
    assert_eq!(scalar(&pool, town).await, 2);
    pause_self(&mut db, second, at(2)).await.expect("pause");
    assert_eq!(scalar(&pool, town).await, 1);
    assert_eq!(coverage_alerts(&pool, "town").await, 1);
    assert_eq!(coverage_alerts(&pool, "trade").await, 0);
}

/// TD-LIC-21 and TD-OPS-01 (OPS-006, LIC-046, D-459): a launch-open trade with 1 active business
/// and a request this month alerts, whether the request or the count is reported first.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_21_a_requested_trade_with_1_business_alerts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    active(&mut db, &[TRADE], 0).await;
    let job = askhava_jobs::api::create(&mut db, at(1), false, Actor::System)
        .await
        .expect("job");
    record_job_facts(&mut db, job, TRADE, &["Fake job type".to_owned()])
        .await
        .expect("facts");
    report_request_counts(&mut db, at(2)).await.expect("count");
    assert_eq!(coverage_alerts(&pool, "trade").await, 1);
    let later = [("water_heater".to_owned(), 1)];
    let raised = record_request_counts(&mut db, &later, at(3))
        .await
        .expect("requests");
    assert!(raised.is_empty());
    active(&mut db, &["water_heater"], 4).await;
    assert_eq!(coverage_alerts(&pool, "trade").await, 2);
}

/// OPS-006: one transaction that reports twice (job types, then towns, as `save_capability`
/// does) and a pause of another business in the same town and trade both finish: one coverage
/// lock, so no lock-order deadlock, and the pause counts after the save commits.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn two_reports_in_one_transaction_never_deadlock(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let first = active(&mut db, &[TRADE], 0).await;
    let second = active(&mut db, &[TRADE], 1).await;
    let mut tx = db.begin().await.expect("tx");
    let ticked = [JobTypeChoice::new(TRADE, "Other fake job type")];
    let allowed: BTreeSet<JobTypeChoice> = ticked.iter().cloned().collect();
    set_job_types(&mut tx, first, &ticked, &allowed, Actor::System)
        .await
        .expect("job types");
    let other = pool.clone();
    let pause = tokio::spawn(async move {
        let mut conn = other.acquire().await.expect("conn");
        pause_self(&mut conn, second, at(2)).await.expect("pause");
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    set_towns(&mut tx, first, &[TOWN, "other_town"], Actor::System)
        .await
        .expect("towns");
    tx.commit().await.expect("commit");
    tokio::time::timeout(std::time::Duration::from_secs(10), pause)
        .await
        .expect("no deadlock")
        .expect("pause task");
    let town =
        "SELECT active::bigint FROM ops.coverage WHERE scope = 'town' AND code = 'fake_town'";
    assert_eq!(scalar(&pool, town).await, 1);
}

/// Postgres brain P5 (OPS-023): the acknowledgement and push facts are append-only for the app
/// role, and safe mode stays one row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_app_role_cannot_rewrite_acknowledgements_or_add_a_safe_mode_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let ok = "UPDATE ops.safe_mode SET \"on\" = false, since = NULL";
    sqlx::query(ok).execute(&mut *db).await.expect("update");
    for sql in [
        "UPDATE ops.alert_ack SET at = now()",
        "DELETE FROM ops.alert_push",
        "INSERT INTO ops.safe_mode (id) VALUES (true)",
    ] {
        assert!(sqlx::query(sql).execute(&mut *db).await.is_err(), "{sql}");
    }
}
