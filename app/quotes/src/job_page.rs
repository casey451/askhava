//! The business job page's data: one page per job, opened from a card's Quote link (BIZ-038),
//! holding the approved card, every intake answer, the details the homeowner adds later
//! (JOB-010) and the number of businesses sent and quotes so far (CARD-019, CARD-023, D-434).
//!
//! This module supplies the data only; the portals route renders it (T148's business actions
//! page, D-491). The card's screened photos and any concept image travel inside the approved
//! [`JobCard`]; T212's [`JobPage::with_design`] adds the design category's project file (gated)
//! and may name which card image is the concept image, so the page shows the photos, the project
//! file and the concept image with its INT-046 label apart (INT-045, INT-046, D-491). Fail-safe:
//! [`job_page`] itself names the concept image when the card carries `build_card`'s
//! "Example picture:" line (the concept ID is the card's last image), so a caller that never
//! calls `with_design` still gets the concept image labeled, never as her photo (D-2874).
//!
//! No homeowner contact detail is on the page: the card has no slot for one (CARD-005, PRIV-006),
//! and every answer and later detail passes the same final gate as the card (CARD-008, CARD-010),
//! so a phone or address she typed is cut before a business sees it. Her contact reaches only the
//! business she picks, through `picks` and `portals` after the PICK-022 share, never this page.

use sqlx::PgConnection;
use sqlx::types::Uuid;

use askhava_privacy::api::CONCEPT_LABEL;

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
    project_file: Option<String>,
    concept: Option<Uuid>,
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

    /// INT-045, INT-046, CARD-023: the page with the design category's project file, through
    /// [`final_gate`], and the concept image's ID; an ID that is not one of the card's images is
    /// ignored, so only an image she accepted onto the card shows, and `None` keeps the concept
    /// image [`job_page`] read from the card.
    #[must_use]
    pub fn with_design(mut self, project_file: Option<&str>, concept: Option<Uuid>) -> Self {
        self.project_file = project_file.map(final_gate).filter(|f| !f.is_empty());
        let named = concept.filter(|id| self.card.photos().contains(id));
        self.concept = named.or(self.concept);
        self
    }

    /// The card's photos, the concept image left out (INT-044, CARD-023).
    #[must_use]
    pub fn photos(&self) -> Vec<Uuid> {
        let concept = self.concept;
        let photos = self.card.photos().iter().copied();
        photos.filter(|id| Some(*id) != concept).collect()
    }

    /// The project file of a design category, through the final gate (INT-045).
    #[must_use]
    pub fn project_file(&self) -> Option<&str> {
        self.project_file.as_deref()
    }

    /// The concept image and the label shown on and beside it (INT-046).
    #[must_use]
    pub fn concept(&self) -> Option<(Uuid, &'static str)> {
        self.concept.map(|id| (id, CONCEPT_LABEL))
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
        project_file: None,
        concept: card_concept(&parts.card),
    }))
}

/// INT-046, D-2874: the card's concept image, read from the card itself: when the card's last
/// line is `build_card`'s "Example picture: <label>" line (written last, CARD-007), its concept
/// ID is the card's last image.
fn card_concept(card: &JobCard) -> Option<Uuid> {
    let line = format!("Example picture: {CONCEPT_LABEL}");
    let has_line = card.text().lines().last() == Some(line.as_str());
    has_line.then(|| card.photos().last().copied()).flatten()
}
