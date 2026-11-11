//! A business's licensing holder, read once for the checklist, the capability step and the card
//! pool (T374, D-2712; LIC-006, LIC-011, LIC-012, LIC-015, LIC-016, LIC-019, LIC-047, LIC-048,
//! BIZ-049, CARD-001).
//!
//! [`holding`] adds what only `businesses` knows, the handyman standing (the Handyman category
//! chosen at sign-up, `handyman` in `businesses.signup.trades`), to the ROC license number and city
//! names the caller holds, and calls `licensing::api::holder_for`; `licensing` never calls
//! `businesses` (`kit/layers.json`). No business table holds a license number or a name (PRIV-001),
//! so the caller passes them; with no number, `licensing` reads the latest daily check's.

use askhava_licensing::api::{CityRecord, HolderInputs, HolderParts, holder_for};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::BusinessesError;

/// The catalog code of the Handyman category, the only no-license category that is
/// `handyman_allowed` (`docs/catalog/categories.yaml`, D-2712).
pub const HANDYMAN_CATEGORY: &str = "handyman";

/// What the caller knows of the business's license identity: the ROC license number, when it has
/// one, and the business's own names for the city list (LIC-006, LIC-011). No `Debug` (PRIV-016).
#[derive(Clone, Copy, Default)]
pub struct Known<'a> {
    /// The ROC license number for a business with no daily check yet; once checked, the latest
    /// check's number is used instead (D-2744).
    pub roc_license: Option<&'a str>,
    /// The business's own names for the city-list match; `None` reads as `Pending`.
    pub city_names: Option<&'a CityRecord>,
}

/// LIC-006, LIC-011, LIC-012, LIC-015, LIC-016, LIC-019, LIC-047, LIC-048, BIZ-049, CARD-001:
/// everything business `id` holds for the license rules: [`HolderParts`] from
/// `licensing::api::holder_for`, with the handyman standing from its sign-up trades; `roc_covers`
/// is set per job type by `HolderParts::for_job`.
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] when no business has `id`; [`BusinessesError::Database`]
/// on a database failure or a stored license the rules refuse.
pub async fn holding(
    conn: &mut PgConnection,
    id: Uuid,
    known: Known<'_>,
) -> Result<HolderParts, BusinessesError> {
    let row = sqlx::query!(
        r#"SELECT COALESCE($2 = ANY (s.trades), false) AS "handyman!"
           FROM businesses.business b LEFT JOIN businesses.signup s ON s.business_id = b.id
           WHERE b.id = $1"#,
        id,
        HANDYMAN_CATEGORY,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| BusinessesError::Database)?
    .ok_or(BusinessesError::UnknownBusiness)?;
    let inputs = HolderInputs {
        roc_license: known.roc_license,
        city_names: known.city_names,
        handyman: row.handyman,
    };
    holder_for(conn, id, inputs)
        .await
        .map_err(|_| BusinessesError::Database)
}
