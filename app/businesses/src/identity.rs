//! The identity chain and automatic activation (BIZ-002, BIZ-003, BIZ-016, BIZ-020; D-059, D-145,
//! D-281, D-313, D-366).
//!
//! Every code goes to a contact from a public record (the state record, or the city license list
//! for a no-license business), never one the business typed: [`RecordContacts`] holds only what
//! the caller read from `licensing`. The chain is a Twilio Verify text to the record phone when it
//! gets texts, then a Verify email to the record email, then a mailed letter, and never a voice
//! call; with no usable record contact the sign-up waits in needs review. [`settle`] activates
//! only when the code and the license or city-list match pass with no flag; the letter path is
//! not a flag by itself (D-281). T196 adds the verified-email check before activation (BIZ-040).

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use subtle::ConstantTimeEq;

use crate::models::{self, BusinessesError};
use crate::rules::{Checks, Event};
use crate::service::apply;
use crate::signup::SignupError;
use crate::status::AccountStatus;

/// The idempotency action of an identity code send (SEC-018).
const SEND_ACTION: &str = "businesses.identity_code";
/// The keyed action the identity letter's six-digit code is derived under.
const LETTER_ACTION: &str = "businesses.identity_letter";

/// The record contacts read from the public record (BIZ-002, D-059); never typed by the business.
#[derive(Debug, Default)]
pub struct RecordContacts {
    /// The record phone, only when it can get texts (a landline is left out, D-366).
    pub texting_phone: Option<SecretString>,
    /// The record email.
    pub email: Option<SecretString>,
    /// Whether the record holds a mailing address.
    pub has_address: bool,
}

/// One step of the identity chain, in order (BIZ-002). There is no voice step (BIZ-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// A Verify text to the record phone.
    Text,
    /// A Verify email to the record email.
    Email,
    /// A letter to the record address.
    Letter,
}

impl Step {
    /// The stored code in `businesses.signup.identity_step`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Email => "email",
            Self::Letter => "letter",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [Self::Text, Self::Email, Self::Letter]
            .into_iter()
            .find(|s| s.code() == code)
    }
}

/// What sends a sign-up to needs review (BIZ-016); the letter path is not one (D-281).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    /// The record does not match what the business entered.
    RecordMismatch,
    /// The name does not match the record or the city list.
    NameMismatch,
    /// A fraud signal.
    FraudSignal,
    /// No record contact can get a code (BIZ-002, D-059).
    NoRecordContact,
}

impl Flag {
    /// The stored code in `businesses.signup.flag`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RecordMismatch => "record_mismatch",
            Self::NameMismatch => "name_mismatch",
            Self::FraudSignal => "fraud_signal",
            Self::NoRecordContact => "no_record_contact",
        }
    }
}

/// BIZ-002: the first chain step after `after` that the record can take, or `None` when no
/// record contact is left.
#[must_use]
pub fn next_step(record: &RecordContacts, after: Option<Step>) -> Option<Step> {
    let usable = [
        (Step::Text, record.texting_phone.is_some()),
        (Step::Email, record.email.is_some()),
        (Step::Letter, record.has_address),
    ];
    usable
        .into_iter()
        .find(|(s, ok)| *ok && after.is_none_or(|a| *s > a))
        .map(|(s, _)| s)
}

/// The six-digit code printed on the identity letter, derived from the business ID under the ID
/// secret, so no code is stored (BIZ-002).
#[must_use]
pub fn letter_code(secret: &SecretString, business: Uuid) -> String {
    let key = IdempotencyKey::derive(secret, LETTER_ACTION, &[&business.to_string()]);
    let n = u64::from_str_radix(&key.as_str()[..12], 16).unwrap_or_default();
    format!("{:06}", n % 1_000_000)
}

/// BIZ-002: sends the next code of the chain after the step already tried and records the step.
/// A letter step is recorded for the Lob mailing, which prints [`letter_code`]. With no record
/// contact at all the sign-up is flagged `NoRecordContact` and waits in needs review; after the
/// last step nothing more is sent. Returns the step taken. Only a waiting sign-up (pending or
/// needs review) gets a code (D-1703).
///
/// # Errors
/// [`SignupError::Verify`] when Verify refuses; [`BusinessesError::Refused`] (in
/// [`SignupError::Businesses`]) for any other status; [`SignupError::Businesses`] otherwise.
pub async fn send_code(
    db: &mut PgConnection,
    (verify, secret): (&impl VerifyAdapter, &SecretString),
    business: Uuid,
    record: &RecordContacts,
    now: Timestamp,
) -> Result<Option<Step>, SignupError> {
    waiting(db, business).await?;
    let tried = models::signup_state(db, business)
        .await?
        .step
        .as_deref()
        .and_then(Step::from_code);
    let Some(step) = next_step(record, tried) else {
        if tried.is_none() {
            record_match(db, business, None, Some(Flag::NoRecordContact), now).await?;
        }
        return Ok(None);
    };
    let to = match step {
        Step::Text => record.texting_phone.as_ref().map(|p| (p, CodeChannel::Sms)),
        Step::Email => record.email.as_ref().map(|e| (e, CodeChannel::Email)),
        Step::Letter => None,
    };
    if let Some((contact, channel)) = to {
        let id = business.to_string();
        let key = IdempotencyKey::derive(secret, SEND_ACTION, &[&id, step.code()]);
        let sent = verify
            .send_code(&key, contact.expose_secret(), channel)
            .await;
        sent.map_err(|_| SignupError::Verify)?;
    }
    models::set_step(db, business, step.code()).await?;
    Ok(Some(step))
}

