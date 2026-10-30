//! The AI-008 gate: when the app may call the AI at all, the owner's "AI on scrubbed text"
//! switch, and the AI-014 weekly counts (AI-008, AI-014, D-412, D-492).
//!
//! Outside the live project (build, `Ci`, the Railway test project on fake data) the AI may run.
//! In live it runs only once the newest provider-terms row holds a zero-retention signed date on
//! or before today in Arizona (a newer row without one closes the gate again), or while the
//! switch is on and [`Readiness::missing`] finds nothing missing: the newest provider-terms row
//! says inputs are not used for training and holds the flagged-content retention period and a
//! Console spend limit of at most 3 times the expected monthly cost, and the privacy-policy version
//! in force today in Arizona holds the AI-018 sentence. Otherwise intake runs keyword-only
//! ([`AiOff::SwitchOff`]) and the adapter is never called.

use askhava_core::api::Environment;
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::models::{AiConfig, AiError, Transport};
use crate::service::{self, AiOff, Intake, IntakeCall};

/// Whether intake may call the AI now (AI-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMode {
    /// AI calls may run, on scrubbed text only (AI-004).
    Ai,
    /// No AI call: keyword rules, the fixed question bank and cards from fields (AI-008).
    KeywordOnly,
}

/// The first thing the switch still needs before it may turn on (AI-008, D-412, D-492).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// No provider-terms row is recorded.
    Terms,
    /// The recorded terms do not say API inputs are not used for training.
    TrainingFree,
    /// The flagged-content retention period is not recorded.
    FlaggedRetention,
    /// No Console spend limit, or one above 3 times the expected monthly cost.
    SpendLimit,
    /// The privacy-policy version in force today lacks the AI-018 sentence.
    PolicyWording,
}

/// Why the switch did not change. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SwitchError {
    /// The switch may not turn on yet (AI-008).
    #[error("the AI switch needs a record first")]
    Refused(Missing),
    /// The database refused or dropped a statement.
    #[error("the ai database statement failed")]
    Database,
}

/// What the gate reads, all as of today in Arizona (AI-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Readiness {
    pub terms: bool,
    pub training_free: bool,
    pub flagged_retention: bool,
    pub spend_limit: bool,
    pub policy_wording: bool,
    pub zero_retention: bool,
    pub switch_on: bool,
}

impl Readiness {
    /// The first record the switch still needs, or none (AI-008, D-492).
    pub(crate) fn missing(self) -> Option<Missing> {
        [
            (self.terms, Missing::Terms),
            (self.training_free, Missing::TrainingFree),
            (self.flagged_retention, Missing::FlaggedRetention),
            (self.spend_limit, Missing::SpendLimit),
            (self.policy_wording, Missing::PolicyWording),
        ]
        .into_iter()
        .find_map(|(held, missing)| (!held).then_some(missing))
    }

    /// The mode in `env` (AI-008): live needs zero retention, or the switch on and nothing missing.
    pub(crate) fn mode(self, env: Environment) -> AiMode {
        let live_ok = self.zero_retention || (self.switch_on && self.missing().is_none());
        if env != Environment::Live || live_ok {
            AiMode::Ai
        } else {
            AiMode::KeywordOnly
        }
    }
}

async fn readiness(db: &mut PgConnection) -> Result<Readiness, AiError> {
    let r = sqlx::query!(
        r#"WITH t AS (SELECT * FROM ai.provider_terms ORDER BY id DESC LIMIT 1),
           d AS (SELECT (now() AT TIME ZONE 'America/Phoenix')::date AS today)
        SELECT EXISTS (SELECT 1 FROM t) AS "terms!",
          COALESCE((SELECT inputs_not_used_for_training FROM t), false) AS "training_free!",
          COALESCE((SELECT flagged_retention_days IS NOT NULL FROM t), false) AS "flagged!",
          COALESCE((SELECT spend_limit_cents <= 3 * expected_monthly_cents FROM t), false) AS "spend!",
          COALESCE((SELECT p.has_ai018_sentence FROM ai.privacy_policy_version p, d
            WHERE p.effective_on <= d.today ORDER BY p.effective_on DESC, p.recorded_at DESC
            LIMIT 1), false) AS "policy!",
          COALESCE((SELECT t.zero_retention_signed_on <= d.today FROM t, d), false)
            AS "zero_retention!",
          COALESCE((SELECT turned_on FROM ai.switch_history ORDER BY seq DESC LIMIT 1), false)
            AS "switch_on!""#
    )
    .fetch_one(db)
    .await
    .map_err(|_| AiError::Database)?;
    Ok(Readiness {
        terms: r.terms,
        training_free: r.training_free,
        flagged_retention: r.flagged,
        spend_limit: r.spend,
        policy_wording: r.policy,
        zero_retention: r.zero_retention,
        switch_on: r.switch_on,
    })
}

