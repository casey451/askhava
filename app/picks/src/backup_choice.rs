//! The homeowner's backup choice at the pick (PICK-047, PICK-046; D-400, D-446, D-491, D-492;
//! T157).
//!
//! On the quote page's pick review step she may name one other quote to go to if the business
//! she picks can't take it; "No backup" is the default and Ask Hava never fills it in.
//! [`choose_backup`] stores what the final Pick asked for, tied to her browser (the SHA256 of the
//! page's CSRF token); it counts for nothing until [`bind_backup`] binds it to the pick in the
//! transaction that stores the pick after her 6-digit code passed, so a forwarded link alone can
//! neither set nor change it, and a pick made another way (a text "<n> YES") never inherits it
//! (D-2661). [`backup_changed`] tells the code step that pressing Pick again on the waiting quote
//! asks for a different backup, so a code is needed. Each row is new
//! (`migrations/0153_picks_backup_choice.sql`); the pick's latest bound row counts.
//! [`backup_choice`] runs whenever a pick ends without an acceptance (the lapse,
//! `confirm_reminders::lapse`; ineligibility at Confirm, `confirm::refused_at_confirm`; "Can't
//! take it", a fee limit, a card not fixed, Stripe unreachable, the business leaving: their own
//! tasks call it after ending the pick): the pick moves at once to the backup when it is still
//! pickable, as a new page pick with its own pick notice and Confirm window (on an emergency job
//! the job's 60 minutes restart, PICK-044), once per ended pick (`picks.backup_move`).
//! [`backup_text`] is her text when it moved; with no backup the caller sends PICK-046's
//! [`crate::confirm_reminders::lapse_text`].

use askhava_jobs::api::{JobStatus, current, restart_timer};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_quotes::api::page_quotes;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use askhava_core::api::Guarded;

use crate::confirm_reminders::short;
use crate::models::{self, PicksError};
use crate::service::{PagePick, page_pick};
use crate::status::{PickStatus, awaits_charge, awaits_confirm};

/// The browser a choice came from: the SHA256 of the quote page's CSRF token (SEC-042), never
/// the token itself.
pub type Browser = [u8; 32];

/// What [`backup_choice`] did with an ended pick (PICK-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backup {
    /// The pick moved to the backup: the new pick, waiting for its own Confirm, notice queued.
    Moved(Uuid),
    /// No backup applies (none named, or no longer pickable): PICK-046's text goes.
    NoBackup,
    /// Nothing to do and no text: already decided, the pick has not ended without an acceptance,
    /// she already has another pick waiting, or she paused the job (CON-029).
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

fn number(n: usize) -> Result<i32, PicksError> {
    i32::try_from(n).map_err(|_| PicksError::Quotes)
}

