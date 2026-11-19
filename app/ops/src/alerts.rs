//! The delivery-rate and coverage alerts and the delivery alert's automatic acknowledgement
//! (OPS-002, OPS-004, OPS-006, OPS-023, OPS-025; D-134, D-163, D-459, T105).
//!
//! `ops` reads nothing above it, so the counts are reported to it (D-491, D-493): `messaging`
//! reports each audience line's failures among its last 50 and last 200 texts
//! ([`record_delivery_counts`]), `businesses` each touched town's and trade's active businesses
//! ([`record_business_counts`]) and `jobs` each trade's requests this Arizona calendar month
//! ([`record_request_counts`]). Every alert waits for Casey; there is no backup person (OPS-002).
//! Safety reports, disputes and complaints raise their own alerts in `businesses`, `jobs` and
//! `payments` (OPS-005); safe mode is T487's.
//!
//! Locks: the delivery report runs under one advisory lock (`ops.delivery`), and every coverage
//! report under one other (`ops.coverage`, [`lock_coverage`]); each is a single key, taken again
//! freely in the same transaction, so two reports in one transaction cannot deadlock on key order.

use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::api::{ALERT_ACKNOWLEDGED_ACTION, audit, raise_alert_once};
use crate::models::{Actor, AuditEntry, Change, NewAlert, OpsError, Severity, is_code_name};
use crate::service::raise_alert;

/// The High text-delivery alert kind; at most one is open at a time (OPS-004, OPS-025, D-075).
pub const DELIVERY_ALERT: &str = "ops.delivery_low";
/// The alert kind of a town, or a trade with a request this month, under 2 businesses (OPS-006).
pub const COVERAGE_ALERT: &str = "ops.coverage_low";
/// The subject kind of a coverage alert: an `ops.coverage` row.
const COVERAGE_SUBJECT: &str = "ops.coverage";

/// The audience line a text went out on; each audience has its own number (TXT-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// The homeowner line.
    Homeowner,
    /// The business line.
    Business,
}

impl Audience {
    /// The stored code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Homeowner => "homeowner",
            Self::Business => "business",
        }
    }
}

/// One line's failed texts among its last 50 and last 200 texts with a final status (OPS-004).
/// A window not yet full counts its missing texts as delivered, so a new line never alerts on one
/// early failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryCounts {
    audience: Audience,
    failed_of_50: u32,
    failed_of_200: u32,
}

impl DeliveryCounts {
    /// Checks the counts: at most 50 and 200 failures, the first window within the second.
    #[must_use]
    pub fn new(audience: Audience, failed_of_50: u32, failed_of_200: u32) -> Option<Self> {
        let fits = failed_of_50 <= 50 && failed_of_200 <= 200 && failed_of_50 <= failed_of_200;
        fits.then_some(Self {
            audience,
            failed_of_50,
            failed_of_200,
        })
    }

    /// OPS-004, D-163: fewer than 90% of the last 50 or fewer than 95% of the last 200 delivered.
    #[must_use]
    pub fn low(self) -> bool {
        (50 - self.failed_of_50) * 100 < 90 * 50 || (200 - self.failed_of_200) * 100 < 95 * 200
    }

    /// OPS-023, OPS-025, D-163: at least 97% of the last 200 delivered, and not [`Self::low`]
    /// (6 failures in the last 50 are 97% of 200 too, but that line is not back).
    #[must_use]
    pub fn recovered(self) -> bool {
        (200 - self.failed_of_200) * 100 >= 97 * 200 && !self.low()
    }
}

/// What one delivery report did (OPS-023, OPS-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// A new High delivery alert, with its ID.
    Raised(Uuid),
    /// The open delivery alert was acknowledged automatically, logged and listed for Casey.
    Acknowledged(Uuid),
    /// The rate is low, but no line reached 97% of 200 since the last acknowledgement: no alert.
    Held,
    /// Nothing changed.
    Nothing,
}

