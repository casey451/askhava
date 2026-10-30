//! The contact vault: sealing, the blind index, audited reads, key rotation and the read-rate alarm (PRIV-001 to PRIV-004, PRIV-024, SEC-047).
//!
//! Each contact field is sealed on its own with AES-GCM (256-bit key) under the active `VAULT_KEY`, a
//! fresh random 96-bit nonce from the OS source and associated data naming the table, column, row
//! and key version, so a sealed value moved to another row or column never opens (PRIV-002,
//! PRIV-024). The phone is found by an HMAC-SHA256 digest under a key derived per purpose from
//! `VAULT_INDEX_KEY` (`homeowner-phone|v1`), never by its plaintext (postgres brain P12). Every key
//! version stays readable, so rows sealed under v1 open while v2 is active and [`rotate_batch`]
//! moves them over (PRIV-024). Every plaintext read writes one `ops.audit_log` row per field in
//! the read's own transaction and counts toward the hourly SEC-047 check, [`check_read_rates`].

use std::collections::BTreeMap;

use aes_gcm::aead::{Aead, Generate, Nonce, Payload};
use aes_gcm::{Aes256Gcm, Key};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use zeroize::Zeroizing;

use crate::models::{
    ContactDetails, Field, GRANTS, Grant, IndexPurpose, NewContact, Purpose, Reader, VaultError,
    normalize_phone,
};

/// A read count above this multiple of its 14-day hourly baseline raises a High alert (SEC-047).
pub const RATE_MULTIPLE: f64 = 3.0;
/// The baseline window in hours: 14 days (SEC-047).
pub const BASELINE_HOURS: f64 = 336.0;
/// The least baseline used, in reads per hour, so a reader's first few reads in a quiet fortnight
/// are not a spike; a burst above three reads in one hour still alerts (SEC-047, D-750).
pub const BASELINE_FLOOR: f64 = 1.0;

const NONCE_LEN: usize = 12;
const VERSION_LEN: usize = 2;
/// The alert raised for a read whose reader, purpose or field is not a grant (SEC-047).
const UNREGISTERED_ALERT: &str = "privacy.vault_read_unregistered";
/// The alert raised for an hour's reads above three times their baseline (SEC-047).
const SPIKE_ALERT: &str = "privacy.vault_read_spike";

type KeyBytes = Zeroizing<[u8; 32]>;

/// The vault's keys: every version of `VAULT_KEY` and `VAULT_INDEX_KEY`, the highest active
/// (PRIV-002, PRIV-024). Built once at start-up from the host's variables; it has no `Debug`, so
/// no key can reach a log (PRIV-016).
pub struct Vault {
    seal_keys: BTreeMap<u16, Aes256Gcm>,
    index_keys: BTreeMap<u16, KeyBytes>,
}

impl Vault {
    /// Builds the vault from `VAULT_KEY` and `VAULT_INDEX_KEY`, each a comma-separated list of
    /// `<version>:<base64 of 32 bytes>` (for example `2:…,1:…` during a rotation). Both must be
    /// set, distinct, and absent from the database credential `database_url`, so a leaked
    /// credential or dump never opens the vault (PRIV-002, PRIV-024).
    ///
    /// # Errors
    ///
    /// [`VaultError::Keys`] naming what is wrong, never the value.
    pub fn new(
        vault_key: &SecretString,
        index_key: &SecretString,
        database_url: &SecretString,
    ) -> Result<Self, VaultError> {
        let seal = parse_keys(vault_key.expose_secret())?;
        let index = parse_keys(index_key.expose_secret())?;
        let url = database_url.expose_secret();
        let parts = vault_key.expose_secret().split(',');
        if parts
            .chain(index_key.expose_secret().split(','))
            .filter_map(|entry| entry.split_once(':'))
            .any(|(_, b64)| url.contains(b64.trim()))
        {
            return Err(VaultError::Keys("a key is inside the database credential"));
        }
        if seal.values().any(|k| index.values().any(|i| **i == **k)) {
            return Err(VaultError::Keys(
                "VAULT_KEY and VAULT_INDEX_KEY share a key",
            ));
        }
        let seal_keys = seal
            .iter()
            .map(|(v, k)| (*v, Aes256Gcm::new(&Key::<Aes256Gcm>::from(**k))))
            .collect();
        Ok(Self {
            seal_keys,
            index_keys: index,
        })
    }

