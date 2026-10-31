//! Storing the daily lists and running each business's daily check (CARD-015, LIC-006, LIC-007,
//! LIC-010, LIC-027).
//!
//! Every function takes the caller's connection. An import writes its `licensing.list_import` row
//! and every row in one transaction, each row with its source and list date; a second run for the
//! same list and day records nothing (R12). The lists are read only by license number or matched
//! against one business's own names, never listed out (LIC-010). The daily job calls an import per
//! list, then `businesses` calls [`check_license`] for every ROC-licensed business. The read-back
//! here is the ROC's only; other agencies' read-back comes with T180's lookup (LIC-027 is partial).

use jiff::Timestamp;
use jiff::civil::Date;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::imports::{self, CityRow, ImportError, ParsedList, RocRow};
use crate::models::{CityListStanding, CityRecord};
use crate::rules::city_list_standing;

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, ImportError> {
    r.map_err(|_| ImportError::Database)
}

/// What one list import did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    /// Rows recorded.
    pub recorded: u32,
    /// Malformed or repeated rows skipped.
    pub skipped: u32,
    /// The list for this source and day was already imported, so nothing new was recorded.
    pub already_imported: bool,
}

/// The result of one daily check of a business's ROC license (CARD-015, LIC-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyCheck {
    /// The latest list shows it Active.
    Active,
    /// The latest list shows it, but not Active.
    Inactive,
    /// The latest list does not show the number.
    NotListed,
}

/// How fresh a daily check must be for a card; the caller sets both limits (CARD-015, D-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    /// The check must have run at or after this instant.
    pub checked_since: Timestamp,
    /// The list the check read must be dated on or after this day.
    pub listed_since: Date,
}

/// What a license covers, read back at sign-up from the state record, never typed (LIC-006, LIC-027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBack {
    /// The classes on the latest ROC list, sorted; the license map turns them into job types.
    pub classes: Vec<String>,
    /// The earliest original issue year, for "ROC license since <year>" (LIC-037).
    pub since_year: Option<i32>,
}

/// Records the day's ROC list (LIC-006, LIC-007). A malformed row is skipped and counted; rows
/// repeating a number and class keep the stricter status (Inactive over Active).
///
/// # Errors
///
/// [`ImportError::BadHeader`] for a changed layout; [`ImportError::TooFewRows`] for an empty or
/// mostly malformed list, or one under half the previous list's rows (D-905; nothing recorded);
/// [`ImportError::Database`] on a database failure.
pub async fn import_roc_list(
    conn: &mut PgConnection,
    list_date: Date,
    text: &str,
) -> Result<ImportSummary, ImportError> {
    let mut list = imports::parse_roc_list(text)?;
    let before = list.rows.len();
    let key = |r: &RocRow| {
        let lenient = r.status.is_active();
        (r.license_number.clone(), r.class_code.clone(), lenient)
    };
    list.rows.sort_by_key(key);
    list.rows
        .dedup_by(|a, b| a.license_number == b.license_number && a.class_code == b.class_code);
    let mut tx = db(conn.begin().await)?;
    let Some(id) = start_import(&mut tx, "roc", list_date, &list, before).await? else {
        return stored_summary(&mut tx, "roc", list_date).await;
    };
    for row in &list.rows {
        insert_roc_row(&mut tx, id, list_date, row).await?;
    }
    db(tx.commit().await)?;
    Ok(summary(&list, before))
}

/// Records the day's city business-license list (LIC-010, LIC-011).
///
/// # Errors
///
/// [`ImportError::BadHeader`] for a changed layout; [`ImportError::TooFewRows`] for an empty or
/// mostly malformed list, or one under half the previous list's rows (D-905; nothing recorded);
/// [`ImportError::Database`] on a database failure.
pub async fn import_city_list(
    conn: &mut PgConnection,
    list_date: Date,
    text: &str,
) -> Result<ImportSummary, ImportError> {
    let list = imports::parse_city_list(text)?;
    let before = list.rows.len();
    let mut tx = db(conn.begin().await)?;
    let Some(id) = start_import(&mut tx, "city", list_date, &list, before).await? else {
        return stored_summary(&mut tx, "city", list_date).await;
    };
    for row in &list.rows {
        insert_city_row(&mut tx, id, list_date, row).await?;
    }
    db(tx.commit().await)?;
    Ok(summary(&list, before))
}

