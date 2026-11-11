//! One business's licenses as the rules read them, and the sign-up writers for its other-agency
//! licenses (T374, D-2712; LIC-006, LIC-011, LIC-012, LIC-015, LIC-016, LIC-047, LIC-048).
//!
//! [`record_pest_attestation`] stores the LIC-047 attestation (`self_attested`) and
//! [`record_agency_license`] an inspector, alarm or septage-hauler license (`agency_record`) in
//! `licensing.business_license`, one row per business and agency (the unique index refuses a
//! second), each with one audit row that names the agency as the field, never the number
//! (PRIV-028). [`holder_for`] reads those rows, the classes on the latest ROC list for the
//! business's ROC license number ([`read_back`]) and its city-list standing ([`city_standing`]) from
//! the caller's names, and returns [`HolderParts`]: the [`Holder`] with `roc_covers` left false, set
//! per job type by [`HolderParts::for_job`] through [`ClassMap::covers`] (LIC-043). Licensing never
//! calls `businesses`; `businesses::api::holding` supplies the handyman standing (`kit/layers.json`).

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::class_map::ClassMap;
use crate::imports::ImportError;
use crate::models::PestRecordCategory::{WeedControl, WoodDestroying};
use crate::models::{Agency, CityRecord, Holder, PestAttestation};
use crate::service::{city_standing, read_back};

/// The longest agency-record license number the table keeps (D-859).
const MAX_AGENCY_NUMBER_LEN: usize = 64;

/// Why a business's license could not be recorded or read. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HolderError {
    /// The business already has a license row for this agency: the unique index refused a second.
    #[error("the business already has a license for this agency")]
    AlreadyRecorded,
    /// The license number is blank or too long, or the pest license was offered as an agency record
    /// (it is only ever self-attested, LIC-047).
    #[error("the license record is not acceptable")]
    BadRecord,
    /// The audit row could not be written; nothing is stored (JOB-005).
    #[error("the license audit row failed")]
    Audit,
    /// The database refused a statement.
    #[error("the licensing database step failed")]
    Database,
}

/// What the caller knows of the business that licensing does not store: its ROC license number,
/// its own names for the city list and whether it chose the Handyman category (D-2712). No `Debug`,
/// so the names never reach a log (PRIV-016).
#[derive(Clone, Copy, Default)]
pub struct HolderInputs<'a> {
    /// The ROC license number; `None` reads the number of the business's latest daily check.
    pub roc_license: Option<&'a str>,
    /// The business's own names for the city-list match; `None` leaves it `Pending` (LIC-011).
    pub city_names: Option<&'a CityRecord>,
    /// Whether it chose the Handyman category at sign-up (LIC-019).
    pub handyman: bool,
}

/// A business's licenses read once: its ROC classes and the [`Holder`] with `roc_covers` false.
#[derive(Debug, Clone, Default)]
pub struct HolderParts {
    /// The classes on the latest ROC list for its license number, sorted; empty when none.
    pub classes: Vec<String>,
    /// Agencies, pest attestation, handyman and city-list standing; `roc_covers` is false.
    pub holder: Holder,
}

impl HolderParts {
    /// LIC-043, CARD-001: the holder for one job type, `roc_covers` set from its classes by the
    /// license map.
    #[must_use]
    pub fn for_job(&self, map: &ClassMap, category: &str, job_type: &str) -> Holder {
        let classes: Vec<&str> = self.classes.iter().map(String::as_str).collect();
        Holder {
            roc_covers: map.covers(&classes, category, job_type),
            ..self.holder.clone()
        }
    }
}

/// LIC-047, LIC-012: stores a pest business's sign-up attestation (`self_attested`, its number and
/// the record categories it ticked) with one audit row, in one transaction.
///
/// # Errors
///
/// [`HolderError::AlreadyRecorded`] when the business already attested; `Audit`; `Database`.
pub async fn record_pest_attestation(
    conn: &mut PgConnection,
    business: Uuid,
    attestation: &PestAttestation,
    actor: Actor,
) -> Result<Uuid, HolderError> {
    let mut tx = conn.begin().await.map_err(|_| HolderError::Database)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO licensing.business_license \
         (business_id, agency, license_number, verification, wood_destroying, weed_control) \
         VALUES ($1, 'pest_management', $2, 'self_attested', $3, $4) RETURNING id",
        business,
        attestation.number(),
        attestation.holds(WoodDestroying),
        attestation.holds(WeedControl),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(insert_error)?;
    finish(tx, Agency::PestManagement, id, actor).await
}

