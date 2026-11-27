//! The Stripe-or-webhook-errors alert (T518, D-3877, D-4280, D-5670, D-5876; TD-OPS-44's Stripe
//! and webhook cases; OPS-004, OPS-023, D-220).
//!
//! The check's cases plant the rows its window reads, at fake times: `server.webhook_error` and
//! `server.webhook_ok` audit rows, and `payments.charge_attempt` rows (`unreachable` for a failed
//! Stripe call, `succeeded` with `decided_at` for a successful one). Each run takes the open
//! `ops.vendor_errors_check` task's payload from the queue, marks it done and runs it as the
//! handler does (`vendor_alerts::run_payload`), so the state the chain carries is what decides.
//! The layer's cases send requests through `vendor_alerts::layer` on fake `/webhooks` handlers
//! (the same layer `build_router` puts over its five webhook routers) and wait for the spawned
//! rows. All data is fake.

use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{Clock, Environment, ManualClock, SystemClock};
use askhava_ops::api::VENDOR_ALERT;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tokio::sync::Notify;
use tower::ServiceExt;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the vendor errors check and its layer are called here"
)]
mod server;
use server::*;

use request_timeout::{RequestTimeout, limit_request};
use vendor_alerts::{
    CHECK_KIND, WEBHOOK_ERROR_ACTION, WEBHOOK_OK_ACTION, layer, run_payload, schedule,
};

/// A 5-minute slot start (fake time).
const START: &str = "2026-11-02T17:00:00Z";
/// The text webhook's route code name.
const SMS: &str = "webhooks.twilio_sms";
/// The Stripe webhook's route code name.
const STRIPE_HOOK: &str = "webhooks.stripe";

fn start() -> Timestamp {
    START.parse().expect("time")
}

fn mins(m: i64) -> Timestamp {
    start() + SignedDuration::from_mins(m)
}

/// Seeds the check's chain at `at`.
async fn seed(pool: &PgPool, at: Timestamp) {
    let mut conn = pool.acquire().await.expect("conn");
    schedule(&mut conn, at).await.expect("seed");
}

/// Runs the open check at `now` in the test environment.
async fn run_at(pool: &PgPool, now: Timestamp) {
    let (id, payload): (i64, Value) = sqlx::query_as(
        "SELECT id, payload FROM core.task WHERE kind = $1 AND done_at IS NULL ORDER BY run_at LIMIT 1",
    )
    .bind(CHECK_KIND)
    .fetch_one(pool)
    .await
    .expect("a queued run");
    sqlx::query("UPDATE core.task SET done_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("done");
    run_payload(pool, Environment::Test, &payload, now)
        .await
        .expect("run");
}

/// Plants one webhook outcome row of `action` on `route` at `at`.
async fn plant_hook(pool: &PgPool, action: &str, route: &str, at: Timestamp) {
    sqlx::query(
        "INSERT INTO ops.audit_log (at, actor_kind, action, subject_kind, subject_id) \
         VALUES ($1::text::timestamptz, 'system', $2, $3, $4)",
    )
    .bind(at.to_string())
    .bind(action)
    .bind(route)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("audit row");
}

/// Plants one charge attempt on a fresh fake pick: `unreachable` (a failed Stripe call) or
/// `succeeded` (a successful one), decided at `at`.
async fn plant_attempt(pool: &PgPool, status: &str, at: Timestamp) {
    let pick = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO payments.pick_intent \
         (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at) \
         VALUES ($1, $2, $3, 2500, $4, false, $5::text::timestamptz)",
    )
    .bind(pick)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(format!("pi_fake{}", pick.simple()))
    .bind(at.to_string())
    .execute(pool)
    .await
    .expect("intent");
    sqlx::query(
        "INSERT INTO payments.charge_attempt \
         (pick_id, try_no, press_id, status, started_at, decided_at) \
         VALUES ($1, 1, $2, $3, $4::text::timestamptz, $4::text::timestamptz)",
    )
    .bind(pick)
    .bind(Uuid::new_v4())
    .bind(status)
    .bind(at.to_string())
    .execute(pool)
    .await
    .expect("attempt");
}

/// The severities of the open vendor alerts and the count of all ever raised.
async fn alerts(pool: &PgPool) -> (Vec<String>, i64) {
    let open = sqlx::query_scalar(
        "SELECT severity FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL",
    )
    .bind(VENDOR_ALERT)
    .fetch_all(pool)
    .await
    .expect("open");
    let all = sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(VENDOR_ALERT)
        .fetch_one(pool)
        .await
        .expect("all");
    (open, all)
}

/// TD-OPS-44, D-4280: two `unreachable` charge attempts 31 minutes apart with nothing between
/// raise one High alert across two runs; a later run raises no second one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_two_unreachable_attempts_31_minutes_apart_raise_one_high_alert(pool: PgPool) {
    seed(&pool, start()).await;
    plant_attempt(&pool, "unreachable", mins(0)).await;
    run_at(&pool, mins(10)).await;
    assert!(alerts(&pool).await.0.is_empty(), "one alone raises none");
    plant_attempt(&pool, "unreachable", mins(31)).await;
    run_at(&pool, mins(35)).await;
    assert_eq!(alerts(&pool).await, (vec!["high".to_owned()], 1));
    plant_attempt(&pool, "unreachable", mins(36)).await;
    run_at(&pool, mins(40)).await;
    assert_eq!(
        alerts(&pool).await.1,
        1,
        "a second run raises no second alert"
    );
}

