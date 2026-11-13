//! The release: the homeowner's first name, job address and shared email, for the signed-in
//! business portal, only for a `ReleaseProof`, once (PICK-022 to PICK-024, PRIV-005, PRIV-020,
//! BIZ-014, D-454, D-491, D-492).
//!
//! `payments` alone turns a verified succeeded charge or a recorded $0 fee decision into a
//! `privacy::api::ReleaseProof` (SEC-030); [`release`] takes it by value and gives it to
//! `privacy::api::read_for_release` with [`RELEASE_FIELDS`], plus the email only when she chose
//! email contact at the pick ([`EMAIL_CHOSEN_FIELDS`], PICK-018); neither includes the phone
//! (PICK-023); [`ReleaseView`] has no phone field. The pure [`plan_release`] decides: a pick
//! already released gives nothing again, and an ended pick or one whose status is not paid (for a
//! paid proof) or free (for a $0 proof) gives nothing, as a backstop to the proof (D-1564). A
//! regular job claims its one `picks.release` row before the vault is opened, so a racing second
//! proof opens nothing. On an emergency job the vault must be read to learn whether the street
//! address is in; with none, nothing is released: the first such acceptance is stored in
//! `picks.address_wait` and one [`ADDRESS_NEEDED_KIND`] event asks `intake` to ask the homeowner
//! again at once, once per pick (INT-029, TD-INT-34). The claim stores the release row and, in the
//! same transaction, one [`BUSINESS_NOTICE_KIND`] task to the business's pick-and-release
//! recipients (`businesses::api::notice_addresses`, opaque address IDs) with the job number only
//! ([`release_notice`]), or one High [`RELEASE_NO_RECIPIENT_ALERT`] when the business has none; no
//! homeowner detail and no call window goes in any notice. The [`ReleaseView`] goes back to the
//! caller once and is stored nowhere (D-1564). The claim binds the release row to the proof's
//! contact ID and her email choice (`contact_id`, `mail_chosen`, both paths), so the business
//! portal's later read, `released_view`, reopens that contact and those fields (D-1739).
//!
//! T077: once shown, [`release`] asks `calls::api::assign_number` for the job's masked line, the
//! homeowner side being the proof's contact (CALL-001, PICK-022, D-491). An emergency job with no
//! address is never shown, so no line opens before the address is in (TD-CALL-12); a
//! released-before pick opens nothing again; a line that fails to open never hides the view and
//! raises one High [`RELEASE_LINE_FAILED_ALERT`] instead, with one [`LINE_RETRY_KIND`] task that
//! `settle::retry_line` runs (T254, D-1688); a failed release queues that task only once its
//! claim stands (D-2017).
//!
//! T318 (TXT-027, D-1687, D-3118): after the line is asked for, outside the claim's transaction,
//! a shown release, or a failed one whose claim committed (D-3508), queues the pick's one
//! who's-calling task (`calls::api::queue_whos_calling`, `follow_up`); `calls` sends the text once the line is open, at once on an
//! emergency job, else in her window (QH-004).
//!
//! T278 (JOB-015, D-1842): the claim's transaction also queues the day-3 and day-14 check-ins of
//! a Connected job, from the release and the picked quote's earliest visit
//! ([`check_ins_at_release`]).
//!
//! T281 (D-1845): the claim's transaction also queues the unreachable-homeowner steps 24, 48, 72
//! and 96 hours after it (`unreachable_steps::schedule_steps`), each once per pick.
//!
//! T355 (PICK-022, PRIV-005, FEE-052, D-2627): the claim's transaction first locks the pick's row
//! and re-checks it ([`claim_still_open`]): an ended pick, one out of the proof's status or one
//! with a `picks.fraud_hold` row (written by `unreachable_refund::fraud_warning` before its refund)
//! is `MovedOn`, before any vault read on a regular job; on an emergency job the refused claim
//! follows the read but shows and releases nothing.

