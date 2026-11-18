//! The active businesses of one catalog category, category-wide (T679; LIC-046, OPS-006,
//! INT-017; D-4360).
//!
//! [`active_in_category`] counts the businesses whose status is `active` and that tick at least
//! one job type in the category, whatever towns they serve; a paused, removed, waitlisted or
//! pending business is not counted. `quotes::api::request_coverage` reads it to apply
//! `licensing::api::request_intake`'s minimum, since `quotes` may not read this crate's tables.
//! The query is `report_coverage`'s trade count (`service.rs`), without the coverage lock.

use sqlx::PgConnection;

use crate::models::BusinessesError;

/// LIC-046, OPS-006, D-4360: the active businesses with a ticked job type in `category`, in any
/// town; paused and removed businesses are not counted.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn active_in_category(
    conn: &mut PgConnection,
    category: &str,
) -> Result<u32, BusinessesError> {
    let n = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.business b WHERE b.status = 'active' \
         AND EXISTS (SELECT 1 FROM businesses.job_type_choice c \
                     WHERE c.business_id = b.id AND c.category = $1)",
        category
    )
    .fetch_one(conn)
    .await
    .map_err(|_| BusinessesError::Database)?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}
