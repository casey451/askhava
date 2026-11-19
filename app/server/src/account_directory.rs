//! The business account emails' address directory over the vault (CON-020, BIZ-017, BIZ-040,
//! SEC-025, PRIV-003, PRIV-004; T490, D-3186, D-3262, D-3844, D-4293).
//!
//! [`VaultAccountDirectory`] is the `businesses::api::AccountDirectory` the composition passes to
//! the welcome and phone-stopped emails, and the address read behind `PortalNoticeParts` for the
//! account event emails. It opens a sign-in mailbox's address with
//! `privacy::api::read_business_mailbox` under `BusinessPurpose::AccountEmail` on the handler's
//! own connection (one audited read; refused inside a transaction, D-752), answers `None` for a
//! mailbox with no row, and `None` for `last4`: no business phone digits are readable (D-3186).
//! No address is stored or logged here.

use std::sync::Arc;

use askhava_businesses::api::AccountDirectory;
use askhava_core::api::HandlerError;
use askhava_privacy::api::{BusinessContactError, BusinessPurpose, Vault, read_business_mailbox};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The account emails' address reads over the one vault (SEC-025, D-3262). No `Debug`.
pub struct VaultAccountDirectory {
    vault: Arc<Vault>,
}

impl VaultAccountDirectory {
    /// The directory reading addresses in `vault`.
    #[must_use]
    pub fn new(vault: Arc<Vault>) -> Self {
        Self { vault }
    }
}

impl AccountDirectory for VaultAccountDirectory {
    /// PRIV-003, PRIV-004, D-3262: the address of `mailbox` under `AccountEmail`, `None` with no
    /// such mailbox; any other failure (a transaction, a seal, the audit row) is a fixed sentence.
    async fn address(
        &self,
        db: &mut PgConnection,
        mailbox: Uuid,
    ) -> Result<Option<SecretString>, HandlerError> {
        let purpose = BusinessPurpose::AccountEmail;
        match read_business_mailbox(db, &self.vault, purpose, mailbox).await {
            Ok(address) => Ok(Some(address)),
            Err(BusinessContactError::NotFound) => Ok(None),
            Err(_) => Err(HandlerError("the account email address could not be read")),
        }
    }

    /// D-3186: no business phone digits are readable, so always `None`.
    async fn last4(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
    ) -> Result<Option<String>, HandlerError> {
        Ok(None)
    }
}