/// OPS-004, OPS-023, OPS-025, D-163: records one line's counts at `at`, in one transaction under
/// the `ops.delivery` lock. A line at 97% of 200 notes its recovery time and acknowledges the
/// open delivery alert raised on that line (by the system, one [`ALERT_ACKNOWLEDGED_ACTION`]
/// audit row, which the digest lists). A low line raises a High [`DELIVERY_ALERT`] only when no
/// delivery alert is open and, if one of this line's own was ever resolved, the line reached 97%
/// at or after that (D-4310).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn record_delivery_counts(
    db: &mut PgConnection,
    counts: DeliveryCounts,
    at: Timestamp,
) -> Result<DeliveryOutcome, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    advisory_lock(&mut tx, "ops.delivery").await?;
    let line = counts.audience.code();
    if counts.recovered() {
        sqlx::query!(
            "INSERT INTO ops.delivery_line (audience, recovered_at) VALUES ($1, $2) \
             ON CONFLICT (audience) DO UPDATE SET recovered_at = EXCLUDED.recovered_at",
            line,
            at.to_sqlx() as _,
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| OpsError::Database)?;
    }
    let open = sqlx::query!(
        "SELECT a.id, d.audience FROM ops.alert a JOIN ops.delivery_alert d ON d.alert_id = a.id \
         WHERE a.resolved_at IS NULL LIMIT 1"
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    let outcome = match open {
        Some(row) if row.audience == line && counts.recovered() => {
            acknowledge(&mut tx, row.id, at).await?
        }
        Some(_) => DeliveryOutcome::Nothing,
        None if counts.low() => raise_delivery(&mut tx, counts.audience).await?,
        None => DeliveryOutcome::Nothing,
    };
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(outcome)
}

/// OPS-023: resolves delivery alert `id` at `at`, with its acknowledgement row and audit row.
async fn acknowledge(
    tx: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<DeliveryOutcome, OpsError> {
    acknowledge_one(tx, id, at).await?;
    Ok(DeliveryOutcome::Acknowledged(id))
}

/// OPS-023: resolves alert `id` at `at` with its `ops.alert_ack` row by the system and one
/// [`ALERT_ACKNOWLEDGED_ACTION`] audit row, which the digest lists as self-acknowledged.
async fn acknowledge_one(tx: &mut PgConnection, id: Uuid, at: Timestamp) -> Result<(), OpsError> {
    sqlx::query!(
        "WITH done AS (UPDATE ops.alert SET resolved_at = $2 WHERE id = $1 RETURNING id) \
         INSERT INTO ops.alert_ack (alert_id, acknowledged_by, at) \
         SELECT id, 'system', $2 FROM done",
        id,
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: ALERT_ACKNOWLEDGED_ACTION,
        subject_kind: "ops.alert",
        subject_id: id,
        change: Change::Action,
    };
    audit(tx, &entry).await.map(drop)
}

/// OPS-025, OPS-004, D-163, D-4310: raises the High delivery alert for `audience` unless held
/// since this line's own last resolved delivery alert (no recovery to 97% of 200 on this line at
/// or after its resolution); another line's acknowledged alert never holds it (TD-OPS-47).
async fn raise_delivery(
    tx: &mut PgConnection,
    audience: Audience,
) -> Result<DeliveryOutcome, OpsError> {
    let held = sqlx::query_scalar!(
        r#"SELECT COALESCE((SELECT max(a.resolved_at) FROM ops.alert a
             JOIN ops.delivery_alert d ON d.alert_id = a.id WHERE d.audience = $1)
           > COALESCE((SELECT recovered_at FROM ops.delivery_line WHERE audience = $1),
                      '-infinity'), false) AS "held!""#,
        audience.code(),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    if held {
        return Ok(DeliveryOutcome::Held);
    }
    let alert = NewAlert {
        kind: DELIVERY_ALERT,
        severity: Severity::High,
        subject: None,
    };
    let id = raise_alert(tx, &alert).await?;
    sqlx::query!(
        "INSERT INTO ops.delivery_alert (alert_id, audience) VALUES ($1, $2)",
        id,
        audience.code(),
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(DeliveryOutcome::Raised(id))
}

/// Whether a coverage count is a town's or a trade's (catalog category's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageScope {
    /// A town code.
    Town,
    /// A trade (catalog category) code.
    Trade,
}

