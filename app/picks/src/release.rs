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
//! `settle::retry_line` runs (T254, D-1688). The who's-calling text (TXT-027) is not sent here
//! yet (D-1687).

use askhava_adapters::api::MaskedPool;
use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_calls::api::{Connected, assign_number};
use askhava_core::api::{NewTask, enqueue};
use askhava_jobs::api::{current, job_number};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_privacy::api::{
    ContactDetails, Field, ReleaseBasis, ReleaseProof, Vault, read_for_release,
};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection, PgPool};

use crate::models::{self, PicksError, ReleaseTarget};
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::settle::LINE_RETRY_KIND;
use crate::status::release_status;

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
/// claim committed, so the release stands and a later proof gets [`Released::Already`]; every
/// error also queues the [`LINE_RETRY_KIND`] task, so that release's line still opens (D-1739). A line
/// that fails to open is no error: the view still comes back, one High
/// [`RELEASE_LINE_FAILED_ALERT`] is raised and one [`LINE_RETRY_KIND`] task queued (D-1688).
pub async fn release(
    pool: &PgPool,
    (vault, masked): (&Vault, &MaskedPool),
    (proof, email_chosen): (ReleaseProof, bool),
    at: Timestamp,
) -> Result<Released, PicksError> {
    let (pick, contact, basis) = (proof.pick_id(), proof.contact_id(), proof.basis());
    let released = match show(pool, vault, (proof, email_chosen), at).await {
        Ok(released) => released,
        Err(error) => {
            line_later(pool, pick, at, false).await;
            return Err(error);
        }
    };
    if let Released::Shown(_) = released
        && open_line(pool, masked, (pick, contact, basis), at)
            .await
            .is_err()
    {
        line_later(pool, pick, at, true).await;
    }
    Ok(released)
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

/// Queues one [`LINE_RETRY_KIND`] task for the pick (deduped per pick) and, when `failed` (a shown
/// release whose line did not open), raises one High [`RELEASE_LINE_FAILED_ALERT`] with it, in one
/// transaction. A release that failed after its claim (the vault read) also queues the retry, so
/// its line still opens; the retry does nothing for a pick with no release row. The release stands
/// or fails regardless, so a failed write is dropped here (CALL-001, D-1688, D-1739).
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
    if failed && raise_alert(&mut tx, &alert).await.is_err() {
        return;
    }
    if enqueue(&mut tx, &retry).await.is_ok() {
        tx.commit().await.ok();
    }
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
    if !record(conn, (pick, target), binding, at).await? {
        return Ok(Released::Already);
    }
    let details = open(conn, vault, proof, email_chosen).await?;
    Ok(Released::Shown(ReleaseView::from_details(details)))
}

/// An emergency job: the vault is read to learn whether the address is in; without it nothing is
/// released and she is asked again, else the release is claimed (INT-029, TD-INT-34). Two racing
/// proofs may both read; only the one whose claim wins is shown (D-1564).
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
    Ok(if record(conn, (pick, target), binding, at).await? {
        Released::Shown(ReleaseView::from_details(details))
    } else {
        Released::Already
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

/// Stores the one release row, bound to the proof's contact and her email choice (D-1739), and
/// queues the notice in one transaction, or one High [`RELEASE_NO_RECIPIENT_ALERT`] when the
/// business has no recipient; false when another call released first (PICK-022, PICK-024, R11,
/// D-1561).
async fn record(
    conn: &mut PgConnection,
    (pick, target): (Uuid, &ReleaseTarget),
    (basis, contact, email_chosen): (ReleaseBasis, Uuid, bool),
    at: Timestamp,
) -> Result<bool, PicksError> {
    let code = match basis {
        ReleaseBasis::PaidCharge => "paid",
        ReleaseBasis::NoFeeDue => "no_fee",
    };
    let (job, business) = (target.job, target.business);
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    let bound = (code, contact, email_chosen);
    if !models::insert_release(&mut tx, (pick, job, business), bound, at).await? {
        return Ok(false);
    }
    let to = notice_addresses(&mut tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| PicksError::Database)?;
    if to.is_empty() {
        let alert = NewAlert {
            kind: RELEASE_NO_RECIPIENT_ALERT,
            severity: Severity::High,
            subject: Some(("pick", pick)),
        };
        raise_alert(&mut tx, &alert)
            .await
            .map_err(|_| PicksError::Database)?;
    } else {
        let number = job_number(&mut tx, job)
            .await
            .map_err(|_| PicksError::Job)?;
        let mut notice = NewTask::new(
            BUSINESS_NOTICE_KIND,
            release_notice(business, pick, &to, number),
            at,
        );
        notice.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{RELEASE_TEMPLATE}.{pick}"));
        enqueue(&mut tx, &notice)
            .await
            .map_err(|_| PicksError::Database)?;
    }
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(true)
}
