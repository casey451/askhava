//! Leaving Ask Hava in the portal and the optional reason (BIZ-051, BIZ-054; D-446, D-463, D-491;
//! T228).
//!
//! [`leave_page`] renders `templates/business_leave.html`: what leaving does, the BIZ-054 question
//! while the business was never asked (`businesses::api::reason_asked`), and one button; the
//! pause screen asks the same question once a pause passed 14 days; the left screen ends it.
//! [`leave`] is the button, for the owner only after a fresh code (SEC-038, as a saved-card
//! change): in one transaction it stores the reason (or none), moves the business to `removed`,
//! ends each waiting pick and moves it to the homeowner's backup (`picks::api::end_for_leaving`,
//! PICK-047), withdraws its open quotes (`quotes::api::withdraw_for_leaving`, PICK-051), marks its
//! saved cards removed (`payments::api::remove_saved_cards`), uses up every unused sign-in link
//! and ends every session of the business; after the commit it detaches each card at Stripe
//! through the caller's [`CardDetach`]. Ledger and audit rows are never touched. [`pause_reason`]
//! stores the pause answer. The routes are not served yet (D-3045).

use askama::Template;
use askhava_adapters::api::AdapterError;
use askhava_businesses::api::{
    AskedBy, BusinessesError, LeaveReason, PAUSE_ASK_AFTER_DAYS, ReasonAnswer, leave as leave_now,
    pause_ask_due, record_reason,
};
use askhava_intake::api::paused_by_homeowner;
use askhava_ops::api::Actor;
use askhava_payments::api::{BusinessId, remove_saved_cards};
use askhava_picks::api::{Backup, end_for_leaving, waiting_picks_of};
use askhava_quotes::api::withdraw_for_leaving;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::business_auth::{BusinessPrincipal, Need};
use crate::business_portal::{SensitiveChange, require_step_up};

/// The leave page's route (not served yet, D-3045).
pub const LEAVE_ROUTE: &str = "/b/leave";
/// The pause question's route (not served yet, D-3045).
pub const PAUSE_REASON_ROUTE: &str = "/b/pause-reason";

/// Which screen [`leave_page`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveScreen {
    /// What leaving does and the button; `ask` adds the BIZ-054 question.
    Leave {
        /// The business was never asked the reason.
        ask: bool,
    },
    /// The BIZ-054 question after a pause past 14 days.
    PauseAsk,
    /// The business has left.
    Left,
}

#[derive(Template)]
#[template(path = "business_leave.html")]
struct LeaveView<'a> {
    name: &'a str,
    left: bool,
    leaving: bool,
    ask: bool,
    reasons: Vec<(&'static str, &'static str)>,
    action: &'static str,
    csrf: &'a str,
}

/// BIZ-051, BIZ-054, A11Y-030: the leave, pause-question or left screen; the choices in
/// [`LeaveReason::ALL`] order, "Busy for a while" first with its BUSY line, none selected.
///
/// # Errors
///
/// A template render failure.
pub fn leave_page(screen: LeaveScreen, csrf: &str) -> Result<String, askama::Error> {
    let (left, leaving, ask) = match screen {
        LeaveScreen::Leave { ask } => (false, true, ask),
        LeaveScreen::PauseAsk => (false, false, true),
        LeaveScreen::Left => (true, false, false),
    };
    LeaveView {
        name: askhava_core::api::brand().display_name(),
        left,
        leaving,
        ask,
        reasons: LeaveReason::ALL.map(|r| (r.code(), r.label())).to_vec(),
        action: if leaving {
            LEAVE_ROUTE
        } else {
            PAUSE_REASON_ROUTE
        },
        csrf,
    }
    .render()
}

/// BIZ-051: removes a saved card at Stripe (`POST /v1/payment_methods/{pm}/detach`); the
/// composition's Stripe client implements it.
pub trait CardDetach {
    /// Detaches `payment_method` (`pm_…`) from its customer.
    fn detach(&self, payment_method: &str)
    -> impl Future<Output = Result<(), AdapterError>> + Send;
}

/// Why [`leave`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LeaveError {
    /// Only the owner may leave.
    #[error("only the owner may leave")]
    NotOwner,
    /// No code from the last 5 minutes (SEC-038); the refusal is logged.
    #[error("a fresh code is needed")]
    NeedsCode,
    /// The business already left.
    #[error("the business already left")]
    AlreadyLeft,
    /// A database call failed; nothing changed.
    #[error("leaving failed")]
    Database,
}

/// What leaving did (BIZ-051).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Left {
    /// Each ended pick's job and what its backup move did; the caller sends her PICK-047 or
    /// PICK-046 text from it.
    pub picks: Vec<(Uuid, Backup)>,
    /// Quotes withdrawn.
    pub quotes: u64,
    /// Saved cards removed.
    pub cards: usize,
    /// Cards Stripe did not detach (marked removed here all the same).
    pub detach_failed: usize,
    /// Sessions ended.
    pub sessions: u64,
}

