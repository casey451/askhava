//! The account checklist's AI records: the owner records the AI provider's commercial API terms
//! and each privacy-policy version, which the AI-008 gate (`switch`) reads (AI-008, AI-018,
//! SEC-006; D-412, D-492, D-4727, D-5534).
//!
//! Each record is one insert and one IDs-only `Actor::Owner` audit row in one transaction. The
//! app role may only insert into `ai.provider_terms` and `ai.privacy_policy_version`, so a record
//! is never edited or deleted: a newer terms row, or a newer policy version, replaces it in the
//! gate's read. The terms' audit subject is the new row's bigint ID as `from_u64_pair(0, id)`;
//! the policy's text key goes nowhere near the audit row, whose subject is `Uuid::nil()`.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::civil::Date;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

/// The audit action of a recorded terms row (SEC-006).
const TERMS_ACTION: &str = "ai.provider_terms_recorded";
/// The audit action of a recorded policy version (SEC-006).
const POLICY_ACTION: &str = "ai.privacy_policy_recorded";

/// The provider's commercial API terms as the owner checked them (AI-008, D-412, D-492). The
/// caller has checked each value against `ai.provider_terms`' constraints (an `https://` URL of
/// at most 300 characters, days 0 to 3,650, cents above 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTerms {
    /// The terms page's URL.
    pub url: String,
    /// The day the owner read them.
    pub checked_on: Date,
    /// Whether they say API inputs are not used for training.
    pub training_free: bool,
    /// How many days the provider keeps API inputs.
    pub retention_days: i32,
    /// How many days the provider may keep text its safety systems flag.
    pub flagged_retention_days: i32,
    /// The monthly spend limit set in the provider's Console, in cents.
    pub spend_limit_cents: i64,
    /// The expected monthly AI cost, in cents.
    pub expected_monthly_cents: i64,
    /// The day the zero-retention agreement was signed, if it has been.
    pub zero_retention_signed_on: Option<Date>,
}

/// One privacy-policy version (AI-008, AI-018): its version name (`^[0-9a-z][0-9a-z.-]{0,31}$`),
/// the day it takes effect and whether it carries the AI-018 sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyVersion {
    /// The version name, such as `2026-11-23`.
    pub version: String,
    /// The day it takes effect, in Arizona.
    pub effective_on: Date,
    /// Whether it carries the AI-018 wording.
    pub has_ai018_sentence: bool,
}

/// Why a record was not stored. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChecklistError {
    /// A policy version with this name is already recorded; nothing changed.
    #[error("this policy version is already recorded")]
    Duplicate,
    /// The database refused or dropped a statement; nothing changed.
    #[error("the ai checklist statement failed")]
    Database,
}

fn store<E>(_: E) -> ChecklistError {
    ChecklistError::Database
}

/// Records `terms` for `owner` with one IDs-only audit row, in one transaction (AI-008, SEC-006).
///
/// # Errors
///
/// [`ChecklistError::Database`]; nothing is stored then.
pub async fn record_provider_terms(
    db: &mut PgConnection,
    terms: &ProviderTerms,
    owner: Uuid,
) -> Result<(), ChecklistError> {
    let mut tx = db.begin().await.map_err(store)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO ai.provider_terms (terms_url, checked_on, inputs_not_used_for_training, \
           retention_days, flagged_retention_days, spend_limit_cents, expected_monthly_cents, \
           zero_retention_signed_on) \
         VALUES ($1, $2::text::date, $3, $4, $5, $6, $7, $8::text::date) RETURNING id",
        terms.url,
        terms.checked_on.to_string(),
        terms.training_free,
        terms.retention_days,
        terms.flagged_retention_days,
        terms.spend_limit_cents,
        terms.expected_monthly_cents,
        terms.zero_retention_signed_on.map(|d| d.to_string()),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(store)?;
    let subject = Uuid::from_u64_pair(0, id.unsigned_abs());
    audited(&mut tx, (TERMS_ACTION, "ai.provider_terms", subject), owner).await?;
    tx.commit().await.map_err(store)
}

/// Records `policy` for `owner` with one IDs-only audit row, in one transaction; a version name
/// already recorded changes nothing (AI-008, AI-018, SEC-006).
///
/// # Errors
///
/// [`ChecklistError::Duplicate`] or [`ChecklistError::Database`]; nothing is stored then.
pub async fn record_policy_version(
    db: &mut PgConnection,
    policy: &PolicyVersion,
    owner: Uuid,
) -> Result<(), ChecklistError> {
    let mut tx = db.begin().await.map_err(store)?;
    let added = sqlx::query_scalar!(
        "INSERT INTO ai.privacy_policy_version (version, effective_on, has_ai018_sentence) \
         VALUES ($1, $2::text::date, $3) ON CONFLICT (version) DO NOTHING RETURNING version",
        policy.version,
        policy.effective_on.to_string(),
        policy.has_ai018_sentence,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(store)?;
    if added.is_none() {
        return Err(ChecklistError::Duplicate);
    }
    let what = (POLICY_ACTION, "ai.privacy_policy_version", Uuid::nil());
    audited(&mut tx, what, owner).await?;
    tx.commit().await.map_err(store)
}

/// The owner's audit row: the action, subject kind and subject ID only (SEC-006).
async fn audited(
    tx: &mut PgConnection,
    (action, subject_kind, subject_id): (&'static str, &'static str, Uuid),
    owner: Uuid,
) -> Result<(), ChecklistError> {
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action,
        subject_kind,
        subject_id,
        change: Change::Action,
    };
    audit(tx, &entry).await.map(drop).map_err(store)
}