/// LIC-015, LIC-016, LIC-013: stores an inspector, alarm or septage-hauler license from the agency
/// record (`agency_record`) with one audit row, in one transaction.
///
/// # Errors
///
/// [`HolderError::BadRecord`] for the pest agency or a blank or over-long number (nothing stored);
/// `AlreadyRecorded`; `Audit`; `Database`.
pub async fn record_agency_license(
    conn: &mut PgConnection,
    business: Uuid,
    (agency, number): (Agency, &str),
    actor: Actor,
) -> Result<Uuid, HolderError> {
    let number = number.trim();
    let bad = number.is_empty() || number.chars().count() > MAX_AGENCY_NUMBER_LEN;
    if agency == Agency::PestManagement || bad {
        return Err(HolderError::BadRecord);
    }
    let mut tx = conn.begin().await.map_err(|_| HolderError::Database)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO licensing.business_license (business_id, agency, license_number, verification) \
         VALUES ($1, $2, $3, 'agency_record') RETURNING id",
        business,
        agency.code(),
        number,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(insert_error)?;
    finish(tx, agency, id, actor).await
}

fn insert_error(e: sqlx::Error) -> HolderError {
    match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => HolderError::AlreadyRecorded,
        _ => HolderError::Database,
    }
}

/// Writes the audit row (the agency code as the field name, PRIV-028) and commits.
async fn finish(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    agency: Agency,
    id: Uuid,
    actor: Actor,
) -> Result<Uuid, HolderError> {
    let entry = AuditEntry {
        actor,
        action: "licensing.license_recorded",
        subject_kind: "licensing.business_license",
        subject_id: id,
        change: Change::Field(agency.code()),
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| HolderError::Audit)?;
    tx.commit().await.map_err(|_| HolderError::Database)?;
    Ok(id)
}

/// LIC-006, LIC-011, LIC-012, LIC-015, LIC-016, LIC-047, LIC-048: reads everything `business`
/// holds for the license rules: its agency rows and pest attestation (a reported one paused), its
/// classes on the latest ROC list for `inputs.roc_license` (or its latest daily check's number),
/// its city-list standing from `inputs.city_names` and the caller's handyman standing. No row and
/// no list read as an empty holder.
///
/// # Errors
///
/// [`HolderError::Database`] on a database failure; [`HolderError::BadRecord`] for a stored pest
/// number the attestation rule refuses.
pub async fn holder_for(
    conn: &mut PgConnection,
    business: Uuid,
    inputs: HolderInputs<'_>,
) -> Result<HolderParts, HolderError> {
    let mut holder = read_rows(conn, business).await?;
    holder.handyman = inputs.handyman;
    if let Some(names) = inputs.city_names {
        holder.city_list = city_standing(conn, names).await.map_err(read_error)?;
    }
    let number = match inputs.roc_license {
        Some(n) => Some(n.to_owned()),
        None => last_checked(conn, business).await?,
    };
    let classes = match number {
        Some(n) => match read_back(conn, &n).await {
            Ok(back) => back.map(|b| b.classes).unwrap_or_default(),
            Err(ImportError::NoList) => Vec::new(),
            Err(e) => return Err(read_error(e)),
        },
        None => Vec::new(),
    };
    Ok(HolderParts { classes, holder })
}

fn read_error(_: ImportError) -> HolderError {
    HolderError::Database
}

/// The business's `licensing.business_license` rows as a holder: agencies and pest attestation.
async fn read_rows(conn: &mut PgConnection, business: Uuid) -> Result<Holder, HolderError> {
    let rows = sqlx::query!(
        r#"SELECT agency, license_number, wood_destroying, weed_control,
                  paused_at IS NOT NULL AS "paused!"
           FROM licensing.business_license WHERE business_id = $1"#,
        business,
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| HolderError::Database)?;
    let mut holder = Holder::default();
    for row in rows {
        let agencies = Agency::ALL.into_iter().filter(|a| a.code() == row.agency);
        for agency in agencies {
            if agency == Agency::PestManagement {
                let ticks = (row.wood_destroying, row.weed_control);
                holder.pest = Some(attestation(&row.license_number, ticks, row.paused)?);
            } else {
                holder.agencies.insert(agency);
            }
        }
    }
    Ok(holder)
}

/// A stored pest row as its attestation, paused when a homeowner reported it wrong (LIC-047).
fn attestation(
    number: &str,
    (wood, weed): (bool, bool),
    paused: bool,
) -> Result<PestAttestation, HolderError> {
    let pest =
        PestAttestation::new(true, number, wood, weed).map_err(|_| HolderError::BadRecord)?;
    Ok(if paused { pest.reported_wrong() } else { pest })
}

/// The ROC license number of the business's latest daily check, if any (CARD-015).
async fn last_checked(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<String>, HolderError> {
    sqlx::query_scalar!(
        "SELECT license_number FROM licensing.license_check WHERE business_id = $1 \
         ORDER BY checked_at DESC, id DESC LIMIT 1",
        business,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| HolderError::Database)
}