/// BIZ-051, BIZ-054, SEC-038, D-446: the owner's leave button with the optional `answer`; see the
/// module comment. The step-up check runs before the transaction, so its refusal event is kept.
///
/// # Errors
///
/// [`LeaveError`]; on any error nothing changed.
pub async fn leave<D: CardDetach + Sync>(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (answer, detach): (Option<&ReasonAnswer>, &D),
    now: Timestamp,
) -> Result<Left, LeaveError> {
    if !principal.owner {
        return Err(LeaveError::NotOwner);
    }
    let step = (SensitiveChange::SavedCard, LEAVE_ROUTE);
    require_step_up(db, principal, step, now)
        .await
        .map_err(|_| LeaveError::NeedsCode)?;
    let (mut left, cards) = leave_in_one(db, principal.business, answer, now).await?;
    for card in &cards {
        if detach.detach(card).await.is_err() {
            left.detach_failed += 1;
        }
    }
    if left.detach_failed > 0 {
        tracing::warn!(failed = left.detach_failed, "saved card detach failed");
    }
    Ok(left)
}

fn db_err<E>(_: E) -> LeaveError {
    LeaveError::Database
}

/// The one transaction of [`leave`]; returns what it did and the cards to detach.
async fn leave_in_one(
    db: &mut PgConnection,
    business: Uuid,
    answer: Option<&ReasonAnswer>,
    now: Timestamp,
) -> Result<(Left, Vec<String>), LeaveError> {
    let mut tx = db.begin().await.map_err(db_err)?;
    record_reason(&mut tx, business, (AskedBy::Left, answer), now)
        .await
        .map_err(db_err)?;
    match leave_now(&mut tx, business, now).await {
        Ok(_) => {}
        Err(BusinessesError::Refused) => return Err(LeaveError::AlreadyLeft),
        Err(_) => return Err(LeaveError::Database),
    }
    let picks = end_picks(&mut tx, business, now).await?;
    let quotes = withdraw_for_leaving(&mut tx, business, now)
        .await
        .map_err(db_err)?;
    let cards = remove_saved_cards(&mut tx, BusinessId(business))
        .await
        .map_err(db_err)?;
    let sessions = end_sessions(&mut tx, business, now).await?;
    tx.commit().await.map_err(db_err)?;
    let count = cards.len();
    let left = Left {
        picks,
        quotes,
        cards: count,
        detach_failed: 0,
        sessions,
    };
    Ok((left, cards))
}

/// BIZ-051, PICK-047, CON-029: ends each waiting pick of `business`, with the job's homeowner
/// pause read under the transaction.
async fn end_picks(
    tx: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<Vec<(Uuid, Backup)>, LeaveError> {
    let mut ended = Vec::new();
    for (pick, job) in waiting_picks_of(tx, business).await.map_err(db_err)? {
        let paused = paused_by_homeowner(tx, job).await.map_err(db_err)?;
        let by = (Actor::Business(business), now);
        let backup = end_for_leaving(tx, (pick, job), paused, by)
            .await
            .map_err(db_err)?;
        ended.push((job, backup));
    }
    Ok(ended)
}

/// BIZ-051, SEC-024: uses up every unused sign-in link of `business` and ends every session
/// signed in to it; returns the sessions ended.
async fn end_sessions(
    tx: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<u64, LeaveError> {
    sqlx::query!(
        "UPDATE portals.login_link SET used_at = $2 WHERE business_id = $1 AND used_at IS NULL",
        business,
        now.to_sqlx(),
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let ended = sqlx::query!(
        "DELETE FROM portals.session WHERE id IN \
         (SELECT session_id FROM portals.business_session WHERE business_id = $1)",
        business,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    Ok(ended.rows_affected())
}

/// BIZ-054, D-463: the pause question's answer (or `None`, skipped), stored once, only from the
/// owner or a "Confirm and billing" member while the business has been paused more than 14 days
/// and was never asked; `false` when nothing was stored. Changes no status, card or fee.
///
/// # Errors
///
/// [`LeaveError::Database`].
pub async fn pause_reason(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    answer: Option<&ReasonAnswer>,
    now: Timestamp,
) -> Result<bool, LeaveError> {
    let business = principal.business;
    if !principal.may(business, Need::ConfirmAndBilling) {
        return Ok(false);
    }
    if !pause_ask_due(db, business, PAUSE_ASK_AFTER_DAYS, now)
        .await
        .map_err(db_err)?
    {
        return Ok(false);
    }
    record_reason(db, business, (AskedBy::Paused, answer), now)
        .await
        .map_err(db_err)
}
