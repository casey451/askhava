//! T487: safe mode and the owner pause switches (TD-OPS-01, TD-OPS-06, TD-FEE-20; OPS-001,
//! OPS-003, OPS-024, FEE-038; D-075, D-133, D-278, D-1427, D-3844, D-3878).
//!
//! Safe mode's flag is set by `ops::api::check_safe_mode` (the worker's minute check) and read,
//! with the database-clock fallback, through `payments::api::charges_paused`. The push record a
//! pushed High alert carries is T204's, so these tests insert it directly (D-3844). The timed
//! cases plant pushes in 2030, so the database-clock fallback stays off and only the check's own
//! clock decides; the fallback cases plant pushes relative to the database's `now()`. The release
//! clauses are T408's (TD-OPS-40). Stripe is a scripted fake that logs every call. Fake data only.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::arizona;
use askhava_ops::api::{
    Actor, DELIVERY_ALERT, NewAlert, SAFE_MODE_OFF_ACTION, SAFE_MODE_ON_ACTION, Severity, Switch,
    check_safe_mode, raise_alert, set_switch,
};
use askhava_payments::api::{
    BusinessId, ChargeOutcome, FeeJob, FeeMode, FeeQuote, IntentStripe, PeriodJob, PickNow, Press,
    RecurringCap, Service, VisitTier, charges_paused, confirm_and_pay, fee_at_pick,
    fix_fee_at_pick, sync_pause,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb487_0001));
type Answer = Result<PaymentIntent, AdapterError>;

/// A scripted Stripe: each confirm takes the next scripted answer; every call is logged.
#[derive(Default)]
struct FakeStripe {
    calls: Mutex<Vec<String>>,
    confirms: Mutex<VecDeque<Answer>>,
}

impl FakeStripe {
    fn new(confirms: Vec<Answer>) -> Self {
        Self {
            calls: Mutex::default(),
            confirms: Mutex::new(confirms.into()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }

    fn log(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }
}

fn intent(status: &str) -> Answer {
    Ok(PaymentIntent {
        id: "pi_FakeT487".to_owned(),
        status: status.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_FakeT487".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
        latest_charge: None,
    })
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(&self, _: &IdempotencyKey, _: Charge<'_>) -> Answer {
        panic!("no second intent is made here")
    }

    async fn fetch_payment_intent(&self, id: &str) -> Answer {
        self.log(format!("fetch {id}"));
        Err(AdapterError::NotSent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        key: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        self.log(format!("create {}", key.as_str()));
        Ok(format!("pi_FakeT487_{}", self.calls().len()))
    }

    async fn confirm_intent(&self, _: &IdempotencyKey, intent: &str, _: &str) -> Answer {
        self.log(format!("confirm {intent}"));
        let next = self.confirms.lock().expect("lock").pop_front();
        next.expect("a scripted confirm answer")
    }
}

/// A fake 2030 instant plus `secs` seconds: far ahead of the database clock.
fn later(secs: i64) -> Timestamp {
    let start: Timestamp = "2030-03-04T17:00:00Z".parse().expect("instant");
    start + SignedDuration::from_secs(secs)
}

/// Arizona wall-clock time on fake days in November 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(arizona())
        .expect("zoned")
        .timestamp()
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

/// Casey acknowledges alert `id` (fake acknowledgement).
async fn acknowledge(pool: &PgPool, id: Uuid) {
    sqlx::query(
        "WITH done AS (UPDATE ops.alert SET resolved_at = now() WHERE id = $1 RETURNING id) \
         INSERT INTO ops.alert_ack (alert_id, acknowledged_by, at) SELECT id, 'casey', now() FROM done",
    )
    .bind(id)
    .execute(pool)
    .await
    .expect("ack");
}

async fn check(pool: &PgPool, at: Timestamp) -> bool {
    let mut conn = pool.acquire().await.expect("conn");
    check_safe_mode(&mut conn, at).await.expect("check")
}

async fn paused(pool: &PgPool) -> bool {
    let mut conn = pool.acquire().await.expect("conn");
    charges_paused(&mut conn).await.expect("paused")
}

async fn count(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn audits(pool: &PgPool, action: &str) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM ops.audit_log WHERE action = $1",
        action,
    )
    .await
}

/// A press on fake pick `pick`, with its fee fixed at the pick first (D-1871).
async fn press(pool: &PgPool, pick: u128) -> Press<'static> {
    let p = Press {
        pick: Uuid::from_u128(pick),
        business: BUSINESS,
        job: Uuid::from_u128(pick + 0x100),
        amount_cents: 2500,
        payment_method: "pm_FakeCard",
        press: Uuid::from_u128(pick * 100 + 1),
        emergency: false,
        now: PickNow::Payable,
    };
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(10_000), None, job, FeeMode::Tiers, None).expect("fee");
    let mut conn = pool.acquire().await.expect("conn");
    fix_fee_at_pick(&mut conn, p.pick, BUSINESS, p.job, &fee, az(1, 9, 0))
        .await
        .expect("fee fixed");
    p
}