/// Whether intake may call the AI now in `env` (AI-008, D-412, D-492).
///
/// # Errors
///
/// [`AiError::Database`] when the records cannot be read.
pub async fn ai_mode(db: &mut PgConnection, env: Environment) -> Result<AiMode, AiError> {
    Ok(readiness(db).await?.mode(env))
}

/// Turns the owner's "AI on scrubbed text" switch on or off, with its history row and audit row
/// in one transaction (AI-008, D-418). Off always works; on is refused while anything is
/// [`Missing`] (D-412, D-492).
///
/// # Errors
///
/// [`SwitchError::Refused`] naming the first missing record, or [`SwitchError::Database`].
pub async fn set_ai_switch(
    db: &mut PgConnection,
    on: bool,
    owner: Uuid,
) -> Result<(), SwitchError> {
    let mut tx = db.begin().await.map_err(|_| SwitchError::Database)?;
    if on
        && let Some(missing) = readiness(&mut tx)
            .await
            .map_err(|_| SwitchError::Database)?
            .missing()
    {
        return Err(SwitchError::Refused(missing));
    }
    let id = sqlx::query_scalar!(
        "INSERT INTO ai.switch_history (turned_on, owner_id) VALUES ($1, $2) RETURNING id",
        on,
        owner
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| SwitchError::Database)?;
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: if on { "ai.switch_on" } else { "ai.switch_off" },
        subject_kind: "ai.switch_history",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| SwitchError::Database)?;
    tx.commit().await.map_err(|_| SwitchError::Database)
}

/// Reads one scrubbed request only when [`ai_mode`] allows it; otherwise returns
/// [`AiOff::SwitchOff`] without calling the adapter, and intake runs keyword-only (AI-008).
///
/// # Errors
///
/// [`AiError::Database`] when the records cannot be read or the call cannot be recorded.
pub async fn read_intake<T: Transport>(
    db: &mut PgConnection,
    transport: &T,
    config: &AiConfig,
    env: Environment,
    call: &IntakeCall<'_>,
) -> Result<Intake, AiError> {
    match ai_mode(db, env).await? {
        AiMode::Ai => service::read_intake(db, transport, config, call).await,
        AiMode::KeywordOnly => Ok(Intake::AiOff(AiOff::SwitchOff)),
    }
}

/// The AI part of the weekly summary: counts only, built by code, never free text (AI-014).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeeklyCounts {
    /// AI calls made (daily-cap refusals excluded).
    pub calls: i64,
    /// Calls that ended without a reading, so the step ran on the AI-off path.
    pub ended: i64,
    /// Readings recorded.
    pub readings: i64,
    /// Readings the AI marked as emergencies (recorded only, INT-026).
    pub emergencies: i64,
    /// Readings in Spanish.
    pub spanish: i64,
}

/// The [`WeeklyCounts`] of the 7 days before now (AI-014).
///
/// # Errors
///
/// [`AiError::Database`] when the counts cannot be read.
pub async fn weekly_counts(db: &mut PgConnection) -> Result<WeeklyCounts, AiError> {
    let r = sqlx::query!(
        r#"SELECT
          (SELECT count(*) FROM ai.call_stat WHERE at > now() - interval '7 days'
             AND outcome <> 'daily_cap') AS "calls!",
          (SELECT count(*) FROM ai.call_stat WHERE at > now() - interval '7 days'
             AND outcome NOT IN ('ok', 'daily_cap')) AS "ended!",
          count(*) AS "readings!", count(*) FILTER (WHERE emergency) AS "emergencies!",
          count(*) FILTER (WHERE spanish) AS "spanish!"
        FROM ai.reading WHERE at > now() - interval '7 days'"#
    )
    .fetch_one(db)
    .await
    .map_err(|_| AiError::Database)?;
    Ok(WeeklyCounts {
        calls: r.calls,
        ended: r.ended,
        readings: r.readings,
        emergencies: r.emergencies,
        spanish: r.spanish,
    })
}
