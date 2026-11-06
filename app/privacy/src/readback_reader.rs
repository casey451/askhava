//! The CALL-005 read-back: the homeowner's first name and street, and nothing else, for `calls`
//! (CALL-005, CALL-025, PRIV-003, PRIV-004; D-026, D-1686).
//!
//! [`read_for_readback`] opens the name and the address of one contact under the registered
//! `(Reader::Calls, Purpose::CallReadback)` grant, audited per field like every vault read, and
//! cuts them down here, so the full name and the town never leave the vault crate: the first word
//! of the name and the address up to its first comma. `calls` asks only after two bad PINs from a
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

    /// The address up to its first comma: house number and street, never the town (CALL-005).
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

/// The part of `address` before its first comma, trimmed (CALL-005).
fn street_of(address: &SecretString) -> Option<SecretString> {
    let street = address.expose_secret().split(',').next()?.trim();
    (!street.is_empty()).then(|| SecretString::from(street.to_owned()))
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
}