async fn pay(pool: &PgPool, stripe: &FakeStripe, p: &Press<'_>, at: Timestamp) -> ChargeOutcome {
    let secret = SecretString::from("fake-id-secret-t487".to_owned());
    confirm_and_pay(pool, stripe, &secret, p, at)
        .await
        .expect("press")
}

async fn with_customer(pool: &PgPool) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT487")
        .execute(pool)
        .await
        .expect("customer");
}

/// TD-OPS-01 (OPS-001, D-278): a pushed High alert unacknowledged at 23:59:59 after its push
/// leaves safe mode off; at 24:00:00 the check turns it on once, with one audit row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_01_safe_mode_at_24_00_00_not_at_23_59_59(pool: PgPool) {
    plant(
        &pool,
        "ops.ledger_difference",
        Severity::High,
        Some("2030-03-04T17:00:00Z"),
    )
    .await;
    assert!(!check(&pool, later(86_399)).await, "23:59:59");
    assert!(!paused(&pool).await);
    assert!(check(&pool, later(86_400)).await, "24:00:00");
    assert!(paused(&pool).await, "charges pause in safe mode");
    assert!(check(&pool, later(86_460)).await);
    assert_eq!(
        audits(&pool, SAFE_MODE_ON_ACTION).await,
        1,
        "a repeat logs nothing"
    );
}

/// TD-OPS-01 (OPS-001, D-075, D-278): a complaint, a delivery alert and an unpushed High alert
/// never start safe mode, however long they stay open.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_01_a_complaint_or_delivery_or_unpushed_alert_never_starts_it(pool: PgPool) {
    let pushed = Some("2026-01-01T00:00:00Z");
    plant(&pool, "businesses.complaint", Severity::Normal, pushed).await;
    plant(&pool, DELIVERY_ALERT, Severity::High, pushed).await;
    plant(&pool, "ops.health_down", Severity::High, None).await;
    assert!(!check(&pool, later(10 * 86_400)).await);
    assert!(!paused(&pool).await, "nor by the database clock");
}

/// TD-OPS-06 (OPS-023, OPS-024, FEE-016): safe mode ends only once no other pushed High alert is
/// open, and its end starts no charge.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_06_safe_mode_ends_when_no_other_high_alert_is_open(pool: PgPool) {
    let first = plant(
        &pool,
        "ops.ledger_difference",
        Severity::High,
        Some("2030-03-04T17:00:00Z"),
    )
    .await;
    let second = plant(
        &pool,
        "ops.critical_emergency",
        Severity::Critical,
        Some("2030-03-05T16:00:00Z"),
    )
    .await;
    assert!(check(&pool, later(86_400)).await);
    acknowledge(&pool, first).await;
    assert!(
        check(&pool, later(86_460)).await,
        "the critical alert is still open"
    );
    acknowledge(&pool, second).await;
    assert!(
        !check(&pool, later(86_520)).await,
        "every High alert acknowledged"
    );
    assert!(!paused(&pool).await);
    assert_eq!(audits(&pool, SAFE_MODE_OFF_ACTION).await, 1);
    let queued = "SELECT count(*) FROM core.task WHERE kind LIKE $1";
    assert_eq!(
        count(&pool, queued, "payments.%").await,
        0,
        "no charge starts by itself"
    );
}

