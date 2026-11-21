//! The Confirm press's eligibility re-check and its job move (JOB-013, JOB-014, PICK-019,
//! PICK-020, PICK-044, FEE-020, OPS-024; D-056, D-4349, D-4897, D-5017; T664).
//!
//! [`recheck`] is PICK-020 at Confirm, called by `confirm_pay`'s `pressed` after the fee terms
//! passed and before the terms row, any $0 decision or any charge. It reads the stored results
//! only (D-056, no live lookup): the business's stored account status, and for a job whose
//! category needs an ROC class the latest stored check of `licensing::api::bound_roc_number`
//! through `picks::api::check_at_confirm`. A job with no facts row, an unreadable license kind,
//! or an ROC category with no stored check is a license refusal; a category needing no ROC class
//! skips the ROC freshness check (D-4897). No end or bond date is stored anywhere, so both are
//! `None` (D-5017). On the press that leaves Awaiting confirm the deadline is
//! `picks::api::confirm_deadline` (none: the window has not started, so nothing expires), and a
//! refusal closes the pick's idle PaymentIntent and ends the pick through
//! `picks::api::refuse_confirm` (her backup or back to Quotes ready, with her PICK-047 or
//! PICK-046 text; JOB-013). On a retry press with the job in Payment pending (FEE-020's card-fix
//! hour, 60 minutes on an emergency job, PICK-044, which may run past the window) the window
//! never expires the press, and an account or license refusal ends the pick through
//! `picks::api::end_pending` as `ineligible` (JOB-014). A refusal commits and charges nothing.
//!
//! [`press_move`] is the move to Payment pending (`picks::api::confirm_move`), made in the
//! press's transaction after the $0 or charge decision and before it commits: always on the $0
//! path, and on the paid path only when a charge will be attempted (a `pm_…` card, charges not
//! paused, OPS-024, and a Stripe customer), so the business keeps its Confirm window (PICK-019)
//! until then. A job that left both statuses rolls the press back, so a free first win recorded
//! in it is not used, and the pick ends in a new transaction (D-5002).

use askhava_businesses::api::business;
use askhava_catalog::api::category;
use askhava_jobs::api::{current, job_facts};
use askhava_licensing::api::{LicenseKind, bound_roc_number};
use askhava_ops::api::Actor;
use askhava_payments::api::{ChargeOutcome, end_intent_unless_charging, press_may_charge};
use askhava_picks::api::{
    ConfirmMove, PendingEnd, Refusal, StoredFacts, check_at_confirm, confirm_deadline,
    confirm_move, end_pending, refuse_confirm,
};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::status::press_retries;

/// The press's own transaction.
pub(crate) type Tx = sqlx::Transaction<'static, sqlx::Postgres>;

/// One press: the waiting pick, its job and business, and the press's time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pressed {
    /// The waiting pick.
    pub pick: Uuid,
    /// Its job.
    pub job: Uuid,
    /// Its business.
    pub business: Uuid,
    /// The press's time.
    pub now: Timestamp,
}

/// Why [`press_move`] moved nothing and the press goes no further (D-4349).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// A paid press carried no `pm_…` card (FEE-014).
    NoCard,
    /// Charges are paused (OPS-024).
    Paused,
    /// The business has no Stripe customer and one could not be made.
    NoCustomer,
    /// The job left Awaiting confirm and Payment pending: the pick ended.
    Ended,
}

/// D-4897 (1): whether `job`'s category needs an ROC class, from its facts row and the catalog's
/// license kind; `None` for a job with no facts row, an unknown category or an unreadable kind.
async fn needs_roc(tx: &mut PgConnection, job: Uuid) -> Result<Option<bool>, ()> {
    let Some(facts) = job_facts(tx, job).await.map_err(|_| ())? else {
        return Ok(None);
    };
    let stored = category(tx, &facts.category).await.map_err(|_| ())?;
    let kind = stored.and_then(|c| LicenseKind::parse(&c.license_kind).ok());
    Ok(kind.map(|k| matches!(k, LicenseKind::Roc | LicenseKind::RocAndStateAgency)))
}

/// PICK-020, D-056, D-4897, D-5017: whether `p`'s business may Confirm before `deadline`, from
/// the stored results only (the stored account status, never the session's), through
/// `check_at_confirm`: the window, then the account, then the latest stored check of the bound
/// ROC number (no number has no check, so it is a license refusal). A category needing no ROC
/// class drops the license refusal; a job whose category or kind cannot be read is one.
async fn verdict(
    tx: &mut PgConnection,
    p: Pressed,
    deadline: Timestamp,
) -> Result<Result<(), Refusal>, ()> {
    let facts = StoredFacts {
        account: business(tx, p.business).await.map_err(|_| ())?.status,
        license_ends_on: None,
        bond_cancelled_on: None,
    };
    let roc = needs_roc(tx, p.job).await?;
    let number = bound_roc_number(tx, p.business).await.map_err(|_| ())?;
    let ids = (p.business, number.as_deref().unwrap_or_default());
    let checked = check_at_confirm(tx, ids, facts, (p.now, deadline)).await;
    Ok(match (checked.map_err(|_| ())?, roc) {
        (Err(Refusal::License), Some(false)) => Ok(()),
        (Ok(()), None) => Err(Refusal::License),
        (checked, _) => checked,
    })
}

