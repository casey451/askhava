//! The "About us" line and the public business name screen (BIZ-022, PICK-007, PICK-041,
//! WORD-003; D-190, D-288, D-336, D-491).
//!
//! `businesses` cannot call `quotes`, so the PICK-007 contact rule and the channel rule are
//! applied here: no phone number (seven or more digits, three number words in a row, or seven
//! digits and number words mixed), no link or email, no "call me"-style contact wording, no
//! direction to another channel ("find us on", search, Google, Facebook, Yelp), none of the WORD-003 claims, no carrier-restricted phrase (CARD-010) and, on a
//! no-license card, no "licensed". Word lists match whole words, so "Asbestos" is not "best"
//! (D-1703). A refused line goes back with its reason.

use askhava_catalog::api::restricted_match;
use askhava_core::api::brand;
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
    "llámame",
    "mi numero",
    "mi número",
];
const CHANNELS: &[&str] = &["find us on", "search", "google", "facebook", "yelp"];
/// Link words matched as whole words; `@` and `.com` are also caught anywhere in the text.
const LINK_WORDS: &[&str] = &["http", "https", "www"];
/// Endings that make a word a domain ("example.us"); "1998.Usually" is not one.
const DOMAIN_ENDS: &[&str] = &["com", "net", "org", "biz", "us"];
const CLAIMS: &[&str] = &[
    "vetted",
    "background checked",
    "best",
    "top",
    "number one",
    "mejor",
    "mejores",
    "verificado",
    "verificada",
    "verificados",
    "verificadas",
];
/// Spoken digits (D-1025's list in `quotes::gate`, copied since `businesses` cannot call it).
const NUMBER_WORDS: &[&str] = &[
    "zero", "oh", "o", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    "ten", "eleven", "twelve", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty",
    "ninety", "hundred", "thousand", "double", "triple", "cero", "uno", "dos", "tres", "cuatro",
    "cinco", "seis", "siete", "ocho", "nueve", "diez",
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
    /// The reason shown beside the returned line; the contact and channel reasons name the brand's
    /// display name (WORD-001, D-1932).
    #[must_use]
    pub fn reason(self) -> String {
        let name = brand().display_name();
        match self {
            Self::Length => {
                "Write a short line about your business, up to 140 characters.".to_owned()
            }
            Self::Contact => format!(
                "Leave out phone numbers, emails, links and \"call me\". Homeowners reach you through {name}."
            ),
            Self::Channel => format!(
                "Leave out other ways to find you, like Google, Facebook, Yelp or \"search for us\". Homeowners reach you through {name}."
            ),
            Self::Claim => {
                "Leave out rankings and praise, like \"#1\" or \"top\". Say what you do instead."
                    .to_owned()
            }
            Self::Licensed => {
                "Your card shows no state license, so leave out \"licensed\".".to_owned()
            }
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

/// The text's lower-case words, split at every character that is not a letter or digit.
fn words(lower: &str) -> Vec<&str> {
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether `words` holds any phrase of `list` as whole words, like `catalog::restricted_match`
/// ("Asbestos" is not "best", "research" is not "search").
fn has_phrase(words: &[&str], list: &[&str]) -> bool {
    list.iter().any(|entry| {
        let phrase: Vec<&str> = entry.split(' ').collect();
        words.windows(phrase.len()).any(|w| w == phrase.as_slice())
    })
}

/// PICK-007, CARD-008: three number words in a row, or seven or more digits and number words
/// mixed ("nine two eight", "928 five five five 0142"), as `quotes::gate::number_run` reads them.
fn spoken_number(lower: &str) -> bool {
    let (mut said, mut digits) = (0, 0);
    for token in lower.split_whitespace() {
        let key = token.trim_matches(|c: char| !c.is_alphanumeric());
        let parts: Vec<&str> = key.split('-').collect();
        if !key.is_empty() && parts.iter().all(|p| NUMBER_WORDS.contains(p)) {
            said += parts.len();
        } else if !key.is_empty() && key.chars().all(|c| c.is_ascii_digit() || c == '-') {
            digits += key.chars().filter(char::is_ascii_digit).count();
        } else {
            (said, digits) = (0, 0);
        }
        if said >= 3 || (said >= 1 && said + digits >= 7) {
            return true;
        }
    }
    false
}

/// A link or email: `@`, `.com`, a link word, or a word ending in a domain ending after a letter
/// or digit ("example.us", not "1998.Usually").
fn link(lower: &str, words: &[&str]) -> bool {
    let domain = |token: &str| {
        let parts: Vec<&str> = token.split('.').collect();
        parts.windows(2).any(|p| {
            let end = p[1].trim_end_matches(|c: char| !c.is_alphanumeric());
            !p[0].is_empty() && DOMAIN_ENDS.contains(&end)
        })
    };
    lower.contains('@')
        || lower.contains(".com")
        || has_phrase(words, LINK_WORDS)
        || lower.split_whitespace().any(domain)
}

/// PICK-007, PICK-041, D-336: a phone number, link or channel direction in a public name or line.
fn contact_or_channel(lower: &str) -> Option<AboutUsRefusal> {
    let words = words(lower);
    if digits_in_a_row(lower) >= 7 || spoken_number(lower) || link(lower, &words) {
        return Some(AboutUsRefusal::Contact);
    }
    has_phrase(&words, CHANNELS).then_some(AboutUsRefusal::Channel)
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
    let words = words(&lower);
    if has_phrase(&words, CONTACT) {
        return Err(AboutUsRefusal::Contact);
    }
    if lower.contains("#1") || has_phrase(&words, CLAIMS) || restricted_match(line).is_some() {
        return Err(AboutUsRefusal::Claim);
    }
    if no_license && has_phrase(&words, &["licensed"]) {
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