/// One town's or trade's active businesses, as `businesses` counted them (OPS-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageCount {
    /// Town or trade.
    pub scope: CoverageScope,
    /// The town or category code.
    pub code: String,
    /// Active businesses serving it.
    pub active: u32,
}

/// Takes the one `ops.coverage` advisory lock for the caller's transaction; `businesses` takes it
/// before counting, so two status changes at once never report a stale count (OPS-006). Taking it
/// again in the same transaction is free.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn lock_coverage(db: &mut PgConnection) -> Result<(), OpsError> {
    advisory_lock(db, "ops.coverage").await
}

/// OPS-006, D-459: stores each count at `at` and raises one [`COVERAGE_ALERT`] (unless one is open)
/// for a town under 2 active businesses, or a trade under 2 that had a request this Arizona
/// calendar month; a trade under 2 with none waits for the weekly digest (OPS-030), which reads
/// `ops.coverage`. Returns the new alerts' IDs.
///
/// # Errors
///
/// [`OpsError::Database`] (also for a code not of the code form).
pub async fn record_business_counts(
    db: &mut PgConnection,
    counts: &[CoverageCount],
    at: Timestamp,
) -> Result<Vec<Uuid>, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    lock_coverage(&mut tx).await?;
    let mut raised = Vec::new();
    for count in counts {
        let scope = match count.scope {
            CoverageScope::Town => "town",
            CoverageScope::Trade => "trade",
        };
        let row = sqlx::query!(
            r#"INSERT INTO ops.coverage (scope, code, active, counted_at) VALUES ($1, $2, $3, $4)
               ON CONFLICT (scope, code) DO UPDATE SET active = EXCLUDED.active,
                 counted_at = EXCLUDED.counted_at
               RETURNING id, active < 2 AND (scope = 'town' OR EXISTS (SELECT 1
                 FROM ops.trade_request r WHERE r.trade = code AND r.requests > 0
                 AND r.month = date_trunc('month', $4 AT TIME ZONE 'America/Phoenix')::date))
                 AS "alert!""#,
            scope,
            count.code,
            i32::try_from(count.active).map_err(|_| OpsError::Database)?,
            at.to_sqlx() as _,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| OpsError::Database)?;
        if row.alert {
            raised.extend(raise_coverage(&mut tx, row.id).await?);
        }
    }
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(raised)
}

/// OPS-006, D-459, D-493: stores each trade's requests this Arizona calendar month (`at`'s month)
/// and raises one [`COVERAGE_ALERT`] (unless one is open) for each trade with a request whose last
/// reported count is under 2 active businesses; a requested trade no business has ever reported
/// gets an `ops.coverage` row with 0 active and alerts (TD-OPS-01, TD-LIC-21). Returns the new
/// alerts' IDs.
///
/// # Errors
///
/// [`OpsError::Database`] (also for a code not of the code form).
pub async fn record_request_counts(
    db: &mut PgConnection,
    counts: &[(String, u32)],
    at: Timestamp,
) -> Result<Vec<Uuid>, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    lock_coverage(&mut tx).await?;
    let mut raised = Vec::new();
    for (trade, requests) in counts {
        let low = sqlx::query_scalar!(
            r#"WITH r AS (INSERT INTO ops.trade_request (trade, month, requests)
                 VALUES ($1, date_trunc('month', $3 AT TIME ZONE 'America/Phoenix')::date, $2)
                 ON CONFLICT (trade, month) DO UPDATE SET requests = EXCLUDED.requests)
               , c AS (INSERT INTO ops.coverage (scope, code, active, counted_at)
                 SELECT 'trade', $1, 0, $3 WHERE $2 > 0
                 ON CONFLICT (scope, code) DO NOTHING RETURNING id)
               SELECT id AS "id!" FROM c
               UNION ALL SELECT id FROM ops.coverage WHERE scope = 'trade' AND code = $1
                 AND active < 2 AND $2 > 0"#,
            trade,
            i32::try_from(*requests).map_err(|_| OpsError::Database)?,
            at.to_sqlx() as _,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| OpsError::Database)?;
        if let Some(id) = low {
            raised.extend(raise_coverage(&mut tx, id).await?);
        }
    }
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(raised)
}