fn summary<T>(list: &ParsedList<T>, before: usize) -> ImportSummary {
    let repeated = u32::try_from(before - list.rows.len()).unwrap_or(u32::MAX);
    ImportSummary {
        recorded: u32::try_from(list.rows.len()).unwrap_or(u32::MAX),
        skipped: list.skipped.saturating_add(repeated),
        already_imported: false,
    }
}

async fn start_import<T>(
    tx: &mut PgConnection,
    source: &str,
    list_date: Date,
    list: &ParsedList<T>,
    before: usize,
) -> Result<Option<Uuid>, ImportError> {
    let s = summary(list, before);
    let (recorded, skipped) = (i32::try_from(s.recorded), i32::try_from(s.skipped));
    let (Ok(recorded), Ok(skipped)) = (recorded, skipped) else {
        return Err(ImportError::Database);
    };
    if imports::shrank_too_far(s.recorded, previous_rows(tx, source, list_date).await?) {
        return Err(ImportError::TooFewRows);
    }
    db(sqlx::query_scalar!(
        "INSERT INTO licensing.list_import (source, list_date, rows_recorded, rows_skipped) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (source, list_date) DO NOTHING RETURNING id",
        source,
        list_date.to_sqlx() as _,
        recorded,
        skipped,
    )
    .fetch_optional(&mut *tx)
    .await)
}

