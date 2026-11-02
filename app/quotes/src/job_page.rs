//! The business job page's data: one page per job, opened from a card's Quote link (BIZ-038),
//! holding the approved card, every intake answer, the details the homeowner adds later
//! (JOB-010) and the number of businesses sent and quotes so far (CARD-019, CARD-023, D-434).
//!
//! This module supplies the data only; the portals route renders it (T148's business actions
//! page, D-491). The card's screened photos and any concept image travel inside the approved
//! [`JobCard`]; showing them and the project file is T090's and T212's (D-491).
//!
//! No homeowner contact detail is on the page: the card has no slot for one (CARD-005, PRIV-006),
//! and every answer and later detail passes the same final gate as the card (CARD-008, CARD-010),
//! so a phone or address she typed is cut before a business sees it. Her contact reaches only the
//! business she picks, through `picks` and `portals` after the PICK-022 share, never this page.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::cards::JobCard;
use crate::gate::final_gate;
use crate::models::{self, QuotesError};

/// What the caller brings to a job page: the card she approved (INT-041), every intake answer as
/// (question, answer) wording and her later details (JOB-010), all already scrubbed. Its `Debug`
/// hides the answers and details, so a log never holds her words (R6).
#[derive(Clone)]
pub struct PageParts {
    /// The card she approved, as businesses received it (INT-041, D-434).
    pub card: JobCard,
    /// Every intake answer, card-field or not, as (question label, answer) wording (CARD-023).
    /// The caller leaves out the contact questions (her name, street address and any on-site
    /// contact, INT-041's name-and-address step): those answers are never on the page, and the
    /// final gate is a backstop for contact she typed elsewhere, not a filter for them (PICK-022).
    pub answers: Vec<(String, String)>,
    /// The details she added after the card went out, oldest first (JOB-010).
    pub later: Vec<String>,
}

impl std::fmt::Debug for PageParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageParts")
            .field("answers", &self.answers.len())
            .field("later", &self.later.len())
            .finish_non_exhaustive()
    }
}

/// One business job page (CARD-023). Its fields are private and [`job_page`] is the only way to
/// make one, so every answer and later detail has passed the final gate (CARD-008).
#[derive(Clone, PartialEq, Eq)]
pub struct JobPage {
    card: JobCard,
    answers: Vec<(String, String)>,
    later: Vec<String>,
    sent: u32,
    quoted: u32,
}

impl std::fmt::Debug for JobPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobPage")
            .field("answers", &self.answers.len())
            .field("later", &self.later.len())
            .field("sent", &self.sent)
            .field("quoted", &self.quoted)
            .finish_non_exhaustive()
    }
}

impl JobPage {
    /// The approved card, its text, town, place and photos as businesses received it (CARD-023).
    #[must_use]
    pub fn card(&self) -> &JobCard {
        &self.card
    }

    /// Every intake answer as (question, answer), each through the final gate (CARD-023).
    #[must_use]
    pub fn answers(&self) -> &[(String, String)] {
        &self.answers
    }

    /// The details she added later, oldest first, each through the final gate (JOB-010).
    #[must_use]
    pub fn later(&self) -> &[String] {
        &self.later
    }

    /// The number of businesses the card went to (CARD-019).
    #[must_use]
    pub fn sent(&self) -> u32 {
        self.sent
    }

    /// The quotes so far: confirmed and not withdrawn (CARD-019, PICK-002).
    #[must_use]
    pub fn quoted(&self) -> u32 {
        self.quoted
    }
}

/// CARD-023, CARD-019, D-434: `job`'s page for `business`, or none when the business holds no card
/// for the job (the page opens only from its own card's Quote link). The answers and later details
/// pass [`final_gate`] (CARD-008, CARD-010); the counts are read now, so the page shows the current
/// number sent and quotes so far.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn job_page(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    parts: &PageParts,
) -> Result<Option<JobPage>, QuotesError> {
    let counts = models::page_counts(db, job, business).await?;
    if !counts.holds_card {
        return Ok(None);
    }
    let answers = parts
        .answers
        .iter()
        .map(|(q, a)| (final_gate(q), final_gate(a)))
        .collect();
    Ok(Some(JobPage {
        card: parts.card.clone(),
        answers,
        later: parts.later.iter().map(|d| final_gate(d)).collect(),
        sent: counts.sent,
        quoted: counts.quoted,
    }))
}