/// TD-OPS-44: one unreachable attempt alone raises none, however long the chain runs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_one_unreachable_attempt_raises_none(pool: PgPool) {
    seed(&pool, start()).await;
    plant_attempt(&pool, "unreachable", mins(0)).await;
    run_at(&pool, mins(10)).await;
    run_at(&pool, mins(60)).await;
    assert_eq!(alerts(&pool).await.1, 0);
}

/// TD-OPS-44, D-5670: a successful Stripe call between two Stripe errors breaks the run.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_a_stripe_success_between_raises_none(pool: PgPool) {
    seed(&pool, start()).await;
    plant_attempt(&pool, "unreachable", mins(0)).await;
    plant_attempt(&pool, "succeeded", mins(15)).await;
    plant_attempt(&pool, "unreachable", mins(31)).await;
    run_at(&pool, mins(35)).await;
    assert_eq!(alerts(&pool).await.1, 0);
}

/// TD-OPS-44: webhook 5xx answers at 0 and 31 minutes raise one alert even with a successful
/// Stripe call between (runs are per source), while a 2xx on the same route between raises none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_webhook_runs_are_per_route(pool: PgPool) {
    seed(&pool, start()).await;
    plant_hook(&pool, WEBHOOK_ERROR_ACTION, STRIPE_HOOK, mins(0)).await;
    plant_hook(&pool, WEBHOOK_ERROR_ACTION, SMS, mins(0)).await;
    plant_hook(&pool, WEBHOOK_OK_ACTION, STRIPE_HOOK, mins(10)).await;
    plant_attempt(&pool, "succeeded", mins(12)).await;
    plant_hook(&pool, WEBHOOK_OK_ACTION, "webhooks.twilio_status", mins(14)).await;
    plant_hook(&pool, WEBHOOK_ERROR_ACTION, STRIPE_HOOK, mins(31)).await;
    run_at(&pool, mins(35)).await;
    assert_eq!(alerts(&pool).await.1, 0, "a 2xx on the same route broke it");
    plant_hook(&pool, WEBHOOK_ERROR_ACTION, SMS, mins(36)).await;
    run_at(&pool, mins(40)).await;
    assert_eq!(alerts(&pool).await, (vec!["high".to_owned()], 1));
}

/// TD-OPS-44, OPS-023, D-220: an open alert stays open 1 hour 59 minutes after the last error and
/// clears after two clean hours.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_two_clean_hours_clear_it(pool: PgPool) {
    seed(&pool, start()).await;
    plant_hook(&pool, WEBHOOK_ERROR_ACTION, SMS, mins(0)).await;
    plant_hook(&pool, WEBHOOK_ERROR_ACTION, SMS, mins(31)).await;
    run_at(&pool, mins(35)).await;
    assert_eq!(alerts(&pool).await.0, ["high"]);
    run_at(&pool, mins(31 + 119)).await;
    assert_eq!(
        alerts(&pool).await.0,
        ["high"],
        "an error 1 h 59 min ago keeps it"
    );
    run_at(&pool, mins(31 + 121)).await;
    assert!(alerts(&pool).await.0.is_empty(), "two clean hours clear it");
}

/// TD-OPS-44: the dev environment reads and chains but raises nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_dev_raises_none(pool: PgPool) {
    seed(&pool, start()).await;
    plant_attempt(&pool, "unreachable", mins(0)).await;
    plant_attempt(&pool, "unreachable", mins(31)).await;
    let payload: Value = sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1")
        .bind(CHECK_KIND)
        .fetch_one(&pool)
        .await
        .expect("seed");
    run_payload(&pool, Environment::Dev, &payload, mins(35))
        .await
        .expect("run");
    assert_eq!(alerts(&pool).await.1, 0);
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND done_at IS NULL AND run_at > $2::text::timestamptz",
    )
    .bind(CHECK_KIND)
    .bind(mins(35).to_string())
    .fetch_one(&pool)
    .await
    .expect("next");
    assert_eq!(open, 1, "the next slot is queued");
}

/// The fake webhook routes: the text route answers 500, the Stripe route 200, the status route
/// 403 (a refused signature), all under the outcome layer at `clock`.
fn hooks(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    let routes = Router::new()
        .route(
            inbound_text::INBOUND_PATH,
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .route(
            askhava_payments::api::STRIPE_WEBHOOK_PATH,
            post(|| async { StatusCode::OK }),
        )
        .route(
            inbound_text::STATUS_PATH,
            post(|| async { StatusCode::FORBIDDEN }),
        );
    layer(routes, pool, clock)
}

async fn send(app: &Router, path: &str) -> StatusCode {
    let request = Request::post(path).body(Body::empty()).expect("request");
    app.clone()
        .oneshot(request)
        .await
        .expect("infallible")
        .status()
}

/// The `(action, subject_kind, field)` of every webhook outcome row, oldest first.
async fn hook_rows(pool: &PgPool) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT action, subject_kind, field FROM ops.audit_log \
         WHERE action IN ($1, $2) ORDER BY id",
    )
    .bind(WEBHOOK_ERROR_ACTION)
    .bind(WEBHOOK_OK_ACTION)
    .fetch_all(pool)
    .await
    .expect("rows")
}

