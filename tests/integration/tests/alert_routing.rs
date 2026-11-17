//! TD-OPS-08, TD-OPS-09, TD-OPS-10 and TD-OPS-13 (T204; OPS-004, OPS-023, OPS-026, OPS-029,
//! OPS-032, OPS-035, SEC-051; D-218, D-220, D-277, D-381, D-2207, D-3877, D-3880, D-4044): only
//! the OPS-029 kinds push; a health alert pushes 10 minutes after it was raised (15 after the first
//! failed check) unless the check passed again; Stripe, health and ledger alerts clear by kind; a
//! ledger difference pushes once, on its second night, and only a matched reconcile clears it; the
//! 9 a.m. and 3 p.m. push lists open items until
//! done and sends nothing when nothing is open; the email-limit alert pushes once a day with the
//! upgrade wording; and a raised pushing alert reaches the fake email provider through the worker,
//! as an email only, with a critical alert's call task queued even when the email fails, while the webhook-silence check runs
//! hourly. The worker cases run `askhava_server::worker_registry`, compiled in place as
//! `session_sweep.rs` does (D-2853's exception, logged for this file in D-4083), against a local
//! recording server shaped as the Resend stand-in. The Stripe and health alerts are planted rows,
//! as T518 raises them (D-3877). Fake data only.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_core::api::{Clock, Lease, ManualClock, Registry, Settings, WorkerId, claim};
use askhava_messaging::api::{
    EmailLimits, HOURLY_CHECKS_KIND, OWNER_ALERT_CALL_KIND, WEBHOOKS_SILENT_ALERT, check_email_use,
    schedule_alert_push, schedule_hourly_checks,
};
use askhava_ops::api::{
    EMAIL_LIMIT_ALERT, HEALTH_ALERT, LEDGER_ALERT, VENDOR_ALERT, VOLUME_HIGH_ALERT,
    acknowledge_cleared, add_push_item, alerts_to_push, away_until, daytime_push_due,
    mark_push_item_done, push_slot, record_daytime_push, record_push, set_away_until, vendor_quiet,
};
use askhava_payments::api::{ReconcileStripe, StripeCharge, reconcile};
use askhava_picks::api::ConfirmSettings;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const MIN: SignedDuration = SignedDuration::from_mins(1);
const HOUR: SignedDuration = SignedDuration::from_hours(1);
/// 2026-11-17 02:00 Arizona (09:00 UTC).
const NIGHT: i64 = 1_794_906_000;
/// 2026-11-17 15:00 Arizona (22:00 UTC).
const AFTERNOON: i64 = 1_794_952_800;
/// 2026-11-18 09:00 Arizona (16:00 UTC).
const NEXT_MORNING: i64 = 1_795_017_600;
const ALERT_TO: &str = "alerts@fake.example";
const CALL_BACK: &str = "intake.call_back_urgent";

/// Each email the recording server took: its recipient and plain text.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("time")
}

