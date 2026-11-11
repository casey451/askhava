//! The homeowner's backup choice at the pick (PICK-047, PICK-046; D-400, D-446, D-491, D-492;
//! T157).
//!
//! On the quote page's pick review step she may name one other quote to go to if the business
//! she picks can't take it; "No backup" is the default and Ask Hava never fills it in.
//! [`choose_backup`] stores each choice as a new `picks.backup_choice` row (the latest row of the
//! job counts, so a change or a removal is a new row; `migrations/0153_picks_backup_choice.sql`).
//! [`backup_choice`] runs whenever a pick ends without an acceptance (the lapse,
//! `confirm_reminders::lapse`; ineligibility at Confirm, `confirm::refused_at_confirm`; "Can't
//! take it", a fee limit, a card not fixed, Stripe unreachable, the business leaving: their own
//! tasks call it after ending the pick): the pick moves at once to the backup when it is still
//! pickable, as a new page pick with its own pick notice and Confirm window, once per ended pick
//! (`picks.backup_move`). [`backup_text`] is her text when it moved; with no backup the caller
//! sends PICK-046's [`crate::confirm_reminders::lapse_text`].

use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_quotes::api::page_quotes;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::confirm_reminders::short;
use crate::models::{self, PicksError};
use crate::service::{PagePick, page_pick};
use crate::status::PickStatus;

/// What [`backup_choice`] did with an ended pick (PICK-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backup {
    /// The pick moved to the backup: the new pick, waiting for its own Confirm, notice queued.
    Moved(Uuid),
    /// No backup applies (none named, or no longer pickable): PICK-046's text goes.
    NoBackup,
    /// Nothing to do: already decided, the pick has not ended without an acceptance, or she
    /// already has another pick waiting.
    Done,
}

/// PICK-047, PICK-035: the named `backup` if it is still pickable after the pick of quote
/// `ended.0` by business `ended.1` ended; `quotes` are the job's page quotes in arrival order as
/// (business, available). Never the ended quote or business; `None` when nothing was named.
#[must_use]
pub fn backup_target(
    backup: Option<usize>,
    ended: (usize, Uuid),
    quotes: &[(Uuid, bool)],
) -> Option<usize> {
    let n = backup.filter(|&n| n != ended.0)?;
    let (business, open) = *quotes.get(n.checked_sub(1)?)?;
    (open && business != ended.1).then_some(n)
}

