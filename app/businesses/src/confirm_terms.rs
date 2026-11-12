//! The Confirm page's business-terms record (TERMS-033, TD-TERMS-11; T232, D-493, D-3055).
//!
//! A business accepts the business terms twice over its life: at sign-up
//! (`signup::accept_terms`, channel `business_signup`) and on every Confirm press (here, channel
//! `confirm_page`). [`record_confirm_terms`] stores one append-only row in
//! `businesses.terms_acceptance` with the pick's ID, the confirming person, the version, the
//! SHA256 of the exact published text, the language and the assent line the page shows
//! ([`confirm_assent_line`]), in the caller's transaction (T261 calls it in the press's). The
//! unique index on `pick_id` (migration 0159) makes a second press for the same pick, after a
//! decline or a repeat, store nothing; a page load never calls it, so it stores nothing.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{BusinessesError, db};

/// The document code of the business terms (TERMS-033).
pub const BUSINESS_TERMS_DOCUMENT: &str = "business_terms";

/// The business terms as the Confirm page showed them (TERMS-033).
#[derive(Debug, Clone, Copy)]
pub struct ConfirmTerms<'a> {
    /// The business terms version the page names.
    pub version: i32,
    /// The exact published text of that version, stored as its SHA256 digest.
    pub published_text: &'a str,
    /// `en` or `es`: the language the page was shown in.
    pub language: &'a str,
}

/// TERMS-033: the assent line the Confirm page shows beside the terms link, word for word.
#[must_use]
pub fn confirm_assent_line(version: i32) -> String {
    format!("You're agreeing to business terms version {version}.")
}

/// TERMS-033, TD-TERMS-11, D-493: stores `person`'s acceptance of `terms` for `pick` of
/// `business`, channel `confirm_page`, at `now`, on `conn` (the caller's transaction). True when
/// a row was stored; false when this pick already has one (a second press stores nothing).
///
/// # Errors
/// [`BusinessesError::Database`] (also for a value the table's checks refuse).
pub async fn record_confirm_terms(
    conn: &mut PgConnection,
    (business, person): (Uuid, Uuid),
    pick: Uuid,
    terms: ConfirmTerms<'_>,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let sha: String = Sha256::digest(terms.published_text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let stored = sqlx::query!(
        "INSERT INTO businesses.terms_acceptance (business_id, person_id, document, version, \
         text_sha256, language, channel, assent_line, accepted_at, pick_id) \
         VALUES ($1, $2, $3, $4, $5, $6, 'confirm_page', $7, $8, $9) \
         ON CONFLICT (pick_id) WHERE pick_id IS NOT NULL DO NOTHING",
        business,
        person,
        BUSINESS_TERMS_DOCUMENT,
        terms.version.to_string(),
        sha,
        terms.language,
        confirm_assent_line(terms.version),
        now.to_sqlx() as _,
        pick,
    )
    .execute(conn)
    .await;
    Ok(db(stored)?.rows_affected() == 1)
}
