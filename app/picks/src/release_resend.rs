//! The owner's "Re-send the release": one more release notice for a release that already stands
//! (PICK-022, PICK-023, PICK-024, SEC-006, PRIV-005; TD-PICK-41, TD-PICK-74; T540, T739, D-074,
//! D-3890, D-3892, D-4727).
//!
//! [`resend_release`] never makes a release. It reads the job's newest `picks.release` row (by
//! `released_at`; only that row is ever re-sent), so a pick with no release, whether paid or not,
//! in safe mode or paused by the homeowner's CANCEL, gets [`Resent::NotReleased`] and nothing is
//! queued. The row is read joined to its `picks.pick` row, locked as the release claim and the
//! fraud end lock it (D-2627), and re-sent only while that pick's status still shows its release
//! ([`release_shown_codes`]: paid or free): a released pick later marked no-show, refunded as
//! fraud or failed in payment also gets [`Resent::NotReleased`], with nothing queued or audited
//! (D-4727). `release::release`, which makes a release from a proof and answers `Already` for a
//! released pick, is never called here. For a standing release it queues, in one transaction, one
//! [`BUSINESS_NOTICE_KIND`] task with [`release_notice`]'s payload (IDs and the job number only,
//! never a homeowner detail, PICK-023, PICK-024) to the business's pick-and-release addresses,
//! deduplicated per press ([`resend_key`]), and one [`RELEASE_RESEND_ACTION`] audit row of IDs
//! only. A repeated submit of the same press queues nothing more and writes no second row. No
//! release row, charge or fee decision is written.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{NewTask, enqueue};
use askhava_jobs::api::job_number;
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::PicksError;
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::release::release_notice;
use crate::status::release_shown_codes;

/// The audit action of one queued re-send, subject the released pick (SEC-006, D-3890).
pub const RELEASE_RESEND_ACTION: &str = "picks.release_resend";

/// One press of the owner's re-send button: the form's one-time press ID and the owner pressing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResendPress {
    /// The press ID the page minted on its `GET`.
    pub press: Uuid,
    /// The owner record ID, the audit row's actor.
    pub owner: Uuid,
}

/// What one press came to (PICK-022, D-074).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resent {
    /// One release notice queued for this press.
    Queued,
    /// This press was already queued; nothing more.
    Repeat,
    /// The job has no release, or its newest release's pick no longer shows it (D-4727); nothing
    /// queued.
    NotReleased,
    /// The business has no pick-and-release address; nothing queued.
    NoRecipient,
}

/// The dedupe key of `press`'s re-send of `pick`'s release: one notice per press (D-3890).
#[must_use]
pub fn resend_key(pick: Uuid, press: Uuid) -> String {
    format!("{BUSINESS_NOTICE_KIND}.release_resend.{pick}.{press}")
}

/// PICK-022, PICK-023, PICK-024, SEC-006, PRIV-005, D-074, D-4727: re-sends job `job`'s newest
/// standing release notice for `press`, at `now`, only while its pick is still paid or free; see
/// the module docs. Creates no release.
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::Job`] (the job number) or [`PicksError::Audit`].
pub async fn resend_release(
    conn: &mut PgConnection,
    job: Uuid,
    press: ResendPress,
    now: Timestamp,
) -> Result<Resent, PicksError> {
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    let newest = sqlx::query!(
        "SELECT r.pick_id, r.business_id, (p.status = ANY($2)) AS \"shown!\" \
         FROM picks.release r JOIN picks.pick p ON p.id = r.pick_id WHERE r.job_id = $1 \
         ORDER BY r.released_at DESC, r.pick_id DESC LIMIT 1 FOR UPDATE OF p",
        job,
        &release_shown_codes()
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| PicksError::Database)?;
    let Some(row) = newest.filter(|row| row.shown) else {
        return Ok(Resent::NotReleased);
    };
    let (pick, business) = (row.pick_id, row.business_id);
    let to = notice_addresses(&mut tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| PicksError::Database)?;
    if to.is_empty() {
        return Ok(Resent::NoRecipient);
    }
    let number = job_number(&mut tx, job)
        .await
        .map_err(|_| PicksError::Job)?;
    let payload = release_notice(business, pick, &to, number);
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, now);
    notice.dedupe_key = Some(resend_key(pick, press.press));
    let queued = enqueue(&mut tx, &notice)
        .await
        .map_err(|_| PicksError::Database)?;
    if queued.is_none() {
        return Ok(Resent::Repeat);
    }
    let entry = AuditEntry {
        actor: Actor::Owner(press.owner),
        action: RELEASE_RESEND_ACTION,
        subject_kind: "picks.pick",
        subject_id: pick,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| PicksError::Audit)?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(Resent::Queued)
}
