//! The daily database-volume check (OPS-052, T525, D-492, D-3880, D-4044, D-4280).
//!
//! [`check_volume`] reads `pg_database_size(current_database())` and compares it with the
//! volume's size in bytes (the setting `ASKHAVA_DB_VOLUME_BYTES`, passed in by `server`'s
//! `watch_checks`, which owns the schedule). The boundary is the pure [`volume_level`]: below 70%
//! nothing, from 70% a Low alert ([`VOLUME_LOW_ALERT`], stored `normal`, shown in the owner portal
//! and the digest), from 85% a High alert (`alerts`' `VOLUME_HIGH_ALERT`, listed in the next
//! OPS-032 daytime push through `DAYTIME_KINDS` and kept out of safe mode). Each level raises only
//! its own kind, through `raise_kind_once`, so a level that lasts leaves one open alert. Neither
//! starts safe mode: safe mode reads only pushed High alerts and skips `ops.volume_high`.
//!
//! T779 (D-5081, F34): each run also resolves, as the system actor, the open volume alerts whose
//! level no longer holds ([`resolve_kind`]: below 70% both, below 85% the High one), so the next
//! breach raises a fresh alert. A resolution is written as `alerts`' private `acknowledge_one`
//! does: `resolved_at`, a `system` `ops.alert_ack` row and an `ops.alert_acknowledged` audit row,
//! in one transaction under `raise_kind_once`'s per-kind lock.

use sqlx::{Connection, PgConnection};

use crate::alerts::VOLUME_HIGH_ALERT;
use crate::api::{ALERT_ACKNOWLEDGED_ACTION, audit};
use crate::models::{Actor, AuditEntry, Change, OpsError, Severity};
use crate::service::raise_kind_once;

/// OPS-052: the Low alert kind, a database at 70% or more of its volume (stored `normal`).
pub const VOLUME_LOW_ALERT: &str = "ops.volume_low";

/// OPS-052: how full the database volume is, as the check sorts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeLevel {
    /// Below 70%: nothing is raised.
    Fine,
    /// 70% or more, below 85%: a Low alert.
    Low,
    /// 85% or more: a High alert.
    High,
}

/// OPS-052, TD-OPS-09: sorts `size` bytes on a `volume` of bytes: 69% is [`VolumeLevel::Fine`],
/// 70% [`VolumeLevel::Low`], 85% [`VolumeLevel::High`]. Exact integer arithmetic (no rounding up);
/// a volume of 0 is full.
#[must_use]
pub fn volume_level(size: u64, volume: u64) -> VolumeLevel {
    let (size, volume) = (u128::from(size) * 100, u128::from(volume));
    if size >= volume * 85 {
        VolumeLevel::High
    } else if size >= volume * 70 {
        VolumeLevel::Low
    } else {
        VolumeLevel::Fine
    }
}

/// OPS-052, D-5081: compares this database's size with `volume` bytes and raises the level's
/// alert once while it lasts (Low as `normal` [`VOLUME_LOW_ALERT`], High as `high`
/// `ops.volume_high`), resolving the ones that no longer hold: below 70% both, below 85% the High
/// one; at 85% or more an open Low one stays beside the High one. Returns the level.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn check_volume(db: &mut PgConnection, volume: u64) -> Result<VolumeLevel, OpsError> {
    let size = sqlx::query_scalar!(r#"SELECT pg_database_size(current_database()) AS "size!""#)
        .fetch_one(&mut *db)
        .await
        .map_err(|_| OpsError::Database)?;
    let level = volume_level(u64::try_from(size).unwrap_or(0), volume);
    match level {
        VolumeLevel::Fine => {
            resolve_kind(db, VOLUME_HIGH_ALERT).await?;
            resolve_kind(db, VOLUME_LOW_ALERT).await?;
        }
        VolumeLevel::Low => {
            resolve_kind(db, VOLUME_HIGH_ALERT).await?;
            raise_kind_once(db, VOLUME_LOW_ALERT, Severity::Normal).await?;
        }
        VolumeLevel::High => {
            raise_kind_once(db, VOLUME_HIGH_ALERT, Severity::High).await?;
        }
    }
    Ok(level)
}

/// OPS-052, D-5081: resolves every open unscoped alert of `kind` as the system, in one
/// transaction under the kind's `raise_kind_once` lock: its `resolved_at`, a `system`
/// `ops.alert_ack` row and one [`ALERT_ACKNOWLEDGED_ACTION`] audit row each. Returns how many.
async fn resolve_kind(db: &mut PgConnection, kind: &'static str) -> Result<usize, OpsError> {
    let mut tx = db.begin().await.map_err(|_| OpsError::Database)?;
    let key = format!("ops.alert_kind:{kind}");
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended($1::text, 21))) AS lock"#,
        key,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    let done = sqlx::query_scalar!(
        "WITH done AS (UPDATE ops.alert SET resolved_at = now() WHERE kind = $1 \
           AND subject_kind IS NULL AND resolved_at IS NULL RETURNING id) \
         INSERT INTO ops.alert_ack (alert_id, acknowledged_by, at) \
         SELECT id, 'system', now() FROM done RETURNING alert_id",
        kind,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| OpsError::Database)?;
    for id in &done {
        let entry = AuditEntry {
            actor: Actor::System,
            action: ALERT_ACKNOWLEDGED_ACTION,
            subject_kind: "ops.alert",
            subject_id: *id,
            change: Change::Action,
        };
        audit(&mut tx, &entry).await?;
    }
    tx.commit().await.map_err(|_| OpsError::Database)?;
    Ok(done.len())
}
