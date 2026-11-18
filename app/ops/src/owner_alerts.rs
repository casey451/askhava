//! The owner portal's pushed-alert list and Casey's acknowledgement (OPS-001, OPS-035; D-3882,
//! D-3890, T108). [`pushed_alerts`] reads T105's push facts (`ops.alert_push`, `ops.alert_ack`,
//! migration 0192): every alert that pushed to Casey's phone, newest push first; [`alert_subject`]
//! gives one alert's kind, severity and subject (T484's call handlers read it). Neither returns a
//! contact detail: IDs, code names and times only (PRIV-006). [`acknowledge_pushed_alert`] is the
//! safe-mode acknowledgement T310 serves: an open pushed alert, other than a text-delivery alert
//! (only the delivery path acknowledges those, OPS-025), is resolved with its `casey` ack row and
//! an [`OWNER_ACK_ACTION`] audit row in one transaction, which ends safe mode's clock for it.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::alerts::DELIVERY_ALERT;
use crate::models::{Actor, AuditEntry, Change, OpsError};

/// The audit action of an alert Casey acknowledged in the owner portal (OPS-001).
pub const OWNER_ACK_ACTION: &str = "ops.alert_owner_acknowledged";

/// One alert as Casey sees it: kind, severity and subject by code name and ID (PRIV-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertSubject {
    /// The alert kind, a code name.
    pub kind: String,
    /// `critical`, `high` or `normal`.
    pub severity: String,
    /// The kind of record it is about, a code name, if any.
    pub subject_kind: Option<String>,
    /// That record's ID, if any.
    pub subject_id: Option<Uuid>,
}

/// One pushed alert with its times (OPS-035).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushedAlert {
    /// The alert's ID.
    pub id: Uuid,
    /// Its kind, severity and subject.
    pub subject: AlertSubject,
    /// When it was raised.
    pub raised_at: Timestamp,
    /// When it was pushed to Casey's phone.
    pub pushed_at: Timestamp,
    /// When it was acknowledged or resolved, if it was: its `ops.alert_ack` time, else its
    /// `resolved_at` (a safety call answered through `acknowledge_alert` writes no ack row).
    pub acknowledged_at: Option<Timestamp>,
}

/// OPS-035, D-3882, D-3890, D-4283: every alert with an `ops.alert_push` row, newest push first;
/// one resolved without an ack row counts as acknowledged at its `resolved_at`.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn pushed_alerts(db: &mut PgConnection) -> Result<Vec<PushedAlert>, OpsError> {
    let rows = sqlx::query!(
        r#"SELECT a.id, a.kind, a.severity, a.subject_kind, a.subject_id, a.raised_at,
             p.pushed_at, coalesce(k.at, a.resolved_at) AS "acknowledged_at?"
           FROM ops.alert a JOIN ops.alert_push p ON p.alert_id = a.id
           LEFT JOIN ops.alert_ack k ON k.alert_id = a.id
           ORDER BY p.pushed_at DESC, a.id"#
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| PushedAlert {
            id: r.id,
            subject: AlertSubject {
                kind: r.kind,
                severity: r.severity,
                subject_kind: r.subject_kind,
                subject_id: r.subject_id,
            },
            raised_at: r.raised_at.to_jiff(),
            pushed_at: r.pushed_at.to_jiff(),
            acknowledged_at: r.acknowledged_at.map(|t| t.to_jiff()),
        })
        .collect())
}

/// D-3890: the kind, severity and subject of alert `alert`, or `None` for an unknown ID.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn alert_subject(
    db: &mut PgConnection,
    alert: Uuid,
) -> Result<Option<AlertSubject>, OpsError> {
    sqlx::query_as!(
        AlertSubject,
        "SELECT kind, severity, subject_kind, subject_id FROM ops.alert WHERE id = $1",
        alert,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-001, OPS-023: Casey (`owner`) acknowledges the open pushed alert `alert` at `at`: its
/// `resolved_at`, a `casey` `ops.alert_ack` row and one [`OWNER_ACK_ACTION`] audit row commit
/// together. Returns false, writing nothing, for an unknown, unpushed, resolved or text-delivery
/// alert, so a second press is safe.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn acknowledge_pushed_alert(
    db: &mut PgConnection,
    alert: Uuid,
    owner: Uuid,
    at: Timestamp,
) -> Result<bool, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    let done = sqlx::query_scalar!(
        "WITH done AS (UPDATE ops.alert a SET resolved_at = $2 WHERE a.id = $1 \
           AND a.resolved_at IS NULL AND a.kind <> $3 \
           AND EXISTS (SELECT 1 FROM ops.alert_push p WHERE p.alert_id = a.id) RETURNING a.id) \
         INSERT INTO ops.alert_ack (alert_id, acknowledged_by, at) \
         SELECT id, 'casey', $2 FROM done RETURNING alert_id",
        alert,
        at.to_sqlx() as _,
        DELIVERY_ALERT,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    if done.is_none() {
        return Ok(false);
    }
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: OWNER_ACK_ACTION,
        subject_kind: "ops.alert",
        subject_id: alert,
        change: Change::Action,
    };
    crate::api::audit(&mut tx, &entry).await?;
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(true)
}