/// PICK-047, D-400, D-2661: stores the backup asked for by `browser` with the final Pick of quote
/// `for_quote` on `job`; `None` is "No backup". A backup that is the same quote or not an
/// available quote on the page is stored as "No backup". Returns what was stored. It counts for
/// nothing until [`bind_backup`] binds it after her code passed.
///
/// # Errors
///
/// [`PicksError::Quotes`] or [`PicksError::Database`].
pub async fn choose_backup(
    db: &mut PgConnection,
    (job, for_quote): (Uuid, usize),
    (backup, browser): (Option<usize>, &Browser),
    at: Timestamp,
) -> Result<Option<usize>, PicksError> {
    let quotes = page_quotes(db, job).await.map_err(|_| PicksError::Quotes)?;
    let kept = backup.filter(|&n| {
        n != for_quote
            && n.checked_sub(1)
                .and_then(|i| quotes.get(i))
                .is_some_and(|q| !q.unavailable)
    });
    let backup_quote = kept.map(number).transpose()?;
    sqlx::query!(
        "INSERT INTO picks.backup_choice (job_id, for_quote, backup_quote, browser, set_at) \
         VALUES ($1, $2, $3, $4, $5)",
        job,
        number(for_quote)?,
        backup_quote,
        browser.as_slice(),
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(kept)
}

/// The latest backup `browser` asked for with the final Pick of quote `quote` on `job`: `None`
/// when it asked for none, `Some(None)` for "No backup".
async fn asked(
    db: &mut PgConnection,
    (job, quote): (Uuid, i32),
    browser: &Browser,
) -> Result<Option<Option<i32>>, PicksError> {
    let row = sqlx::query!(
        "SELECT backup_quote FROM picks.backup_choice WHERE job_id = $1 AND for_quote = $2 \
         AND browser = $3 AND pick_id IS NULL ORDER BY set_at DESC, id DESC LIMIT 1",
        job,
        quote,
        browser.as_slice(),
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row.map(|r| r.backup_quote))
}

/// The backup bound to `pick` (its latest bound row; with `ended_only`, made at or before the
/// pick's end, so a change after the end never counts: PICK-047).
async fn bound(
    db: &mut PgConnection,
    pick: Uuid,
    ended_only: bool,
) -> Result<Option<i32>, PicksError> {
    let row = sqlx::query!(
        "SELECT backup_quote FROM picks.backup_choice WHERE pick_id = $1 AND (NOT $2 \
         OR set_at <= (SELECT ended_at FROM picks.pick WHERE id = $1)) \
         ORDER BY set_at DESC, id DESC LIMIT 1",
        pick,
        ended_only,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row.and_then(|r| r.backup_quote))
}

/// Waiting pick `pick`'s job and quote number, locked; `None` when it no longer waits.
async fn waiting(tx: &mut PgConnection, pick: Uuid) -> Result<Option<(Uuid, i32)>, PicksError> {
    let row = sqlx::query!(
        "SELECT job_id, quote_number FROM picks.pick WHERE id = $1 AND status = $2 FOR UPDATE",
        pick,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_optional(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row.map(|r| (r.job_id, r.quote_number)))
}

/// PICK-047, D-2661: inside the transaction that stores (or keeps) waiting pick `pick` after her
/// code passed, binds the latest backup `browser` asked for with the final Pick of that pick's
/// quote; returns the bound backup. A pick no longer waiting, or nothing asked by this browser
/// for that quote, binds nothing (`None`).
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn bind_backup(
    tx: &mut PgConnection,
    pick: Uuid,
    browser: &Browser,
    at: Timestamp,
) -> Result<Option<usize>, PicksError> {
    let Some((job, quote)) = waiting(tx, pick).await? else {
        return Ok(None);
    };
    let Some(backup) = asked(tx, (job, quote), browser).await? else {
        return Ok(None);
    };
    sqlx::query!(
        "INSERT INTO picks.backup_choice \
         (job_id, for_quote, backup_quote, browser, pick_id, set_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
        job,
        quote,
        backup,
        browser.as_slice(),
        pick,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(backup.and_then(|n| usize::try_from(n).ok()))
}

/// PICK-047, D-2661: whether pressing Pick again on quote `quote` of `job`, already the waiting
/// pick, asked from `browser` for a backup other than the one bound to it: then the code step
/// sends a code instead of showing the result, and the change counts only after it passes.
///
/// # Errors
///
/// [`PicksError::Database`] or [`PicksError::Quotes`].
pub async fn backup_changed(
    db: &mut PgConnection,
    (job, quote): (Uuid, usize),
    browser: &Browser,
) -> Result<bool, PicksError> {
    let quote = number(quote)?;
    let Some((pick, waiting, _)) = models::waiting_pick(db, job).await? else {
        return Ok(false);
    };
    if waiting != quote {
        return Ok(false);
    }
    let Some(wanted) = asked(db, (job, quote), browser).await? else {
        return Ok(false);
    };
    Ok(wanted != bound(db, pick, false).await?)
}

/// PICK-047, PICK-046, D-400, D-446, D-2662: after pick `pick` ended without an acceptance,
/// moves it at once to the backup bound to it (only after her code, [`bind_backup`]) when still
/// pickable, inside the caller's transaction, once per pick. `paused` is the job's homeowner pause
/// (CON-029), read by the caller: a paused job gets [`Backup::Done`] and no text. The new pick is
/// a page pick with its fee, audit row and pick notice (PICK-037), so its business gets its own
/// window; on an emergency job the job's 60 clock minutes restart from `at` (PICK-044).
///
/// The caller's contract: the pick is ended first with `ended_at` set, as `expired`, `cancelled`
/// (other than `replaced` or `paused`, her own acts), or by `payments` as `payment_failed` or
/// `needs_card` (FEE-041, FEE-018, FEE-020); any other pick is [`Backup::Done`]. A job still in
/// Payment pending is [`PicksError::Job`] with nothing recorded: the payments caller moves the job
/// back to Quotes ready (JOB-001) before calling, in the same transaction.
///
/// # Errors
///
/// [`PicksError::Job`] (Payment pending, or the job could not be read or its timer restarted),
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
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    if awaits_charge(status) {
        return Err(PicksError::Job);
    }
    if paused || models::waiting_pick(tx, job).await?.is_some() {
        record(tx, pick, None, at).await?;
        return Ok(Backup::Done);
    }
    let quotes: Vec<(Uuid, bool)> = page_quotes(tx, job)
        .await
        .map_err(|_| PicksError::Quotes)?
        .iter()
        .map(|q| (q.business, !q.unavailable))
        .collect();
    let named = named_backup(tx, pick).await?;
    let moved = match backup_target(named, (quote, business), &quotes) {
        Some(n) => match page_pick(tx, (job, false), n, actor, at).await? {
            PagePick::Picked(id) => Some(id),
            PagePick::Same(_) | PagePick::Taken => None,
        },
        None => None,
    };
    if moved.is_some() {
        own_window(tx, job, at).await?;
    }
    record(tx, pick, moved, at).await?;
    Ok(moved.map_or(Backup::NoBackup, Backup::Moved))
}

/// PICK-044, PICK-047: on an emergency job still in Awaiting confirm, restarts `jobs`' 60 clock
/// minutes from `at`, so the backup gets its own window (a non-emergency one starts at its
/// notice's send, `handlers::notice_sent`; a job just moved from Quotes ready already counts
/// from `at`).
async fn own_window(tx: &mut PgConnection, job: Uuid, at: Timestamp) -> Result<(), PicksError> {
    let row = current(tx, job).await.map_err(|_| PicksError::Job)?;
    if !row.emergency || !awaits_confirm(row.status) || row.timer_from >= at {
        return Ok(());
    }
    let restarted = restart_timer(tx, job, (JobStatus::AwaitingConfirm, at)).await;
    match restarted.map_err(|_| PicksError::Job)? {
        Guarded::Applied(()) => Ok(()),
        Guarded::MovedOn => Err(PicksError::MovedOn),
    }
}

/// The ended pick `pick` (job, business, quote number), locked, when it ended without an
/// acceptance and no backup was decided for it yet. `payment_failed` and `needs_card` are
/// `payments`' end states (`migrations/0079_picks_code.sql`), not yet [`PickStatus`] variants.
async fn ended_pick(
    tx: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<(Uuid, Uuid, usize)>, PicksError> {
    let row = sqlx::query!(
        "SELECT job_id, business_id, quote_number FROM picks.pick p \
         WHERE id = $1 AND ended_at IS NOT NULL \
         AND status IN ($2, $3, 'payment_failed', 'needs_card') \
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

/// The backup bound to `pick` before it ended: a change after the end never counts (PICK-047).
async fn named_backup(tx: &mut PgConnection, pick: Uuid) -> Result<Option<usize>, PicksError> {
    Ok(bound(tx, pick, true)
        .await?
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
