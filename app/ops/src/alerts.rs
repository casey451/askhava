//! The delivery-rate and coverage alerts and the delivery alert's automatic acknowledgement
//! (OPS-002, OPS-004, OPS-006, OPS-023, OPS-025; D-134, D-163, D-459, T105).
//!
//! `ops` reads nothing above it, so the counts are reported to it (D-491, D-493): `messaging`
//! reports each audience line's failures among its last 50 and last 200 texts
//! ([`record_delivery_counts`]), `businesses` each touched town's and trade's active businesses
//! ([`record_business_counts`]) and `jobs` each trade's requests this Arizona calendar month
//! ([`record_request_counts`]). Every alert waits for Casey; there is no backup person (OPS-002).
//! Safety reports, disputes and complaints raise their own alerts in `businesses`, `jobs` and
//! `payments` (OPS-005); safe mode and the push are T487's and T204's.
//!
//! Locks: the delivery report runs under one advisory lock (`ops.delivery`), and every coverage
//! report under one other (`ops.coverage`, [`lock_coverage`]); each is a single key, taken again
//! freely in the same transaction, so two reports in one transaction cannot deadlock on key order.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::api::{ALERT_ACKNOWLEDGED_ACTION, audit, raise_alert_once};
use crate::models::{Actor, AuditEntry, Change, NewAlert, OpsError, Severity};
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
/// delivery alert is open and, if one was ever resolved, the line reached 97% at or after that.
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
    audit(tx, &entry).await?;
    Ok(DeliveryOutcome::Acknowledged(id))
}

/// OPS-025, D-163: raises the High delivery alert for `audience` unless held since the last
/// resolved delivery alert (no recovery to 97% of 200 on this line at or after its resolution).
async fn raise_delivery(
    tx: &mut PgConnection,
    audience: Audience,
) -> Result<DeliveryOutcome, OpsError> {
    let held = sqlx::query_scalar!(
        r#"SELECT COALESCE((SELECT max(a.resolved_at) FROM ops.alert a
             JOIN ops.delivery_alert d ON d.alert_id = a.id)
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
/// reported count is under 2 active businesses. Returns the new alerts' IDs.
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
               SELECT id FROM ops.coverage WHERE scope = 'trade' AND code = $1 AND active < 2
                 AND $2 > 0"#,
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