/// TD-OPS-06, OPS-024, D-3878: a push 24 hours and 1 minute ago with no check run: a "Confirm
/// and pay" press answers `Paused` ("try again later") with no Stripe call; after the
/// acknowledgement and the check nothing was charged or queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_06_a_press_in_safe_mode_is_refused_before_any_stripe_call(pool: PgPool) {
    with_customer(&pool).await;
    let alert = plant(&pool, "ops.ledger_difference", Severity::High, None).await;
    sqlx::query("INSERT INTO ops.alert_push (alert_id, pushed_at) VALUES ($1, now() - interval '24 hours 1 minute')")
        .bind(alert)
        .execute(&pool)
        .await
        .expect("push");
    let stripe = FakeStripe::default();
    let p = press(&pool, 0x4870).await;
    assert_eq!(
        pay(&pool, &stripe, &p, Timestamp::now()).await,
        ChargeOutcome::Paused
    );
    assert!(stripe.calls().is_empty(), "no Stripe call");
    acknowledge(&pool, alert).await;
    assert!(!check(&pool, Timestamp::now()).await);
    let attempts = "SELECT count(*) FROM payments.charge_attempt WHERE pick_id::text = $1";
    assert_eq!(count(&pool, attempts, &p.pick.to_string()).await, 0);
    let queued = "SELECT count(*) FROM core.task WHERE kind LIKE $1";
    assert_eq!(
        count(&pool, queued, "payments.%").await,
        0,
        "no charge starts by itself"
    );
}

async fn timer_due(pool: &PgPool, pick: u128, kind: &str) -> Option<Timestamp> {
    let due: Option<jiff_sqlx::Timestamp> = sqlx::query_scalar(
        "SELECT due_at FROM payments.pay_timer WHERE pick_id = $1 AND kind = $2 AND ended_at IS NULL",
    )
    .bind(Uuid::from_u128(pick))
    .bind(kind)
    .fetch_one(pool)
    .await
    .expect("timer");
    due.map(|d| d.to_jiff())
}

/// TD-FEE-20 (FEE-038, D-135): with safe mode on, the 1-business-hour card-fix timer and the
/// 4-hour Stripe release stop, and resume with the time they had left once it ends.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_safe_mode_stops_and_resumes_both_payment_timers(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![
        intent("requires_payment_method"),
        Err(AdapterError::NotSent),
    ]);
    let (fix, down) = (0x4871, 0x4872);
    for pick in [fix, down] {
        let p = press(&pool, pick).await;
        pay(&pool, &stripe, &p, az(3, 10, 0)).await;
    }
    assert_eq!(timer_due(&pool, fix, "card-fix").await, Some(az(3, 11, 0)));
    assert_eq!(
        timer_due(&pool, down, "stripe-down").await,
        Some(az(3, 14, 0))
    );
    let alert = plant(
        &pool,
        "ops.ledger_difference",
        Severity::High,
        Some("2026-01-01T00:00:00Z"),
    )
    .await;
    sync_pause(&pool, az(3, 10, 20)).await.expect("stop");
    assert_eq!(timer_due(&pool, fix, "card-fix").await, None, "stopped");
    assert_eq!(timer_due(&pool, down, "stripe-down").await, None, "stopped");
    acknowledge(&pool, alert).await;
    sync_pause(&pool, az(3, 15, 0)).await.expect("resume");
    assert_eq!(
        timer_due(&pool, fix, "card-fix").await,
        Some(az(3, 15, 40)),
        "40 minutes left"
    );
    assert_eq!(
        timer_due(&pool, down, "stripe-down").await,
        Some(az(3, 18, 40)),
        "3h40 left"
    );
}

