//! The release minter: the only code that turns a payment into a [`ReleaseProof`] (PICK-022,
//! SEC-030, SEC-039, SEC-049, D-488, D-1475).
//!
//! `server` takes `privacy`'s once-per-process mint in `register_handlers()` and hands it to
//! [`Minter::new`]; nothing else in the app holds one. [`Minter::mint`] turns only a
//! [`Releasable`] into a proof, and [`paid_release`] makes one only from a [`VerifiedStripeEvent`], which only a webhook `adapters` signature-checked can become,
//! of type `payment_intent.succeeded`, whose livemode matches the configured mode, whose account
//! (when the event names one) is the configured one, for the pick's own PaymentIntent; the
//! intent is then re-fetched from Stripe and must say succeeded, in the same mode, for exactly
//! the fee the server fixed, with metadata naming this pick (SEC-049). The success is stored on
//! the latest attempt (with its FEE-042 evidence record) before the proof is made, and a pick that
//! has ended, before or while the intent was fetched, is never cleared (D-1478): its verified
//! success and evidence are still stored, and its one late-success refund is queued in that
//! transaction, so it answers `PickEnded` with the money on its way back (FEE-021, D-2290). A redirect to
//! a success page carries no verified event, so it can never make one (SEC-030).
//! [`no_fee_release`] makes one from a recorded $0 decision only: a free first win, a
//! repeat customer or a fee-free job type in the ledger (FEE-010, FEE-013, FEE-045), for the
//! pick's own business and job as its `payments.pick_fee` row stores them (D-2302), never for a
//! pick with a PaymentIntent (D-1478). Releasing
//! once however often a proof arrives is the release's own record (PICK-022).

use askhava_adapters::api::{PaymentIntent, PaymentsAdapter, VerifiedWebhook, WebhookVendor};
use askhava_privacy::api::{ReleaseBasis, ReleaseMint, ReleaseProof};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::charge::latest;
use crate::handlers::record;
use crate::late::late_success;
use crate::models::PickNow;
use crate::pick_fee::pick_fee;
use crate::status::AttemptStatus;

/// The one Stripe event type a paid release is built from (PICK-022).
pub const SUCCEEDED_EVENT: &str = "payment_intent.succeeded";

/// Why no proof was made. No variant carries a Stripe or database message (R5).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReleaseError {
    /// The webhook was not Stripe's, or its body is not a Stripe event.
    #[error("not a Stripe event")]
    NotStripeEvent,
    /// The event is not `payment_intent.succeeded`.
    #[error("the event is not a payment success")]
    NotSuccess,
    /// The event or the fetched intent is from the other mode, or does not say (SEC-039).
    #[error("the event is from the wrong mode")]
    WrongMode,
    /// The event names another Stripe account (SEC-039).
    #[error("the event is from another account")]
    WrongAccount,
    /// The event is for another PaymentIntent, or the pick has none.
    #[error("the event is not for this pick's PaymentIntent")]
    WrongIntent,
    /// The re-fetched intent has not succeeded (SEC-049).
    #[error("the payment has not succeeded")]
    NotPaid,
    /// The amount or currency is not the server's fee (SEC-039).
    #[error("the amount is not the server fee")]
    WrongAmount,
    /// The intent's metadata names another pick (SEC-039).
    #[error("the payment is for another pick")]
    WrongPick,
    /// Stripe could not be asked; the event is tried again later (SEC-049).
    #[error("Stripe could not be reached")]
    Stripe,
    /// No $0 decision is recorded for this business and job, or the pick has a fee to pay
    /// (FEE-036).
    #[error("no fee-free decision is recorded")]
    NoFeeDecision,
    /// The pick has ended (the 4-hour release, the card-fix window, the monthly fee limit or a
    /// refund), so a later success clears nothing (FEE-018, FEE-020, FEE-041, D-1478).
    #[error("the pick has ended")]
    PickEnded,
    /// The database refused or dropped the statement.
    #[error("the payments database statement failed")]
    Database,
}

/// A Stripe event from a webhook whose signature `adapters` checked: only
/// [`VerifiedStripeEvent::from_webhook`] makes one, from a [`VerifiedWebhook`], which only
/// `adapters::verify_webhook` makes (SEC-016, SEC-030, SEC-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStripeEvent {
    kind: String,
    livemode: bool,
    account: Option<String>,
    intent: String,
}

#[derive(serde::Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    livemode: bool,
    #[serde(default)]
    account: Option<String>,
    data: EventData,
}

#[derive(serde::Deserialize)]
struct EventData {
    object: EventObject,
}

#[derive(serde::Deserialize)]
struct EventObject {
    id: String,
}

