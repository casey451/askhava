//! The weekly digest's `ops` readers and writers (OPS-030, D-219, D-3882, D-3883, D-4044, D-4202):
//! alerts of given kinds raised or resolved in a window with their self-acknowledged flag, the
//! active-business count per trade `businesses` last reported, the system actor's status-change
//! audit rows of one subject kind, and the `ops.owner_digest` row (its sent, opened and reminder
//! times) with its three one-statement writers. Every result is IDs, code names, counts and times,
//! never a contact detail or free text (PRIV-006, PRIV-028). Every window is `[from, to)`.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OpsError;

/// One alert raised or resolved in a digest window (OPS-030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestAlert {
    /// The alert's ID.
    pub id: Uuid,
    /// Its kind, a code name such as `licensing.list_failing`.
    pub kind: String,
    /// The record it is about, by kind and ID, if any.
    pub subject: Option<(String, Uuid)>,
    /// When it was raised.
    pub raised_at: Timestamp,
    /// When it was resolved, if it is.
    pub resolved_at: Option<Timestamp>,
    /// True when its `ops.alert_ack` row is by the system, not Casey (D-4044).
    pub self_acknowledged: bool,
}

/// OPS-030, D-3882, D-4044: every alert of one of `kinds` raised, or resolved, at or after `from`
/// and before `to`, oldest raised first (ties by ID), with whether the system acknowledged it.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn digest_alerts(
    db: &mut PgConnection,
    kinds: &[&str],
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<DigestAlert>, OpsError> {
    let kinds: Vec<String> = kinds.iter().map(|k| (*k).to_owned()).collect();
    let rows = sqlx::query!(
        r#"SELECT a.id, a.kind, a.subject_kind, a.subject_id, a.raised_at, a.resolved_at,
             EXISTS (SELECT 1 FROM ops.alert_ack k WHERE k.alert_id = a.id
               AND k.acknowledged_by = 'system') AS "self_acknowledged!"
           FROM ops.alert a WHERE a.kind = ANY($1)
             AND ((a.raised_at >= $2 AND a.raised_at < $3)
               OR (a.resolved_at >= $2 AND a.resolved_at < $3))
           ORDER BY a.raised_at, a.id"#,
        &kinds,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| DigestAlert {
            id: r.id,
            kind: r.kind,
            subject: r.subject_kind.zip(r.subject_id),
            raised_at: r.raised_at.to_jiff(),
            resolved_at: r.resolved_at.map(|t| t.to_jiff()),
            self_acknowledged: r.self_acknowledged,
        })
        .collect())
}

/// One trade's active businesses, as `businesses` last reported them (OPS-006, OPS-030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeActive {
    /// The trade (catalog category) code.
    pub trade: String,
    /// Active businesses serving it at the last report.
    pub active: u32,
    /// When that count was reported.
    pub counted_at: Timestamp,
}

/// OPS-030, D-3882: the last reported active-business count of every trade, by trade code (the
/// `ops.coverage` trade rows `record_business_counts` keeps; towns are left out).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn trade_active_counts(db: &mut PgConnection) -> Result<Vec<TradeActive>, OpsError> {
    let rows = sqlx::query!(
        "SELECT code, active, counted_at FROM ops.coverage WHERE scope = 'trade' ORDER BY code"
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    rows.into_iter()
        .map(|r| {
            Ok(TradeActive {
                trade: r.code,
                active: u32::try_from(r.active).map_err(|_| OpsError::Database)?,
                counted_at: r.counted_at.to_jiff(),
            })
        })
        .collect()
}

/// One status-change (or field-change) audit row by the system (OPS-030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChangeRow {
    /// The changed record's ID.
    pub subject_id: Uuid,
    /// The status it left, for a status change.
    pub from_status: Option<String>,
    /// The status it entered, for a status change.
    pub to_status: Option<String>,
    /// The field changed, for a field row (such as a rule's pause over a self-pause, D-3883).
    pub field: Option<String>,
    /// When the row was written.
    pub at: Timestamp,
}

/// OPS-030, D-3883: every audit row by the system actor about `subject_kind` that records a
/// status change or a field change, written at or after `from` and before `to`, oldest first
/// (ties by row ID). Automatic activations and pauses of `businesses.business` are its status
/// rows; a rule's pause over a self-pause is its `pause_reason` field row.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn system_status_changes(
    db: &mut PgConnection,
    subject_kind: &str,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<StatusChangeRow>, OpsError> {
    let rows = sqlx::query!(
        "SELECT subject_id, from_status, to_status, field, at FROM ops.audit_log \
         WHERE actor_kind = 'system' AND subject_kind = $1 \
           AND (to_status IS NOT NULL OR field IS NOT NULL) AND at >= $2 AND at < $3 \
         ORDER BY at, id",
        subject_kind,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| StatusChangeRow {
            subject_id: r.subject_id,
            from_status: r.from_status,
            to_status: r.to_status,
            field: r.field,
            at: r.at.to_jiff(),
        })
        .collect())
}

/// One weekly digest sent to Casey: when it was sent, opened and reminded (OPS-030, D-3883).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigestRow {
    /// The `ops.owner_digest` row's ID.
    pub id: Uuid,
    /// When the digest was sent.
    pub sent_at: Timestamp,
    /// When Casey first opened it, if he has.
    pub opened_at: Option<Timestamp>,
    /// When its one reminder went, if it has.
    pub reminded_at: Option<Timestamp>,
}

/// OPS-030: the newest digest sent (by `sent_at`, ties by ID), or `None` before the first.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn latest_digest(db: &mut PgConnection) -> Result<Option<DigestRow>, OpsError> {
    let row = sqlx::query!(
        "SELECT id, sent_at, opened_at, reminded_at FROM ops.owner_digest \
         ORDER BY sent_at DESC, id DESC LIMIT 1"
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(row.map(|r| DigestRow {
        id: r.id,
        sent_at: r.sent_at.to_jiff(),
        opened_at: r.opened_at.map(|t| t.to_jiff()),
        reminded_at: r.reminded_at.map(|t| t.to_jiff()),
    }))
}

/// OPS-030: records a digest sent at `at`, in one statement. Returns the new row's ID.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn digest_sent(db: &mut PgConnection, at: Timestamp) -> Result<Uuid, OpsError> {
    sqlx::query_scalar!(
        "INSERT INTO ops.owner_digest (sent_at) VALUES ($1) RETURNING id",
        at.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-030: marks digest `id` opened at `at`, in one statement; only the first open is kept.
/// Returns true when this call set it.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn digest_opened(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<bool, OpsError> {
    let done = sqlx::query!(
        "UPDATE ops.owner_digest SET opened_at = $2 WHERE id = $1 AND opened_at IS NULL",
        id,
        at.to_sqlx()
    )
    .execute(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// OPS-030, TD-OPS-09: marks digest `id` reminded at `at`, in one statement, only while it is unopened
/// and not yet reminded, so it gets at most one reminder. Returns true when this call set it.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn digest_reminded(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<bool, OpsError> {
    let done = sqlx::query!(
        "UPDATE ops.owner_digest SET reminded_at = $2 \
         WHERE id = $1 AND reminded_at IS NULL AND opened_at IS NULL",
        id,
        at.to_sqlx()
    )
    .execute(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(done.rows_affected() == 1)
}