/// BIZ-002, BIZ-016: checks the code entered for the step reached and, when right, marks the code
/// done and settles the status. Returns whether the code was right. Only a waiting sign-up is
/// checked; the letter code is compared in constant time. This function keeps no attempt count:
/// the sign-up route that calls it MUST limit code attempts per business (D-1703).
///
/// # Errors
/// [`SignupError::Verify`] when Verify fails; [`BusinessesError::Refused`] (in
/// [`SignupError::Businesses`]) for a status that is not waiting; [`SignupError::Businesses`]
/// otherwise.
pub async fn check_code(
    db: &mut PgConnection,
    (verify, secret): (&impl VerifyAdapter, &SecretString),
    business: Uuid,
    record: &RecordContacts,
    (code, now): (&str, Timestamp),
) -> Result<bool, SignupError> {
    waiting(db, business).await?;
    let state = models::signup_state(db, business).await?;
    let right = match state.step.as_deref().and_then(Step::from_code) {
        Some(Step::Text) => check(verify, record.texting_phone.as_ref(), code).await?,
        Some(Step::Email) => check(verify, record.email.as_ref(), code).await?,
        Some(Step::Letter) => {
            let want = letter_code(secret, business);
            bool::from(want.as_bytes().ct_eq(code.as_bytes()))
        }
        None => false,
    };
    if right {
        models::set_code_done(db, business).await?;
        settle(db, business, now).await?;
    }
    Ok(right)
}

/// D-1703: refuses a code send or check unless the sign-up is still waiting.
async fn waiting(db: &mut PgConnection, business: Uuid) -> Result<(), SignupError> {
    if models::read(db, business)
        .await?
        .status
        .takes_identity_code()
    {
        Ok(())
    } else {
        Err(BusinessesError::Refused.into())
    }
}

async fn check(
    verify: &impl VerifyAdapter,
    to: Option<&SecretString>,
    code: &str,
) -> Result<bool, SignupError> {
    let Some(to) = to else { return Ok(false) };
    let ok = verify.check_code(to.expose_secret(), code).await;
    ok.map_err(|_| SignupError::Verify)
}

/// BIZ-016, LIC-011: records the license or city-list match (`None` keeps it) and any flag, then
/// settles; a later daily list showing the name activates a pending business with its code, its
/// join date kept.
///
/// # Errors
/// [`SignupError::Businesses`].
pub async fn record_match(
    db: &mut PgConnection,
    business: Uuid,
    passed: Option<bool>,
    flag: Option<Flag>,
    now: Timestamp,
) -> Result<AccountStatus, SignupError> {
    models::set_match(db, business, passed, flag.map(Flag::code)).await?;
    settle(db, business, now).await
}

/// BIZ-003, BIZ-016: moves a pending sign-up by its checks (active with the code and the match
/// and no flag, needs review on a flag) and stamps when it began waiting (BIZ-020).
///
/// # Errors
/// [`SignupError::Businesses`].
pub async fn settle(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<AccountStatus, SignupError> {
    let checks = models::signup_state(db, business).await?.checks();
    let status = match apply(db, business, Event::Checked(checks), Actor::System, now).await {
        Err(BusinessesError::Refused) => models::read(db, business).await?.status,
        moved => moved?,
    };
    models::start_review(db, business, now).await?;
    Ok(status)
}

/// BIZ-003: Casey approves a needs-review sign-up; it activates only with the code and the match.
///
/// # Errors
/// [`BusinessesError::Refused`] when the code or the match is missing.
pub async fn approve(
    db: &mut PgConnection,
    business: Uuid,
    (owner, now): (Uuid, Timestamp),
) -> Result<AccountStatus, SignupError> {
    let checks: Checks = models::signup_state(db, business).await?.checks();
    let event = Event::OwnerApproved(checks);
    Ok(apply(db, business, event, Actor::Owner(owner), now).await?)
}