/// The rows recorded by the latest list of `source` dated before `list_date`, if any (D-905).
async fn previous_rows(
    tx: &mut PgConnection,
    source: &str,
    list_date: Date,
) -> Result<Option<u32>, ImportError> {
    let rows = db(sqlx::query_scalar!(
        "SELECT rows_recorded FROM licensing.list_import WHERE source = $1 AND list_date < $2 \
         ORDER BY list_date DESC LIMIT 1",
        source,
        list_date.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    Ok(rows.map(|n| u32::try_from(n).unwrap_or(0)))
}

async fn stored_summary(
    tx: &mut PgConnection,
    source: &str,
    list_date: Date,
) -> Result<ImportSummary, ImportError> {
    let row = db(sqlx::query!(
        "SELECT rows_recorded, rows_skipped FROM licensing.list_import \
         WHERE source = $1 AND list_date = $2",
        source,
        list_date.to_sqlx() as _,
    )
    .fetch_one(&mut *tx)
    .await)?;
    Ok(ImportSummary {
        recorded: u32::try_from(row.rows_recorded).unwrap_or(0),
        skipped: u32::try_from(row.rows_skipped).unwrap_or(0),
        already_imported: true,
    })
}

async fn insert_roc_row(
    tx: &mut PgConnection,
    import_id: Uuid,
    list_date: Date,
    row: &RocRow,
) -> Result<(), ImportError> {
    db(sqlx::query!(
        "INSERT INTO licensing.agency_record \
         (import_id, source, list_date, license_number, class_code, status, issued_on) \
         VALUES ($1, 'roc', $2, $3, $4, $5, $6)",
        import_id,
        list_date.to_sqlx() as _,
        row.license_number,
        row.class_code,
        row.status.code(),
        row.issued_on.map(ToSqlx::to_sqlx) as _,
    )
    .execute(&mut *tx)
    .await)?;
    Ok(())
}

async fn insert_city_row(
    tx: &mut PgConnection,
    import_id: Uuid,
    list_date: Date,
    row: &CityRow,
) -> Result<(), ImportError> {
    let r = &row.record;
    db(sqlx::query!(
        "INSERT INTO licensing.city_license_entry (import_id, source, list_date, license_number, \
         business_name, trade_name, owner_name, issued_on) VALUES ($1, 'city', $2, $3, $4, $5, $6, $7)",
        import_id,
        list_date.to_sqlx() as _,
        r.license_number,
        r.business_name,
        r.trade_name,
        r.owner_name,
        row.issued_on.map(ToSqlx::to_sqlx) as _,
    )
    .execute(&mut *tx)
    .await)?;
    Ok(())
}

/// The sign-up read-back of an ROC license from the latest ROC list: its classes and issue year;
/// `None` when that list does not show the number (LIC-006, LIC-027).
///
/// # Errors
///
/// [`ImportError::NoList`] before any ROC list; [`ImportError::Database`] on a database failure.
pub async fn read_back(
    conn: &mut PgConnection,
    license_number: &str,
) -> Result<Option<ReadBack>, ImportError> {
    let latest = latest_import(conn, "roc").await?;
    let rows = db(sqlx::query!(
        "SELECT class_code, EXTRACT(YEAR FROM issued_on)::int AS year FROM licensing.agency_record \
         WHERE import_id = $1 AND license_number = $2 ORDER BY class_code",
        latest,
        imports::normalise_license_number(license_number),
    )
    .fetch_all(&mut *conn)
    .await)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let since_year = rows.iter().filter_map(|r| r.year).min();
    let classes = rows.into_iter().map(|r| r.class_code).collect();
    Ok(Some(ReadBack {
        classes,
        since_year,
    }))
}

async fn latest_import(conn: &mut PgConnection, source: &str) -> Result<Uuid, ImportError> {
    db(sqlx::query_scalar!(
        "SELECT id FROM licensing.list_import WHERE source = $1 ORDER BY list_date DESC LIMIT 1",
        source,
    )
    .fetch_optional(&mut *conn)
    .await)?
    .ok_or(ImportError::NoList)
}

/// Runs and records one business's daily check against the latest ROC list (CARD-015, LIC-007).
/// The check is per license number: `active` when any class on the number is Active (D-906).
///
/// # Errors
///
/// [`ImportError::NoList`] before any ROC list (nothing is recorded); [`ImportError::Database`].
pub async fn check_license(
    conn: &mut PgConnection,
    business_id: Uuid,
    license_number: &str,
) -> Result<DailyCheck, ImportError> {
    let number = imports::normalise_license_number(license_number);
    let latest = latest_import(conn, "roc").await?;
    let statuses = db(sqlx::query_scalar!(
        "SELECT status FROM licensing.agency_record WHERE import_id = $1 AND license_number = $2",
        latest,
        number,
    )
    .fetch_all(&mut *conn)
    .await)?;
    let (check, code) = match statuses.iter().any(|s| s == "active") {
        true => (DailyCheck::Active, "active"),
        false if statuses.is_empty() => (DailyCheck::NotListed, "not_listed"),
        false => (DailyCheck::Inactive, "inactive"),
    };
    db(sqlx::query!(
        "INSERT INTO licensing.license_check (business_id, source, license_number, list_date, status) \
         SELECT $1, 'roc', $2, list_date, $3 FROM licensing.list_import WHERE id = $4",
        business_id,
        &number,
        code,
        latest,
    )
    .execute(&mut *conn)
    .await)?;
    Ok(check)
}

/// Whether the ROC license `license_number` that covers a job lets the business receive its card:
/// only if the business's latest daily check of that license was Active and is within `fresh`
/// (CARD-015, LIC-007). For ROC-licensed categories only: a pest card is attested (LIC-048), and
/// other-agency, no-license and handyman cards are decided by `eligibility`, so none calls this.
///
/// # Errors
///
/// [`ImportError::Database`] on a database failure.
pub async fn roc_check_allows_card(
    conn: &mut PgConnection,
    business_id: Uuid,
    license_number: &str,
    fresh: Freshness,
) -> Result<bool, ImportError> {
    let latest = db(sqlx::query!(
        "SELECT status, checked_at >= $3 AND list_date >= $4 AS \"fresh!\" \
         FROM licensing.license_check WHERE business_id = $1 AND license_number = $2 \
         ORDER BY checked_at DESC, id DESC LIMIT 1",
        business_id,
        imports::normalise_license_number(license_number),
        fresh.checked_since.to_sqlx() as _,
        fresh.listed_since.to_sqlx() as _,
    )
    .fetch_optional(&mut *conn)
    .await)?;
    Ok(latest.is_some_and(|r| r.status == "active" && r.fresh))
}

/// Matches one business's own names on the latest city list (LIC-010, LIC-011); `Pending` before
/// any city list.
///
/// # Errors
///
/// [`ImportError::Database`] on a database failure.
pub async fn city_standing(
    conn: &mut PgConnection,
    business: &CityRecord,
) -> Result<CityListStanding, ImportError> {
    let latest = match latest_import(conn, "city").await {
        Err(ImportError::NoList) => return Ok(CityListStanding::Pending),
        other => other?,
    };
    let rows = db(sqlx::query_as!(
        CityRecord,
        "SELECT business_name, trade_name, owner_name, license_number \
         FROM licensing.city_license_entry WHERE import_id = $1",
        latest,
    )
    .fetch_all(&mut *conn)
    .await)?;
    Ok(city_list_standing(business, &rows))
}