use askhava_adapters::api::MaskedPool;
use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_calls::api::{Connected, WhosCalling, assign_number, queue_whos_calling};
use askhava_core::api::{NewTask, enqueue};
use askhava_jobs::api::{current, job_number, schedule_check_ins};
use askhava_ops::api::{NewAlert, Severity, raise_alert, raise_alert_once};
use askhava_privacy::api::{
    ContactDetails, Field, ReleaseBasis, ReleaseProof, Vault, read_for_release,
};
use askhava_quotes::api::earliest_visit;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection, PgPool};

use crate::models::{self, PicksError, ReleaseTarget};
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::settle::LINE_RETRY_KIND;
use crate::status::{PickStatus, release_status};
use crate::unreachable_steps::schedule_steps;

/// The release email's template (PICK-024, D-454): job number and sign-in link only.
pub const RELEASE_TEMPLATE: &str = "release_notice";
/// The release text for a business that chose text (PICK-024, TXT-036, D-306).
pub const RELEASE_TEXT_TEMPLATE: &str = "release_text";
/// The event asking `intake` to ask the homeowner for her address again (INT-029, D-492).
pub const ADDRESS_NEEDED_KIND: &str = "picks.address_needed";
/// The High alert raised when a released pick's business has no pick-and-release recipient
/// (PICK-024, D-1561, D-1564).
pub const RELEASE_NO_RECIPIENT_ALERT: &str = "picks.release_no_recipient";
/// The High alert raised when a shown release's masked line fails to open; the view still goes
/// back, and one `picks.line_retry` task opens the line later (CALL-001, D-1688).
pub const RELEASE_LINE_FAILED_ALERT: &str = "picks.release_line_failed";

/// The High alert raised when a claimed release's who's-calling task could not be queued, about
/// the pick, once per pick (TXT-027, D-3508).
pub const WHOS_CALLING_FAILED_ALERT: &str = "picks.whos_calling_failed";
/// The vault fields a release opens; never the phone (PICK-023, PRIV-020).
pub const RELEASE_FIELDS: [Field; 2] = [Field::Name, Field::Address];
/// The fields when the homeowner chose email contact at the pick (PICK-018); never the phone.
pub const EMAIL_CHOSEN_FIELDS: [Field; 3] = [Field::Name, Field::Address, Field::Email];

/// What [`plan_release`] reads: the pick's record and, once read, whether the address is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseFacts {
    /// The pick already has its `picks.release` row.
    pub released: bool,
    /// The pick ended (expired, replaced, withdrawn).
    pub ended: bool,
    /// The pick's status is paid for a paid proof or free for a $0 proof (PICK-022, D-1564).
    pub confirmed: bool,
    /// The homeowner chose emergency (INT-029).
    pub emergency: bool,
    /// Whether the vault holds a street address; `None` before the vault is read.
    pub address_in: Option<bool>,
}

/// The release step for a pick that holds a proof (PICK-022, INT-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseStep {
    /// Released before: nothing again (PICK-022 "exactly once").
    Already,
    /// The pick ended or is not paid or free: nothing is released.
    MovedOn,
    /// An emergency job: read the release fields from the vault, then plan again.
    Read,
    /// An emergency job with no address yet: release nothing, ask again (TD-INT-34).
    WaitForAddress,
    /// Claim the release, queue the notice, then read the vault for the view.
    Release,
}

/// The pure release rule (PICK-022, INT-029, D-492, D-1564); see the module docs.
#[must_use]
pub const fn plan_release(facts: ReleaseFacts) -> ReleaseStep {
    if facts.released {
        return ReleaseStep::Already;
    }
    if facts.ended || !facts.confirmed {
        return ReleaseStep::MovedOn;
    }
    match (facts.emergency, facts.address_in) {
        (true, None) => ReleaseStep::Read,
        (true, Some(false)) => ReleaseStep::WaitForAddress,
        (false, _) | (true, Some(true)) => ReleaseStep::Release,
    }
}

