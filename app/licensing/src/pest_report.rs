//! A homeowner's wrong-license report pauses a pest business's pest jobs until it sends a license
//! number the homeowner or Casey can find on the Department's search (T389, D-3055; LIC-047, D-417).
//!
//! [`report_pest_license_wrong`] sets `paused_at` on the business's `pest_management` row in
//! `licensing.business_license`, writes one `licensing.pest_license_paused` audit row (field
//! `paused_at`) and raises one `licensing.pest_license_reported` owner-inbox alert whose subject is
//! the business ID, so Casey can check; [`replace_pest_number`] stores the found number under
//! [`PestAttestation::new`]'s rule, clears `paused_at` and writes one
//! `licensing.pest_number_replaced` audit row (field `license_number`). Each runs in one
//! transaction (JOB-005, R11); no audit row, alert or error carries the number or a homeowner
//! detail (D-2742, PRIV-028). `holder_for` reads a paused row as `reported_wrong`, so the rules give
//! `NoCard(PestPaused)` for every pest job type until the replacement.

use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, Postgres, Transaction};

use crate::holder::HolderError;
use crate::models::PestAttestation;

/// The owner-inbox alert a wrong-license report raises; its subject is the business (D-3055).
pub const PEST_LICENSE_REPORTED_ALERT: &str = "licensing.pest_license_reported";

/// What a wrong-license report did (LIC-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PestReport {
    /// The pest row is now paused; one audit row and one alert were written.
    Paused,
    /// The pest row was already paused; nothing changed and no alert was raised.
    AlreadyPaused,
    /// The business has no pest row; nothing changed and no alert was raised.
    NoPestLicense,
}

/// What a replacement number did (LIC-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PestReplacement {
    /// The number was stored and the pause cleared; one audit row was written.
    Replaced,
    /// The pest row is not paused: a number changes only after a report; nothing changed.
    NotPaused,
    /// The business has no pest row; nothing changed.
    NoPestLicense,
}

/// LIC-047, D-3055: a homeowner reported `business`'s pest license wrong; pauses its pest jobs from
/// `at` with one audit row and one owner-inbox alert, in one transaction. The actor is
/// `Actor::System`: no homeowner detail is recorded.
///
/// # Errors
///
/// [`HolderError::Audit`] when the audit row or alert fails (nothing changes); `Database`.
pub async fn report_pest_license_wrong(
    conn: &mut PgConnection,
    business: Uuid,
    at: Timestamp,
) -> Result<PestReport, HolderError> {
    let mut tx = conn.begin().await.map_err(|_| HolderError::Database)?;
    let Some((id, paused)) = pest_row(&mut tx, business).await? else {
        return Ok(PestReport::NoPestLicense);
    };
    if paused {
        return Ok(PestReport::AlreadyPaused);
    }
    sqlx::query!(
        "UPDATE licensing.business_license SET paused_at = $2 WHERE id = $1",
        id,
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| HolderError::Database)?;
    log(&mut tx, "licensing.pest_license_paused", id, "paused_at").await?;
    let alert = NewAlert {
        kind: PEST_LICENSE_REPORTED_ALERT,
        severity: Severity::Normal,
        subject: Some(("businesses.business", business)),
    };
    raise_alert(&mut tx, &alert)
        .await
        .map_err(|_| HolderError::Audit)?;
    tx.commit().await.map_err(|_| HolderError::Database)?;
    Ok(PestReport::Paused)
}

/// LIC-047, D-3055: once the homeowner or Casey found `number` on the Department's search, stores
/// it on `business`'s paused pest row (its record categories kept) and clears the pause, with one
/// audit row naming the field, never the number, in one transaction; `recorded_at` becomes `at`,
/// when the current number was recorded.
///
/// # Errors
///
/// [`HolderError::BadRecord`] for a number [`PestAttestation::new`] refuses (nothing changes, the
/// row stays paused); `Audit`; `Database`.
pub async fn replace_pest_number(
    conn: &mut PgConnection,
    business: Uuid,
    number: &str,
    at: Timestamp,
) -> Result<PestReplacement, HolderError> {
    let checked =
        PestAttestation::new(true, number, false, false).map_err(|_| HolderError::BadRecord)?;
    let mut tx = conn.begin().await.map_err(|_| HolderError::Database)?;
    let Some((id, paused)) = pest_row(&mut tx, business).await? else {
        return Ok(PestReplacement::NoPestLicense);
    };
    if !paused {
        return Ok(PestReplacement::NotPaused);
    }
    sqlx::query!(
        "UPDATE licensing.business_license \
         SET license_number = $2, recorded_at = $3, paused_at = NULL WHERE id = $1",
        id,
        checked.number(),
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| HolderError::Database)?;
    log(
        &mut tx,
        "licensing.pest_number_replaced",
        id,
        "license_number",
    )
    .await?;
    tx.commit().await.map_err(|_| HolderError::Database)?;
    Ok(PestReplacement::Replaced)
}

/// The business's pest row, locked for the change: its ID and whether it is paused.
async fn pest_row(
    tx: &mut Transaction<'_, Postgres>,
    business: Uuid,
) -> Result<Option<(Uuid, bool)>, HolderError> {
    let row = sqlx::query!(
        r#"SELECT id, paused_at IS NOT NULL AS "paused!" FROM licensing.business_license
           WHERE business_id = $1 AND agency = 'pest_management' FOR UPDATE"#,
        business,
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| HolderError::Database)?;
    Ok(row.map(|r| (r.id, r.paused)))
}

/// One audit row on the pest row, the changed field by name only (D-2742, PRIV-028).
async fn log(
    tx: &mut Transaction<'_, Postgres>,
    action: &'static str,
    id: Uuid,
    field: &'static str,
) -> Result<(), HolderError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "licensing.business_license",
        subject_id: id,
        change: Change::Field(field),
    };
    audit(tx, &entry).await.map_err(|_| HolderError::Audit)?;
    Ok(())
}
