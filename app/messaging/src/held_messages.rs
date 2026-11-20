//! What `messaging` holds for one homeowner's number, read for her own portal (PRIV-021, CON-035;
//! T307, D-3877, D-4025).
//!
//! [`texts_on`] answers whether her number is off the one suppression list, so the portal home
//! shows the texts-off switch only while texts are on. It derives the list's blind-index digests
//! itself, as `optout.rs`'s private `digests` does (`normalize_phone`, then
//! `Vault::blind_index(IndexPurpose::HomeownerPhone, ..)`, one digest per key version), and the
//! in-file round-trip test fails the build if the two ever drift. A number it cannot digest is an
//! error, never "not listed", so the portal shows its error state, not "texts on".
//!
//! [`held_words`] (T561, D-3877) returns the words of the texts to her that `messaging` still
//! holds, for her "Download my data" file (PRIV-031): only an outbox row's body is kept, and only
//! until its send is marked (a sent, blocked, failed or expired row keeps none, TXT-020), so these
//! are her texts not yet sent. Inbound words are not kept here (`privacy.message_body` is the
//! owner's read). Each body has its phone numbers and emails masked (`scrub_text`, PRIV-017).

use askhava_core::api::scrub_text;
use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, OutboxError};

/// PRIV-021, CON-035: whether `phone` is off the suppression list (texts on), matched under every
/// key version of the homeowner-phone index (PRIV-024).
///
/// # Errors
///
/// [`OutboxError::BadPhone`] when `phone` is not a US number, [`OutboxError::Database`] when no
/// digest can be made or the read fails.
pub async fn texts_on(
    db: &mut PgConnection,
    vault: &Vault,
    phone: &str,
) -> Result<bool, OutboxError> {
    let phone = normalize_phone(phone).map_err(|_| OutboxError::BadPhone)?;
    let digests = vault.blind_index(IndexPurpose::HomeownerPhone, &phone);
    if digests.is_empty() {
        return Err(OutboxError::Database);
    }
    Ok(models::listing(db, &digests).await?.is_none())
}

/// PRIV-031, PRIV-017: the held words of the texts to the homeowner whose vault contact is
/// `contact`, oldest first, each with when it was queued and with any phone number or email
/// masked; no other recipient's row is read.
///
/// # Errors
///
/// [`OutboxError::Database`] when the read fails.
pub async fn held_words(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Vec<(Timestamp, String)>, OutboxError> {
    let rows = sqlx::query!(
        r#"SELECT created_at, body AS "body!" FROM messaging.outbox
           WHERE recipient_kind = 'homeowner' AND recipient_id = $1 AND body IS NOT NULL
           ORDER BY created_at, id"#,
        contact,
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    let words = rows
        .into_iter()
        .map(|r| (r.created_at.to_jiff(), scrub_text(&r.body)));
    Ok(words.collect())
}

#[cfg(test)]
mod tests {
    //! CON-035 (TD-PRIV-21): the reader agrees with `suppress`. Fake numbers only.

    use askhava_privacy::api::Vault;
    use jiff::Timestamp;
    use secrecy::SecretString;
    use sqlx::PgPool;

    use super::texts_on;
    use crate::api::{RevokeChannel, suppress};
    use crate::models::OutboxError;

    fn vault() -> Vault {
        let key = |k: &str| SecretString::from(format!("1:{k}"));
        let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
        let (seal, index) = ("UlJS".repeat(10) + "UlI=", "YmJi".repeat(10) + "YmI=");
        Vault::new(&key(&seal), &key(&index), &url).expect("vault")
    }

    /// CON-035: a number `suppress` listed reads texts off, in any written shape; another reads
    /// texts on; a number that is not a US number is an error.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn the_reader_sees_what_suppress_listed(pool: PgPool) {
        let (mut db, vault) = (pool.acquire().await.expect("db"), vault());
        let now = Timestamp::from_second(1_800_000_000).expect("time");
        let how = (RevokeChannel::Portal, "portal:switch");
        let listed = suppress(&mut db, &vault, "+19285550100", how, now).await;
        assert_eq!(listed, Ok(true));
        assert_eq!(texts_on(&mut db, &vault, "(928) 555-0100").await, Ok(false));
        assert_eq!(texts_on(&mut db, &vault, "+19285550101").await, Ok(true));
        let bad = texts_on(&mut db, &vault, "12").await;
        assert_eq!(bad, Err(OutboxError::BadPhone));
    }
}
