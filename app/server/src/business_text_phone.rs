//! The outbox sender's business phone reader (BIZ-037, TXT-035, SEC-047, PRIV-003; D-4913, T573).
//!
//! `messaging` never names the vault's business reads, so [`BusinessTextPhone`] is the production
//! `messaging::api::LinePhones`: it opens a linked phone's number through
//! `privacy::api::read_staff_phone` under [`BusinessPurpose::BusinessText`], on the sender's bare
//! connection (the read refuses inside a transaction), with one `vault_read.server.business_text`
//! audit row and read count per call. The sender asks only for a row it is about to send. The
//! number is returned as a `SecretString` and never logged or stored here (PRIV-016).

use askhava_messaging::api::{LinePhones, PhoneUnread};
use askhava_privacy::api::{BusinessContactError, BusinessPurpose, Vault, read_staff_phone};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The production business phone reader over the app's vault.
pub struct BusinessTextPhone {
    vault: Vault,
}

impl BusinessTextPhone {
    /// The reader over the app's vault.
    #[must_use]
    pub const fn new(vault: Vault) -> Self {
        Self { vault }
    }
}

impl LinePhones for BusinessTextPhone {
    /// BIZ-037, D-4913: a line with no number, or one that will not open, can never be texted
    /// (`Never`: the row expires, as D-3149 does for a homeowner); any other failed read may pass
    /// (`Later`: the row is held).
    async fn phone(
        &self,
        conn: &mut PgConnection,
        line: Uuid,
    ) -> Result<SecretString, PhoneUnread> {
        read_staff_phone(conn, &self.vault, BusinessPurpose::BusinessText, line)
            .await
            .map_err(|error| match error {
                BusinessContactError::NotFound | BusinessContactError::Sealed => PhoneUnread::Never,
                _ => PhoneUnread::Later,
            })
    }
}

#[cfg(test)]
mod tests {
    use askhava_messaging::api::{LinePhones, PhoneUnread};
    use askhava_privacy::api::{Vault, store_staff_phone};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use secrecy::{ExposeSecret, SecretString};
    use sqlx::PgPool;
    use sqlx::types::Uuid;

    use super::BusinessTextPhone;

    fn vault() -> Vault {
        let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
        let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
        Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
    }

    /// BIZ-037, D-4913: a stored line's fake number opens, a line with none can never be read,
    /// and a read refused inside a transaction may pass later. The audit row of each read is
    /// counted in `tests/integration/tests/outbox_business_rows.rs` (SEC-047).
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_line_opens_under_business_text_and_a_missing_one_never(pool: PgPool) {
        let mut db = pool.acquire().await.expect("connection");
        let (line, reader) = (Uuid::from_u128(0x573), BusinessTextPhone::new(vault()));
        let missing = reader.phone(&mut db, line).await;
        assert_eq!(missing.err(), Some(PhoneUnread::Never));
        let stored = store_staff_phone(&mut db, &vault(), line, "+19285550142").await;
        stored.expect("stored");
        let phone = reader.phone(&mut db, line).await.expect("phone");
        assert_eq!(phone.expose_secret(), "+19285550142");
        let mut tx = sqlx::Connection::begin(&mut *db).await.expect("begin");
        let inside = reader.phone(&mut tx, line).await;
        assert_eq!(inside.err(), Some(PhoneUnread::Later));
        drop(tx);
    }
}