    /// The active `VAULT_KEY` version, the one new values are sealed under.
    #[must_use]
    pub fn seal_version(&self) -> u16 {
        self.seal_keys
            .keys()
            .next_back()
            .copied()
            .unwrap_or_default()
    }

    /// The active `VAULT_INDEX_KEY` version.
    #[must_use]
    pub fn index_version(&self) -> u16 {
        self.index_keys
            .keys()
            .next_back()
            .copied()
            .unwrap_or_default()
    }

    /// Seals `plain` for `column` of contact `id` under the active key with a random nonce from the
    /// OS source; the result is the key version, the nonce and the ciphertext (PRIV-002, PRIV-024).
    ///
    /// # Panics
    ///
    /// When the operating system's random source fails, as no value may be sealed without it.
    fn seal(&self, column: &str, id: Uuid, plain: &str) -> Result<Vec<u8>, VaultError> {
        let version = self.seal_version();
        let cipher = self.seal_keys.get(&version).ok_or(VaultError::Sealed)?;
        let nonce = Nonce::<Aes256Gcm>::generate();
        let aad = associated_data(column, id, version);
        let payload = Payload {
            msg: plain.as_bytes(),
            aad: aad.as_bytes(),
        };
        let sealed = cipher
            .encrypt(&nonce, payload)
            .map_err(|_| VaultError::Sealed)?;
        Ok([&version.to_be_bytes()[..], &nonce[..], &sealed].concat())
    }

    /// Opens a value sealed for `column` of contact `id` under whichever key version it names.
    fn open(&self, column: &str, id: Uuid, sealed: &[u8]) -> Result<SecretString, VaultError> {
        let (head, body) = sealed
            .split_at_checked(VERSION_LEN + NONCE_LEN)
            .ok_or(VaultError::Sealed)?;
        let version = u16::from_be_bytes([head[0], head[1]]);
        let cipher = self.seal_keys.get(&version).ok_or(VaultError::Sealed)?;
        let nonce =
            Nonce::<Aes256Gcm>::try_from(&head[VERSION_LEN..]).map_err(|_| VaultError::Sealed)?;
        let aad = associated_data(column, id, version);
        let plain = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: body,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| VaultError::Sealed)?;
        String::from_utf8(plain)
            .map(SecretString::from)
            .map_err(|_| VaultError::Sealed)
    }

    /// The blind-index digests of `value` for `purpose` under every index key version, the active
    /// first: HMAC-SHA256 under the key derived as HMAC(`VAULT_INDEX_KEY` vN, `<purpose>|vN`)
    /// (PRIV-024). Equality-only; it leaks which rows share a value (postgres brain P12).
    #[must_use]
    pub fn blind_index(&self, purpose: IndexPurpose, value: &str) -> Vec<(u16, [u8; 32])> {
        self.index_keys
            .iter()
            .rev()
            .map(|(version, master)| {
                let label = format!("{}|v{version}", purpose.label());
                let derived = hmac(&master[..], label.as_bytes());
                (*version, hmac(&derived, value.as_bytes()))
            })
            .collect()
    }
}

/// HMAC-SHA256 of `msg` under `key`.
fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).unwrap_or_else(|_| unreachable!());
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

/// The associated data binding a sealed value to its place and key version (PRIV-002, PRIV-024).
fn associated_data(column: &str, id: Uuid, version: u16) -> String {
    format!("privacy.contact|{column}|{id}|v{version}")
}