/// What the signed-in business portal shows: first name, address and the shared email; there is
/// no phone field (PICK-022, PICK-023, PRIV-020). `Debug` prints no value (PRIV-016).
#[derive(Debug)]
pub struct ReleaseView {
    first_name: Option<SecretString>,
    address: Option<SecretString>,
    email: Option<SecretString>,
}

impl ReleaseView {
    /// The view of a release read: the first word of the name; the phone is dropped (PICK-023).
    #[must_use]
    pub fn from_details(details: ContactDetails) -> Self {
        let first = |name: SecretString| {
            let word = name.expose_secret().split_whitespace().next()?.to_owned();
            Some(SecretString::from(word))
        };
        Self {
            first_name: details.name.and_then(first),
            address: details.address,
            email: details.email,
        }
    }

    /// The homeowner's first name.
    #[must_use]
    pub fn first_name(&self) -> Option<&SecretString> {
        self.first_name.as_ref()
    }

    /// The job address.
    #[must_use]
    pub fn address(&self) -> Option<&SecretString> {
        self.address.as_ref()
    }

    /// The email she chose to share (PICK-018).
    #[must_use]
    pub fn email(&self) -> Option<&SecretString> {
        self.email.as_ref()
    }
}

/// What [`release`] did.
#[derive(Debug)]
pub enum Released {
    /// Released now: the view, given to the caller this once and stored nowhere (D-1564).
    Shown(ReleaseView),
    /// Released before; nothing again.
    Already,
    /// An emergency job's address is not in; nothing released, the homeowner asked again.
    WaitingForAddress,
    /// The pick is unknown, ended, or not paid or free; nothing released.
    MovedOn,
}

/// The release notice's payload: templates, IDs and the job number only, never a homeowner detail
/// or a call window (PICK-024, D-454, D-491).
#[must_use]
pub fn release_notice(business: Uuid, pick: Uuid, to: &[Uuid], job: i64) -> Value {
    json!({
        "template": RELEASE_TEMPLATE, "text_template": RELEASE_TEXT_TEMPLATE,
        "business": business, "pick": pick, "to": to, "values": {"job": job},
    })
}

/// Releases the pick `proof` names (PICK-022 to PICK-024, PRIV-005, BIZ-014, INT-029, D-1564);
/// see the module docs; once shown, the job's masked line opens from `masked` (CALL-001,
/// D-1688). `email_chosen` is the homeowner's PICK-018 choice of email contact, the
/// only case the email is opened. The vault is read on a bare connection, outside any transaction
/// (D-753).
///
/// # Errors
/// [`PicksError::Database`] (also for a refused or failed vault read, which `privacy` has
/// already audited or alerted) or [`PicksError::Job`]. A regular job's vault read fails after its
/// claim committed, so the release stands and a later proof gets [`Released::Already`]; an
/// error after the claim also queues the [`LINE_RETRY_KIND`] task and the who's-calling task
/// (D-3508), so that release's line still opens and its text still goes, and one before the claim queues nothing (D-1739, D-2017). A line
/// that fails to open is no error: the view still comes back, one High
/// [`RELEASE_LINE_FAILED_ALERT`] is raised and one [`LINE_RETRY_KIND`] task queued (D-1688).
pub async fn release(
    pool: &PgPool,
    (vault, masked): (&Vault, &MaskedPool),
    (proof, email_chosen): (ReleaseProof, bool),
    at: Timestamp,
) -> Result<Released, PicksError> {
    let ids = (proof.pick_id(), proof.contact_id(), proof.basis());
    let released = show(pool, vault, (proof, email_chosen), at).await;
    let shown = match &released {
        Ok(Released::Shown(_)) => Shown::Yes,
        Ok(_) => Shown::No,
        Err(_) => Shown::Failed,
    };
    follow_up(pool, masked, (ids, email_chosen), shown, at).await;
    released
}

