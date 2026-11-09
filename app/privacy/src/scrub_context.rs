//! The contact-aware inbound read: a homeowner's text scrubbed with her vault name and every name
//! she stated in an earlier text (AI-003, AI-020, PRIV-001 to PRIV-003; D-1752, T257).
//!
//! [`read_inbound_for`] opens her vault name under the existing `(Messaging, Inbound)` grant
//! (audited and counted for SEC-047 once per text) and her sealed stated names
//! (`privacy.stated_name`), scrubs with both, and seals any name the text newly states for the
//! next one. It returns only the flags and the `ScrubbedText`: no plaintext leaves `privacy`, so,
//! unlike the plaintext reads, it may run inside the caller's transaction, and its audit rows
//! commit or roll back with it (D-1752 narrows D-752). The scrubber keeps its memory private, so
//! the stored names reach it as further words of `Known::vault_name` (every word of which it
//! removes), and a newly stated name is found by asking the scrubber, word by word, whether only
//! this text's memory removes it.

use std::collections::BTreeSet;

use askhava_ops::api::Actor;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{Field, Purpose, Reader, VaultError, grant_for};
use crate::reader::{Inbound, InboundWords, read_inbound};
use crate::scrub::{CommonWords, Known, NameMemory, REMOVED, normalize, scrub};
use crate::vault::{self, Vault};

fn db_err(_: sqlx::Error) -> VaultError {
    VaultError::Database
}

/// Reads homeowner `contact`'s inbound text `raw` (AI-003, AI-020, PRIV-003, PRIV-026, D-1752):
/// the flags from the raw text, and the text scrubbed with her vault name and her stated names;
/// a name this text newly states is sealed into `privacy.stated_name`. The vault read is audited
/// as `vault_read.messaging.inbound` and counted once for SEC-047; on a connection inside a
/// transaction those rows and the new names commit or roll back with it.
///
/// # Errors
///
/// [`VaultError::NotFound`], [`VaultError::Sealed`], [`VaultError::Audit`] or
/// [`VaultError::Database`].
pub async fn read_inbound_for(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    raw: &str,
    words: &InboundWords<'_>,
) -> Result<Inbound, VaultError> {
    let grant = grant_for(Reader::Messaging, Purpose::Inbound, &[Field::Name])
        .unwrap_or_else(|| unreachable!());
    let gate = (grant, Actor::System, None);
    let details = vault::read_audited(db, vault, gate, contact, &[Field::Name]).await?;
    let stored = stated_names(db, vault, contact).await?;
    let mut all: Vec<&str> = details.name.iter().map(|n| n.expose_secret()).collect();
    all.extend(stored.iter().map(|n| n.expose_secret()));
    let name = (!all.is_empty()).then(|| SecretString::from(all.join(" ")));
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: name.as_ref(),
    };
    let mut memory = NameMemory::default();
    let read = read_inbound(raw, words, &known, &mut memory);
    for word in newly_stated(raw, &common, &mut memory) {
        if !stored.iter().any(|n| n.expose_secret() == word) {
            store(db, vault, contact, &word).await?;
        }
    }
    Ok(read)
}

/// Every stated name of `contact`, opened (AI-020, D-1752).
async fn stated_names(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
) -> Result<Vec<SecretString>, VaultError> {
    let sealed = sqlx::query_scalar!(
        "SELECT name FROM privacy.stated_name WHERE contact_id = $1 ORDER BY created_at, id",
        contact,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(db_err)?;
    sealed
        .iter()
        .map(|s| vault.open_stated(contact, s))
        .collect()
}

/// The words of `raw`, in the scrubber's lower-cased key form without a possessive `'s`, that `memory` (this text's stated
/// names) removes when alone and an empty memory does not: the names `raw` states (AI-020).
fn newly_stated(raw: &str, common: &CommonWords, memory: &mut NameMemory) -> BTreeSet<String> {
    let known = Known {
        common,
        vault_name: None,
    };
    let keys: BTreeSet<String> = normalize(raw)
        .split_whitespace()
        .map(|t| {
            t.to_lowercase()
                .trim_matches(|c: char| !(c.is_alphanumeric() || c == '@'))
                .trim_end_matches("'s")
                .to_owned()
        })
        .filter(|k| !k.is_empty())
        .collect();
    keys.into_iter()
        .filter(|k| {
            scrub(k, &known, memory).as_str() == REMOVED
                && scrub(k, &known, &mut NameMemory::default()).as_str() != REMOVED
        })
        .collect()
}

/// Seals stated name `word` of `contact` once: a repeat of its digest adds nothing (D-1752).
async fn store(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    word: &str,
) -> Result<(), VaultError> {
    let sealed = vault.seal_stated(contact, word)?;
    let (index_version, digest) = vault.stated_digest(word);
    sqlx::query!(
        "INSERT INTO privacy.stated_name (contact_id, name, key_version, digest, index_version) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (contact_id, digest) DO NOTHING",
        contact,
        sealed,
        i32::from(vault.seal_version()),
        &digest[..],
        i32::from(index_version),
    )
    .execute(&mut *db)
    .await
    .map_err(db_err)?;
    Ok(())
}
