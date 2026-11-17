//! The CALL-005 read-back: the homeowner's first name and street, and nothing else, for `calls`
//! (CALL-005, CALL-025, PRIV-003, PRIV-004; D-026, D-1686).
//!
//! [`read_for_readback`] opens the name and the address of one contact under the registered
//! `(Reader::Calls, Purpose::CallReadback)` grant, audited per field like every vault read, and
//! cuts them down here, so the full name never leaves the vault crate: the first word of the name
//! and the address's street line (up to its first comma, or with none up to its last street-type
//! word, else less a trailing ZIP and state, D-4052 B35). The town goes only when a comma follows
//! the street or a street-type word ends it; otherwise it can remain. `calls` asks only after two bad PINs from a
//! phone linked to the business holding the job's open line, on an A-attested call (D-026, D-480).

use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::api::read_contact;
use crate::models::{Field, Purpose, Reader, VaultError};
use crate::vault::Vault;

/// What the masked line reads back: first name and street, each `SecretString` so `Debug` prints
/// neither (PRIV-016). A field the vault does not hold is `None`.
#[derive(Debug)]
pub struct Readback {
    first_name: Option<SecretString>,
    street: Option<SecretString>,
}

impl Readback {
    /// The first word of the name (CALL-005).
    #[must_use]
    pub fn first_name(&self) -> Option<&SecretString> {
        self.first_name.as_ref()
    }

    /// The address's street line: house number and street, never the town (CALL-005, D-4052).
    #[must_use]
    pub fn street(&self) -> Option<&SecretString> {
        self.street.as_ref()
    }
}

/// The first word of `name` (CALL-005).
fn first_word(name: &SecretString) -> Option<SecretString> {
    let word = name.expose_secret().split_whitespace().next()?;
    Some(SecretString::from(word.to_owned()))
}

/// Street-type words, lower case, a trailing period dropped before the match (CALL-005, D-4052).
const STREET_WORDS: [&str; 21] = [
    "st",
    "street",
    "ave",
    "avenue",
    "rd",
    "road",
    "dr",
    "drive",
    "ln",
    "lane",
    "blvd",
    "boulevard",
    "way",
    "ct",
    "court",
    "pl",
    "place",
    "cir",
    "circle",
    "hwy",
    "highway",
];

/// The street line of `address`, trimmed (CALL-005, D-4052 B35): the part before its first comma;
/// with no comma, up to and including the last street-type word ([`STREET_WORDS`], any case),
/// else the address less a trailing ZIP and state. The town is dropped only when a comma follows the
/// street or a street-type word ends it; with neither (or a comma only before the ZIP) it remains.
fn street_of(address: &SecretString) -> Option<SecretString> {
    let text = address.expose_secret();
    let street = match text.split_once(',') {
        Some((head, _)) => head.trim().to_owned(),
        None => uncommaed_street(text),
    };
    (!street.is_empty()).then(|| SecretString::from(street))
}

/// The street of a one-line address with no comma (CALL-005, D-4052 B35): its words up to the last
/// street-type word after the house number, else its words less a trailing 5-digit (or ZIP+4)
/// ZIP and a two-letter state.
fn uncommaed_street(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let is_street =
        |w: &&str| STREET_WORDS.contains(&w.trim_end_matches('.').to_lowercase().as_str());
    if let Some(last) = words.iter().skip(1).rposition(is_street) {
        return words[..last + 2].join(" ");
    }
    let mut end = words.len();
    if end > 1 && words[end - 1].split('-').next().is_some_and(is_zip) {
        end -= 1;
    }
    if end > 1
        && words[end - 1].len() == 2
        && words[end - 1].bytes().all(|b| b.is_ascii_alphabetic())
    {
        end -= 1;
    }
    words[..end].join(" ")
}

/// Whether `word` is a 5-digit ZIP.
fn is_zip(word: &str) -> bool {
    word.len() == 5 && word.bytes().all(|b| b.is_ascii_digit())
}

/// Reads contact `id`'s first name and street for the CALL-005 read-back (CALL-005, PRIV-003,
/// PRIV-004): one audited read per field (`vault_read.calls.call_readback`), refused on a
/// connection inside a transaction. Only `calls` calls it, and only for a line the rule allows
/// (D-026, D-1686).
///
/// # Errors
///
/// [`VaultError::InTransaction`], [`VaultError::NotFound`], [`VaultError::Sealed`],
/// [`VaultError::Audit`] or [`VaultError::Database`].
pub async fn read_for_readback(
    db: &mut PgConnection,
    vault: &Vault,
    id: Uuid,
) -> Result<Readback, VaultError> {
    let access = (Reader::Calls, Purpose::CallReadback);
    let details = read_contact(db, vault, access, id, &[Field::Name, Field::Address]).await?;
    Ok(Readback {
        first_name: details.name.as_ref().and_then(first_word),
        street: details.address.as_ref().and_then(street_of),
    })
}

/// The trimming rules on fake values (CALL-005).
#[cfg(test)]
mod tests {
    use super::{first_word, street_of};
    use secrecy::{ExposeSecret, SecretString};

    fn plain(value: Option<SecretString>) -> Option<String> {
        value.map(|v| v.expose_secret().to_owned())
    }

    #[test]
    fn only_the_first_name_and_the_street_are_kept() {
        let s = |v: &str| SecretString::from(v.to_owned());
        assert_eq!(
            plain(first_word(&s("  Testa Q. Example"))).as_deref(),
            Some("Testa")
        );
        assert_eq!(plain(first_word(&s("   "))), None);
        let street = street_of(&s(" 123 Fake St , Lake Havasu City, AZ"));
        assert_eq!(plain(street).as_deref(), Some("123 Fake St"));
        assert_eq!(
            plain(street_of(&s("123 Fake St"))).as_deref(),
            Some("123 Fake St")
        );
        assert_eq!(plain(street_of(&s(" , Town"))), None);
    }

    fn street(address: &str) -> Option<String> {
        plain(street_of(&SecretString::from(address.to_owned())))
    }

    /// D-4052 B35: a comma address keeps only the part before the comma.
    #[test]
    fn a_comma_address_is_cut_at_the_comma() {
        assert_eq!(
            street("4400 Sample Way Apt 2, Faketown, AZ 00000").as_deref(),
            Some("4400 Sample Way Apt 2")
        );
    }

    /// D-4052 B35: with no comma the town, state and ZIP go; the last street word ends the street.
    #[test]
    fn a_no_comma_address_is_cut_after_the_street_word() {
        assert_eq!(
            street("123 Fake St Faketown AZ 00000").as_deref(),
            Some("123 Fake St")
        );
        assert_eq!(
            street("77 north example BLVD. Lake Faketown az 00000-0000").as_deref(),
            Some("77 north example BLVD.")
        );
        assert_eq!(
            street("9 Example Court Street Faketown AZ 00000").as_deref(),
            Some("9 Example Court Street")
        );
    }

    /// D-4052 B35: with no street word, a trailing ZIP and state go.
    #[test]
    fn a_no_street_word_address_drops_the_zip_and_state() {
        assert_eq!(
            street("55 Testing Loop Faketown AZ 00000").as_deref(),
            Some("55 Testing Loop Faketown")
        );
        assert_eq!(
            street("55 Testing Loop").as_deref(),
            Some("55 Testing Loop")
        );
    }
}
