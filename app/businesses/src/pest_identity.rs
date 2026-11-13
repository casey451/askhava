//! A pest-control business's identity: the city business-license list match plus its Department
//! of Agriculture attestation (LIC-048; T215, D-446, D-3335).
//!
//! A pest business has no state record Ask Hava checks (LIC-047: the license is self-reported),
//! so its identity match is the city list (LIC-011) together with the sign-up attestation, and it
//! activates on that match as BIZ-016 says, through [`crate::identity::record_match`] and
//! `settle`, never around them. [`pest_identity`] reads both from [`crate::holding::holding`];
//! [`record_pest_match`] records the match only when both hold, so a business still waiting for
//! the city list stays pending and a later list showing its name activates it with its code. The
//! daily state license check (CARD-015) never applies to it: `licensing::api::eligibility` gives a
//! pest card from the attestation alone, termite and pre-emergent jobs only with the attested
//! category (LIC-043, LIC-012), and a reported license pauses its pest jobs (LIC-047, T389).

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use askhava_licensing::api::CityListStanding;

use crate::holding::{Known, holding};
use crate::identity::record_match;
use crate::models::BusinessesError;
use crate::signup::SignupError;
use crate::status::AccountStatus;

/// Where a pest business's identity match stands (LIC-048).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PestIdentity {
    /// The city list matched its names and it attested to a Department license: the match passes.
    Matched,
    /// It has not attested to a Department pest-management license (LIC-047).
    NoAttestation,
    /// Its attested license was reported wrong and waits for a found number (LIC-047).
    Paused,
    /// It attested, but no daily city list has matched its names yet (LIC-011, D-177).
    CityListPending,
}

/// LIC-048, LIC-047, LIC-011: business `id`'s pest identity, from its attestation and the city
/// list match for the names in `known` (the attestation first, then the pause, then the list).
///
/// # Errors
///
/// As [`holding`]: [`BusinessesError::UnknownBusiness`] or [`BusinessesError::Database`].
pub async fn pest_identity(
    conn: &mut PgConnection,
    id: Uuid,
    known: Known<'_>,
) -> Result<PestIdentity, BusinessesError> {
    let parts = holding(conn, id, known).await?;
    let Some(attested) = parts.holder.pest else {
        return Ok(PestIdentity::NoAttestation);
    };
    if attested.is_paused() {
        return Ok(PestIdentity::Paused);
    }
    Ok(match parts.holder.city_list {
        CityListStanding::Matched => PestIdentity::Matched,
        CityListStanding::Pending => PestIdentity::CityListPending,
    })
}

/// LIC-048, BIZ-016: records a pest business's identity match and settles it: a
/// [`PestIdentity::Matched`] business passes the match (active once its code is done, no flag and
/// the sign-up email verified); any other standing records nothing and keeps the business as it
/// is. Returns the identity and the status after it.
///
/// # Errors
///
/// [`SignupError::Businesses`] as [`pest_identity`] and `record_match`; `settle`'s errors.
pub async fn record_pest_match(
    conn: &mut PgConnection,
    id: Uuid,
    known: Known<'_>,
    now: Timestamp,
) -> Result<(PestIdentity, AccountStatus), SignupError> {
    let identity = pest_identity(conn, id, known).await?;
    let passed = (identity == PestIdentity::Matched).then_some(true);
    let status = record_match(conn, id, passed, None, now).await?;
    Ok((identity, status))
}
