//! The homeowner's share box at the pick (PICK-049; D-443, D-492, D-3032; T214).
//!
//! The pick review step's box "Share my first name and the job address with <Business> once they
//! accept." starts unticked and is required to pick: the final Pick refuses an unticked box and
//! stores nothing. [`tick_share`] logs a ticked final Pick, tied to her browser (the SHA256 of the
//! page's CSRF token), keyed like `backup_choice` (`migrations/0158_picks_share_box.sql`);
//! [`share_ticked`] tells the code step whether that browser ticked it for that quote, and
//! [`bind_share`] logs the tick with the pick, in the transaction that stores (or keeps) the pick
//! after her code passed. Each row is new; nothing is updated or deleted.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::Browser;
use crate::models::PicksError;

fn number(n: usize) -> Result<i32, PicksError> {
    i32::try_from(n).map_err(|_| PicksError::Quotes)
}

/// PICK-049, D-3032: logs the ticked share box `browser` sent with the final Pick of quote
/// `for_quote` on `job`. It is bound to a pick only by [`bind_share`] after her code passed.
///
/// # Errors
///
/// [`PicksError::Quotes`] or [`PicksError::Database`].
pub async fn tick_share(
    db: &mut PgConnection,
    (job, for_quote): (Uuid, usize),
    browser: &Browser,
    at: Timestamp,
) -> Result<(), PicksError> {
    sqlx::query!(
        "INSERT INTO picks.share_consent (job_id, for_quote, browser, set_at) \
         VALUES ($1, $2, $3, $4)",
        job,
        number(for_quote)?,
        browser.as_slice(),
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(())
}

/// PICK-049, D-3032: whether `browser` ticked the share box with a final Pick of quote `quote` on
/// `job`; the code step stores no pick without it.
///
/// # Errors
///
/// [`PicksError::Quotes`] or [`PicksError::Database`].
pub async fn share_ticked(
    db: &mut PgConnection,
    (job, quote): (Uuid, usize),
    browser: &Browser,
) -> Result<bool, PicksError> {
    let row = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM picks.share_consent WHERE job_id = $1 AND for_quote = $2 \
         AND browser = $3 AND pick_id IS NULL) AS \"ticked!\"",
        job,
        number(quote)?,
        browser.as_slice(),
    )
    .fetch_one(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row)
}

/// PICK-049, D-3032: inside the transaction that stores (or keeps) pick `pick` of quote `quote`
/// on `job` after her code passed, logs `browser`'s tick with the pick (a new row with
/// `pick_id`).
///
/// # Errors
///
/// [`PicksError::Quotes`] or [`PicksError::Database`].
pub async fn bind_share(
    tx: &mut PgConnection,
    ((job, quote), pick): ((Uuid, usize), Uuid),
    browser: &Browser,
    at: Timestamp,
) -> Result<(), PicksError> {
    sqlx::query!(
        "INSERT INTO picks.share_consent (job_id, for_quote, browser, pick_id, set_at) \
         VALUES ($1, $2, $3, $4, $5)",
        job,
        number(quote)?,
        browser.as_slice(),
        pick,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(())
}
