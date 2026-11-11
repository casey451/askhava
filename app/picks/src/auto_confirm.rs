//! The PICK-048 auto-confirm of a pick whose quote's box was ticked (T209; PICK-048, PICK-056,
//! FEE-014, FEE-016, D-401, D-480, D-491).
//!
//! [`auto_confirm_plan`] is the pure rule: only a pick proven by the homeowner's Verify code on
//! the quote page or by her signed-in portal session ([`PickProof`]) may auto-confirm, never a
//! text pick; only while Stripe's written answer is filed and the box is ticked; and the tick is
//! void when its author is no longer the owner or a current "Confirm and billing" member, the saved
//! card changed, or the fee or the authorization version differs from the one shown at the tick.
//! [`auto_confirm`] reads those facts (the tick through `quotes::api`, the written answer, the
//! saved card and the fee fixed at the pick through `payments::api`, the members through
//! `businesses::api`) and either makes the one off-session charge through
//! `payments::api::charge_off_session`, or voids the tick and queues the
//! [`AUTO_CONFIRM_VOID_KIND`] event the business's notice is built from (D-2785). Anything but a
//! charge leaves the pick waiting for the normal Confirm step and window (PICK-019) with the pick
//! notice `place` already queued; a success is released, as for a press, only from Stripe's
//! verified event (PICK-022).

use askhava_businesses::api::members;
use askhava_core::api::{NewTask, enqueue};
use askhava_payments::api::{
    BusinessId, OffSessionCharge, OffSessionOutcome, OffSessionStripe, charge_off_session,
    pick_fee, saved_card_of, stripe_answer_filed,
};
use askhava_quotes::api::{
    AUTO_CONFIRM_VERSION, AutoConfirmTick, Untick, auto_confirm_of, untick_auto_confirm,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::PicksError;
use crate::status::PickStatus;

/// The `core.task` kind of the event that a tick was voided at a pick (IDs and the reason only);
/// the business's notice is built from it once its template exists (D-2785).
pub const AUTO_CONFIRM_VOID_KIND: &str = "picks.auto_confirm_voided";

/// How the homeowner proved the pick (PICK-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickProof {
    /// A Verify code entered on the quote page.
    PageCode,
    /// A signed-in homeowner portal session.
    PortalSession,
    /// A "<number> YES" text: never auto-confirmed.
    Text,
}

/// The facts [`auto_confirm_plan`] decides on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one PICK-048 or PICK-056 condition the rule names"
)]
pub struct AutoFacts {
    /// How the pick was proven.
    pub proof: PickProof,
    /// Stripe's written answer is filed and allows the charge.
    pub filed: bool,
    /// The quote's box is ticked.
    pub ticked: bool,
    /// The pick's fee is charged (above $0); a fee-free pick has nothing to charge.
    pub fee_charged: bool,
    /// The tick's author is the owner or a current "Confirm and billing" member.
    pub author_current: bool,
    /// The tick's card is the business's saved card now.
    pub card_same: bool,
    /// The tick's fee equals the fee fixed at the pick.
    pub fee_same: bool,
    /// The tick's authorization version is the one in force.
    pub version_same: bool,
}

/// Why no auto-confirm was tried, the tick left as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// A text pick, or one proven no other way (PICK-056).
    NotProven,
    /// Stripe's written answer is not filed (PICK-048).
    NotFiled,
    /// The box is not ticked.
    NotTicked,
    /// The pick's fee is $0, or none is stored, or the pick no longer waits.
    NothingToCharge,
}

/// Why a tick is void (PICK-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Void {
    /// The member who ticked it was removed or lost "Confirm and billing".
    AuthorGone,
    /// The saved card changed since the tick.
    CardChanged,
    /// The fee differs from the one shown at the tick.
    FeeChanged,
    /// The authorization version differs from the one shown at the tick.
    VersionChanged,
}

impl Void {
    /// The reason's code in the [`AUTO_CONFIRM_VOID_KIND`] event.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AuthorGone => "author_gone",
            Self::CardChanged => "card_changed",
            Self::FeeChanged => "fee_changed",
            Self::VersionChanged => "version_changed",
        }
    }
}

/// What [`auto_confirm_plan`] says to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPlan {
    /// Make the one off-session charge.
    Charge,
    /// Do nothing; the normal Confirm step runs.
    Skip(Skip),
    /// Void the tick, tell the business; the normal Confirm step runs.
    Void(Void),
}

/// PICK-048, PICK-056, FEE-014, D-401, D-480: the pure auto-confirm rule, in this order: the
/// proof, the filed answer, the tick, a fee to charge, then each void condition.
#[must_use]
pub const fn auto_confirm_plan(f: AutoFacts) -> AutoPlan {
    if matches!(f.proof, PickProof::Text) {
        AutoPlan::Skip(Skip::NotProven)
    } else if !f.filed {
        AutoPlan::Skip(Skip::NotFiled)
    } else if !f.ticked {
        AutoPlan::Skip(Skip::NotTicked)
    } else if !f.fee_charged {
        AutoPlan::Skip(Skip::NothingToCharge)
    } else if !f.author_current {
        AutoPlan::Void(Void::AuthorGone)
    } else if !f.card_same {
        AutoPlan::Void(Void::CardChanged)
    } else if !f.fee_same {
        AutoPlan::Void(Void::FeeChanged)
    } else if !f.version_same {
        AutoPlan::Void(Void::VersionChanged)
    } else {
        AutoPlan::Charge
    }
}