/// OPS-006: one open coverage alert per `ops.coverage` row.
async fn raise_coverage(tx: &mut PgConnection, id: Uuid) -> Result<Option<Uuid>, OpsError> {
    raise_alert_once(tx, COVERAGE_ALERT, Severity::Normal, (COVERAGE_SUBJECT, id)).await
}

/// Takes the transaction-scoped advisory lock named `key` (namespace 21, as `api`'s locks).
async fn advisory_lock(db: &mut PgConnection, key: &str) -> Result<(), OpsError> {
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended($1::text, 21))) AS lock"#,
        key,
    )
    .fetch_one(db)
    .await
    .map(|_| ())
    .map_err(|_| OpsError::Database)
}

/// OPS-004, OPS-023: the alert kind of Stripe or webhook errors over 30 minutes (raised by T518).
pub const VENDOR_ALERT: &str = "ops.vendor_errors";
/// OPS-004, OPS-029: the alert kind of a health check failing twice (raised by T518).
pub const HEALTH_ALERT: &str = "ops.health_check_failed";
/// FEE-030, OPS-029: `payments`' nightly ledger difference (`payments::api::MISMATCH_ALERT`).
pub const LEDGER_ALERT: &str = "payments.ledger_mismatch";
/// OPS-052: a database volume above 85% full (raised by T525); listed in the daytime push only.
pub const VOLUME_HIGH_ALERT: &str = "ops.volume_high";
/// OPS-004, D-381: email use at 70% of the daily or 80% of the monthly limit, once a day.
pub const EMAIL_LIMIT_ALERT: &str = "ops.email_limit";
/// OPS-029: a Twilio number suspension notice.
pub const NUMBER_SUSPENDED_ALERT: &str = "messaging.number_suspended";
/// OPS-032, TXT-024: Twilio's notice that a suspended number is restored.
pub const NUMBER_RESTORED_ALERT: &str = "messaging.number_restored";
/// INT-023, TXT-015 (D-1378): `messaging`'s failed-text call-back alert; the daytime push lists
/// only its High (urgent) rows, never the Normal (no-rush) ones.
pub const FAILED_TEXT_CALL_BACK_ALERT: &str = "messaging.call_back_due";
/// OPS-023, D-220: how long Stripe and webhook errors must stay clear before their alert clears.
pub const VENDOR_QUIET: SignedDuration = SignedDuration::from_hours(2);
/// OPS-029, D-3877: a health alert is raised at the second failed check (about 5 minutes in), so
/// one still open 10 minutes later is a check failing 15 minutes after its first failure.
pub const HEALTH_PUSH_AFTER: SignedDuration = SignedDuration::from_mins(10);
/// OPS-029: two ledger alerts this far apart are two nightly checks, not one run's.
const LEDGER_NIGHTS_APART: SignedDuration = SignedDuration::from_hours(12);
/// OPS-029, SEC-047 (D-3881): the kinds that push as soon as they are open, besides any critical
/// alert, a [`HEALTH_ALERT`] after [`HEALTH_PUSH_AFTER`] and a [`LEDGER_ALERT`] on its second night.
pub const PUSH_AT_ONCE: &[&str] = &[
    NUMBER_SUSPENDED_ALERT,
    EMAIL_LIMIT_ALERT,
    "privacy.vault_read_spike",
    "privacy.vault_read_unregistered",
];
/// OPS-032, OPS-005, LIC-039, OPS-052, INT-023: the open alerts the 9 a.m. and 3 p.m. push lists.
pub const DAYTIME_KINDS: &[&str] = &[
    "intake.call_back_urgent",
    "businesses.complaint_safety",
    "jobs.problem_report_safety",
    "licensing.list_still_failing",
    NUMBER_RESTORED_ALERT,
    VOLUME_HIGH_ALERT,
];
/// The kinds an OPS-023 clearing acknowledges ([`acknowledge_cleared`]).
const CLEARING_KINDS: &[&str] = &[VENDOR_ALERT, HEALTH_ALERT, LEDGER_ALERT];
/// OPS-032: how late a 9 a.m. or 3 p.m. push may still go out; a slot older than this is skipped.
const PUSH_SLOT_GRACE: SignedDuration = SignedDuration::from_hours(1);
/// Arizona's fixed offset from UTC (no daylight saving).
const ARIZONA: SignedDuration = SignedDuration::from_hours(-7);