/// PICK-047, D-400: stores the homeowner's backup for her pick of quote `for_quote` on `job`;
/// `None` is "No backup". A backup that is the same quote or not an available quote on the page
/// is stored as "No backup". Returns what was stored. A change after the pick ended never counts.
///
/// # Errors
///
/// [`PicksError::Quotes`] or [`PicksError::Database`].
pub async fn choose_backup(
    db: &mut PgConnection,
    (job, for_quote): (Uuid, usize),
    backup: Option<usize>,
    at: Timestamp,
) -> Result<Option<usize>, PicksError> {
    let quotes = page_quotes(db, job).await.map_err(|_| PicksError::Quotes)?;
    let kept = backup.filter(|&n| {
        n != for_quote
            && n.checked_sub(1)
                .and_then(|i| quotes.get(i))
                .is_some_and(|q| !q.unavailable)
    });
    let number = |n: usize| i32::try_from(n).map_err(|_| PicksError::Quotes);
    let backup_quote = kept.map(number).transpose()?;
    sqlx::query!(
        "INSERT INTO picks.backup_choice (job_id, for_quote, backup_quote, set_at) \
         VALUES ($1, $2, $3, $4)",
        job,
        number(for_quote)?,
        backup_quote,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(kept)
}

/// PICK-047, PICK-046, D-400, D-446: after pick `pick` ended without an acceptance (expired, or
/// cancelled other than replaced or paused by the homeowner), moves it at once to her backup when
/// it is still pickable, inside the caller's transaction, once per pick. `paused` is the job's
/// homeowner pause (CON-029), read by the caller. The new pick is a page pick with its fee, audit
/// row and pick notice (PICK-037), so its business gets its own window.
///
/// # Errors
///
/// [`PicksError::MovedOn`] (roll back), or the errors of `page_pick`.
pub async fn backup_choice(
    tx: &mut PgConnection,
    pick: Uuid,
    paused: bool,
    (actor, at): (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    let Some((job, business, quote)) = ended_pick(tx, pick).await? else {
        return Ok(Backup::Done);
    };
    if models::waiting_pick(tx, job).await?.is_some() {
        record(tx, pick, None, at).await?;
        return Ok(Backup::Done);
    }
    let quotes: Vec<(Uuid, bool)> = page_quotes(tx, job)
        .await
        .map_err(|_| PicksError::Quotes)?
        .iter()
        .map(|q| (q.business, !q.unavailable))
        .collect();
    let named = named_backup(tx, (job, pick), quote).await?;
    let moved = match backup_target(named, (quote, business), &quotes) {
        Some(n) => match page_pick(tx, (job, paused), n, actor, at).await? {
            PagePick::Picked(id) => Some(id),
            PagePick::Same(_) | PagePick::Taken => None,
        },
        None => None,
    };
    record(tx, pick, moved, at).await?;
    Ok(moved.map_or(Backup::NoBackup, Backup::Moved))
}

/// The ended pick `pick` (job, business, quote number), locked, when it ended without an
/// acceptance and no backup was decided for it yet.
async fn ended_pick(
    tx: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<(Uuid, Uuid, usize)>, PicksError> {
    let row = sqlx::query!(
        "SELECT job_id, business_id, quote_number FROM picks.pick p \
         WHERE id = $1 AND ended_at IS NOT NULL AND status IN ($2, $3) \
         AND end_reason IS DISTINCT FROM 'replaced' AND end_reason IS DISTINCT FROM 'paused' \
         AND NOT EXISTS (SELECT 1 FROM picks.backup_move m WHERE m.pick_id = p.id) FOR UPDATE",
        pick,
        PickStatus::Expired.code(),
        PickStatus::Cancelled.code(),
    )
    .fetch_optional(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row.and_then(|r| {
        Some((
            r.job_id,
            r.business_id,
            usize::try_from(r.quote_number).ok()?,
        ))
    }))
}

/// The backup named for quote `quote`: the job's latest choice made before `pick` ended, when it
/// was made for that quote (a choice for another quote means none, PICK-047).
async fn named_backup(
    tx: &mut PgConnection,
    (job, pick): (Uuid, Uuid),
    quote: usize,
) -> Result<Option<usize>, PicksError> {
    let row = sqlx::query!(
        "SELECT for_quote, backup_quote FROM picks.backup_choice WHERE job_id = $1 \
         AND set_at <= (SELECT ended_at FROM picks.pick WHERE id = $2) \
         ORDER BY set_at DESC, id DESC LIMIT 1",
        job,
        pick,
    )
    .fetch_optional(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row
        .filter(|r| usize::try_from(r.for_quote).ok() == Some(quote))
        .and_then(|r| r.backup_quote)
        .and_then(|n| usize::try_from(n).ok()))
}

/// Records the decision for `pick` once: the new pick when it moved.
async fn record(
    tx: &mut PgConnection,
    pick: Uuid,
    moved: Option<Uuid>,
    at: Timestamp,
) -> Result<(), PicksError> {
    let done = sqlx::query!(
        "INSERT INTO picks.backup_move (pick_id, new_pick_id, decided_at) VALUES ($1, $2, $3) \
         ON CONFLICT (pick_id) DO NOTHING",
        pick,
        moved,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    match done.rows_affected() {
        1 => Ok(()),
        _ => Err(PicksError::MovedOn),
    }
}

/// PICK-047: the homeowner's text when her pick moved from `name` to her backup `backup`, in
/// English or Spanish, built from the brand file (D-1971).
#[must_use]
pub fn backup_text(name: &str, backup: &str, language: Language) -> String {
    let b = short(language);
    match language {
        Language::En => format!(
            "{b}: {name} couldn't take it, so your backup, {backup}, has your pick now. We'll text you when they accept."
        ),
        Language::Es => format!(
            "{b}: {name} no pudo tomar el trabajo, así que pasamos su elección a su segunda opción, {backup}. Le avisaremos por texto cuando acepte."
        ),
    }
}