/// What [`auto_confirm`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoConfirmed {
    /// Nothing tried; the normal Confirm step runs.
    Skipped(Skip),
    /// The tick was voided and the business told; the normal Confirm step runs.
    Voided(Void),
    /// The one off-session charge was tried; `payments` says how it went (PICK-048).
    Tried(OffSessionOutcome),
}

/// The pick's job and business, while it waits for Confirm.
async fn waiting(db: &mut PgConnection, pick: Uuid) -> Result<Option<(Uuid, Uuid)>, PicksError> {
    let row = sqlx::query!(
        "SELECT job_id, business_id FROM picks.pick WHERE id = $1 AND status = $2",
        pick,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(row.map(|r| (r.job_id, r.business_id)))
}

/// Reads the facts for `pick` of `business`; the tick and the fee in cents with them.
async fn facts(
    db: &mut PgConnection,
    (pick, business, proof): (Uuid, Uuid, PickProof),
    tick: Option<&AutoConfirmTick>,
) -> Result<(AutoFacts, u64), PicksError> {
    let filed = stripe_answer_filed(db)
        .await
        .map_err(|_| PicksError::Database)?;
    let fee = pick_fee(db, pick)
        .await
        .map_err(|_| PicksError::Database)?
        .filter(|f| f.charged && f.business.0 == business)
        .map_or(0, |f| f.cents);
    let saved = saved_card_of(db, BusinessId(business))
        .await
        .map_err(|_| PicksError::Database)?;
    let team = members(db, business)
        .await
        .map_err(|_| PicksError::Database)?;
    let author_current = tick.is_some_and(|t| {
        team.iter()
            .any(|m| m.id == t.member && (m.owner || m.roles.confirm_and_billing()))
    });
    let facts = AutoFacts {
        proof,
        filed,
        ticked: tick.is_some(),
        fee_charged: fee > 0,
        author_current,
        card_same: tick.is_some_and(|t| saved.as_deref() == Some(t.payment_method.as_str())),
        fee_same: tick.is_some_and(|t| t.fee_cents == fee),
        version_same: tick.is_some_and(|t| t.authorization_version == AUTO_CONFIRM_VERSION),
    };
    Ok((facts, fee))
}

/// PICK-048, PICK-056, FEE-014, FEE-016, D-401, D-480: runs the auto-confirm for `pick` right
/// after the pick that `proof` proved committed: the one off-session charge when
/// [`auto_confirm_plan`] says so, the void and its notice when the tick is void, else nothing.
/// `emergency` is the job's emergency flag (PICK-044). A text pick never charges.
///
/// # Errors
/// [`PicksError::Database`], [`PicksError::Quotes`] or [`PicksError::Outbox`]; a failed charge
/// step is `Database` with nothing charged that `payments` did not record.
pub async fn auto_confirm(
    pool: &PgPool,
    stripe: &(impl OffSessionStripe + Sync),
    secret: &SecretString,
    (pick, proof, emergency): (Uuid, PickProof, bool),
    at: Timestamp,
) -> Result<AutoConfirmed, PicksError> {
    let mut db = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let Some((job, business)) = waiting(&mut db, pick).await? else {
        return Ok(AutoConfirmed::Skipped(Skip::NothingToCharge));
    };
    let tick = auto_confirm_of(&mut db, (job, business))
        .await
        .map_err(|_| PicksError::Quotes)?;
    let (facts, fee) = facts(&mut db, (pick, business, proof), tick.as_ref()).await?;
    drop(db);
    let tick = match (auto_confirm_plan(facts), tick) {
        (AutoPlan::Charge, Some(tick)) => tick,
        (AutoPlan::Void(why), _) => return void(pool, (pick, job, business), why, at).await,
        (AutoPlan::Skip(why), _) => return Ok(AutoConfirmed::Skipped(why)),
        (AutoPlan::Charge, None) => return Ok(AutoConfirmed::Skipped(Skip::NotTicked)),
    };
    let charge = OffSessionCharge {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: u32::try_from(fee).map_err(|_| PicksError::Database)?,
        payment_method: &tick.payment_method,
        emergency,
    };
    let outcome = charge_off_session(pool, stripe, secret, &charge, at)
        .await
        .map_err(|_| PicksError::Database)?;
    if outcome == OffSessionOutcome::CardChanged {
        return void(pool, (pick, job, business), Void::CardChanged, at).await;
    }
    Ok(AutoConfirmed::Tried(outcome))
}

/// PICK-056, D-2785: voids the business's tick on `job` and queues one [`AUTO_CONFIRM_VOID_KIND`]
/// event (IDs and the reason only), in one transaction, once per pick: the business is told from it
/// once a notice template exists (D-2785).
async fn void(
    pool: &PgPool,
    (pick, job, business): (Uuid, Uuid, Uuid),
    why: Void,
    at: Timestamp,
) -> Result<AutoConfirmed, PicksError> {
    let mut tx = pool.begin().await.map_err(|_| PicksError::Database)?;
    untick_auto_confirm(&mut tx, (job, business), Untick::Void, at)
        .await
        .map_err(|_| PicksError::Quotes)?;
    let payload = json!({"pick": pick, "job": job, "business": business, "why": why.code()});
    let mut event = NewTask::new(AUTO_CONFIRM_VOID_KIND, payload, at);
    event.dedupe_key = Some(format!("{AUTO_CONFIRM_VOID_KIND}.{pick}"));
    enqueue(&mut tx, &event)
        .await
        .map_err(|_| PicksError::Outbox)?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(AutoConfirmed::Voided(why))
}