/// How [`show`] ended, for [`follow_up`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    /// `Released::Shown`: the claim committed and the view came back.
    Yes,
    /// `Already`, `WaitingForAddress` or `MovedOn`: nothing follows.
    No,
    /// An error, before or after the claim; only a committed claim gets a follow-up.
    Failed,
}

/// What follows a release, outside its transaction (CALL-001, TXT-027, D-1688, D-3508): a shown
/// release opens the line (else [`line_later`] with its alert) and queues the who's-calling task;
/// a failed one whose claim stands queues the line retry and the who's-calling task too, since a
/// later proof gets `Already` and queues neither.
pub(crate) async fn follow_up(
    pool: &PgPool,
    masked: &MaskedPool,
    ((pick, contact, basis), email_chosen): ((Uuid, Uuid, ReleaseBasis), bool),
    shown: Shown,
    at: Timestamp,
) {
    match shown {
        Shown::No => return,
        Shown::Yes => {
            if open_line(pool, masked, (pick, contact, basis), at)
                .await
                .is_err()
            {
                line_later(pool, pick, at, true).await;
            }
        }
        Shown::Failed => line_later(pool, pick, at, false).await,
    }
    whos_calling_later(pool, (pick, contact, basis, email_chosen), at).await;
}

/// TXT-027, D-1687, D-3118, D-3508: queues the pick's one `calls.whos_calling` task through
/// `calls::api::queue_whos_calling`, after the claim committed and the line was asked for, never
/// in the claim's transaction; its handler sends once the line is open (now, or after a
/// `picks.line_retry` opens it). One dedupe key per pick, so a repeated release queues none; a
/// pick with no `picks.release` row queues none. The release stands regardless, so a failed
/// write raises one High [`WHOS_CALLING_FAILED_ALERT`] about the pick instead.
async fn whos_calling_later(
    pool: &PgPool,
    (pick, contact, basis, email_chosen): (Uuid, Uuid, ReleaseBasis, bool),
    at: Timestamp,
) {
    let Ok(mut conn) = pool.acquire().await else {
        return;
    };
    let ids = (pick, contact, basis, email_chosen);
    if queue_released(&mut conn, ids, at).await.is_err() {
        let subject = ("pick", pick);
        raise_alert_once(
            &mut conn,
            WHOS_CALLING_FAILED_ALERT,
            Severity::High,
            subject,
        )
        .await
        .ok();
    }
}

/// Queues the who's-calling task of a claimed pick; an unknown or unclaimed pick queues nothing
/// (TXT-027, D-3508).
async fn queue_released(
    conn: &mut PgConnection,
    (pick, contact, basis, email_chosen): (Uuid, Uuid, ReleaseBasis, bool),
    at: Timestamp,
) -> Result<(), PicksError> {
    let target = models::release_target(conn, pick, release_status(basis)).await?;
    let Some(target) = target.filter(|t| t.released) else {
        return Ok(());
    };
    let job = current(conn, target.job)
        .await
        .map_err(|_| PicksError::Database)?;
    let job_number = job_number(conn, target.job)
        .await
        .map_err(|_| PicksError::Database)?;
    let call = WhosCalling {
        pick,
        job: target.job,
        job_number,
        business: target.business,
        homeowner: contact,
        email_contact: email_chosen,
        emergency: job.emergency,
    };
    queue_whos_calling(conn, &call, at)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(())
}

/// Opens the released job's masked line, the homeowner side being the proof's contact (CALL-001).
async fn open_line(
    pool: &PgPool,
    masked: &MaskedPool,
    (pick, contact, basis): (Uuid, Uuid, ReleaseBasis),
    at: Timestamp,
) -> Result<(), PicksError> {
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let target = models::release_target(&mut conn, pick, release_status(basis)).await?;
    let target = target.ok_or(PicksError::Database)?;
    let job = Connected {
        job: target.job,
        homeowner: contact,
        business: target.business,
    };
    assign_number(&mut conn, masked, job, at)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(())
}

