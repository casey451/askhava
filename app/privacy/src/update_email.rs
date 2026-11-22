//! The homeowner's quote-page update address (TXT-018, PICK-029, PRIV-001, PRIV-002, PRIV-007,
//! CON-035; T729, D-4727, D-5340, D-5442).
//!
//! An address she types on her quote page for quote updates lives in `privacy.update_email`
//! (migration 0215), one row per contact, never in `privacy.contact.email`: the pick-code email
//! (`picks::api::ConversationEmail`), the release and every other contact read open only
//! `privacy.contact`, so no code path can send a code to it (PICK-029, D-477). The address is
//! sealed under `VAULT_KEY` with associated data naming the row ID (the vault's `stated_name`
//! sealing, keyed by this table's own random row IDs, which are never a contact ID), so a
//! ciphertext moved to another row or table fails to open (PRIV-002).
//!
//! [`store_update_address`] replaces any earlier address with a new row ID and a fresh confirm
//! salt and clears `confirmed_at` and `bounced_at`, so a confirm link (`core::api::link_token`,
//! `LinkKind::Action`, over the row ID and that salt, D-5340) or a late bounce
//! ([`mark_update_bounced`], keyed by row) for the earlier address never touches the new one.
//! [`update_status`] reads no plaintext; [`read_update_address`] opens it for `messaging`'s update
//! email (T730) only under the `(Messaging, Send)` grant's `Email` field, audited as
//! `vault_read.messaging.send`. The contact purge deletes the row ([`forget_update_address`]).

use askhava_core::api::LinkSalt;
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection};

use crate::business_contacts::bare_address;
use crate::models::{Field, Purpose, Reader, VaultError, grant_for};
use crate::vault::{Vault, count_read, db_err, ensure_bare, refuse};

/// The audit action of a stored update address (TXT-018).
pub const UPDATE_EMAIL_STORED: &str = "update_email_stored";
/// The audit action of a confirmed update address (TXT-018, D-5340).
pub const UPDATE_EMAIL_CONFIRMED: &str = "update_email_confirmed";
/// The audit action of a hard bounce of the update address (TXT-018, D-5442).
pub const UPDATE_EMAIL_BOUNCED: &str = "update_email_bounced";
/// The audit subject kind of an update address row.
const SUBJECT: &str = "privacy.update_email";

/// A well-formed update address, checked once at the edge (rust-production R1, R2). No `Debug`
/// or `Display`: it is a contact value (PRIV-016).
pub struct UpdateAddress(SecretString);

impl UpdateAddress {
    /// TXT-018, D-4727: `typed` as sign-up's address check reads it (`bare_address`: trimmed,
    /// lower-cased, an `@`, no space, control character, `,`, `;`, `<` or `>`, at most
    /// `MAX_ADDRESS_LEN` characters), and only with a name before one `@` and a dotted domain
    /// with no empty part after it, so "pat@" is refused; `None` for anything else.
    #[must_use]
    pub fn parse(typed: &str) -> Option<Self> {
        let bare = bare_address(typed).ok()?;
        let (name, domain) = bare.split_once('@')?;
        let shaped = !name.is_empty()
            && !domain.contains('@')
            && domain.contains('.')
            && domain.split('.').all(|part| !part.is_empty());
        shaped.then(|| Self(SecretString::from(bare)))
    }
}

/// Her update address's state, with no plaintext: the row, the confirm link's salt (with the row
/// ID, the parts of its `LinkKind::Action` token, D-5340) and whether it is confirmed or bounced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateStatus {
    /// The address row's ID; a new address gets a new one.
    pub row: Uuid,
    /// The confirm link's 16-byte salt, replaced with each new address.
    pub salt: LinkSalt,
    /// Her one tap on the first email's link confirmed it (TXT-018).
    pub confirmed: bool,
    /// A hard bounce marked it (D-5442).
    pub bounced: bool,
}

/// Her update address opened for the update email (T730), with its state. No `Debug`.
pub struct UpdateEmail {
    /// The state and the confirm token's parts.
    pub status: UpdateStatus,
    /// The address itself; never logged (PRIV-016).
    pub address: SecretString,
}