impl VerifiedStripeEvent {
    /// Reads the event in a checked Stripe webhook (SEC-039).
    ///
    /// # Errors
    /// [`ReleaseError::NotStripeEvent`] for another vendor's webhook or a body without the event
    /// type, livemode and object ID.
    pub fn from_webhook(hook: &VerifiedWebhook) -> Result<Self, ReleaseError> {
        if hook.vendor() != WebhookVendor::Stripe {
            return Err(ReleaseError::NotStripeEvent);
        }
        let event: Event =
            serde_json::from_slice(hook.body()).map_err(|_| ReleaseError::NotStripeEvent)?;
        Ok(Self {
            kind: event.kind,
            livemode: event.livemode,
            account: event.account,
            intent: event.data.object.id,
        })
    }

    /// The event type, such as `payment_intent.succeeded`.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
}

/// The Stripe mode and account this process is configured for (`APP_ENV` live or not, and the
/// pinned `acct_…`, SEC-036, SEC-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripeAccount {
    /// True only in the live environment.
    pub live: bool,
    /// The configured `acct_…`.
    pub account: String,
}

/// A pick cleared for release, and why: made only by [`paid_release`] and [`no_fee_release`],
/// with private fields and no `Clone`, so the one way to a [`ReleaseProof`] is through those
/// checks and [`Minter::mint`] (PICK-022, SEC-030).
#[derive(Debug, PartialEq, Eq)]
pub struct Releasable {
    pick: Uuid,
    basis: ReleaseBasis,
}

impl Releasable {
    /// The pick cleared.
    #[must_use]
    pub fn pick(&self) -> Uuid {
        self.pick
    }

    /// Why: a paid charge or no fee due.
    #[must_use]
    pub fn basis(&self) -> ReleaseBasis {
        self.basis
    }
}

/// Holds `privacy`'s release mint; the only maker of [`ReleaseProof`]s (SEC-030, D-488).
#[derive(Debug)]
pub struct Minter {
    mint: ReleaseMint,
}

impl Minter {
    /// Wraps the mint `server` took once in `register_handlers()` (D-488).
    #[must_use]
    pub fn new(mint: ReleaseMint) -> Self {
        Self { mint }
    }

    /// The proof for a cleared pick and its homeowner's contact `contact`, which `picks` names
    /// (SEC-030, D-753); consuming `cleared` so one clearing makes one proof.
    #[must_use]
    pub fn mint(&self, cleared: Releasable, contact: Uuid) -> ReleaseProof {
        self.mint.mint(cleared.pick, contact, cleared.basis)
    }
}

/// Clears pick `pick` for a paid release from a verified success event, after re-fetching the
/// intent and storing the success (PICK-022, SEC-030, SEC-039, SEC-049); see the module docs. No
/// connection is held across the fetch. A pick that has ended gets its success stored and its
/// late-success refund queued, then `PickEnded` (FEE-021, D-2290).
///
/// # Errors
/// The [`ReleaseError`] of the first check that fails; nothing is cleared.
pub async fn paid_release(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    account: &StripeAccount,
    event: &VerifiedStripeEvent,
    pick: Uuid,
    at: Timestamp,
) -> Result<Releasable, ReleaseError> {
    check_event(event, account)?;
    let (stored, cents, _) = pick_intent(pool, pick).await?;
    if stored != event.intent {
        return Err(ReleaseError::WrongIntent);
    }
    let fetched = stripe
        .fetch_payment_intent(&stored)
        .await
        .map_err(|_| ReleaseError::Stripe)?;
    check_intent(&fetched, account, cents, pick)?;
    store_success(pool, pick, at).await?;
    if pick_intent(pool, pick).await?.2 {
        ended_late(pool, pick, at).await?;
        return Err(ReleaseError::PickEnded);
    }
    let basis = ReleaseBasis::PaidCharge;
    Ok(Releasable { pick, basis })
}

/// Clears pick `pick` with no fee due, only when the ledger holds the $0 decision for the pick's
/// own business and job: a free first win, a repeat customer or a fee-free job type (FEE-010,
/// FEE-012, FEE-013, FEE-036, FEE-045, PICK-022). The pick's own pair is its `payments.pick_fee`
/// row, fixed at the pick (D-1871): a pick with no row, or a caller pair other than the row's, is
/// refused before the ledger is read, so a waiver for one job of a business never clears a pick
/// of another job (D-2302, D-2713). A pick with a PaymentIntent has a fee to pay and is never
/// cleared here (D-1478).
///
/// # Errors
/// [`ReleaseError::NoFeeDecision`]; [`ReleaseError::Database`].
pub async fn no_fee_release(
    conn: &mut PgConnection,
    pick: Uuid,
    (business, job): (Uuid, Uuid),
) -> Result<Releasable, ReleaseError> {
    let stored = pick_fee(conn, pick)
        .await
        .map_err(|_| ReleaseError::Database)?
        .ok_or(ReleaseError::NoFeeDecision)?;
    if (stored.business.0, stored.job) != (business, job) {
        return Err(ReleaseError::NoFeeDecision);
    }
    let decided = sqlx::query_scalar!(
        r#"SELECT NOT EXISTS (SELECT 1 FROM payments.pick_intent WHERE pick_id = $3)
             AND EXISTS (SELECT 1 FROM payments.ledger_entry WHERE business_id = $1
             AND job_id = $2 AND amount_cents = 0
             AND kind IN ('free-first-win', 'repeat-customer', 'fee-free-job-type')) AS "found!""#,
        stored.business.0,
        stored.job,
        pick
    )
    .fetch_one(conn)
    .await
    .map_err(|_| ReleaseError::Database)?;
    if !decided {
        return Err(ReleaseError::NoFeeDecision);
    }
    let basis = ReleaseBasis::NoFeeDue;
    Ok(Releasable { pick, basis })
}