/// Queues one [`LINE_RETRY_KIND`] task for the pick (deduped per pick, once ever) and, when
/// `failed` (a shown release whose line did not open), raises one High
/// [`RELEASE_LINE_FAILED_ALERT`] with it, in one transaction. A release that failed after its
/// claim (the vault read) also queues the retry, so its line still opens; one that failed before
/// the claim (no `picks.release` row yet) queues nothing, so the one dedupe key is never spent on
/// a retry with nothing to open (D-2017). The release stands or fails regardless, so a failed
/// write is dropped here (CALL-001, D-1688, D-1739).
async fn line_later(pool: &PgPool, pick: Uuid, at: Timestamp, failed: bool) {
    let alert = NewAlert {
        kind: RELEASE_LINE_FAILED_ALERT,
        severity: Severity::High,
        subject: Some(("pick", pick)),
    };
    let mut retry = NewTask::new(LINE_RETRY_KIND, json!({"pick": pick}), at);
    retry.dedupe_key = Some(format!("{LINE_RETRY_KIND}.{pick}"));
    let Ok(mut tx) = pool.begin().await else {
        return;
    };
    if !matches!(claimed(&mut tx, pick).await, Ok(true)) {
        return;
    }
    if failed && raise_alert(&mut tx, &alert).await.is_err() {
        return;
    }
    if enqueue(&mut tx, &retry).await.is_ok() {
        tx.commit().await.ok();
    }
}

/// Whether pick `pick` has its `picks.release` row, the claim (PICK-022, D-2017).
async fn claimed(conn: &mut PgConnection, pick: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM picks.release WHERE pick_id = $1) AS "claimed!""#,
        pick
    )
    .fetch_one(conn)
    .await
}

/// The T069 release: the checks, the claim, the vault read and the view (see the module docs).
async fn show(
    pool: &PgPool,
    vault: &Vault,
    (proof, email_chosen): (ReleaseProof, bool),
    at: Timestamp,
) -> Result<Released, PicksError> {
    let pick = proof.pick_id();
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let Some(target) =
        models::release_target(&mut conn, pick, release_status(proof.basis())).await?
    else {
        return Ok(Released::MovedOn);
    };
    let row = current(&mut conn, target.job)
        .await
        .map_err(|_| PicksError::Job)?;
    let facts = ReleaseFacts {
        released: target.released,
        ended: target.ended,
        confirmed: target.confirmed,
        emergency: row.emergency,
        address_in: None,
    };
    let read = (proof, email_chosen);
    match plan_release(facts) {
        ReleaseStep::Already => Ok(Released::Already),
        ReleaseStep::MovedOn => Ok(Released::MovedOn),
        ReleaseStep::Release => claim_then_read(&mut conn, vault, read, (pick, &target), at).await,
        ReleaseStep::Read | ReleaseStep::WaitForAddress => {
            read_then_claim(&mut conn, vault, read, (facts, pick, &target), at).await
        }
    }
}

/// A regular job: the release row and notice commit first, then the vault is read (D-1564).
async fn claim_then_read(
    conn: &mut PgConnection,
    vault: &Vault,
    (proof, email_chosen): (ReleaseProof, bool),
    (pick, target): (Uuid, &ReleaseTarget),
    at: Timestamp,
) -> Result<Released, PicksError> {
    let binding = (proof.basis(), proof.contact_id(), email_chosen);
    match record(conn, (pick, target), binding, at).await? {
        Claim::Claimed => {}
        Claim::Already => return Ok(Released::Already),
        Claim::MovedOn => return Ok(Released::MovedOn),
    }
    let details = open(conn, vault, proof, email_chosen).await?;
    Ok(Released::Shown(ReleaseView::from_details(details)))
}