/// TXT-018, PICK-029, PRIV-001, D-5340: seals `address` as contact `contact`'s update address
/// from the quote page at `now`, replacing any earlier one (new row ID, new confirm salt,
/// `confirmed_at` and `bounced_at` cleared), with one audit row naming her and the `email` field;
/// returns the row ID. A savepoint inside the caller's transaction, so the channel change commits
/// with it. `privacy.contact` is not touched.
///
/// # Errors
/// [`VaultError::NotFound`] for an unknown contact (nothing stored), [`VaultError::Sealed`] when
/// no salt or seal could be made, [`VaultError::Audit`], [`VaultError::Database`].
pub async fn store_update_address(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    address: &UpdateAddress,
    now: Timestamp,
) -> Result<Uuid, VaultError> {
    let salt = LinkSalt::generate().map_err(|_| VaultError::Sealed)?;
    let mut tx = db.begin().await.map_err(db_err)?;
    let row = sqlx::query_scalar!(r#"SELECT gen_random_uuid() AS "id!""#)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let sealed = vault.seal_stated(row, address.0.expose_secret())?;
    let stored = sqlx::query!(
        "INSERT INTO privacy.update_email (id, contact_id, email, source, confirm_salt, stored_at) \
         SELECT $1, c.id, $3, 'quote_page', $4, $5 FROM privacy.contact c WHERE c.id = $2 \
         ON CONFLICT (contact_id) DO UPDATE SET id = EXCLUDED.id, email = EXCLUDED.email, \
         confirm_salt = EXCLUDED.confirm_salt, stored_at = EXCLUDED.stored_at, \
         confirmed_at = NULL, bounced_at = NULL",
        row,
        contact,
        sealed,
        &salt.as_bytes()[..],
        now.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if stored == 0 {
        return Err(VaultError::NotFound);
    }
    record(
        &mut tx,
        (Actor::Homeowner(contact), UPDATE_EMAIL_STORED),
        row,
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(row)
}

/// Contact `contact`'s update address state, or `None` when she gave none; reads no plaintext.
///
/// # Errors
/// [`VaultError::Sealed`] for a stored salt that is not 16 bytes, [`VaultError::Database`].
pub async fn update_status(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Option<UpdateStatus>, VaultError> {
    let row = sqlx::query!(
        r#"SELECT id, confirm_salt, confirmed_at IS NOT NULL AS "confirmed!",
           bounced_at IS NOT NULL AS "bounced!"
           FROM privacy.update_email WHERE contact_id = $1"#,
        contact,
    )
    .fetch_optional(db)
    .await
    .map_err(db_err)?;
    row.map(|r| {
        Ok(UpdateStatus {
            row: r.id,
            salt: LinkSalt::from_bytes(&r.confirm_salt).map_err(|_| VaultError::Sealed)?,
            confirmed: r.confirmed,
            bounced: r.bounced,
        })
    })
    .transpose()
}

/// TXT-018, D-5340: sets address row `row`'s `confirmed_at` to `now` once; `false` (nothing
/// changed) for a row already confirmed or replaced. The caller checks the confirm token first.
///
/// # Errors
/// [`VaultError::Audit`], [`VaultError::Database`].
pub async fn confirm_update_address(
    db: &mut PgConnection,
    row: Uuid,
    now: Timestamp,
) -> Result<bool, VaultError> {
    let mut tx = db.begin().await.map_err(db_err)?;
    let contact = sqlx::query_scalar!(
        "UPDATE privacy.update_email SET confirmed_at = $2 WHERE id = $1 \
         AND confirmed_at IS NULL RETURNING contact_id",
        row,
        now.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let Some(contact) = contact else {
        return Ok(false);
    };
    record(
        &mut tx,
        (Actor::Homeowner(contact), UPDATE_EMAIL_CONFIRMED),
        row,
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    Ok(true)
}

/// TXT-018, D-5442: marks address row `row` bounced at `now`, keyed by the row the bounced email
/// went to, so a late bounce for a replaced address never marks the new one; `false` when the
/// row is gone or already marked. T730's hard-bounce path calls it beside setting the channel
/// back to text.
///
/// # Errors
/// [`VaultError::Audit`], [`VaultError::Database`].
pub async fn mark_update_bounced(
    db: &mut PgConnection,
    row: Uuid,
    now: Timestamp,
) -> Result<bool, VaultError> {
    let mut tx = db.begin().await.map_err(db_err)?;
    let marked = sqlx::query!(
        "UPDATE privacy.update_email SET bounced_at = $2 WHERE id = $1 AND bounced_at IS NULL",
        row,
        now.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if marked == 0 {
        return Ok(false);
    }
    record(&mut tx, (Actor::System, UPDATE_EMAIL_BOUNCED), row).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(true)
}

/// TXT-018, PRIV-003, PRIV-004: opens contact `contact`'s update address for the update email
/// (T730) under the `(Messaging, Send)` grant's `Email` field: one audit row naming the grant's
/// action and the `email` field, the read counted for SEC-047, committed before the plaintext
/// leaves; refused on a connection inside a transaction (D-753). `None` when she gave none.
///
/// # Errors
/// [`VaultError::InTransaction`], [`VaultError::UnregisteredPurpose`], [`VaultError::Sealed`],
/// [`VaultError::Audit`], [`VaultError::Database`].
pub async fn read_update_address(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
) -> Result<Option<UpdateEmail>, VaultError> {
    let access = (Reader::Messaging, Purpose::Send);
    let Some(grant) = grant_for(access.0, access.1, &[Field::Email]) else {
        return refuse(db, access, contact).await.map(|_| None);
    };
    ensure_bare(db).await?;
    let mut tx = db.begin().await.map_err(db_err)?;
    let Some(status) = update_status(&mut tx, contact).await? else {
        return Ok(None);
    };
    let sealed = sqlx::query_scalar!(
        "SELECT email FROM privacy.update_email WHERE id = $1",
        status.row
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    let address = vault.open_stated(status.row, &sealed)?;
    record(&mut tx, (Actor::System, grant.action), status.row).await?;
    count_read(&mut tx, grant.action).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some(UpdateEmail { status, address }))
}

/// PRIV-007, D-4727: deletes contact `contact`'s update address, in the caller's transaction (the
/// contact purge's); how many rows went.
///
/// # Errors
/// [`VaultError::Database`].
pub(crate) async fn forget_update_address(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<u64, VaultError> {
    sqlx::query!(
        "DELETE FROM privacy.update_email WHERE contact_id = $1",
        contact
    )
    .execute(db)
    .await
    .map(|done| done.rows_affected())
    .map_err(db_err)
}

/// One audit row by `actor` for `action` on address row `row`'s `email` field (PRIV-003,
/// PRIV-028: the field's name only).
async fn record(
    db: &mut PgConnection,
    (actor, action): (Actor, &'static str),
    row: Uuid,
) -> Result<(), VaultError> {
    let entry = AuditEntry {
        actor,
        action,
        subject_kind: SUBJECT,
        subject_id: row,
        change: Change::Field("email"),
    };
    audit(db, &entry)
        .await
        .map(drop)
        .map_err(|_| VaultError::Audit)
}