/// PICK-020, JOB-013, JOB-014, FEE-020, D-4349: the Confirm press's re-check; see the module
/// docs. `Ok(tx)` lets the press go on in its transaction. A refusal ends the pick, commits and
/// answers [`ChargeOutcome::MovedOn`] (the "ended" answer, naming no license detail); a refusal
/// while an attempt of the pick can still succeed changes nothing and answers
/// [`ChargeOutcome::Pending`]. A job in neither status is left to [`press_move`].
///
/// # Errors
/// `()` when a read or the ending fails; the transaction rolls back.
pub(crate) async fn recheck(mut tx: Tx, p: Pressed) -> Result<Result<Tx, ChargeOutcome>, ()> {
    let status = current(&mut tx, p.job).await.map_err(|_| ())?.status;
    let Some(retry) = press_retries(status) else {
        return Ok(Ok(tx));
    };
    let deadline = if retry {
        Timestamp::MAX
    } else {
        let due = confirm_deadline(&mut tx, p.pick).await.map_err(|_| ())?;
        due.unwrap_or(Timestamp::MAX)
    };
    let Err(refusal) = verdict(&mut tx, p, deadline).await? else {
        return Ok(Ok(tx));
    };
    let by = (Actor::Business(p.business), p.now);
    let ids = (p.pick, p.job);
    if retry {
        let ended = end_pending(&mut tx, ids, PendingEnd::Ineligible, by).await;
        ended.map_err(|_| ())?;
    } else {
        let idle = end_intent_unless_charging(&mut tx, p.pick, p.now).await;
        if !idle.map_err(|_| ())? {
            tx.rollback().await.map_err(|_| ())?;
            return Ok(Err(ChargeOutcome::Pending));
        }
        let ended = refuse_confirm(&mut tx, ids, refusal, false, by).await;
        ended.map_err(|_| ())?;
    }
    tx.commit().await.map_err(|_| ())?;
    Ok(Err(ChargeOutcome::MovedOn))
}

/// D-4349, OPS-024: why a paid press (`paid`: its card and whether its business has a Stripe
/// customer) will attempt no charge now; `None` for a $0 press or one that will.
async fn paid_stop(tx: &mut PgConnection, paid: Option<(&str, bool)>) -> Result<Option<Stop>, ()> {
    let Some((card, customer)) = paid else {
        return Ok(None);
    };
    if !card.starts_with("pm_") {
        return Ok(Some(Stop::NoCard));
    }
    if !press_may_charge(tx).await.map_err(|_| ())? {
        return Ok(Some(Stop::Paused));
    }
    Ok((!customer).then_some(Stop::NoCustomer))
}

/// JOB-013, JOB-014, PICK-019, FEE-020, OPS-024, D-4349: the press's move to Payment pending,
/// after the $0 decision (`paid` none) or the charge decision, ending `tx`. A paid press that
/// will attempt no charge commits and moves nothing ([`Stop::NoCard`], [`Stop::Paused`],
/// [`Stop::NoCustomer`]). Otherwise `picks::api::confirm_move` runs and the press commits
/// (`None`: go on to the release or the charge), a job already in Payment pending (a retry)
/// included; a job that left both statuses rolls the press back and ends the pick through
/// `picks::api::end_pending` as `payment_failed` in a new transaction ([`Stop::Ended`], D-5002).
///
/// # Errors
/// `()` when a read, the move or the ending fails.
pub(crate) async fn press_move(
    mut tx: Tx,
    pool: &PgPool,
    p: Pressed,
    paid: Option<(&str, bool)>,
) -> Result<Option<Stop>, ()> {
    if let Some(stop) = paid_stop(&mut tx, paid).await? {
        tx.commit().await.map_err(|_| ())?;
        return Ok(Some(stop));
    }
    let ids = (p.pick, p.job);
    let moved = confirm_move(&mut tx, ids, p.now).await.map_err(|_| ())?;
    if moved != ConfirmMove::Lost {
        tx.commit().await.map_err(|_| ())?;
        return Ok(None);
    }
    tx.rollback().await.map_err(|_| ())?;
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let by = (Actor::Business(p.business), p.now);
    let ended = end_pending(&mut tx, ids, PendingEnd::PaymentFailed, by).await;
    ended.map_err(|_| ())?;
    tx.commit().await.map_err(|_| ())?;
    Ok(Some(Stop::Ended))
}