/// An emergency job: the vault is read to learn whether the address is in; without it nothing is
/// released and she is asked again, else the release is claimed (INT-029, TD-INT-34). Two racing
/// proofs may both read; only the one whose claim wins is shown (D-1564). A claim refused under
/// the pick's lock (ended, moved on or held for a fraud warning) follows the read but shows and
/// releases nothing (D-2627).
async fn read_then_claim(
    conn: &mut PgConnection,
    vault: &Vault,
    (proof, email_chosen): (ReleaseProof, bool),
    (mut facts, pick, target): (ReleaseFacts, Uuid, &ReleaseTarget),
    at: Timestamp,
) -> Result<Released, PicksError> {
    let binding = (proof.basis(), proof.contact_id(), email_chosen);
    let details = open(conn, vault, proof, email_chosen).await?;
    facts.address_in = Some(details.address.is_some());
    if plan_release(facts) == ReleaseStep::WaitForAddress {
        ask_again(conn, (pick, target.job), at).await?;
        return Ok(Released::WaitingForAddress);
    }
    Ok(match record(conn, (pick, target), binding, at).await? {
        Claim::Claimed => Released::Shown(ReleaseView::from_details(details)),
        Claim::Already => Released::Already,
        Claim::MovedOn => Released::MovedOn,
    })
}

/// Reads the release fields, with the email only when she chose email contact (PICK-018, PICK-023).
async fn open(
    conn: &mut PgConnection,
    vault: &Vault,
    proof: ReleaseProof,
    email_chosen: bool,
) -> Result<ContactDetails, PicksError> {
    let fields: &[Field] = if email_chosen {
        &EMAIL_CHOSEN_FIELDS
    } else {
        &RELEASE_FIELDS
    };
    read_for_release(conn, vault, proof, fields)
        .await
        .map_err(|_| PicksError::Database)
}

/// Stores the wait and queues the ask-again event, once per pick (INT-029, TD-INT-34, D-1564).
async fn ask_again(
    conn: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    at: Timestamp,
) -> Result<(), PicksError> {
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    if models::insert_address_wait(&mut tx, (pick, job), at).await? {
        let mut task = NewTask::new(ADDRESS_NEEDED_KIND, json!({"job": job, "pick": pick}), at);
        task.dedupe_key = Some(format!("{ADDRESS_NEEDED_KIND}.{pick}"));
        enqueue(&mut tx, &task)
            .await
            .map_err(|_| PicksError::Database)?;
    }
    tx.commit().await.map_err(|_| PicksError::Database)
}

/// JOB-015, D-1811 (2), D-1842: when job `job` is Connected, queues its day-3 and day-14
/// check-ins (`jobs::api::schedule_check_ins`) in the caller's transaction, from the release at
/// `at` and `business`'s quote's earliest visit (`quotes::api::earliest_visit`); `true` when
/// queued. A job not Connected gets none: nothing moves a job to Connected yet (D-2015 (1)), and
/// a check-in tagged with an earlier version would never send (D-1812). The release's claim
/// calls it, so a second release of the pick queues nothing more; the dedupe key is per job,
/// version and day anyway.
///
/// # Errors
/// [`PicksError::Job`] or [`PicksError::Quotes`].
pub async fn check_ins_at_release(
    tx: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    at: Timestamp,
) -> Result<bool, PicksError> {
    let row = current(tx, job).await.map_err(|_| PicksError::Job)?;
    if !row.status.takes_check_ins() {
        return Ok(false);
    }
    let visit = earliest_visit(tx, (job, business))
        .await
        .map_err(|_| PicksError::Quotes)?;
    schedule_check_ins(tx, job, at, visit)
        .await
        .map_err(|_| PicksError::Job)?;
    Ok(true)
}

/// What the claim found under the pick's row lock (PICK-022, FEE-052, D-2627).
enum Claim {
    /// The release row and its notice committed now.
    Claimed,
    /// Another call released the pick first.
    Already,
    /// The pick ended, left the wanted status or holds a fraud warning: nothing released.
    MovedOn,
}