/// The event's own checks: type, mode and account (SEC-039). Stripe names `account` only on
/// events of another account; an event without one came over this account's signed endpoint and
/// is re-fetched with this account's key (D-1475).
fn check_event(event: &VerifiedStripeEvent, account: &StripeAccount) -> Result<(), ReleaseError> {
    if event.kind != SUCCEEDED_EVENT {
        return Err(ReleaseError::NotSuccess);
    }
    if event.livemode != account.live {
        return Err(ReleaseError::WrongMode);
    }
    if event
        .account
        .as_ref()
        .is_some_and(|a| *a != account.account)
    {
        return Err(ReleaseError::WrongAccount);
    }
    Ok(())
}

/// The re-fetched intent's checks: succeeded, mode (a missing livemode never matches), the
/// server's fee in dollars, and this pick (SEC-039, SEC-049).
fn check_intent(
    found: &PaymentIntent,
    account: &StripeAccount,
    cents: i64,
    pick: Uuid,
) -> Result<(), ReleaseError> {
    if !AttemptStatus::from_intent(&found.status).paid() {
        return Err(ReleaseError::NotPaid);
    }
    if found.livemode != Some(account.live) {
        return Err(ReleaseError::WrongMode);
    }
    if found.amount != cents || found.currency != "usd" {
        return Err(ReleaseError::WrongAmount);
    }
    if found.metadata.pick_id != pick.to_string() {
        return Err(ReleaseError::WrongPick);
    }
    Ok(())
}

/// The pick's stored PaymentIntent, the fee it was made for and whether the pick has ended
/// (FEE-018, FEE-020, FEE-041, D-1478).
async fn pick_intent(pool: &PgPool, pick: Uuid) -> Result<(String, i64, bool), ReleaseError> {
    let row = sqlx::query!(
        "SELECT stripe_intent_id, amount_cents, ended_at IS NOT NULL AS \"ended!\"
         FROM payments.pick_intent WHERE pick_id = $1",
        pick
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| ReleaseError::Database)?
    .ok_or(ReleaseError::WrongIntent)?;
    Ok((row.stripe_intent_id, row.amount_cents, row.ended))
}

/// A verified success stored on a pick that has ended clears nothing; its one late-success refund
/// and alert are queued (FEE-021, FEE-026, D-2290). `record` already did so in the transaction
/// that stored the success when the pick had ended by then; this repeat, which writes nothing
/// then, covers a pick that ended after that transaction read it.
async fn ended_late(pool: &PgPool, pick: Uuid, at: Timestamp) -> Result<(), ReleaseError> {
    let mut tx = pool.begin().await.map_err(|_| ReleaseError::Database)?;
    late_success(&mut tx, pick, PickNow::Payable, at)
        .await
        .map_err(|_| ReleaseError::Database)?;
    tx.commit().await.map_err(|_| ReleaseError::Database)
}

/// Stores the verified success on the pick's latest attempt, with its evidence record, when the
/// confirm answer never arrived (FEE-037, FEE-042); an attempt already succeeded stays.
async fn store_success(pool: &PgPool, pick: Uuid, at: Timestamp) -> Result<(), ReleaseError> {
    let mut conn = pool.acquire().await.map_err(|_| ReleaseError::Database)?;
    let last = latest(&mut conn, pick)
        .await
        .map_err(|_| ReleaseError::Database)?
        .ok_or(ReleaseError::WrongIntent)?;
    drop(conn);
    record(pool, pick, last.try_no, AttemptStatus::Succeeded, at)
        .await
        .map_err(|_| ReleaseError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use askhava_privacy::api::{ReleaseBasis, release_mint};
    use sqlx::types::Uuid;

    use super::{Minter, Releasable};

    /// SEC-030, D-488: this test binary's one mint; a cleared pick becomes a proof naming that
    /// pick, the contact `picks` gave and the basis of the clearing (TD-SEC-11 no-fee case).
    #[test]
    fn a_cleared_pick_mints_one_proof_for_its_contact() {
        let minter = Minter::new(release_mint());
        let (pick, contact) = (Uuid::from_u128(0x189a), Uuid::from_u128(0x189c));
        for basis in [ReleaseBasis::PaidCharge, ReleaseBasis::NoFeeDue] {
            let proof = minter.mint(Releasable { pick, basis }, contact);
            assert_eq!(proof.pick_id(), pick);
            assert_eq!(proof.contact_id(), contact);
            assert_eq!(proof.basis(), basis);
        }
    }
}