/// TD-OPS-01 property (OPS-003, OPS-024, D-1427): for every combination of the `all`,
/// `charging` and `texting` switches and safe mode, charges pause exactly when `all`, `charging`
/// or safe mode is on; texting alone never pauses them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_01_charges_pause_for_exactly_their_switches_and_safe_mode(pool: PgPool) {
    let alert = plant(
        &pool,
        "ops.ledger_difference",
        Severity::High,
        Some("2030-03-04T17:00:00Z"),
    )
    .await;
    let owner = Actor::Owner(Uuid::from_u128(0x0e));
    for bits in 0u8..16 {
        let on = |bit: u8| bits & (1 << bit) != 0;
        let mut conn = pool.acquire().await.expect("conn");
        for (bit, switch) in [
            (0, Switch::All),
            (1, Switch::Charging),
            (2, Switch::Texting),
        ] {
            set_switch(&mut conn, switch, on(bit), owner)
                .await
                .expect("switch");
        }
        sqlx::query(r#"UPDATE ops.safe_mode SET "on" = $1, since = CASE WHEN $1 THEN $2 END"#)
            .bind(on(3))
            .bind(later(86_400).to_sqlx())
            .execute(&pool)
            .await
            .expect("flag");
        assert_eq!(
            paused(&pool).await,
            on(0) || on(1) || on(3),
            "combination {bits:04b}"
        );
    }
    acknowledge(&pool, alert).await;
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// TD-OPS-01 property (OPS-001, OPS-003, D-133, D-4026): every action gate on `main` reads the
/// `all` switch with its own (texts: `texting`; charges: `charging`; cards: `trade`), and only the
/// charge and release gates read safe mode (the release gate in `server`'s `release_sink.rs` and
/// `release_hold.rs`, each read inside `settle_and_release`, `address_in`, `run_held_release` or
/// a non-`pub` `fn`), and no texting-gate file names either release file, so safety texts, STOP,
/// HELP, consent and quote texts still go out in safe mode. The town switch's card-pool read is
/// T251's.
#[test]
fn td_ops_01_every_action_checks_its_switches_and_only_charges_and_releases_read_safe_mode() {
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
    let mut files = Vec::new();
    sources(&app, &mut files);
    let gates = [
        ("messaging/src/mode.rs", "[Switch::Texting, Switch::All]"),
        ("messaging/src/optout.rs", "[Switch::Texting, Switch::All]"),
        (
            "server/src/inbound_text.rs",
            "[Switch::Texting, Switch::All]",
        ),
        (
            "payments/src/handlers.rs",
            "[Switch::Charging, Switch::All]",
        ),
        ("quotes/src/card_pool.rs", "Switch::Trade("),
        ("quotes/src/card_pool.rs", "Switch::All)"),
    ];
    for (file, reads) in gates {
        let text = std::fs::read_to_string(app.join(file)).expect("gate file");
        assert!(text.contains(reads), "{file} must read {reads}");
    }
    for path in files {
        let text = std::fs::read_to_string(&path).expect("source");
        let name = path.to_string_lossy().replace('\\', "/");
        if text.contains("safe_mode_on(") {
            assert!(
                name.ends_with("payments/src/handlers.rs")
                    || name.contains("/ops/src/")
                    || RELEASE_GATES.iter().any(|g| name.ends_with(g)),
                "only charges and releases read safe mode: {name}"
            );
        }
        if name.contains("/messaging/src/") || name.ends_with("server/src/inbound_text.rs") {
            assert!(
                !text.contains("release_sink") && !text.contains("release_hold"),
                "a texting gate names a release file: {name}"
            );
        }
        if text.contains("charges_paused(") {
            assert!(
                name.contains("/payments/src/"),
                "no text path reads the charge pause: {name}"
            );
        }
    }
    for gate in RELEASE_GATES {
        release_gate_reads_safe_mode(&app.join(gate), gate);
    }
}

/// The two `server` files whose release gate reads safe mode (OPS-001, D-4026).
const RELEASE_GATES: [&str; 2] = ["server/src/release_sink.rs", "server/src/release_hold.rs"];

/// The `fn`s of a release gate allowed to read safe mode, besides a non-`pub` one (D-4026).
const RELEASE_READERS: [&str; 3] = ["settle_and_release", "address_in", "run_held_release"];

/// D-4026 (iii), (iv): `gate` reads `safe_mode_on(` on a non-comment line, and each such read's
/// nearest enclosing `fn` above it is one of [`RELEASE_READERS`] or not `pub`, so no exported
/// wrapper of the safe-mode read exists.
fn release_gate_reads_safe_mode(path: &Path, gate: &str) {
    let text = std::fs::read_to_string(path).expect("release gate file");
    let lines: Vec<&str> = text.lines().collect();
    let mut reads = 0;
    for (n, line) in lines.iter().enumerate() {
        if line.trim().starts_with("//") || !line.contains("safe_mode_on(") {
            continue;
        }
        reads += 1;
        let header = lines[..n]
            .iter()
            .rev()
            .map(|l| l.trim())
            .find(|l| !l.starts_with("//") && fn_name(l).is_some())
            .unwrap_or_else(|| panic!("{gate}:{}: no enclosing fn", n + 1));
        let name = fn_name(header).unwrap_or_default();
        assert!(
            RELEASE_READERS.contains(&name) || !header.starts_with("pub"),
            "{gate}:{}: safe mode read in exported fn {name}",
            n + 1
        );
    }
    assert!(
        reads > 0,
        "{gate} must read safe_mode_on( on a non-comment line"
    );
}

/// The name of the `fn` a trimmed line declares, if it declares one.
fn fn_name(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("fn ")?;
    let before = &line[..line.len() - rest.len() - 3];
    let words = [
        "", "pub ", "async ", "const ", "unsafe ", "(crate) ", "(super) ",
    ];
    let decl = before
        .split_inclusive(' ')
        .all(|w| words.contains(&w) || w.starts_with("pub(") || w.starts_with("extern"));
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    (decl && end > 0).then(|| &rest[..end])
}