/// OPS-023, D-220: whether Stripe and webhook errors, last seen at `last_error`, have been clear
/// for [`VENDOR_QUIET`] at `now`; the caller then clears [`VENDOR_ALERT`], never [`LEDGER_ALERT`].
#[must_use]
pub fn vendor_quiet(last_error: Timestamp, now: Timestamp) -> bool {
    now.duration_since(last_error) >= VENDOR_QUIET
}

/// OPS-023, D-220, D-3877: acknowledges every open alert of `kind` at `at` when its symptom
/// cleared (a [`VENDOR_ALERT`] after [`vendor_quiet`], a [`HEALTH_ALERT`] when the check passes,
/// a [`LEDGER_ALERT`] only when the next nightly reconcile matched): each is resolved with an
/// `ops.alert_ack` row by the system and an audit row, in one transaction, and listed in the
/// digest. Any other kind is never cleared this way and nothing is written. Returns the IDs.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn acknowledge_cleared(
    db: &mut PgConnection,
    kind: &str,
    at: Timestamp,
) -> Result<Vec<Uuid>, OpsError> {
    if !CLEARING_KINDS.contains(&kind) {
        return Ok(Vec::new());
    }
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    let ids = sqlx::query_scalar!(
        "SELECT id FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL ORDER BY raised_at \
         FOR UPDATE",
        kind,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    for id in &ids {
        acknowledge_one(&mut tx, *id, at).await?;
    }
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(ids)
}

/// One open alert to push or list: its ID, kind, severity and subject ID, never text (PRIV-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushAlert {
    /// The alert's ID.
    pub id: Uuid,
    /// The alert kind, a code name.
    pub kind: String,
    /// `critical`, `high` or `normal`.
    pub severity: String,
    /// The record it is about, by ID, if any (a job, a business).
    pub subject_id: Option<Uuid>,
}

/// OPS-029, OPS-035, D-218: the open alerts not yet pushed that push to Casey's phone at `now`:
/// every critical safety alert not acknowledged (OPS-028), a [`HEALTH_ALERT`] open
/// [`HEALTH_PUSH_AFTER`] after it was raised, a [`LEDGER_ALERT`] still there at the next nightly
/// check (the newest open one, raised a night after another open one, and only while no open one
/// has pushed: one push per difference, D-218), and each [`PUSH_AT_ONCE`] kind; nothing else
/// pushes, everything else waits for the portal and the digest. Oldest first.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn alerts_to_push(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Vec<PushAlert>, OpsError> {
    let health = now.saturating_sub(HEALTH_PUSH_AFTER).unwrap_or(now);
    let kinds: Vec<String> = PUSH_AT_ONCE.iter().map(|k| (*k).to_owned()).collect();
    sqlx::query_as!(
        PushAlert,
        r#"SELECT a.id, a.kind, a.severity, a.subject_id FROM ops.alert a
           WHERE a.resolved_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM ops.alert_push p WHERE p.alert_id = a.id)
             AND (a.severity = 'critical' OR a.kind = ANY($1)
               OR (a.kind = $2 AND a.raised_at <= $3)
               OR (a.kind = $4 AND EXISTS (SELECT 1 FROM ops.alert b WHERE b.kind = $4
                 AND b.resolved_at IS NULL
                 AND extract(epoch FROM a.raised_at - b.raised_at)::float8 >= $5::float8)
                 AND NOT EXISTS (SELECT 1 FROM ops.alert c WHERE c.kind = $4
                   AND c.resolved_at IS NULL AND (c.raised_at > a.raised_at
                     OR EXISTS (SELECT 1 FROM ops.alert_push q WHERE q.alert_id = c.id)))))
           ORDER BY a.raised_at, a.id"#,
        &kinds,
        HEALTH_ALERT,
        health.to_sqlx() as _,
        LEDGER_ALERT,
        LEDGER_NIGHTS_APART.as_secs_f64(),
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-001, OPS-035: records that alert `id` was pushed (emailed to the alert address) at `at`;
/// T487's safe-mode clock starts from this row. A repeat keeps the first time.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn record_push(db: &mut PgConnection, id: Uuid, at: Timestamp) -> Result<(), OpsError> {
    sqlx::query!(
        "INSERT INTO ops.alert_push (alert_id, pushed_at) VALUES ($1, $2) \
         ON CONFLICT (alert_id) DO NOTHING",
        id,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map(drop)
    .map_err(|_| OpsError::Database)
}

