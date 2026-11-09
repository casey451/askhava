//! The WEB-007 site status banner: hidden by default, turned on automatically by an emergency
//! message while homeowner texting is off or suspended, and cleared only by Casey (TXT-024,
//! WEB-007; D-144, D-1843, D-2185).
//!
//! `ops.status_banner` is append-only history (migration `0170_ops_status_banner.sql`): each change
//! inserts a row naming who or what made it, and the banner's state is the latest row. A change
//! takes a transaction-scoped advisory lock first, so two concurrent changes queue and a repeat
//! writes nothing. T101's base layout reads [`banner_shown`] through `ops::api`; T108's owner portal
//! calls [`clear_banner`] (and [`show_banner`] for Casey's own switch).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{OpsError, is_code_name};

/// Who changed the banner: the system for an emergency (with the record that caused it), or Casey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerBy {
    /// Turned on by the system for the record `(kind, id)`, such as a `messaging.inbox` message.
    System(&'static str, Uuid),
    /// Casey, by his owner record ID.
    Owner(Uuid),
}

/// WEB-007: whether the site status banner is shown now (a banner never set is hidden).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn banner_shown(db: &mut PgConnection) -> Result<bool, OpsError> {
    let shown = sqlx::query_scalar!("SELECT shown FROM ops.status_banner ORDER BY id DESC LIMIT 1")
        .fetch_optional(db)
        .await
        .map_err(|_| OpsError::Database)?;
    Ok(shown.unwrap_or(false))
}

/// TXT-024, WEB-007, D-144: turns the banner on, by the system for an emergency or by Casey; true
/// when this call turned it on, false (nothing written) when it was already on.
///
/// # Errors
///
/// [`OpsError::NotACodeName`] for a cause kind that is not a code name, [`OpsError::Database`].
pub async fn show_banner(
    db: &mut PgConnection,
    by: BannerBy,
    now: Timestamp,
) -> Result<bool, OpsError> {
    if let BannerBy::System(kind, _) = by
        && !is_code_name(kind)
    {
        return Err(OpsError::NotACodeName("cause_kind"));
    }
    change(db, true, by, now).await
}

/// TXT-024, D-144, D-1843: Casey clears the banner; nothing else can (the table's check refuses a
/// hidden row by any other actor). True when this call hid it, false when it was already hidden.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn clear_banner(
    db: &mut PgConnection,
    owner: Uuid,
    now: Timestamp,
) -> Result<bool, OpsError> {
    change(db, false, BannerBy::Owner(owner), now).await
}

/// Inserts a row setting the banner to `shown` unless it already is, in a transaction (a savepoint
/// inside the caller's) under the banner's advisory lock, held to the end of the outermost one.
async fn change(
    db: &mut PgConnection,
    shown: bool,
    by: BannerBy,
    now: Timestamp,
) -> Result<bool, OpsError> {
    let (actor_kind, actor_id, cause_kind, cause_id) = match by {
        BannerBy::System(kind, id) => ("system", None, Some(kind), Some(id)),
        BannerBy::Owner(id) => ("owner", Some(id), None, None),
    };
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended('ops.status_banner', 0))) AS lock"#
    )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| OpsError::Database)?;
    let row = sqlx::query_scalar!(
        "INSERT INTO ops.status_banner (shown, actor_kind, actor_id, cause_kind, cause_id, \
         changed_at) SELECT $1, $2, $3, $4, $5, $6 WHERE $1 IS DISTINCT FROM COALESCE((SELECT \
         b.shown FROM ops.status_banner b ORDER BY b.id DESC LIMIT 1), false) RETURNING id",
        shown,
        actor_kind,
        actor_id,
        cause_kind,
        cause_id,
        now.to_sqlx() as _
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(row.is_some())
}