/// Plants one open alert of `kind` and `severity` raised at `raised`.
async fn plant(pool: &PgPool, kind: &str, severity: &str, raised: Timestamp) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO ops.alert (kind, severity, raised_at) VALUES ($1, $2, $3::text::timestamptz) \
         RETURNING id",
    )
    .bind(kind)
    .bind(severity)
    .bind(raised.to_string())
    .fetch_one(pool)
    .await
    .expect("alert")
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn pushed(pool: &PgPool, now: Timestamp) -> Vec<Uuid> {
    let mut conn = pool.acquire().await.expect("conn");
    let due = alerts_to_push(&mut conn, now).await.expect("due");
    due.into_iter().map(|a| a.id).collect()
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_only_the_ops_029_kinds_push(pool: PgPool) {
    let now = at(NIGHT);
    let mut pushes = Vec::new();
    for (kind, severity) in [
        ("jobs.problem_report_safety", "critical"),
        ("messaging.number_suspended", "high"),
        (EMAIL_LIMIT_ALERT, "normal"),
        ("privacy.vault_read_spike", "high"),
        ("privacy.vault_read_unregistered", "high"),
    ] {
        pushes.push(plant(&pool, kind, severity, now).await);
    }
    pushes.push(plant(&pool, HEALTH_ALERT, "high", now - 10 * MIN).await);
    plant(&pool, LEDGER_ALERT, "high", now - 24 * HOUR).await;
    pushes.push(plant(&pool, LEDGER_ALERT, "high", now).await);
    for (kind, severity) in [
        ("ops.delivery_low", "high"),
        ("ops.coverage_low", "normal"),
        ("businesses.complaint", "normal"),
        (VOLUME_HIGH_ALERT, "high"),
        (CALL_BACK, "high"),
        (VENDOR_ALERT, "high"),
    ] {
        plant(&pool, kind, severity, now).await;
    }
    plant(&pool, HEALTH_ALERT, "high", now - 9 * MIN).await;
    let mut due = pushed(&pool, now).await;
    due.sort();
    pushes.sort();
    assert_eq!(due, pushes, "the OPS-029 kinds and nothing else");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_a_health_check_that_recovers_never_pushes_and_one_still_failing_does(
    pool: PgPool,
) {
    let first_failure = at(NIGHT);
    let health = plant(&pool, HEALTH_ALERT, "high", first_failure + 5 * MIN).await;
    let mut conn = pool.acquire().await.expect("conn");
    let cleared = acknowledge_cleared(&mut conn, HEALTH_ALERT, first_failure + 10 * MIN).await;
    assert_eq!(
        cleared.expect("ack"),
        vec![health],
        "passing again clears it"
    );
    assert!(
        pushed(&pool, first_failure + 15 * MIN).await.is_empty(),
        "no push"
    );
    let ack =
        "SELECT count(*) FROM ops.alert_ack WHERE alert_id = $1 AND acknowledged_by = 'system'";
    assert_eq!(count(&pool, ack, health).await, 1, "self-acknowledged");
    let logged = "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 \
                  AND action = 'ops.alert_acknowledged'";
    assert_eq!(
        count(&pool, logged, health).await,
        1,
        "logged for the digest"
    );
    let failing = plant(&pool, HEALTH_ALERT, "high", first_failure + 5 * MIN).await;
    assert!(
        pushed(&pool, first_failure + 14 * MIN).await.is_empty(),
        "not yet at 14"
    );
    assert_eq!(
        pushed(&pool, first_failure + 15 * MIN).await,
        vec![failing],
        "push at 15"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_cleared_alerts_are_acknowledged_by_kind_and_only_a_match_clears_a_ledger(
    pool: PgPool,
) {
    let now = at(NIGHT);
    assert!(!vendor_quiet(
        now,
        now + 2 * HOUR - SignedDuration::from_secs(1)
    ));
    assert!(vendor_quiet(now, now + 2 * HOUR), "clear for 2 hours");
    let vendor = plant(&pool, VENDOR_ALERT, "high", now).await;
    let ledger = plant(&pool, LEDGER_ALERT, "high", now).await;
    let delivery = plant(&pool, "ops.delivery_low", "high", now).await;
    let mut conn = pool.acquire().await.expect("conn");
    let later = now + 2 * HOUR;
    let vendor_ack = acknowledge_cleared(&mut conn, VENDOR_ALERT, later).await;
    assert_eq!(vendor_ack.expect("vendor"), vec![vendor]);
    let other = acknowledge_cleared(&mut conn, "ops.delivery_low", later).await;
    assert!(
        other.expect("other").is_empty(),
        "no other kind clears this way"
    );
    let open = "SELECT count(*) FROM ops.alert WHERE id = $1 AND resolved_at IS NULL";
    assert_eq!(
        count(&pool, open, ledger).await,
        1,
        "the 2-hour path leaves the ledger"
    );
    assert_eq!(count(&pool, open, delivery).await, 1);
    let matched = acknowledge_cleared(&mut conn, LEDGER_ALERT, now + 24 * HOUR).await;
    assert_eq!(
        matched.expect("ledger"),
        vec![ledger],
        "a matched reconcile clears it"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_a_ledger_difference_pushes_only_when_still_there_the_next_night(pool: PgPool) {
    let night_one = at(NIGHT);
    let first = plant(&pool, LEDGER_ALERT, "high", night_one).await;
    assert!(
        pushed(&pool, night_one + HOUR).await.is_empty(),
        "night 1: no push"
    );
    let second = plant(&pool, LEDGER_ALERT, "high", night_one + 24 * HOUR).await;
    let due = pushed(&pool, night_one + 24 * HOUR).await;
    assert_eq!(due, vec![second], "night 2: one push, the newest");
    let mut conn = pool.acquire().await.expect("conn");
    record_push(&mut conn, second, night_one + 24 * HOUR)
        .await
        .expect("push");
    plant(&pool, LEDGER_ALERT, "high", night_one + 48 * HOUR).await;
    let open = "SELECT count(*) FROM ops.alert WHERE id = $1 AND resolved_at IS NULL";
    assert_eq!(count(&pool, open, first).await, 1);
    assert!(
        pushed(&pool, night_one + 48 * HOUR).await.is_empty(),
        "night 3: no second push for the same difference"
    );
}

/// Stripe listing the given charges only; nothing else is called.
struct ListOnly(Vec<StripeCharge>);

impl PaymentsAdapter for ListOnly {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl ReconcileStripe for ListOnly {
    async fn list_charges(&self, _: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        Ok(self.0.clone())
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_only_a_matched_reconcile_acknowledges_the_ledger_alert(pool: PgPool) {
    let ledger = plant(&pool, LEDGER_ALERT, "high", at(NIGHT)).await;
    let secret = SecretString::from("fake-id-secret-t204".to_owned());
    let malformed = StripeCharge {
        id: "bad".to_owned(),
        payment_intent: "pi_FakeT204".to_owned(),
        pick: None,
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: "txn_FakeT204".to_owned(),
        fee_cents: 248,
    };
    let night_two = at(NIGHT) + 24 * HOUR;
    let report = reconcile(&pool, &ListOnly(vec![malformed]), &secret, night_two).await;
    assert!(!report.expect("run").matched());
    let ack =
        "SELECT count(*) FROM ops.alert_ack WHERE alert_id = $1 AND acknowledged_by = 'system'";
    let open = "SELECT count(*) FROM ops.alert WHERE id = $1 AND resolved_at IS NULL";
    assert_eq!(
        count(&pool, ack, ledger).await,
        0,
        "a mismatch leaves it open"
    );
    assert_eq!(count(&pool, open, ledger).await, 1);
    let report = reconcile(&pool, &ListOnly(Vec::new()), &secret, night_two + 24 * HOUR).await;
    assert!(report.expect("run").matched());
    assert_eq!(
        count(&pool, ack, ledger).await,
        1,
        "a match clears it, by the system"
    );
    assert_eq!(count(&pool, open, ledger).await, 0);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_the_daytime_push_lists_open_items_until_done(pool: PgPool) {
    assert_eq!(
        push_slot(at(AFTERNOON) - MIN),
        at(AFTERNOON) - 6 * HOUR,
        "14:59 → 9:00"
    );
    assert_eq!(
        push_slot(at(NEXT_MORNING) - MIN),
        at(AFTERNOON),
        "8:59 → yesterday's 15:00"
    );
    let mut conn = pool.acquire().await.expect("conn");
    let call_back = plant(&pool, CALL_BACK, "high", at(AFTERNOON) + MIN).await;
    let afternoon = daytime_push_due(&mut conn, at(AFTERNOON) + 2 * MIN).await;
    assert_eq!(
        afternoon.expect("15:00"),
        None,
        "a 15:01 call-back waits for 9:00"
    );
    let volume = plant(&pool, VOLUME_HIGH_ALERT, "high", at(AFTERNOON) + HOUR).await;
    let help = Uuid::from_u128(0x0f4e);
    let item = add_push_item(
        &mut conn,
        "help.request_due",
        ("help.request", help),
        at(AFTERNOON),
    );
    let item = item.await.expect("item").expect("new item");
    let morning = daytime_push_due(&mut conn, at(NEXT_MORNING)).await;
    let morning = morning.expect("9:00").expect("a push");
    let listed: Vec<Uuid> = morning.alerts.iter().map(|a| a.id).collect();
    assert_eq!(listed, vec![call_back, volume]);
    assert_eq!(morning.items.len(), 1, "the help request is listed");
    record_daytime_push(&mut conn, &morning, at(NEXT_MORNING))
        .await
        .expect("record");
    let push_row = "SELECT count(*) FROM ops.alert_push WHERE alert_id = $1";
    assert_eq!(
        count(&pool, push_row, call_back).await,
        1,
        "safe mode's clock starts"
    );
    assert_eq!(
        count(&pool, push_row, volume).await,
        0,
        "volume never starts safe mode"
    );
    let again = daytime_push_due(&mut conn, at(NEXT_MORNING) + HOUR).await;
    assert_eq!(again.expect("again"), None, "one push per slot");
    assert!(
        mark_push_item_done(&mut conn, item, at(NEXT_MORNING))
            .await
            .expect("done")
    );
    let later = daytime_push_due(&mut conn, at(NEXT_MORNING) + 6 * HOUR).await;
    let later = later.expect("15:00").expect("still open");
    assert_eq!(later.alerts.len(), 2, "listed until done");
    assert!(later.items.is_empty(), "a done item leaves the list");
    sqlx::query("UPDATE ops.alert SET resolved_at = now()")
        .execute(&pool)
        .await
        .expect("resolve");
    let empty = daytime_push_due(&mut conn, at(NEXT_MORNING) + 24 * HOUR).await;
    assert_eq!(empty.expect("9:00"), None, "nothing open sends nothing");
    plant(&pool, CALL_BACK, "high", at(NEXT_MORNING) + 25 * HOUR).await;
    let late = daytime_push_due(&mut conn, at(NEXT_MORNING) + 41 * HOUR).await;
    assert_eq!(
        late.expect("02:00"),
        None,
        "a missed 15:00 push never goes out at night"
    );
    let next = daytime_push_due(&mut conn, at(NEXT_MORNING) + 48 * HOUR).await;
    assert!(next.expect("9:00").is_some(), "the next slot lists it");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_the_away_date_is_stored_and_cleared(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(away_until(&mut conn).await.expect("read"), None);
    let date = jiff::civil::date(2026, 12, 1);
    let owner = Uuid::from_u128(0x0a);
    set_away_until(&mut conn, Some(date), owner, at(NIGHT))
        .await
        .expect("set");
    assert_eq!(away_until(&mut conn).await.expect("read"), Some(date));
    set_away_until(&mut conn, None, owner, at(NIGHT) + HOUR)
        .await
        .expect("clear");
    assert_eq!(away_until(&mut conn).await.expect("read"), None);
}

/// A recording server shaped as the Resend stand-in on 127.0.0.1; returns its base URL.
async fn recorder(seen: Seen) -> String {
    async fn emails(State(seen): State<Seen>, body: String) -> String {
        let email: serde_json::Value = serde_json::from_str(&body).expect("json");
        let field = |name: &str| email[name].as_str().unwrap_or_default().to_owned();
        let to = email["to"][0]
            .as_str()
            .map_or_else(|| field("to"), str::to_owned);
        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.push((to, field("text")));
        format!("{{\"id\":\"re_fake_{}\"}}", seen.len())
    }
    let app = Router::new()
        .route("/resend/emails", post(emails))
        .with_state(seen);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{address}/resend")
}

/// The registry the `worker` binary runs, from fake Ci settings with the provider at `base`.
fn registry(clock: Arc<dyn Clock>, base: &str, name: &str) -> Registry {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t204-{name}"));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let storage = format!("file://{}", dir.display());
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_STORAGE_BASE_URL", storage.as_str()),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-for-t204-not-a-real-key",
        ),
        ("ASKHAVA_RESEND_DOMAIN", "testfake.example.com"),
        ("ASKHAVA_RESEND_API_KEY", "re_fake_key_never_real_0204"),
        ("ASKHAVA_RESEND_BASE_URL", base),
        ("ASKHAVA_POSTAL_ADDRESS", "PO Box 1234, Faketown, AZ 00000"),
        ("ASKHAVA_ALERT_EMAIL", ALERT_TO),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    worker_registry(&settings, stream, (None, ConfirmSettings::default()), pairs).expect("registry")
}

/// Runs every task due at the clock's now through `registry`, as the worker would, until none is;
/// with `strict` every task but the call must succeed.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock, strict: bool) {
    let owner = WorkerId::new(pool).await.expect("worker");
    for _ in 0..10 {
        let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            let kind = task.kind.clone();
            let done = registry.run(pool, clock, owner, task, Lease::WORKER).await;
            if strict && kind != OWNER_ALERT_CALL_KIND {
                done.expect("run");
            }
        }
    }
}

fn emails(seen: &Seen) -> Vec<(String, String)> {
    seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_10_a_critical_alert_reaches_the_email_provider_through_the_worker(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let clock = Arc::new(ManualClock::new(at(NIGHT)));
    let registry = registry(clock.clone(), &base, "critical");
    let safety = plant(&pool, "jobs.problem_report_safety", "critical", at(NIGHT)).await;
    plant(&pool, "businesses.complaint", "normal", at(NIGHT)).await;
    let mut conn = pool.acquire().await.expect("conn");
    schedule_alert_push(&mut conn, clock.now())
        .await
        .expect("seed");
    schedule_hourly_checks(&mut conn, clock.now())
        .await
        .expect("seed");
    clock.advance(MIN);
    run_due(&pool, &registry, &clock, true).await;
    let sent = emails(&seen);
    assert_eq!(
        sent.len(),
        1,
        "one email at 02:00, only for the pushing alert"
    );
    assert_eq!(sent[0].0, ALERT_TO, "to the alert address");
    assert!(
        sent[0].1.contains("jobs.problem_report_safety"),
        "names the kind"
    );
    let call = "SELECT count(*) FROM core.task WHERE kind = 'calls.owner_alert_call' \
                AND payload->>'alert' = $1::text";
    assert_eq!(
        count(&pool, call, safety).await,
        1,
        "one call task, the alert ID only"
    );
    let push_row = "SELECT count(*) FROM ops.alert_push WHERE alert_id = $1";
    assert_eq!(count(&pool, push_row, safety).await, 1);
    clock.advance(MIN);
    run_due(&pool, &registry, &clock, true).await;
    assert_eq!(emails(&seen).len(), 1, "pushed once");
    let texts: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(&pool)
        .await
        .expect("texts");
    assert_eq!(texts, 0, "no alert ever goes by text");
    clock.advance(2 * HOUR);
    run_due(&pool, &registry, &clock, true).await;
    let silent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL",
    )
    .bind(WEBHOOKS_SILENT_ALERT)
    .fetch_one(&pool)
    .await
    .expect("silent");
    assert_eq!(silent, 1, "SEC-051's hourly check ran through the worker");
    let next: i64 =
        sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1 AND done_at IS NULL")
            .bind(HOURLY_CHECKS_KIND)
            .fetch_one(&pool)
            .await
            .expect("next");
    assert_eq!(next, 1, "the next hour's check is queued");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_10_the_call_is_queued_when_the_email_fails(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(at(NIGHT)));
    let registry = registry(clock.clone(), "http://127.0.0.1:9/resend", "down");
    let earlier = plant(&pool, "messaging.number_suspended", "high", at(NIGHT) - MIN).await;
    let safety = plant(&pool, "jobs.problem_report_safety", "critical", at(NIGHT)).await;
    let mut conn = pool.acquire().await.expect("conn");
    schedule_alert_push(&mut conn, clock.now())
        .await
        .expect("seed");
    clock.advance(MIN);
    run_due(&pool, &registry, &clock, false).await;
    let call = "SELECT count(*) FROM core.task WHERE kind = 'calls.owner_alert_call' \
                AND payload->>'alert' = $1::text";
    assert_eq!(
        count(&pool, call, safety).await,
        1,
        "the call does not wait on the email, nor on an earlier alert"
    );
    let push_row = "SELECT count(*) FROM ops.alert_push WHERE alert_id = $1";
    assert_eq!(
        count(&pool, push_row, earlier).await,
        0,
        "not emailed, not pushed"
    );
    assert_eq!(count(&pool, push_row, safety).await, 0);
}

/// Plants `n` sent emails at `sent` (fake rows, no body).
async fn sent_emails(pool: &PgPool, n: i32, sent: Timestamp, tag: &str) {
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
           recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at) \
         SELECT encode(sha256(($3 || g)::bytea), 'hex'), 'fake_sent', gen_random_uuid(), \
           'business', '{}', 'notice', 'sent', $1::text::timestamptz, 're_fake_' || $3 || g, \
           $1::text::timestamptz, $1::text::timestamptz FROM generate_series(1, $2) g",
    )
    .bind(sent.to_string())
    .bind(n)
    .bind(tag)
    .execute(pool)
    .await
    .expect("sent rows");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_13_email_use_pushes_once_a_day_with_the_upgrade_wording(pool: PgPool) {
    let limits = EmailLimits::new(100, 3_000).expect("limits");
    assert_eq!(limits.alert_percent(69, 0), None, "69 of 100: no alert");
    assert_eq!(limits.alert_percent(70, 0), Some(70));
    assert_eq!(limits.alert_percent(0, 2_399), None);
    assert_eq!(limits.alert_percent(0, 2_400), Some(80), "the monthly push");
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let clock = Arc::new(ManualClock::new(at(NIGHT)));
    let registry = registry(clock.clone(), &base, "limit");
    sent_emails(&pool, 69, at(NIGHT) - MIN, "a").await;
    let mut conn = pool.acquire().await.expect("conn");
    assert_eq!(
        check_email_use(&mut conn, limits, clock.now())
            .await
            .expect("69"),
        None
    );
    sent_emails(&pool, 1, at(NIGHT) - MIN, "b").await;
    schedule_alert_push(&mut conn, clock.now())
        .await
        .expect("seed");
    schedule_hourly_checks(&mut conn, clock.now() - HOUR)
        .await
        .expect("seed");
    run_due(&pool, &registry, &clock, true).await;
    clock.advance(MIN);
    run_due(&pool, &registry, &clock, true).await;
    let sent = emails(&seen);
    assert_eq!(sent.len(), 1, "one push");
    assert!(
        sent[0].1.contains("email use is at 70% of the plan limit."),
        "{}",
        sent[0].1
    );
    assert!(
        sent[0]
            .1
            .contains("Upgrade the email plan in the provider dashboard.")
    );
    let second = check_email_use(&mut conn, limits, clock.now() + MIN).await;
    assert_eq!(
        second.expect("again"),
        None,
        "a second crossing the same day: none"
    );
    clock.advance(MIN);
    run_due(&pool, &registry, &clock, true).await;
    assert_eq!(emails(&seen).len(), 1, "no second push");
    let alert: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'normal'",
    )
    .bind(EMAIL_LIMIT_ALERT)
    .fetch_one(&pool)
    .await
    .expect("alert");
    assert_eq!(alert, 1, "below High, so never safe mode");
}