/// Waits up to 10 seconds for `n` webhook outcome rows (they are written by spawned tasks).
async fn wait_rows(pool: &PgPool, n: usize) -> Vec<(String, String, Option<String>)> {
    for _ in 0..200 {
        let rows = hook_rows(pool).await;
        if rows.len() >= n {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    hook_rows(pool).await
}

fn row(action: &str, route: &str) -> (String, String, Option<String>) {
    (action.to_owned(), route.to_owned(), None)
}

/// TD-OPS-44: a 5xx writes one `server.webhook_error` row naming only the route (no body, no
/// sender, no field), a refused signature writes nothing, the first 2xx of a window one
/// `server.webhook_ok` row and a second 2xx in it nothing; a restart (a new layer) between a 5xx
/// and the next 2xx still records the success, and a new window records again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_the_layer_writes_one_row_per_5xx_and_per_window(pool: PgPool) {
    let before = hooks(pool.clone(), Arc::new(ManualClock::new(mins(0))));
    assert_eq!(send(&before, inbound_text::INBOUND_PATH).await, 500);
    assert_eq!(send(&before, inbound_text::STATUS_PATH).await, 403);
    assert_eq!(wait_rows(&pool, 1).await, [row(WEBHOOK_ERROR_ACTION, SMS)]);
    let after = hooks(pool.clone(), Arc::new(ManualClock::new(mins(1))));
    let stripe = askhava_payments::api::STRIPE_WEBHOOK_PATH;
    assert_eq!(send(&after, stripe).await, 200);
    assert_eq!(
        wait_rows(&pool, 2).await.len(),
        2,
        "the restart kept the success"
    );
    assert_eq!(send(&after, stripe).await, 200);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let rows = hook_rows(&pool).await;
    assert_eq!(rows[1], row(WEBHOOK_OK_ACTION, STRIPE_HOOK));
    assert_eq!(rows.len(), 2, "a second 2xx in the window writes nothing");
    let later = hooks(pool.clone(), Arc::new(ManualClock::new(mins(5))));
    assert_eq!(send(&later, stripe).await, 200);
    assert_eq!(
        wait_rows(&pool, 3).await.len(),
        3,
        "a new window records again"
    );
}

/// TD-OPS-44: when the audit write fails (no database), the answers are unchanged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_44_a_failed_audit_write_leaves_the_answer_unchanged(pool: PgPool) {
    let nowhere = PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(1))
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("lazy pool");
    let app = hooks(nowhere, Arc::new(SystemClock));
    assert_eq!(send(&app, inbound_text::INBOUND_PATH).await, 500);
    assert_eq!(
        send(&app, askhava_payments::api::STRIPE_WEBHOOK_PATH).await,
        200
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(hook_rows(&pool).await.is_empty());
}

/// D-5876: a stalled webhook handler cut by `limit_request` (an outer layer the handler's answer
/// never reaches) still writes its error row through the drop guard; with a planted error 31
/// minutes before it, the run raises one alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn d_5876_a_stalled_webhook_under_the_time_limit_counts_as_an_error(pool: PgPool) {
    let started = Arc::new(Notify::new());
    let entered = Arc::clone(&started);
    let stalled = post(move || {
        let entered = Arc::clone(&entered);
        async move {
            entered.notify_one();
            std::future::pending::<StatusCode>().await
        }
    });
    let routes = Router::new().route(inbound_text::INBOUND_PATH, stalled);
    let app = layer(routes, pool.clone(), Arc::new(SystemClock)).layer(
        axum::middleware::from_fn_with_state(RequestTimeout::DEFAULT, limit_request),
    );
    let request = Request::post(inbound_text::INBOUND_PATH)
        .body(Body::empty())
        .expect("request");
    let call = tokio::spawn(app.oneshot(request));
    started.notified().await;
    tokio::time::pause();
    tokio::time::advance(RequestTimeout::DEFAULT.duration()).await;
    let response = call.await.expect("joined").expect("infallible");
    tokio::time::resume();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(wait_rows(&pool, 1).await, [row(WEBHOOK_ERROR_ACTION, SMS)]);
    let now = Timestamp::now();
    plant_hook(
        &pool,
        WEBHOOK_ERROR_ACTION,
        SMS,
        now - SignedDuration::from_mins(31),
    )
    .await;
    seed(&pool, now - SignedDuration::from_mins(40)).await;
    run_at(&pool, now + SignedDuration::from_mins(2)).await;
    assert_eq!(alerts(&pool).await, (vec!["high".to_owned()], 1));
}