/// Parses `<version>:<base64>` entries, each a distinct version from 1 and a 32-byte key.
fn parse_keys(value: &str) -> Result<BTreeMap<u16, KeyBytes>, VaultError> {
    let mut keys = BTreeMap::new();
    for entry in value.split(',').map(str::trim) {
        let (version, b64) = entry
            .split_once(':')
            .ok_or(VaultError::Keys("not <version>:<base64>"))?;
        let version: u16 = version
            .parse()
            .map_err(|_| VaultError::Keys("bad version"))?;
        let bytes = Zeroizing::new(
            STANDARD
                .decode(b64)
                .map_err(|_| VaultError::Keys("bad base64"))?,
        );
        let key: [u8; 32] = bytes[..]
            .try_into()
            .map_err(|_| VaultError::Keys("not 32 bytes"))?;
        if version == 0 || keys.insert(version, Zeroizing::new(key)).is_some() {
            return Err(VaultError::Keys("bad or repeated version"));
        }
    }
    Ok(keys)
}

fn db_err(_: sqlx::Error) -> VaultError {
    VaultError::Database
}

/// Stores a new contact, every field sealed and the phone blind-indexed, and audits the creation;
/// returns its ID (PRIV-001, PRIV-002).
///
/// # Errors
///
/// [`VaultError::BadPhone`], [`VaultError::PhoneTaken`], [`VaultError::Audit`] or
/// [`VaultError::Database`]; nothing is stored then.
pub async fn store_contact(
    db: &mut PgConnection,
    vault: &Vault,
    new: &NewContact,
) -> Result<Uuid, VaultError> {
    let phone = new
        .phone
        .as_ref()
        .map(|p| normalize_phone(p.expose_secret()))
        .transpose()?;
    if let Some(phone) = &phone
        && find_by_phone(db, vault, phone).await?.is_some()
    {
        return Err(VaultError::PhoneTaken);
    }
    let mut tx = db.begin().await.map_err(db_err)?;
    let id = sqlx::query_scalar!(r#"SELECT gen_random_uuid() AS "id!""#)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
    let seal = |f: Field, v: Option<&str>| v.map(|v| vault.seal(f.column(), id, v)).transpose();
    let get = |s: &Option<SecretString>| s.as_ref().map(|s| s.expose_secret().to_owned());
    let name = seal(Field::Name, get(&new.name).as_deref())?;
    let phone_sealed = seal(Field::Phone, phone.as_deref())?;
    let email = seal(Field::Email, get(&new.email).as_deref())?;
    let address = seal(Field::Address, get(&new.address).as_deref())?;
    let onsite = seal(Field::OnsiteContact, get(&new.onsite_contact).as_deref())?;
    sqlx::query!(
        "INSERT INTO privacy.contact (id, name, phone, email, address, onsite_contact) VALUES ($1, $2, $3, $4, $5, $6)",
        id, name, phone_sealed, email, address, onsite,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if let Some(phone) = &phone {
        index_phone(&mut tx, vault, id, phone).await?;
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: "contact_created",
        subject_kind: "privacy.contact",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| VaultError::Audit)?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

/// Replaces contact `id`'s phone digests with one under the active index key.
async fn index_phone(
    db: &mut PgConnection,
    vault: &Vault,
    id: Uuid,
    phone: &str,
) -> Result<(), VaultError> {
    let purpose = IndexPurpose::HomeownerPhone;
    let (version, digest) = vault.blind_index(purpose, phone)[0];
    sqlx::query!(
        "DELETE FROM privacy.contact_index WHERE contact_id = $1 AND purpose = $2",
        id,
        purpose.label()
    )
    .execute(&mut *db)
    .await
    .map_err(db_err)?;
    sqlx::query!(
        "INSERT INTO privacy.contact_index (purpose, key_version, digest, contact_id) VALUES ($1, $2, $3, $4)",
        purpose.label(), i32::from(version), &digest[..], id,
    )
    .execute(db)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// The contact whose phone is `phone`, looked up by its digest under every index key version, so
/// a number indexed under v1 is still found while v2 is active (PRIV-024). Reads no plaintext.
///
/// # Errors
///
/// [`VaultError::BadPhone`] or [`VaultError::Database`].
pub async fn find_by_phone(
    db: &mut PgConnection,
    vault: &Vault,
    phone: &str,
) -> Result<Option<Uuid>, VaultError> {
    let phone = normalize_phone(phone)?;
    let (versions, digests): (Vec<i32>, Vec<Vec<u8>>) = vault
        .blind_index(IndexPurpose::HomeownerPhone, &phone)
        .into_iter()
        .map(|(v, d)| (i32::from(v), d.to_vec()))
        .unzip();
    sqlx::query_scalar!(
        "SELECT i.contact_id FROM privacy.contact_index i \
         JOIN unnest($2::int4[], $3::bytea[]) AS k(v, d) ON i.key_version = k.v AND i.digest = k.d \
         WHERE i.purpose = $1 LIMIT 1",
        IndexPurpose::HomeownerPhone.label(),
        &versions,
        &digests,
    )
    .fetch_optional(db)
    .await
    .map_err(db_err)
}

/// Refuses a read that matches no grant and raises a High alert at once (SEC-047).
///
/// # Errors
///
/// Always [`VaultError::UnregisteredPurpose`], or [`VaultError::Audit`] if the alert failed.
pub(crate) async fn refuse(db: &mut PgConnection) -> Result<ContactDetails, VaultError> {
    let alert = NewAlert {
        kind: UNREGISTERED_ALERT,
        severity: Severity::High,
        subject: None,
    };
    raise_alert(db, &alert)
        .await
        .map_err(|_| VaultError::Audit)?;
    Err(VaultError::UnregisteredPurpose)
}

/// Reads `fields` of contact `id` under `grant`: one audit row per field naming the reader and
/// purpose (the grant's action) and the read counted for SEC-047, all in one transaction that
/// commits before any plaintext is returned (PRIV-003, PRIV-004).
///
/// # Errors
///
/// [`VaultError::NotFound`], [`VaultError::Sealed`], [`VaultError::Audit`] or [`VaultError::Database`].
pub(crate) async fn read(
    db: &mut PgConnection,
    vault: &Vault,
    access: (&Grant, Actor),
    id: Uuid,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    let mut tx = db.begin().await.map_err(db_err)?;
    let details = open_row(&mut tx, vault, id, fields).await?;
    audit_read(&mut tx, access, id, fields).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(details)
}

/// Opens `fields` of contact `id`, locking the row for the transaction.
async fn open_row(
    db: &mut PgConnection,
    vault: &Vault,
    id: Uuid,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    let row = sqlx::query!(
        "SELECT name, phone, email, address, onsite_contact FROM privacy.contact WHERE id = $1 FOR UPDATE",
        id,
    )
    .fetch_optional(db)
    .await
    .map_err(db_err)?
    .ok_or(VaultError::NotFound)?;
    let sealed = [
        row.name,
        row.phone,
        row.email,
        row.address,
        row.onsite_contact,
    ];
    let mut details = ContactDetails {
        id,
        ..ContactDetails::default()
    };
    for (field, value) in Field::ALL.into_iter().zip(sealed) {
        if let (true, Some(value)) = (fields.contains(&field), value) {
            *details.slot(field) = Some(vault.open(field.column(), id, &value)?);
        }
    }
    Ok(details)
}

/// Writes one audit row per field read and adds the read to this hour's count (PRIV-003, SEC-047).
async fn audit_read(
    db: &mut PgConnection,
    (grant, actor): (&Grant, Actor),
    id: Uuid,
    fields: &[Field],
) -> Result<(), VaultError> {
    for field in fields {
        let entry = AuditEntry {
            actor,
            action: grant.action,
            subject_kind: "privacy.contact",
            subject_id: id,
            change: Change::Field(field.column()),
        };
        audit(db, &entry).await.map_err(|_| VaultError::Audit)?;
    }
    sqlx::query!(
        "INSERT INTO privacy.read_count (hour, action, reads) VALUES (date_trunc('hour', now(), 'UTC'), $1, 1) \
         ON CONFLICT (action, hour) DO UPDATE SET reads = privacy.read_count.reads + 1",
        grant.action,
    )
    .execute(db)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// The grant `privacy` reads under for `purpose`.
fn own_grant(purpose: Purpose) -> &'static Grant {
    GRANTS
        .iter()
        .find(|g| g.reader == Reader::Privacy && g.purpose == purpose)
        .unwrap_or_else(|| unreachable!())
}

/// Re-seals up to `limit` contacts still under an older `VAULT_KEY` or `VAULT_INDEX_KEY` version
/// with the active ones, rebuilding each phone digest; returns how many it moved. Run it until it
/// returns 0: both versions stay readable throughout (PRIV-024). Each move is its own transaction
/// and audits its plaintext read as `privacy`'s rotation (PRIV-003).
///
/// # Errors
///
/// [`VaultError::Sealed`], [`VaultError::Audit`] or [`VaultError::Database`].
pub async fn rotate_batch(
    db: &mut PgConnection,
    vault: &Vault,
    limit: i64,
) -> Result<u64, VaultError> {
    let ids = sqlx::query_scalar!(
        "SELECT id FROM privacy.contact WHERE oldest_key_version < $1 \
         OR id IN (SELECT contact_id FROM privacy.contact_index WHERE key_version < $2) ORDER BY id LIMIT $3",
        i32::from(vault.seal_version()), i32::from(vault.index_version()), limit,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(db_err)?;
    for id in &ids {
        let mut tx = db.begin().await.map_err(db_err)?;
        let details = open_row(&mut tx, vault, *id, &Field::ALL).await?;
        audit_read(
            &mut tx,
            (own_grant(Purpose::Rotation), Actor::System),
            *id,
            &Field::ALL,
        )
        .await?;
        reseal(&mut tx, vault, details).await?;
        tx.commit().await.map_err(db_err)?;
    }
    Ok(ids.len() as u64)
}

/// Writes `details` back sealed under the active key and re-indexes its phone.
async fn reseal(
    db: &mut PgConnection,
    vault: &Vault,
    mut details: ContactDetails,
) -> Result<(), VaultError> {
    let id = details.id;
    let mut sealed = Vec::with_capacity(Field::ALL.len());
    for field in Field::ALL {
        let value = details
            .slot(field)
            .as_ref()
            .map(|v| vault.seal(field.column(), id, v.expose_secret()));
        sealed.push(value.transpose()?);
    }
    let [name, phone, email, address, onsite] =
        <[Option<Vec<u8>>; 5]>::try_from(sealed).unwrap_or_else(|_| unreachable!());
    sqlx::query!(
        "UPDATE privacy.contact SET name = $2, phone = $3, email = $4, address = $5, onsite_contact = $6 WHERE id = $1",
        id, name, phone, email, address, onsite,
    )
    .execute(&mut *db)
    .await
    .map_err(db_err)?;
    if let Some(phone) = &details.phone {
        index_phone(db, vault, id, phone.expose_secret()).await?;
    }
    Ok(())
}

/// Compares each reader-purpose count of the hour starting `hour` with its 14-day hourly baseline
/// and raises one High alert for each count above three times it (floored at
/// [`BASELINE_FLOOR`]); returns how many it raised. Safe to run twice: an hour alerts once (SEC-047).
///
/// # Errors
///
/// [`VaultError::Audit`] or [`VaultError::Database`].
pub async fn check_read_rates(db: &mut PgConnection, hour: Timestamp) -> Result<usize, VaultError> {
    let mut tx = db.begin().await.map_err(db_err)?;
    let spikes = sqlx::query_scalar!(
        "UPDATE privacy.read_count c SET alerted = true WHERE c.hour = $1 AND NOT c.alerted \
         AND c.reads > $2::float8 * greatest($3::float8, (SELECT coalesce(sum(p.reads), 0) FROM privacy.read_count p \
           WHERE p.action = c.action AND p.hour >= $1 - interval '14 days' AND p.hour < $1) / $4::float8) \
         RETURNING c.id",
        hour.to_sqlx() as _, RATE_MULTIPLE, BASELINE_FLOOR, BASELINE_HOURS,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    for id in &spikes {
        let alert = NewAlert {
            kind: SPIKE_ALERT,
            severity: Severity::High,
            subject: Some(("privacy.read_count", *id)),
        };
        raise_alert(&mut tx, &alert)
            .await
            .map_err(|_| VaultError::Audit)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(spikes.len())
}
