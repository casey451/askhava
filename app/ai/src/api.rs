//! The public face of `askhava-ai`: everything other crates may call (D-481).
//!
//! [`read_intake`] reads one scrubbed request through the AI adapter once the AI-008 gate
//! ([`ai_mode`], [`set_ai_switch`]) allows it (AI-001, AI-005, AI-008, AI-010, AI-013, AI-021,
//! AI-023 to AI-025); [`weekly_counts`] gives the weekly summary's counts (AI-014); [`open_trades`] gives it the catalog's open trades (AI-005);
//! [`screen_text`] screens any AI-written text before use (AI-021); [`accept_question`] and
//! [`accept_document`] run the AI-019 checks on an AI-written question, card or project file and
//! fall back to the fixed template or her scrubbed words (AI-019, AI-021); [`quote_summary`] returns no
//! summary while AI quote summaries are off (AI-006, PICK-003, D-373). Nothing here sends a text,
//! moves money, releases a detail or chooses a recipient (AI-006).

use askhava_privacy::api::ScrubbedText;
use sqlx::PgConnection;

pub use crate::models::{
    AdapterTransport, AiConfig, AiError, DAILY_CAP_KEY, DEFAULT_MODEL, Endpoint, Fault,
    INSTRUCTIONS, INTAKE_READING, Instruction, MODEL_KEY, Reply, TIMEOUT_KEY, Transport,
};
pub use crate::output_checks::CheckHit;
pub use crate::schemas::{
    IntakeReading, Language, OpenTrades, ScreenHit, Screened, Tag, TextKind, screen_text,
};
pub use crate::service::{AiOff, Intake, IntakeCall, Origin};
pub use crate::switch::{
    AiMode, Missing, SwitchError, WeeklyCounts, ai_mode, read_intake, set_ai_switch, weekly_counts,
};
pub use crate::writer::{Document, WriteSources, Written, accept_document, accept_question};

/// The categories switched on in the catalog with their job types, for the AI-005 check.
///
/// # Errors
///
/// [`AiError::Database`] when the catalog cannot be read.
pub async fn open_trades(db: &mut PgConnection) -> Result<OpenTrades, AiError> {
    let codes = askhava_catalog::api::enabled_categories(db)
        .await
        .map_err(|_| AiError::Database)?;
    let mut entries = Vec::with_capacity(codes.len());
    for code in codes {
        let category = askhava_catalog::api::category(db, &code)
            .await
            .map_err(|_| AiError::Database)?;
        if let Some(category) = category.filter(|c| c.enabled) {
            let jobs = category.job_types.into_iter().map(|j| j.name).collect();
            entries.push((category.code, jobs));
        }
    }
    Ok(OpenTrades::new(entries))
}

/// The AI summary of a business quote: none, for every quote, while AI quote summaries are off,
/// as they are at launch (AI-006, PICK-003, D-373). Turning them on needs Casey's recorded
/// decision and the privacy-policy sentence first, then a task that builds the summary.
#[must_use]
pub fn quote_summary(_quote: &ScrubbedText) -> Option<String> {
    None
}
