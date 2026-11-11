//! A quote's notes box: up to 300 characters about the job only (PICK-052, PICK-007, BIZ-022;
//! D-442, D-485).
//!
//! [`check_note`] is pure: it sends a note back with the reason when it is too long, holds a
//! contact detail the card gate cuts (the same `text_bounce` the quote uses) or holds a phrase on
//! [`OFF_PLATFORM`] (or "outside <brand>"). [`screen_note`] adds the AI check while AI is on: the
//! caller's [`NoteScreener`] (the `ai` crate's, or the stand-in during the build) flags a note
//! that steers the homeowner away, criticizes another business or makes a WORD-003 claim. Run it
//! before the quote's transaction, never inside it (rust brain R11). A note that passes is kept
//! and shown exactly as written. [`NoteProblem`] never carries the note's words (R6).

use std::future::Future;

use askhava_core::api::brand;

use crate::intake::{Bounce, text_bounce};

/// The most characters a note may hold (PICK-052).
pub const NOTE_LIMIT: usize = 300;

/// PICK-052: the off-platform phrases a note may not hold, matched as whole words in any case;
/// "outside <brand>" is added from the brand file (D-1932).
pub const OFF_PLATFORM: [&str; 11] = [
    "call me",
    "text me",
    "message me",
    "dm me",
    "whatsapp",
    "pay cash",
    "cash discount",
    "venmo",
    "zelle",
    "cash app",
    "off the app",
];

/// What the AI check flagged in a note (PICK-052, D-442).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteFlag {
    /// It steers the homeowner away from the platform.
    SteersAway,
    /// It criticizes another business.
    CriticizesBusiness,
    /// It makes a WORD-003 claim (best, vetted, guaranteed and the like).
    Claim,
}

/// Why a note goes back to the business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteProblem {
    /// Over [`NOTE_LIMIT`] characters.
    TooLong,
    /// A phone number, link, email or other contact shape (PICK-007).
    Contact,
    /// A carrier-restricted phrase (CARD-010).
    Restricted,
    /// A phrase on the off-platform list.
    OffPlatform,
    /// The AI check flagged it.
    Flagged(NoteFlag),
}

impl NoteProblem {
    /// The reason the business reads beside the notes box.
    #[must_use]
    pub fn message(self) -> String {
        let name = brand().display_name();
        match self {
            Self::TooLong => format!("Please keep your note to {NOTE_LIMIT} characters or fewer."),
            Self::Contact | Self::OffPlatform => format!(
                "Please take out any phone number, email or link. Also take out any request to call, text, message or pay you outside {name}. If the homeowner picks your quote, {name} puts you in touch."
            ),
            Self::Restricted => {
                "Your note has a word we can't pass on to the homeowner by text. Please reword it.".to_owned()
            }
            Self::Flagged(NoteFlag::SteersAway) => format!(
                "This sounds like it asks the homeowner to deal with you outside {name}. Please keep the note about the job."
            ),
            Self::Flagged(NoteFlag::CriticizesBusiness) => {
                "Please keep the note about your own work, not other businesses."
                    .to_owned()
            }
            Self::Flagged(NoteFlag::Claim) => {
                "Please take out claims about how good your business is. Say what you'll do on this job instead."
                    .to_owned()
            }
        }
    }
}

/// The note's words, lowercased, joined by single spaces.
fn words(text: &str) -> String {
    let parts = text.split(|c: char| !c.is_alphanumeric());
    let kept: Vec<String> = parts
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    kept.join(" ")
}

/// PICK-052: true when `note` holds an off-platform phrase as whole words.
fn off_platform(note: &str) -> bool {
    let padded = format!(" {} ", words(note));
    let outside = format!("outside {}", words(brand().display_name()));
    OFF_PLATFORM
        .iter()
        .copied()
        .chain(std::iter::once(outside.as_str()))
        .any(|p| padded.contains(&format!(" {p} ")))
}

/// PICK-052, PICK-007, BIZ-022: the code checks on a note; an empty note passes.
///
/// # Errors
///
/// The first [`NoteProblem`]: length, then contact and restricted words, then the list.
pub fn check_note(note: &str) -> Result<(), NoteProblem> {
    if note.chars().count() > NOTE_LIMIT {
        return Err(NoteProblem::TooLong);
    }
    text_bounce(note).map_err(|b| match b {
        Bounce::Restricted => NoteProblem::Restricted,
        _ => NoteProblem::Contact,
    })?;
    if off_platform(note) {
        return Err(NoteProblem::OffPlatform);
    }
    Ok(())
}

/// The AI check on a note that passed [`check_note`] (PICK-052, D-442); the `ai` crate's
/// implementation sees only the note, never a homeowner detail, and has no tools (AI-006).
pub trait NoteScreener {
    /// The flag for `note`, or none when it reads as about the job only.
    fn screen(&self, note: &str) -> impl Future<Output = Option<NoteFlag>> + Send;
}

/// PICK-052, D-442: [`check_note`], then, while AI is on (`ai` is some), the AI check.
///
/// # Errors
///
/// The first [`NoteProblem`], code checks first, so a contact note never reaches the AI.
pub async fn screen_note<S: NoteScreener + Sync>(
    note: &str,
    ai: Option<&S>,
) -> Result<(), NoteProblem> {
    check_note(note)?;
    if note.trim().is_empty() {
        return Ok(());
    }
    match ai {
        Some(screener) => match screener.screen(note).await {
            Some(flag) => Err(NoteProblem::Flagged(flag)),
            None => Ok(()),
        },
        None => Ok(()),
    }
}