/// PICK-022, PRIV-005, FEE-052, D-2627: in the caller's transaction, locks pick `pick`'s row
/// (`SELECT ... FOR UPDATE`, the lock the fraud path takes too), then, in a separate statement
/// under that lock, answers whether it still has status `want`, no end and no `picks.fraud_hold`
/// row; an unknown pick answers `false`. The release claim asks it before its row is written, so
/// a pick whose fraud warning committed its hold is never released, and a fraud warning that
/// waits on a claim's lock then sees the claim's release row. It writes nothing.
///
/// # Errors
/// `Database`.
pub async fn claim_still_open(
    tx: &mut PgConnection,
    pick: Uuid,
    want: PickStatus,
) -> Result<bool, PicksError> {
    let locked = sqlx::query_scalar!("SELECT id FROM picks.pick WHERE id = $1 FOR UPDATE", pick)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| PicksError::Database)?;
    if locked.is_none() {
        return Ok(false);
    }
    sqlx::query_scalar!(
        r#"SELECT (p.status = $2 AND p.ended_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM picks.fraud_hold h WHERE h.pick_id = p.id)) AS "open!"
           FROM picks.pick p WHERE p.id = $1"#,
        pick,
        want.code(),
    )
    .fetch_one(tx)
    .await
    .map_err(|_| PicksError::Database)
}

/// Stores the one release row, bound to the proof's contact and her email choice (D-1739), and
/// queues the notice in one transaction, with the check-ins of a Connected job
/// ([`check_ins_at_release`]), or one High [`RELEASE_NO_RECIPIENT_ALERT`] when the
/// business has no recipient; `Already` when another call released first (PICK-022, PICK-024,
/// R11, D-1561). It first takes the pick's row lock through [`claim_still_open`] and answers
/// `MovedOn`, writing nothing, for a pick ended, no longer in the proof's status or held by a
/// fraud warning (FEE-052, D-2627).
async fn record(
    conn: &mut PgConnection,
    (pick, target): (Uuid, &ReleaseTarget),
    (basis, contact, email_chosen): (ReleaseBasis, Uuid, bool),
    at: Timestamp,
) -> Result<Claim, PicksError> {
    let code = match basis {
        ReleaseBasis::PaidCharge => "paid",
        ReleaseBasis::NoFeeDue => "no_fee",
    };
    let (job, business) = (target.job, target.business);
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    if !claim_still_open(&mut tx, pick, release_status(basis)).await? {
        return Ok(Claim::MovedOn);
    }
    let bound = (code, contact, email_chosen);
    if !models::insert_release(&mut tx, (pick, job, business), bound, at).await? {
        return Ok(Claim::Already);
    }
    check_ins_at_release(&mut tx, (job, business), at).await?;
    schedule_steps(&mut tx, pick, at).await?;
    notify_business(&mut tx, (pick, job, business), at).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(Claim::Claimed)
}

/// Queues the release notice to `business`'s pick-and-release recipients, or raises one High
/// [`RELEASE_NO_RECIPIENT_ALERT`] when it has none, in the claim's transaction (PICK-024, D-1561).
async fn notify_business(
    tx: &mut PgConnection,
    (pick, job, business): (Uuid, Uuid, Uuid),
    at: Timestamp,
) -> Result<(), PicksError> {
    let to = notice_addresses(tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| PicksError::Database)?;
    if to.is_empty() {
        let alert = NewAlert {
            kind: RELEASE_NO_RECIPIENT_ALERT,
            severity: Severity::High,
            subject: Some(("pick", pick)),
        };
        raise_alert(tx, &alert)
            .await
            .map_err(|_| PicksError::Database)?;
    } else {
        let number = job_number(tx, job).await.map_err(|_| PicksError::Job)?;
        let mut notice = NewTask::new(
            BUSINESS_NOTICE_KIND,
            release_notice(business, pick, &to, number),
            at,
        );
        notice.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{RELEASE_TEMPLATE}.{pick}"));
        enqueue(tx, &notice)
            .await
            .map_err(|_| PicksError::Database)?;
    }
    Ok(())
}
