//! Each business's service area: its base ZIP and how far it will travel for a job (BIZ-058,
//! BIZ-049, INT-048; D-1515, D-1518, D-3116).
//!
//! [`ServiceArea::parse`] reads the two sign-up fields once at the edge: any well-formed 5-digit
//! base ZIP and a whole number of miles from 1 to [`MAX_RADIUS_MILES`]. [`save_service_area`]
//! writes or replaces the one `businesses.service_area` row with audit rows naming the fields
//! only (PRIV-028). [`service_area`] reads it back with the base ZIP's centroid from the catalog's
//! reviewed table, or no location for a ZIP the table lacks (an out-of-state base): its radius is
//! then unused and its towns served alone decide its cards (BIZ-058). Matching by radius and the
//! portal edit are T249's.

use askhava_catalog::api::{Centroid, is_zip, zip_location};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, BusinessesError, db};

/// The smallest radius a business may give, in miles (BIZ-058).
pub const MIN_RADIUS_MILES: u8 = 1;
/// The largest radius a business may give, in miles (BIZ-058).
pub const MAX_RADIUS_MILES: u8 = 100;

/// A sign-up field the service area refuses (BIZ-058). No variant carries the typed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAreaProblem {
    /// The base ZIP is blank or not exactly 5 digits.
    BaseZip,
    /// The radius is blank, not a whole number, or outside 1 to 100 miles.
    Radius,
}

/// A checked base ZIP and radius (BIZ-058): build it with [`ServiceArea::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceArea {
    base_zip: String,
    radius_miles: u8,
}

impl ServiceArea {
    /// BIZ-058: the typed base ZIP and radius, or every problem in page order. Spaces around a
    /// value are ignored; `0`, `2.5`, `+5`, `101` and a ZIP+4 are refused.
    ///
    /// # Errors
    /// The [`ServiceAreaProblem`]s found.
    pub fn parse(base_zip: &str, radius: &str) -> Result<Self, Vec<ServiceAreaProblem>> {
        let zip = base_zip.trim();
        let radius = radius.trim();
        let miles = radius
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| radius.parse::<u8>().ok())
            .flatten()
            .filter(|m| (MIN_RADIUS_MILES..=MAX_RADIUS_MILES).contains(m));
        let mut problems = Vec::new();
        if !is_zip(zip) {
            problems.push(ServiceAreaProblem::BaseZip);
        }
        match miles {
            Some(radius_miles) if problems.is_empty() => Ok(Self {
                base_zip: zip.to_owned(),
                radius_miles,
            }),
            Some(_) => Err(problems),
            None => Err([problems, vec![ServiceAreaProblem::Radius]].concat()),
        }
    }

    /// The base ZIP, 5 digits.
    #[must_use]
    pub fn base_zip(&self) -> &str {
        &self.base_zip
    }

    /// The radius in whole miles, 1 to 100.
    #[must_use]
    pub const fn radius_miles(&self) -> u8 {
        self.radius_miles
    }
}

/// A stored service area with its base ZIP's centroid, or none (BIZ-058, INT-048).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredServiceArea {
    /// The base ZIP and radius as stored.
    pub area: ServiceArea,
    /// The base ZIP's centroid; none when the reviewed table lacks it, so the radius is unused.
    pub location: Option<Centroid>,
}

/// BIZ-058, PRIV-028: writes or replaces `business`'s base ZIP and radius, with one audit row per
/// field naming the field only, in one transaction.
///
/// # Errors
/// `UnknownBusiness`, `Database` or `Audit`.
pub async fn save_service_area(
    conn: &mut PgConnection,
    business: Uuid,
    area: &ServiceArea,
    (actor, now): (Actor, Timestamp),
) -> Result<(), BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    models::read(&mut tx, business).await?;
    db(sqlx::query!(
        "INSERT INTO businesses.service_area (business_id, base_zip, radius_miles, changed_at)
         VALUES ($1, $2, $3, $4) ON CONFLICT (business_id) DO UPDATE SET
         base_zip = EXCLUDED.base_zip, radius_miles = EXCLUDED.radius_miles,
         changed_at = EXCLUDED.changed_at",
        business,
        area.base_zip,
        i16::from(area.radius_miles),
        now.to_sqlx() as _
    )
    .execute(&mut *tx)
    .await)?;
    for field in ["base_zip", "service_radius"] {
        let entry = AuditEntry {
            actor,
            action: "businesses.field_changed",
            subject_kind: "businesses.business",
            subject_id: business,
            change: Change::Field(field),
        };
        audit(&mut tx, &entry)
            .await
            .map_err(|_| BusinessesError::Audit)?;
    }
    db(tx.commit().await)
}

/// BIZ-058: `business`'s base ZIP and radius with the base's centroid, or none before sign-up
/// asked them.
///
/// # Errors
/// `Database`; `Database` too if the compiled-in centroid table cannot be read.
pub async fn service_area(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<StoredServiceArea>, BusinessesError> {
    let row = db(sqlx::query!(
        "SELECT base_zip, radius_miles FROM businesses.service_area WHERE business_id = $1",
        business
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let radius_miles = u8::try_from(row.radius_miles).map_err(|_| BusinessesError::Database)?;
    let location = zip_location(&row.base_zip).map_err(|_| BusinessesError::Database)?;
    Ok(Some(StoredServiceArea {
        area: ServiceArea {
            base_zip: row.base_zip,
            radius_miles,
        },
        location,
    }))
}