/// OPS-032: the latest 9 a.m. or 3 p.m. Arizona push slot at or before `now`.
#[must_use]
pub fn push_slot(now: Timestamp) -> Timestamp {
    let local = now.as_second() + ARIZONA.as_secs();
    let day = local.div_euclid(86_400) * 86_400;
    let into = local - day;
    let slot = match into {
        i if i >= 15 * 3_600 => day + 15 * 3_600,
        i if i >= 9 * 3_600 => day + 9 * 3_600,
        _ => day - 9 * 3_600,
    };
    Timestamp::from_second(slot - ARIZONA.as_secs()).unwrap_or(now)
}

/// One OPS-032 item that is not an open alert (a help request due within 24 hours or held back by
/// the OPS-036 daily limit): kind and subject by code name and ID only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushItem {
    /// The item's ID, which Casey's done mark names.
    pub id: Uuid,
    /// The item kind, a code name.
    pub kind: String,
    /// The kind of record it is about.
    pub subject_kind: String,
    /// The record's ID.
    pub subject_id: Uuid,
}

/// One 9 a.m. or 3 p.m. push: its slot and what it lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaytimePush {
    /// The slot this push is for.
    pub slot: Timestamp,
    /// The open [`DAYTIME_KINDS`] alerts raised by the slot.
    pub alerts: Vec<PushAlert>,
    /// The open push items shown from the slot or earlier.
    pub items: Vec<PushItem>,
}

/// OPS-032, D-277, D-492: the push due for `now`'s slot ([`push_slot`]), unless that slot was
/// handled or is over an hour old (a missed 3 p.m. push is never sent at night; the
/// next slot lists its items): every open [`DAYTIME_KINDS`] alert raised at or before the slot and every push item
/// not marked done with `show_from` at or before it, so each stays listed until Casey marks it
/// done and one raised after the slot waits for the next. With nothing open the slot is recorded
/// as handled and `None` is returned: nothing is sent.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn daytime_push_due(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Option<DaytimePush>, OpsError> {
    let slot = push_slot(now);
    if now.duration_since(slot) > PUSH_SLOT_GRACE {
        return Ok(None);
    }
    let handled = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM ops.push_slot WHERE slot = $1) AS "handled!""#,
        slot.to_sqlx() as _,
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| OpsError::Database)?;
    if handled {
        return Ok(None);
    }
    let push = DaytimePush {
        slot,
        alerts: daytime_alerts(db, slot).await?,
        items: open_items(db, slot).await?,
    };
    if push.alerts.is_empty() && push.items.is_empty() {
        record_daytime_push(db, &push, now).await?;
        return Ok(None);
    }
    Ok(Some(push))
}

