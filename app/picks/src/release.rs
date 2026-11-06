//! The release: the homeowner's first name, job address and shared email, shown only in the
//! signed-in business portal, only for a `ReleaseProof`, once (PICK-022 to PICK-024, PRIV-005,
//! PRIV-020, BIZ-014, D-454, D-491, D-492).
//!
//! `payments` alone turns a verified succeeded charge or a recorded $0 fee decision into a
//! `privacy::api::ReleaseProof` (SEC-030); [`release`] takes it by value and gives it to
//! `privacy::api::read_for_release` with [`RELEASE_FIELDS`], plus the email only when she chose
//! email contact at the pick ([`EMAIL_CHOSEN_FIELDS`], PICK-018); neither includes the phone
//! (PICK-023); [`ReleaseView`] has no phone field. The pure [`plan_release`] decides: a pick
//! already released gives nothing again, an ended pick nothing, and on an emergency job whose
//! street address is not in yet nothing is released: the acceptance is stored in
//! `picks.address_wait` and one [`ADDRESS_NEEDED_KIND`] event asks `intake` to ask the homeowner
//! again at once (INT-029, TD-INT-34). A release stores its one `picks.release` row and, in the
//! same transaction, one [`BUSINESS_NOTICE_KIND`] task to the business's pick-and-release
//! recipients (`businesses::api::notice_addresses`, opaque address IDs) with the job number only:
//! the email says "Job <n> is confirmed. Sign in to see the homeowner's details." and a text
//! "The details are at askhava.com/business." ([`release_notice`]); no homeowner detail and no
//! call window goes in any notice.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{NewTask, enqueue};
use askhava_jobs::api::{current, job_number};
use askhava_privacy::api::{
    ContactDetails, Field, ReleaseBasis, ReleaseProof, Vault, read_for_release,
};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection, PgPool};

use crate::models::{self, PicksError};
use crate::money_notices::BUSINESS_NOTICE_KIND;

/// The release email's template (PICK-024, D-454): job number and sign-in link only.
pub const RELEASE_TEMPLATE: &str = "release_notice";
/// The release text for a business that chose text (PICK-024, TXT-036, D-306).
pub const RELEASE_TEXT_TEMPLATE: &str = "release_text";
/// The event asking `intake` to ask the homeowner for her address again (INT-029, D-492).
pub const ADDRESS_NEEDED_KIND: &str = "picks.address_needed";
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
    /// The pick ended: nothing is released.
    MovedOn,
    /// Read the release fields from the vault, then plan again.
    Read,
    /// An emergency job with no address yet: release nothing, ask again (TD-INT-34).
    WaitForAddress,
    /// Release to the portal and queue the notice.
    Release,
}

/// The pure release rule (PICK-022, INT-029, D-492); see the module docs.
#[must_use]
pub const fn plan_release(facts: ReleaseFacts) -> ReleaseStep {
    if facts.released {
        return ReleaseStep::Already;
    }
    if facts.ended {
        return ReleaseStep::MovedOn;
    }
    match facts.address_in {
        None => ReleaseStep::Read,
        Some(false) if facts.emergency => ReleaseStep::WaitForAddress,
        Some(_) => ReleaseStep::Release,
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
    /// Released now: the portal shows this view once.
    Shown(ReleaseView),
    /// Released before; nothing again.
    Already,
    /// An emergency job's address is not in; nothing released, the homeowner asked again.
    WaitingForAddress,
    /// The pick is unknown or ended; nothing released.
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

/// Releases the pick `proof` names (PICK-022 to PICK-024, PRIV-005, BIZ-014, INT-029); see the
/// module docs. `email_chosen` is the homeowner's PICK-018 choice of email contact, the only case
/// the email is opened. The vault is read on a bare connection, outside any transaction (D-753).
///
/// # Errors
/// [`PicksError::Database`] (also for a refused or failed vault read, which `privacy` has
/// already audited or alerted) or [`PicksError::Job`]; nothing is released.
pub async fn release(
    pool: &PgPool,
    vault: &Vault,
    (proof, email_chosen): (ReleaseProof, bool),
    at: Timestamp,
) -> Result<Released, PicksError> {
    let pick = proof.pick_id();
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let Some((job, business, ended, released)) = models::release_target(&mut conn, pick).await?
    else {
        return Ok(Released::MovedOn);
    };
    let row = current(&mut conn, job).await.map_err(|_| PicksError::Job)?;
    let mut facts = ReleaseFacts {
        released,
        ended,
        emergency: row.emergency,
        address_in: None,
    };
    match plan_release(facts) {
        ReleaseStep::Already => return Ok(Released::Already),
        ReleaseStep::MovedOn => return Ok(Released::MovedOn),
        ReleaseStep::Read | ReleaseStep::WaitForAddress | ReleaseStep::Release => {}
    }
    let basis = proof.basis();
    let fields: &[Field] = if email_chosen {
        &EMAIL_CHOSEN_FIELDS
    } else {
        &RELEASE_FIELDS
    };
    let details = read_for_release(&mut conn, vault, proof, fields)
        .await
        .map_err(|_| PicksError::Database)?;
    facts.address_in = Some(details.address.is_some());
    if plan_release(facts) == ReleaseStep::WaitForAddress {
        ask_again(&mut conn, (pick, job), at).await?;
        return Ok(Released::WaitingForAddress);
    }
    let new = record(&mut conn, (pick, job, business), basis, at).await?;
    Ok(if new {
        Released::Shown(ReleaseView::from_details(details))
    } else {
        Released::Already
    })
}

/// Stores the wait and queues the ask-again event, once per pick (INT-029, TD-INT-34).
async fn ask_again(
    conn: &mut PgConnection,
    (pick, job): (Uuid, Uuid),
    at: Timestamp,
) -> Result<(), PicksError> {
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    models::insert_address_wait(&mut tx, (pick, job), at).await?;
    let mut task = NewTask::new(ADDRESS_NEEDED_KIND, json!({"job": job, "pick": pick}), at);
    task.dedupe_key = Some(format!("{ADDRESS_NEEDED_KIND}.{pick}"));
    enqueue(&mut tx, &task)
        .await
        .map_err(|_| PicksError::Database)?;
    tx.commit().await.map_err(|_| PicksError::Database)
}

/// Stores the one release row and queues the notice in one transaction; false when another call
/// released first (PICK-022, PICK-024, R11).
async fn record(
    conn: &mut PgConnection,
    (pick, job, business): (Uuid, Uuid, Uuid),
    basis: ReleaseBasis,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let code = match basis {
        ReleaseBasis::PaidCharge => "paid",
        ReleaseBasis::NoFeeDue => "no_fee",
    };
    let mut tx = conn.begin().await.map_err(|_| PicksError::Database)?;
    if !models::insert_release(&mut tx, (pick, job, business), code, at).await? {
        return Ok(false);
    }
    let to = notice_addresses(&mut tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| PicksError::Database)?;
    let number = job_number(&mut tx, job)
        .await
        .map_err(|_| PicksError::Job)?;
    let payload = release_notice(business, pick, &to, number);
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, at);
    notice.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{RELEASE_TEMPLATE}.{pick}"));
    enqueue(&mut tx, &notice)
        .await
        .map_err(|_| PicksError::Database)?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    Ok(true)
}
