//! The identity chain and automatic activation (BIZ-002, BIZ-003, BIZ-016, BIZ-020; D-059, D-145,
//! D-281, D-313, D-366).
//!
//! Every code goes to a contact from a public record (the state record, or the city license list
//! for a no-license business), never one the business typed: [`RecordContacts`] holds only what
//! the caller read from `licensing`. The chain is a Twilio Verify text to the record phone when it
//! gets texts, then a Verify email to the record email, then a mailed letter, and never a voice
//! call; with no usable record contact the sign-up waits in needs review. [`settle`] activates
//! only when the code and the license or city-list match pass with no flag; the letter path is
//! not a flag by itself (D-281). Activation also waits while the sign-up email has an
//! unverified [`crate::email_verification`] check (BIZ-040, T196).
//!
//! The identity episode (D-1712, D-1756): each step stores a keyed digest of the destination it
//! last went to; a send to a changed destination raises `identity_attempt`, which joins the
//! Verify idempotency key and the letter code and resets the wrong-code count, so an identical
//! retry keeps its key and a new record phone, email or address gets a new one.

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;
use subtle::ConstantTimeEq;

use crate::email_verification::signup_email_waits;
use crate::models::{self, BusinessesError};
use crate::rules::{Checks, Event};
use crate::service::apply;
use crate::signup::{RecordSeen, SignupError};
use crate::status::AccountStatus;

/// The idempotency action of an identity code send (SEC-018).
const SEND_ACTION: &str = "businesses.identity_code";
/// The keyed action the identity letter's six-digit code is derived under.
const LETTER_ACTION: &str = "businesses.identity_letter";
/// The keyed action of a step's destination digest (D-1756).
const DEST_ACTION: &str = "businesses.identity_dest";
/// Wrong identity codes allowed per episode before every check fails (D-1703, D-1756).
pub const IDENTITY_CODE_TRIES: i32 = 5;

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

/// The six-digit code printed on the first episode's identity letter, derived from the business ID
/// under the ID secret, so no code is stored (BIZ-002).
#[must_use]
pub fn letter_code(secret: &SecretString, business: Uuid) -> String {
    episode_letter_code(secret, business, 0)
}

/// The letter code of identity episode `attempt` (BIZ-002, D-1756): a new record address gets a
/// new code; episode 0 is [`letter_code`].
#[must_use]
pub fn episode_letter_code(secret: &SecretString, business: Uuid, attempt: i32) -> String {
    let (id, n) = (business.to_string(), attempt.to_string());
    let parts: &[&str] = if attempt == 0 { &[&id] } else { &[&id, &n] };
    let key = IdempotencyKey::derive(secret, LETTER_ACTION, parts);
    let n = u64::from_str_radix(&key.as_str()[..12], 16).unwrap_or_default();
    format!("{:06}", n % 1_000_000)
}

/// BIZ-002: sends the next code of the chain after the step already tried and records the step,
/// as [`send_code_seen`] with no record address (the letter's episode then never changes).
///
/// # Errors
/// As [`send_code_seen`].
pub async fn send_code(
    db: &mut PgConnection,
    keys: (&impl VerifyAdapter, &SecretString),
    business: Uuid,
    record: &RecordContacts,
    now: Timestamp,
) -> Result<Option<Step>, SignupError> {
    let seen = RecordSeen {
        contacts: record,
        address: None,
    };
    send_code_seen(db, keys, business, seen, now).await
}

/// BIZ-002, D-1756: sends the next code of the chain after the step already tried and records the
/// step. A letter step is recorded for the Lob mailing, which prints [`episode_letter_code`].
/// With no record contact at all the sign-up is flagged `NoRecordContact`; after the last step
/// nothing more is sent. The Verify key carries the episode, raised only when this step's
/// destination changed. Only a waiting sign-up gets a code (D-1703).
///
/// # Errors
/// [`SignupError::Verify`] when Verify refuses; [`BusinessesError::Refused`] (in
/// [`SignupError::Businesses`]) for any other status; [`SignupError::Businesses`] otherwise.
pub async fn send_code_seen(
    db: &mut PgConnection,
    (verify, secret): (&impl VerifyAdapter, &SecretString),
    business: Uuid,
    seen: RecordSeen<'_>,
    now: Timestamp,
) -> Result<Option<Step>, SignupError> {
    waiting(db, business).await?;
    let tried = models::signup_state(db, business).await?.step;
    let tried = tried.as_deref().and_then(Step::from_code);
    let Some(step) = next_step(seen.contacts, tried) else {
        if tried.is_none() {
            record_match(db, business, None, Some(Flag::NoRecordContact), now).await?;
        }
        return Ok(None);
    };
    let record = seen.contacts;
    let to = match step {
        Step::Text => record
            .texting_phone
            .as_ref()
            .map(|p| (p, Some(CodeChannel::Sms))),
        Step::Email => record.email.as_ref().map(|e| (e, Some(CodeChannel::Email))),
        Step::Letter => seen.address.map(|a| (a, None)),
    };
    let id = business.to_string();
    let dest = to.map(|(v, _)| dest_key(secret, &id, step, v));
    let attempt = episode(db, business, step, dest.as_deref()).await?;
    if let Some((contact, Some(channel))) = to {
        let n = attempt.to_string();
        let parts: &[&str] = if attempt == 0 {
            &[&id, step.code()]
        } else {
            &[&id, step.code(), &n]
        };
        let key = IdempotencyKey::derive(secret, SEND_ACTION, parts);
        let sent = verify
            .send_code(&key, contact.expose_secret(), channel)
            .await;
        sent.map_err(|_| SignupError::Verify)?;
    }
    models::set_step(db, business, step.code()).await?;
    Ok(Some(step))
}

