//! The class changes between recorded ROC lists, for the daily license-map re-apply (T372, D-2705;
//! BIZ-050, LIC-043, LIC-006, CARD-015).
//!
//! [`roc_lists`] lists every recorded ROC list, oldest first; [`class_changes`] reads, one bounded
//! batch at a time, each license number whose set of Active classes differs between a list and the
//! one recorded before it, with the old and new classes. A number missing from the newer list, or
//! with no Active class on it, is not a change: a truncated list must not strip and pause everyone,
//! and a lapsed license is the daily check's (CARD-015) and the lookup's, never a class change.
//! [`bound_businesses`] maps license numbers to the businesses whose latest daily check is under
//! them, and [`latest_check_active`] gives that check's standing. The lists are read only by
//! number here, never listed out with names (LIC-010).

use jiff::civil::Date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::imports::ImportError;

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, ImportError> {
    r.map_err(|_| ImportError::Database)
}

/// One recorded ROC list: its import row and its date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RocList {
    /// The `licensing.list_import` row.
    pub id: Uuid,
    /// The list's date.
    pub list_date: Date,
}

/// One license number whose Active classes changed between two lists (BIZ-050); it holds only
/// the number and class codes, which are public record, never a contact detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassChange {
    /// The normalised license number.
    pub license_number: String,
    /// The Active classes on the older list, sorted (empty when the number was not on it).
    pub old: Vec<String>,
    /// The Active classes on the newer list, sorted (never empty).
    pub new: Vec<String>,
}

/// BIZ-050, LIC-006: every recorded ROC list, oldest first.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn roc_lists(conn: &mut PgConnection) -> Result<Vec<RocList>, ImportError> {
    let rows = db(sqlx::query!(
        r#"SELECT id, list_date::text AS "list_date!" FROM licensing.list_import
           WHERE source = 'roc' ORDER BY list_date"#
    )
    .fetch_all(conn)
    .await)?;
    rows.into_iter()
        .map(|r| {
            let list_date = r.list_date.parse().map_err(|_| ImportError::Database)?;
            Ok(RocList {
                id: r.id,
                list_date,
            })
        })
        .collect()
}

/// BIZ-050, LIC-043, D-2705: of the license numbers after `after` (sorted), at most `limit` whose
/// set of Active classes on list `new` differs from the one on list `old`, with both sets. A
/// number absent from `new`, or with no Active class on it, is never a change (a truncated list or
/// a lapsed license, CARD-015's); a number new on `new` is a change from no classes.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn class_changes(
    conn: &mut PgConnection,
    (old, new): (Uuid, Uuid),
    (after, limit): (&str, i64),
) -> Result<Vec<ClassChange>, ImportError> {
    let rows = db(sqlx::query!(
        r#"WITH n AS (
             SELECT license_number, array_agg(DISTINCT class_code ORDER BY class_code) AS classes
             FROM licensing.agency_record
             WHERE import_id = $2 AND status = 'active' AND license_number > $3
             GROUP BY license_number),
           o AS (
             SELECT license_number, array_agg(DISTINCT class_code ORDER BY class_code) AS classes
             FROM licensing.agency_record
             WHERE import_id = $1 AND status = 'active' AND license_number > $3
             GROUP BY license_number)
           SELECT n.license_number AS "number!",
                  COALESCE(o.classes, '{}'::text[]) AS "old!",
                  n.classes AS "new!"
           FROM n LEFT JOIN o USING (license_number)
           WHERE n.classes IS DISTINCT FROM COALESCE(o.classes, '{}'::text[])
           ORDER BY n.license_number LIMIT $4"#,
        old,
        new,
        after,
        limit,
    )
    .fetch_all(conn)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| ClassChange {
            license_number: r.number,
            old: r.old,
            new: r.new,
        })
        .collect())
}

/// BIZ-050, CARD-015, D-2744: each business whose latest daily check is under one of `numbers`,
/// with that number, sorted by business.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn bound_businesses(
    conn: &mut PgConnection,
    numbers: &[String],
) -> Result<Vec<(Uuid, String)>, ImportError> {
    let rows = db(sqlx::query!(
        r#"SELECT business_id AS "business!", license_number AS "number!" FROM (
             SELECT DISTINCT ON (business_id) business_id, license_number
             FROM licensing.license_check ORDER BY business_id, checked_at DESC, id DESC) latest
           WHERE license_number = ANY($1) ORDER BY business_id"#,
        numbers,
    )
    .fetch_all(conn)
    .await)?;
    Ok(rows.into_iter().map(|r| (r.business, r.number)).collect())
}

/// CARD-015, LIC-007: whether the business's latest daily check showed Active; `None` when it has
/// never been checked (an agency, pest or no-license business).
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn latest_check_active(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<bool>, ImportError> {
    let status = db(sqlx::query_scalar!(
        "SELECT status FROM licensing.license_check WHERE business_id = $1 \
         ORDER BY checked_at DESC, id DESC LIMIT 1",
        business,
    )
    .fetch_optional(conn)
    .await)?;
    Ok(status.map(|s| s == "active"))
}