/// OPS-032, INT-023: the open [`DAYTIME_KINDS`] alerts and High [`FAILED_TEXT_CALL_BACK_ALERT`]
/// alerts raised at or before `slot`, oldest first.
async fn daytime_alerts(
    db: &mut PgConnection,
    slot: Timestamp,
) -> Result<Vec<PushAlert>, OpsError> {
    let kinds: Vec<String> = DAYTIME_KINDS.iter().map(|k| (*k).to_owned()).collect();
    sqlx::query_as!(
        PushAlert,
        "SELECT id, kind, severity, subject_id FROM ops.alert \
         WHERE resolved_at IS NULL AND raised_at <= $2 \
         AND (kind = ANY($1) OR (kind = $3 AND severity = 'high')) \
         ORDER BY raised_at, id",
        &kinds,
        slot.to_sqlx() as _,
        FAILED_TEXT_CALL_BACK_ALERT,
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// The push items not marked done and shown from `slot` or earlier, oldest first.
async fn open_items(db: &mut PgConnection, slot: Timestamp) -> Result<Vec<PushItem>, OpsError> {
    sqlx::query_as!(
        PushItem,
        "SELECT id, kind, subject_kind, subject_id FROM ops.push_item \
         WHERE done_at IS NULL AND show_from <= $1 ORDER BY show_from, id",
        slot.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-032, OPS-001, OPS-052: records `push`'s slot as handled at `at` and, for each listed alert
/// but a [`VOLUME_HIGH_ALERT`] (which never starts safe mode), its push time, in one transaction.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn record_daytime_push(
    db: &mut PgConnection,
    push: &DaytimePush,
    at: Timestamp,
) -> Result<(), OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    let count = push.alerts.len() + push.items.len();
    sqlx::query!(
        "INSERT INTO ops.push_slot (slot, items, handled_at) VALUES ($1, $2, $3) \
         ON CONFLICT (slot) DO NOTHING",
        push.slot.to_sqlx() as _,
        i32::try_from(count).unwrap_or(i32::MAX),
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    for alert in push.alerts.iter().filter(|a| a.kind != VOLUME_HIGH_ALERT) {
        record_push(&mut tx, alert.id, at).await?;
    }
    tx.commit().await.map_err(|_| OpsError::Database)
}

/// OPS-032, D-4044: adds one push item of `kind` about `subject`, listed from `show_from` until
/// Casey marks it done; at most one open item per kind and subject (a repeat returns `None`).
///
/// # Errors
///
/// [`OpsError::NotACodeName`] (nothing is written), or [`OpsError::Database`].
pub async fn add_push_item(
    db: &mut PgConnection,
    kind: &'static str,
    subject: (&'static str, Uuid),
    show_from: Timestamp,
) -> Result<Option<Uuid>, OpsError> {
    if !is_code_name(kind) {
        return Err(OpsError::NotACodeName("kind"));
    }
    if !is_code_name(subject.0) {
        return Err(OpsError::NotACodeName("subject_kind"));
    }
    sqlx::query_scalar!(
        "INSERT INTO ops.push_item (kind, subject_kind, subject_id, show_from) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (kind, subject_kind, subject_id) WHERE done_at IS NULL DO NOTHING RETURNING id",
        kind,
        subject.0,
        subject.1,
        show_from.to_sqlx() as _,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-032: Casey's done mark on push item `id` at `at`; returns false for an item already done
/// or unknown, so a repeat is safe.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn mark_push_item_done(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<bool, OpsError> {
    sqlx::query_scalar!(
        "UPDATE ops.push_item SET done_at = $2 WHERE id = $1 AND done_at IS NULL RETURNING id",
        id,
        at.to_sqlx() as _,
    )
    .fetch_optional(db)
    .await
    .map(|row| row.is_some())
    .map_err(|_| OpsError::Database)
}

/// The audit action of a change to the owner's away date (OPS-026).
pub const AWAY_SET_ACTION: &str = "ops.away_set";

/// OPS-026, D-207: stores the date Casey is away until (`None` when he is back) at `at`, with one
/// audit row by `owner`, in one transaction; it changes no timer. T483 shows it to businesses and
/// in the HELP reply.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn set_away_until(
    db: &mut PgConnection,
    until: Option<Date>,
    owner: Uuid,
    at: Timestamp,
) -> Result<(), OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    sqlx::query!(
        "UPDATE ops.owner_away SET away_until = $1::text::date, set_at = $2",
        until.map(|d| d.to_string()),
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: AWAY_SET_ACTION,
        subject_kind: "ops.owner_away",
        subject_id: Uuid::nil(),
        change: Change::Field("away_until"),
    };
    audit(&mut tx, &entry).await?;
    tx.commit().await.map_err(|_| OpsError::Database)
}

/// OPS-026: the date Casey is away until, if one is set.
///
/// # Errors
///
/// [`OpsError::Database`] (also for a stored date that does not read back).
pub async fn away_until(db: &mut PgConnection) -> Result<Option<Date>, OpsError> {
    let stored = sqlx::query_scalar!("SELECT away_until::text FROM ops.owner_away")
        .fetch_optional(db)
        .await
        .map_err(|_| OpsError::Database)?
        .flatten();
    stored
        .map(|d| d.parse::<Date>().map_err(|_| OpsError::Database))
        .transpose()
}
