//! The class changes between recorded ROC lists, for the daily license-map re-apply (T372, D-2705;
//! BIZ-050, LIC-043, LIC-006, CARD-015).
//!
//! [`roc_lists`] lists every recorded ROC list, oldest first; [`class_changes`] reads, one bounded
//! batch at a time, each license number whose set of Active classes on a list differs from the one
//! on the latest earlier list where the number had any Active class (D-3355), with the old and new
//! classes. A number missing from the newer list, or with no Active class on it, is not a change: a
//! truncated list must not strip and pause everyone, and a lapsed license is the daily check's
//! (CARD-015) and the lookup's, never a class change.
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
    /// The Active classes on the latest earlier list where the number had any, sorted (empty when
    /// it had none on any earlier list).
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

/// BIZ-050, LIC-043, D-2705, D-3355: of the license numbers after `after` (sorted), at most
/// `limit` whose set of Active classes on list `new` differs from the one on the latest earlier
/// ROC list (by date) where the number had any Active class, with both sets. A number absent from
/// `new`, or with no Active class on it, is never a change (a truncated list or a lapsed license,
/// CARD-015's), and it is skipped, not forgotten: when it comes back it is compared with the last
/// list it was Active on, so a class lost while it was missing is still a removal. A number never
/// Active before `new` is a change from no classes.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn class_changes(
    conn: &mut PgConnection,
    new: Uuid,
    (after, limit): (&str, i64),
) -> Result<Vec<ClassChange>, ImportError> {
    let rows = db(sqlx::query!(
        r#"WITH n AS (
             SELECT license_number, array_agg(DISTINCT class_code ORDER BY class_code) AS classes,
                    min(list_date) AS list_date
             FROM licensing.agency_record
             WHERE import_id = $1 AND status = 'active' AND license_number > $2
             GROUP BY license_number),
           c AS (
             SELECT n.license_number, n.classes AS new_classes,
                    COALESCE((SELECT array_agg(DISTINCT r.class_code ORDER BY r.class_code)
                              FROM licensing.agency_record r
                              WHERE r.license_number = n.license_number AND r.status = 'active'
                                AND r.list_date = (
                                  SELECT max(e.list_date) FROM licensing.agency_record e
                                  WHERE e.license_number = n.license_number
                                    AND e.status = 'active' AND e.list_date < n.list_date)),
                             '{}'::text[]) AS old_classes
             FROM n)
           SELECT license_number AS "number!", old_classes AS "old!", new_classes AS "new!"
           FROM c WHERE new_classes IS DISTINCT FROM old_classes
           ORDER BY license_number LIMIT $3"#,
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
