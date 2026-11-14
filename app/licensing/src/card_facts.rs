//! The license facts one quote card shows for one business and one job category (T393, IC-71;
//! PICK-041, LIC-024, LIC-026, LIC-034, LIC-037, LIC-038, LIC-047, D-3080, D-3642).
//!
//! [`card_facts`] reads the category's license rule from the stored catalog and, from the rows
//! `holder_for` reads, the license that makes the business eligible: for an ROC category its
//! latest daily check, the one `holder_for` reads, only when it showed Active (the number as
//! checked, the Arizona day of that check and the original-issue year the ROC list gives), for an
//! agency category its `business_license` row unless a homeowner's report paused it, as
//! `holder_for` reads a paused row as reported wrong (the day it was recorded as the check day; a
//! pest row is self-attested, so `licensed_lines` gives it no check line, LIC-047; D-3646). With
//! none of these the card is a no-license card: whether the latest city list matches the business
//! and the year of the matching entry, when it has one (LIC-038, D-3646). Read only: no table,
//! column or writer. Licensing never calls `businesses`, so the handyman tick and the insurance
//! answer of [`NoLicenseFacts`] are left `false` and `None` for `portals::quote_cards` to fill.

use askhava_catalog::api::category;
use askhava_core::api::arizona_time;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::lines::{Issuer, LicenseFacts, NoLicenseFacts};
use crate::models::{Agency, CityListStanding, CityRecord, LicenseKind};
use crate::rules::city_list_standing;

/// Why a card's license facts could not be read. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CardFactsError {
    /// The catalog or licensing read failed.
    #[error("the card license facts read failed")]
    Database,
}

/// What one card says about a business's license (PICK-041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardFacts {
    /// The licenses that make it eligible, ROC first (LIC-026, LIC-034, LIC-047).
    Licensed(Vec<LicenseFacts>),
    /// A no-license card; `handyman` and `insurance` are the caller's to fill (LIC-024, LIC-038).
    NoLicense {
        /// The card's facts; `city_since_year` is the matching city entry's year.
        facts: NoLicenseFacts,
        /// Whether the latest city list matches the business; without a match the card makes no
        /// city-license claim (D-3646).
        city_listed: bool,
    },
}

/// PICK-041, LIC-026, LIC-034, LIC-038, LIC-047, D-3642: the card facts of `business` on a job of
/// catalog `category`, or none for a category the catalog does not hold. `city_names` are the
/// business's own names for the city-list match; `None` matches nothing.
///
/// # Errors
///
/// [`CardFactsError::Database`].
pub async fn card_facts(
    conn: &mut PgConnection,
    business: Uuid,
    code: &str,
    city_names: Option<&CityRecord>,
) -> Result<Option<CardFacts>, CardFactsError> {
    let stored = category(conn, code)
        .await
        .map_err(|_| CardFactsError::Database)?;
    let Some(kind) = stored.and_then(|c| LicenseKind::parse(&c.license_kind).ok()) else {
        return Ok(None);
    };
    let mut licenses = Vec::new();
    if matches!(kind, LicenseKind::Roc | LicenseKind::RocAndStateAgency) {
        licenses.extend(roc_facts(conn, business).await?);
    }
    let agency = Agency::for_category(code);
    if let (LicenseKind::StateAgency | LicenseKind::RocAndStateAgency, Some(a)) = (kind, agency) {
        licenses.extend(agency_facts(conn, business, a).await?);
    }
    if !licenses.is_empty() {
        return Ok(Some(CardFacts::Licensed(licenses)));
    }
    let city = match city_names {
        Some(names) => city_year(conn, names).await?,
        None => None,
    };
    let facts = NoLicenseFacts {
        handyman: false,
        city_since_year: city.flatten(),
        insurance: None,
    };
    Ok(Some(CardFacts::NoLicense {
        facts,
        city_listed: city.is_some(),
    }))
}

/// LIC-026, LIC-037, D-3646: the business's latest daily check, when it showed the ROC license
/// Active, with the earliest original-issue year any ROC list gives that number; none without a
/// check or when the latest one did not show Active.
async fn roc_facts(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<LicenseFacts>, CardFactsError> {
    let row = sqlx::query!(
        r#"SELECT c.license_number, c.checked_at AS "checked_at!: jiff_sqlx::Timestamp",
                  (SELECT EXTRACT(YEAR FROM MIN(r.issued_on))::int2 FROM licensing.agency_record r
                   WHERE r.license_number = c.license_number) AS "since_year",
                  c.status = 'active' AS "active!"
           FROM licensing.license_check c WHERE c.business_id = $1
           ORDER BY c.checked_at DESC, c.id DESC LIMIT 1"#,
        business,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| CardFactsError::Database)?;
    Ok(row.filter(|r| r.active).map(|r| LicenseFacts {
        issuer: Issuer::Roc,
        number: r.license_number,
        checked_on: arizona_time(r.checked_at.to_jiff()).date(),
        since_year: r.since_year,
    }))
}

/// LIC-034, LIC-047, D-3646: the business's `agency` license row, its recorded day as the check
/// day; the row carries no issue year. A row a homeowner's report paused gives none.
async fn agency_facts(
    conn: &mut PgConnection,
    business: Uuid,
    agency: Agency,
) -> Result<Option<LicenseFacts>, CardFactsError> {
    let row = sqlx::query!(
        r#"SELECT license_number, recorded_at AS "recorded_at!: jiff_sqlx::Timestamp"
           FROM licensing.business_license WHERE business_id = $1 AND agency = $2
                 AND paused_at IS NULL"#,
        business,
        agency.code(),
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| CardFactsError::Database)?;
    Ok(row.map(|r| LicenseFacts {
        issuer: Issuer::Agency(agency),
        number: r.license_number,
        checked_on: arizona_time(r.recorded_at.to_jiff()).date(),
        since_year: None,
    }))
}

/// LIC-038, D-3646: none when no entry of the latest city list matches `names` by the LIC-011
/// rule, else the earliest issue year of the matching entries (none when none has a date).
async fn city_year(
    conn: &mut PgConnection,
    names: &CityRecord,
) -> Result<Option<Option<i16>>, CardFactsError> {
    let rows = sqlx::query!(
        r#"SELECT business_name, trade_name, owner_name, license_number,
                  EXTRACT(YEAR FROM issued_on)::int2 AS "year"
           FROM licensing.city_license_entry
           WHERE import_id = (SELECT id FROM licensing.list_import WHERE source = 'city'
                              ORDER BY list_date DESC LIMIT 1)"#,
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| CardFactsError::Database)?;
    let years: Vec<Option<i16>> = rows
        .into_iter()
        .filter_map(|r| {
            let entry = CityRecord {
                business_name: r.business_name,
                trade_name: r.trade_name,
                owner_name: r.owner_name,
                license_number: r.license_number,
            };
            let matched = city_list_standing(names, std::slice::from_ref(&entry));
            (matched == CityListStanding::Matched).then_some(r.year)
        })
        .collect();
    Ok((!years.is_empty()).then(|| years.into_iter().flatten().min()))
}
