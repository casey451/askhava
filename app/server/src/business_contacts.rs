//! The composition of the business contact store (BIZ-037, BIZ-040, BIZ-051, SEC-024, PRIV-002,
//! PRIV-016; D-963, D-3262, T415).
//!
//! [`ContactDirectory`] is the production `businesses::api::SenderLink`: a linked or changed
//! phone writes `messaging`'s sender-directory digest and `privacy`'s sealed number for the same
//! line in the caller's transaction, so both move together or not at all. [`sweep_business_contacts`]
//! runs on `jobs`' daily sweep task: it reads the IDs `businesses` and unsent business emails still
//! name and lets `privacy` forget every other mailbox and number. No address or number passes
//! through here except the one being linked, never logged (PRIV-016).

use askhava_businesses::api::{SenderLink, TeamError, contact_ids_in_use};
use askhava_core::api::HandlerError;
use askhava_messaging::api::{OutboxError, sender_add, unsent_business_mailboxes};
use askhava_privacy::api::{BusinessContactError, Vault, forget_unused, store_staff_phone};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// How long a mailbox is kept after it stops being used: past SEC-024's change notice to the old
/// address and T196's two soft bounces 14 days apart (BIZ-040), with margin.
pub const KEEP_AFTER_USE: SignedDuration = SignedDuration::from_hours(30 * 24);

/// A mailbox or number written this recently is never forgotten, so a sign-up or address change
/// in progress (a mailbox minted before any row names it) is safe.
pub const FRESH_FOR: SignedDuration = SignedDuration::from_hours(2 * 24);

const SWEEP_FAILED: HandlerError = HandlerError("business contact sweep database call failed");

/// The production phone link (D-963, SEC-004, PRIV-002): the sender directory digest through
/// `messaging::api::sender_add` and the sealed number through `privacy::api::store_staff_phone`.
pub struct ContactDirectory {
    vault: Vault,
}

impl ContactDirectory {
    /// The link over the app's vault.
    #[must_use]
    pub const fn new(vault: Vault) -> Self {
        Self { vault }
    }
}

impl SenderLink for ContactDirectory {
    /// BIZ-037, PRIV-002, D-963: links `line` of `business` to `number` in the sender directory
    /// and seals the number under the line, replacing both earlier values, on the caller's
    /// transaction; `BadPhone` and `SenderTaken` map to [`TeamError::BadPhone`] and
    /// [`TeamError::PhoneTaken`].
    async fn link(
        &self,
        db: &mut PgConnection,
        (business, line): (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &self.vault, (business, line), number.expose_secret())
            .await
            .map_err(|e| match e {
                OutboxError::BadPhone => TeamError::BadPhone,
                OutboxError::SenderTaken => TeamError::PhoneTaken,
                _ => TeamError::Database,
            })?;
        store_staff_phone(db, &self.vault, line, number.expose_secret())
            .await
            .map_err(|e| match e {
                BusinessContactError::BadPhone => TeamError::BadPhone,
                _ => TeamError::Database,
            })
    }
}

/// BIZ-040, BIZ-051, SEC-024, PRIV-002, D-3262: forgets every business mailbox and staff phone
/// no longer named, as of `now`; returns how many rows went.
///
/// The in-use list is `businesses`' [`contact_ids_in_use`] with mailboxes out of use for under
/// [`KEEP_AFTER_USE`], plus every mailbox an unsent business email names; `privacy`'s
/// `forget_unused` then deletes what is absent from that list and older than [`FRESH_FOR`]. Each
/// step deletes only what is absent from the list read in this run, so a rerun or a crash
/// between steps is harmless.
///
/// # Errors
///
/// A [`HandlerError`] when a database call fails; the task is tried again.
pub async fn sweep_business_contacts(pool: &PgPool, now: Timestamp) -> Result<u64, HandlerError> {
    let since = now.checked_sub(KEEP_AFTER_USE).map_err(|_| SWEEP_FAILED)?;
    let before = now.checked_sub(FRESH_FOR).map_err(|_| SWEEP_FAILED)?;
    let mut conn = pool.acquire().await.map_err(|_| SWEEP_FAILED)?;
    let mut ids = contact_ids_in_use(&mut conn, since)
        .await
        .map_err(|_| SWEEP_FAILED)?;
    let unsent = unsent_business_mailboxes(&mut conn, now)
        .await
        .map_err(|_| SWEEP_FAILED)?;
    ids.mailboxes.extend(unsent);
    forget_unused(&mut conn, (&ids.mailboxes, &ids.lines), before)
        .await
        .map_err(|_| SWEEP_FAILED)
}
