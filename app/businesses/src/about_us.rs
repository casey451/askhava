//! The "About us" line and the public business name screen (BIZ-022, PICK-007, PICK-041,
//! WORD-003; D-190, D-288, D-336, D-491).
//!
//! `businesses` cannot call `quotes`, so the PICK-007 contact rule and the channel rule are
//! applied here: no phone number (seven or more digits), no link or email, no "call me"-style
//! contact wording, no direction to another channel ("find us on", search, Google, Facebook,
//! Yelp), none of the WORD-003 claims, no carrier-restricted phrase (CARD-010) and, on a
//! no-license card, no "licensed". A refused line goes back with its reason.

use askhava_catalog::api::restricted_match;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models;
use crate::signup::SignupError;

/// The longest About us line (BIZ-022).
pub const ABOUT_US_MAX: usize = 140;

const CONTACT: &[&str] = &[
    "call me",
    "text me",
    "email me",
    "message me",
    "dm me",
    "contact me",
    "my number",
    "my cell",
    "whatsapp",
    "llamame",
    "mi numero",
];
const CHANNELS: &[&str] = &["find us on", "search", "google", "facebook", "yelp"];
const LINKS: &[&str] = &["http", "www", ".com", ".net", ".org", ".biz", ".us", "@"];
const CLAIMS: &[&str] = &[
    "vetted",
    "background checked",
    "background-checked",
    "best",
    "top rated",
    "top-rated",
    "#1",
    "number one",
    "el mejor",
    "la mejor",
    "los mejores",
];

/// Why a line went back to the business (BIZ-022); [`AboutUsRefusal::reason`] is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AboutUsRefusal {
    /// Empty or over 140 characters.
    Length,
    /// A phone number, link, email or contact wording.
    Contact,
    /// A direction to another channel.
    Channel,
    /// A WORD-003 claim or a carrier-restricted phrase.
    Claim,
    /// "Licensed" on a no-license card.
    Licensed,
}

impl AboutUsRefusal {
    /// The reason shown beside the returned line.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Length => "Keep it between 1 and 140 characters.",
            Self::Contact => {
                "Leave out phone numbers, emails, links and \"call me\". Homeowners reach you through Ask Hava."
            }
            Self::Channel => {
                "Leave out other places to find you, like Google, Facebook, Yelp or \"search for us\"."
            }
            Self::Claim => {
                "Leave out words like \"best\", \"#1\" or \"vetted\". Say what you do instead."
            }
            Self::Licensed => "Your card has no state license, so leave out \"licensed\".",
        }
    }
}

fn digits_in_a_row(text: &str) -> usize {
    let mut best = 0;
    let mut run = 0;
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
            best = best.max(run);
        } else if !matches!(c, ' ' | '-' | '.' | '(' | ')') {
            run = 0;
        }
    }
    best
}

/// PICK-007, PICK-041, D-336: a phone number, link or channel direction in a public name or line.
fn contact_or_channel(lower: &str) -> Option<AboutUsRefusal> {
    if digits_in_a_row(lower) >= 7 || LINKS.iter().any(|l| lower.contains(l)) {
        return Some(AboutUsRefusal::Contact);
    }
    CHANNELS
        .iter()
        .any(|c| lower.contains(c))
        .then_some(AboutUsRefusal::Channel)
}

/// BIZ-022, WORD-003, LIC-019: screens an About us line; `no_license` for a no-license card.
///
/// # Errors
/// The [`AboutUsRefusal`] to return with the line.
pub fn screen(line: &str, no_license: bool) -> Result<(), AboutUsRefusal> {
    let count = line.trim().chars().count();
    if count == 0 || line.chars().count() > ABOUT_US_MAX {
        return Err(AboutUsRefusal::Length);
    }
    let lower = line.to_lowercase();
    if let Some(r) = contact_or_channel(&lower) {
        return Err(r);
    }
    if CONTACT.iter().any(|c| lower.contains(c)) {
        return Err(AboutUsRefusal::Contact);
    }
    if CLAIMS.iter().any(|c| lower.contains(c)) || restricted_match(line).is_some() {
        return Err(AboutUsRefusal::Claim);
    }
    if no_license && lower.contains("licensed") {
        return Err(AboutUsRefusal::Licensed);
    }
    Ok(())
}

/// PICK-041, D-336: screens the public business name for a phone number, a link or a direction
/// to another channel.
///
/// # Errors
/// [`AboutUsRefusal::Contact`] or [`AboutUsRefusal::Channel`].
pub fn screen_public_name(name: &str) -> Result<(), AboutUsRefusal> {
    contact_or_channel(&name.to_lowercase()).map_or(Ok(()), Err)
}

/// BIZ-022: screens and stores the line (`None` clears it).
///
/// # Errors
/// [`SignupError::AboutUs`] with the reason, or [`SignupError::Businesses`].
pub async fn set_about_us(
    db: &mut PgConnection,
    business: Uuid,
    line: Option<&str>,
    no_license: bool,
) -> Result<(), SignupError> {
    if let Some(l) = line {
        screen(l, no_license).map_err(SignupError::AboutUs)?;
    }
    if models::set_about_us(db, business, line).await? {
        Ok(())
    } else {
        Err(models::BusinessesError::UnknownBusiness.into())
    }
}
