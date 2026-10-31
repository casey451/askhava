//! The AI-019 writer's last step: an AI-written question, job card or project file is used only
//! when it passes every code check, else the fixed template or her own scrubbed words go instead
//! (AI-019, AI-021, D-430, D-492, D-988).
//!
//! The caller makes the AI call (no call for an unverified web chat or past the daily cap,
//! AI-021) and hands the reply's text here with what she gave and the business names to keep
//! out (D-493). A question falls back to the bank's fixed wording; a card or project file to her
//! scrubbed words, which are never AI output. The result is what the homeowner is shown; an
//! AI-written card still needs her approval or her having seen it before it goes out (INT-041,
//! the caller's step). Nothing here sends anything (AI-006).

use askhava_privacy::api::ScrubbedText;

use crate::output_checks::{Against, CheckHit, check_output};
use crate::schemas::TextKind;

/// What the homeowner gave and what an AI-written text must leave out (AI-019, D-493).
#[derive(Debug, Clone, Copy)]
pub struct WriteSources<'a> {
    /// Her scrubbed request (AI-004), and the fallback for a card or project file.
    pub words: &'a ScrubbedText,
    /// Her answers to the bank questions, as the fixed answer wording she chose.
    pub answers: &'a [String],
    /// The photo notes the card may draw on.
    pub photo_notes: &'a [String],
    /// The names of the businesses open for this job, from `businesses` through the caller.
    pub business_names: &'a [String],
}

/// Which written document a card-side AI text is (AI-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Document {
    /// The job card the homeowner approves (CARD-007, INT-041).
    JobCard,
    /// The project file behind the card.
    ProjectFile,
}

/// The text to show, and the check that refused the AI's text when it fell back (AI-019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    text: String,
    hit: Option<CheckHit>,
}

impl Written {
    /// The text to use: the AI's when it passed, else the fallback.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The check that refused the AI's text, or `None` when the AI's text is used.
    #[must_use]
    pub fn hit(&self) -> Option<CheckHit> {
        self.hit
    }
}

/// A reworded bank question or the one follow-up: the AI's wording when it passes every AI-019
/// and AI-021 check against her words and the bank's `template`, else `template` (AI-019, AI-021).
#[must_use]
pub fn accept_question(ai_text: &str, template: &str, sources: &WriteSources<'_>) -> Written {
    let written = sources_of(sources, Some(template));
    finish(TextKind::Question, ai_text, sources, &written, template)
}

/// An AI-written job card or project file: the AI's text when it passes every AI-019 and AI-021
/// check, else her own scrubbed words (AI-019, AI-021, CARD-007).
#[must_use]
pub fn accept_document(document: Document, ai_text: &str, sources: &WriteSources<'_>) -> Written {
    let kind = match document {
        Document::JobCard => TextKind::JobCard,
        Document::ProjectFile => TextKind::ProjectFile,
    };
    let written = sources_of(sources, None);
    finish(kind, ai_text, sources, &written, sources.words.as_str())
}

fn sources_of<'a>(sources: &WriteSources<'a>, template: Option<&'a str>) -> Vec<&'a str> {
    let given = sources.answers.iter().chain(sources.photo_notes);
    std::iter::once(sources.words.as_str())
        .chain(given.map(String::as_str))
        .chain(template)
        .collect()
}

fn finish(
    kind: TextKind,
    ai_text: &str,
    sources: &WriteSources<'_>,
    written: &[&str],
    fallback: &str,
) -> Written {
    let against = Against {
        sources: written,
        business_names: sources.business_names,
    };
    match check_output(kind, ai_text, &against) {
        Ok(text) => Written { text, hit: None },
        Err(hit) => Written {
            text: fallback.to_owned(),
            hit: Some(hit),
        },
    }
}
