//! The business text-consent page (CON-019, BIZ-036, TXT-035; T522, D-3879, D-3881).
//!
//! [`text_consent_page`] lists each phone the signed-in member linked (`businesses::api::
//! member_text_lines`): its last 4 digits and either "Texts on" (the text box is ticked for its
//! current number, `may_text`) or `businesses::api::consent_box`'s unticked text box, with the
//! phone's current number version in a hidden field. While the business-texting switch is off
//! (TXT-035) it says texts are off and shows no box. [`record_text_consent`] stores a ticked box
//! through `record_consent_for` for one of the member's own lines only, refused for a box shown
//! for an earlier number version (D-2707), and stores nothing while texting is off. The page
//! extends `business_layout.html` (Get help link, BIZ-047). Only IDs and last 4 digits are shown.

use askama::Template;
use askhava_businesses::api::{
    ConsentChannel, ConsentError, MemberTextLine, consent_box, member_text_lines,
    record_consent_for,
};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::business_auth::BusinessPrincipal;

/// The page's path in the business portal.
pub const TEXT_CONSENT_ROUTE: &str = "/b/texts";

/// The line the page shows after a `POST`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextConsentNote {
    /// Nothing to say (a first visit, or a `POST` naming none of the member's phones).
    Nothing,
    /// The box was stored for the phone ending in these digits.
    Saved(Option<String>),
    /// The box for this line was posted unticked; nothing stored.
    NotTicked(Uuid),
    /// The phone's number changed after the box was shown; nothing stored (D-2707).
    NumberChanged(Option<String>),
}

/// What a `POST` did, for the route to draw the page with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextConsentOutcome {
    /// Texting is off: nothing stored, the texts-off page (TXT-035).
    TextingOff,
    /// The page again with this note.
    Page(TextConsentNote),
}

/// Why a consent `POST` failed outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextConsentError {
    /// A database call failed.
    #[error("text consent database call failed")]
    Database,
}

/// One phone as the template shows it.
struct ShownPhone {
    line: Uuid,
    ending: String,
    version: i32,
    on: bool,
}

#[derive(Template)]
#[template(path = "text_consent.html")]
struct TextConsentView<'a> {
    texting_on: bool,
    phones: Vec<ShownPhone>,
    wording: String,
    said: String,
    note: &'a TextConsentNote,
    csrf: &'a str,
}

/// The one error line the page can show.
const NOT_TICKED: &str = "Tick the box to get texts at this phone.";

impl TextConsentView<'_> {
    /// The unticked line and its error, if the note is one.
    fn error(&self) -> Option<(Uuid, &'static str)> {
        match self.note {
            TextConsentNote::NotTicked(line) => Some((*line, NOT_TICKED)),
            _ => None,
        }
    }

    /// The error beside `line`'s box, if that box was posted unticked.
    fn wrong(&self, line: &Uuid) -> Option<&'static str> {
        self.error().filter(|(l, _)| l == line).map(|(_, e)| e)
    }

    /// Whether any text box is on the page (its wording then carries the no-calls line).
    fn boxes_shown(&self) -> bool {
        self.texting_on && self.phones.iter().any(|p| !p.on)
    }
}

/// "ending in 0123", or "you linked by text reply" when `businesses` never saw the number (a LINK
/// reply's phone).
fn ending(digits: Option<&String>) -> String {
    digits.map_or_else(
        || "you linked by text reply".to_owned(),
        |d| format!("ending in {d}"),
    )
}

/// The status line for `note`: empty unless a box was stored or its number changed.
fn said(note: &TextConsentNote) -> String {
    match note {
        TextConsentNote::Saved(d) => {
            format!("Saved. Texts are on for the phone {}.", ending(d.as_ref()))
        }
        TextConsentNote::NumberChanged(d) => {
            let rest = d.as_ref().map_or_else(
                || "Tick the box again to get texts at the new number.".to_owned(),
                |d| format!("It now ends in {d}, so tick the box again to get texts there."),
            );
            format!("Someone changed this phone's number while you had this page open. {rest}")
        }
        TextConsentNote::Nothing | TextConsentNote::NotTicked(_) => String::new(),
    }
}

/// CON-019, BIZ-036, TXT-035: the page for the member's `lines`, with `note` and the CSRF token
/// `csrf`; while `texting_on` is false it shows no box.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn text_consent_page(
    lines: &[MemberTextLine],
    texting_on: bool,
    note: &TextConsentNote,
    csrf: &str,
) -> Result<String, askama::Error> {
    let phones = lines
        .iter()
        .map(|l| ShownPhone {
            line: l.line,
            ending: ending(l.number_end.as_ref()),
            version: l.number_version,
            on: l.may_text,
        })
        .collect();
    let view = TextConsentView {
        texting_on,
        phones,
        wording: consent_box(&[ConsentChannel::Text]).wording,
        said: said(note),
        note,
        csrf,
    };
    view.render()
}

/// The last 4 digits of `line`, read again after a number-change refusal so the page names the
/// new number.
async fn digits_now(
    db: &mut PgConnection,
    who: &BusinessPrincipal,
    line: Uuid,
) -> Result<Option<String>, TextConsentError> {
    let lines = member_text_lines(db, who.business, who.member).await;
    let lines = lines.map_err(|_| TextConsentError::Database)?;
    Ok(lines
        .into_iter()
        .find(|l| l.line == line)
        .and_then(|l| l.number_end))
}

/// CON-019, BIZ-036, TXT-035, D-2707: stores the text box `ticked` by `who` for `phone` (its line
/// ID and the number version the page showed) at `now`, only while `texting_on` and only for a
/// phone `who` linked; another phone, no phone or an unticked box stores nothing, and a box
/// shown for an earlier number version is refused with the new last 4 digits.
///
/// # Errors
/// [`TextConsentError::Database`].
pub async fn record_text_consent(
    db: &mut PgConnection,
    who: &BusinessPrincipal,
    (texting_on, phone, ticked): (bool, Option<(Uuid, i32)>, bool),
    now: Timestamp,
) -> Result<TextConsentOutcome, TextConsentError> {
    if !texting_on {
        return Ok(TextConsentOutcome::TextingOff);
    }
    let own = member_text_lines(db, who.business, who.member).await;
    let own = own.map_err(|_| TextConsentError::Database)?;
    let Some((line, shown)) = phone.filter(|(id, _)| own.iter().any(|l| l.line == *id)) else {
        return Ok(TextConsentOutcome::Page(TextConsentNote::Nothing));
    };
    let ids = (who.business, who.member);
    let text = [ConsentChannel::Text];
    let note = match record_consent_for(db, ids, (line, shown), &text, ticked, now).await {
        Ok(()) => {
            let digits = own.into_iter().find(|l| l.line == line);
            TextConsentNote::Saved(digits.and_then(|l| l.number_end))
        }
        Err(ConsentError::NotTicked) => TextConsentNote::NotTicked(line),
        Err(ConsentError::NumberChanged) => {
            TextConsentNote::NumberChanged(digits_now(db, who, line).await?)
        }
        Err(ConsentError::UnknownPhone | ConsentError::UnknownMember) => TextConsentNote::Nothing,
        Err(_) => return Err(TextConsentError::Database),
    };
    Ok(TextConsentOutcome::Page(note))
}
