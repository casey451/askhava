//! The Confirm route's $0 decision and its one release (FEE-010, FEE-012, FEE-013, FEE-045,
//! PICK-022; D-152, D-195, D-2302, D-3100, D-3779, D-3813; T494).
//!
//! [`join_dates`] builds `payments::api::JoinDates` from T486's readers: `joined` is the sign-up's
//! submit time (`businesses::api::join_facts`, D-152), `trades_opened` the latest switch-on time
//! of each sign-up trade, on or off now (`catalog::api::trades_opened`, D-3779), `launch` the
//! launch-date setting the caller passes. A business with no sign-up row joins at
//! `Timestamp::MAX`, and no launch setting is a launch at `Timestamp::MAX`, so neither ever gives
//! a free first win: test and live never wrongly waive.
//!
//! [`decide_at_confirm`] runs `payments::api::waive_at_confirm` on those dates on the caller's
//! Confirm transaction, before any charge, and answers [`ConfirmFee::NoFee`] for a free first win,
//! a repeat customer or a fee-free job type, else [`ConfirmFee::Charge`]. After that transaction
//! commits, [`release_no_fee`] takes `payments::api::no_fee_release` for the pick's own business
//! and job and hands the `Releasable` to the `ReleaseSink` (in production `server`'s
//! `PickReleaser`, which settles and releases once). A press repeated after a crash between the
//! two finds the waiver already in the ledger (`Appended::Already`), writes nothing new and hands
//! the release again; the sink's release row keeps it to one release. The route adds no reader or
//! SQL of its own.

use askhava_businesses::api::join_facts;
use askhava_catalog::api::trades_opened;
use askhava_payments::api::{
    BusinessId, FreeWinError, JoinDates, PickedFee, ReleaseError, ReleaseSink, Waiver, WinCheck,
    no_fee_release, waive_at_confirm,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The pick being confirmed: what [`decide_at_confirm`] and [`release_no_fee`] need.
#[derive(Clone, Copy, Debug)]
pub struct ConfirmedPick<'a> {
    /// The pick.
    pub pick: Uuid,
    /// The business confirming it, the pick's own (its `payments.pick_fee` row).
    pub business: BusinessId,
    /// The pick's job.
    pub job: Uuid,
    /// The introduction fee by job size fixed at the pick (FEE-005).
    pub picked: &'a PickedFee,
    /// The customer's other jobs still on record (FEE-013).
    pub same_customer_jobs: &'a [Uuid],
}

/// What Confirm does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmFee {
    /// A fee is due: the route charges it.
    Charge,
    /// No fee is due ($0 recorded): the route calls [`release_no_fee`]. `Waiver::NotWaived` here
    /// is a fee-free job type (FEE-045).
    NoFee(Waiver),
}

/// Why the $0 decision or its release failed. No variant carries a value (R5, PRIV-016).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfirmWaiverError {
    /// The sign-up or category read failed.
    #[error("the join dates could not be read")]
    JoinDates,
    /// `waive_at_confirm` failed; `FreeWinError::Race` means press again.
    #[error("the free first win check failed")]
    FreeWin(#[from] FreeWinError),
    /// `no_fee_release` refused or the sink failed.
    #[error("the no-fee release failed")]
    Release(#[from] ReleaseError),
}

/// FEE-010, D-152, D-195, D-3779, D-3813: the dates `business` is judged by, with `launch` the
/// launch-date setting (`None`: no launch window). No sign-up row joins at `Timestamp::MAX`, so
/// no free first win. Read-only.
///
/// # Errors
/// [`ConfirmWaiverError::JoinDates`] when a read fails.
pub async fn join_dates(
    conn: &mut PgConnection,
    business: BusinessId,
    launch: Option<Timestamp>,
) -> Result<JoinDates, ConfirmWaiverError> {
    let facts = join_facts(conn, business.0)
        .await
        .map_err(|_| ConfirmWaiverError::JoinDates)?;
    let (joined, opened) = match facts {
        Some(facts) => {
            let opened = trades_opened(conn, facts.trades())
                .await
                .map_err(|_| ConfirmWaiverError::JoinDates)?;
            (facts.joined(), opened)
        }
        None => (Timestamp::MAX, Vec::new()),
    };
    Ok(JoinDates {
        joined,
        launch: launch.unwrap_or(Timestamp::MAX),
        trades_opened: opened,
    })
}

/// FEE-010 to FEE-013, FEE-045, D-3813: reads the join dates and runs `waive_at_confirm` on
/// `conn`, the caller's Confirm transaction, before any charge; a repeat for the same job writes
/// nothing new (D-1422). `secret` is the entity-key secret.
///
/// # Errors
/// [`ConfirmWaiverError::JoinDates`]; [`ConfirmWaiverError::FreeWin`].
pub async fn decide_at_confirm(
    conn: &mut PgConnection,
    secret: &SecretString,
    confirmed: ConfirmedPick<'_>,
    launch: Option<Timestamp>,
) -> Result<ConfirmFee, ConfirmWaiverError> {
    let dates = join_dates(conn, confirmed.business, launch).await?;
    let check = WinCheck {
        business: confirmed.business,
        job: confirmed.job,
        picked: confirmed.picked,
        dates: &dates,
        same_customer_jobs: confirmed.same_customer_jobs,
    };
    let waiver = waive_at_confirm(conn, secret, check).await?;
    Ok(match waiver {
        Waiver::NotWaived if confirmed.picked.cents() > 0 => ConfirmFee::Charge,
        decided => ConfirmFee::NoFee(decided),
    })
}

/// FEE-012, PICK-022, D-2302, D-3100: after the $0 decision committed, clears the pick through
/// `no_fee_release` with its own business and job and hands it to `sink` once per call; the sink
/// releases once however often it is called.
///
/// # Errors
/// [`ConfirmWaiverError::Release`]: no $0 decision for the pick's own pair, or the sink failed.
pub async fn release_no_fee<R: ReleaseSink>(
    conn: &mut PgConnection,
    sink: &R,
    confirmed: ConfirmedPick<'_>,
) -> Result<(), ConfirmWaiverError> {
    let pair = (confirmed.business.0, confirmed.job);
    let cleared = no_fee_release(conn, confirmed.pick, pair).await?;
    sink.release(cleared).await?;
    Ok(())
}