fn dest_key(secret: &SecretString, id: &str, step: Step, to: &SecretString) -> String {
    let parts = [id, step.code(), to.expose_secret()];
    IdempotencyKey::derive(secret, DEST_ACTION, &parts)
        .as_str()
        .to_owned()
}

/// D-1756: stores the step's destination digest and returns the episode, raised (and the
/// wrong-code count reset) only when the step last went to a different destination. One guarded
/// `UPDATE`, so two concurrent sends to one destination raise it once.
async fn episode(
    db: &mut PgConnection,
    business: Uuid,
    step: Step,
    dest: Option<&str>,
) -> Result<i32, BusinessesError> {
    let q = sqlx::query_scalar!(
        "UPDATE businesses.signup SET \
         identity_attempt = identity_attempt + CASE WHEN COALESCE(CASE $2 WHEN 'text' THEN \
           sent_text_key WHEN 'email' THEN sent_mail_key ELSE sent_letter_key END <> $3, false) \
           THEN 1 ELSE 0 END, \
         code_tries = CASE WHEN COALESCE(CASE $2 WHEN 'text' THEN sent_text_key WHEN 'email' \
           THEN sent_mail_key ELSE sent_letter_key END <> $3, false) THEN 0 ELSE code_tries END, \
         sent_text_key = CASE WHEN $2 = 'text' THEN COALESCE($3, sent_text_key) ELSE sent_text_key END, \
         sent_mail_key = CASE WHEN $2 = 'email' THEN COALESCE($3, sent_mail_key) ELSE sent_mail_key END, \
         sent_letter_key = CASE WHEN $2 = 'letter' THEN COALESCE($3, sent_letter_key) \
           ELSE sent_letter_key END \
         WHERE business_id = $1 RETURNING identity_attempt",
        business,
        step.code(),
        dest
    );
    q.fetch_one(db).await.map_err(|_| BusinessesError::Database)
}

/// BIZ-002, BIZ-016: checks the code entered for the step reached and, when right, marks the code
/// done and settles the status. Returns whether the code was right. Only a waiting sign-up is
/// checked; the letter code is the episode's, compared in constant time. After
/// [`IDENTITY_CODE_TRIES`] wrong codes in one episode every check is false until a send to a new
/// destination starts the next episode (D-1703, D-1756).
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
    let Some(attempt) = take_try(db, business).await? else {
        return Ok(false);
    };
    let right = match state.step.as_deref().and_then(Step::from_code) {
        Some(Step::Text) => check(verify, record.texting_phone.as_ref(), code).await?,
        Some(Step::Email) => check(verify, record.email.as_ref(), code).await?,
        Some(Step::Letter) => {
            let want = episode_letter_code(secret, business, attempt);
            bool::from(want.as_bytes().ct_eq(code.as_bytes()))
        }
        None => false,
    };
    if right {
        let q = sqlx::query!(
            "UPDATE businesses.signup SET code_tries = code_tries - 1 \
             WHERE business_id = $1 AND code_tries > 0",
            business
        );
        q.execute(&mut *db)
            .await
            .map_err(|_| BusinessesError::Database)?;
        models::set_code_done(db, business).await?;
        settle(db, business, now).await?;
    }
    Ok(right)
}

/// D-1703, D-1756: takes one of the episode's [`IDENTITY_CODE_TRIES`] in one guarded `UPDATE`
/// before the code is checked, so parallel guesses cannot pass the cap; a right code gives it back.
/// Returns the episode, or `None` when no try is left.
async fn take_try(db: &mut PgConnection, business: Uuid) -> Result<Option<i32>, BusinessesError> {
    let q = sqlx::query_scalar!(
        "UPDATE businesses.signup SET code_tries = code_tries + 1 \
         WHERE business_id = $1 AND code_tries < $2 RETURNING identity_attempt",
        business,
        IDENTITY_CODE_TRIES
    );
    q.fetch_optional(db)
        .await
        .map_err(|_| BusinessesError::Database)
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

/// BIZ-003, BIZ-016, BIZ-040: moves a pending sign-up by its checks (active with the code and the
/// match and no flag, needs review on a flag) and stamps when it began waiting (BIZ-020). While
/// the sign-up email's check is unverified nothing activates; a flag still moves it to review.
///
/// # Errors
/// [`SignupError::Businesses`].
pub async fn settle(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<AccountStatus, SignupError> {
    let checks = models::signup_state(db, business).await?.checks();
    let held = !checks.flagged && signup_email_waits(db, business).await?;
    let moved = if held {
        Err(BusinessesError::Refused)
    } else {
        apply(db, business, Event::Checked(checks), Actor::System, now).await
    };
    let status = match moved {
        Err(BusinessesError::Refused) => models::read(db, business).await?.status,
        moved => moved?,
    };
    models::start_review(db, business, now).await?;
    Ok(status)
}

/// BIZ-003: Casey approves a needs-review sign-up; it activates only with the code and the match.
///
/// The sign-up email must be verified first (BIZ-040).
///
/// # Errors
/// [`BusinessesError::Refused`] when the code or the match is missing or the email is unverified.
pub async fn approve(
    db: &mut PgConnection,
    business: Uuid,
    (owner, now): (Uuid, Timestamp),
) -> Result<AccountStatus, SignupError> {
    if signup_email_waits(db, business).await? {
        return Err(BusinessesError::Refused.into());
    }
    let checks: Checks = models::signup_state(db, business).await?.checks();
    let event = Event::OwnerApproved(checks);
    Ok(apply(db, business, event, Actor::Owner(owner), now).await?)
}
