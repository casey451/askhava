//! Business members' sign-in addresses and texting phones, sealed apart from the homeowner vault (SEC-004, BIZ-037, BIZ-040, PRIV-002, PRIV-003, PRIV-004, PRIV-016, PRIV-024, PRIV-026, SEC-047; D-3262).
//!
//! A business member's contacts are not homeowner contact fields (PRIV-001), so they stay out of
//! the homeowner record, `Field`, `Reader` and `GRANTS`: `privacy.business_mailbox` seals one
//! sign-in address per mailbox ID (the opaque ID every `businesses` table stores) and finds it only
//! by the business email blind index [`email_index`] derives, so `read_inbound_email` matches the
//! same mailbox (PRIV-026); `privacy.staff_phone` seals the texting number of one
//! `businesses.staff_phone` line. Values are sealed only through the vault's closed
//! `BusinessTable` wrappers (PRIV-002). Plaintext leaves only through [`read_business_mailbox`] and
//! [`read_staff_phone`], read by the `server` composition alone on a bare connection, each read
//! audited and counted under `vault_read.server.<purpose>` for the SEC-047 rate check (PRIV-003,
//! D-752). No value is logged, put in an error or shown by `Debug` (PRIV-016). [`forget_unused`]
//! is the sweep's step and [`rotate_business_batch`] runs in the rotation drill beside
//! `rotate_batch` (PRIV-024).

use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::models::{Purpose, VaultError, normalize_phone};
use crate::reader::email_index;
use crate::vault::{
    BusinessTable, UNREGISTERED_ALERT, Vault, count_read, db_err, ensure_bare, own_grant,
};

/// The longest sign-in address stored, in characters (RFC 5321's path limit).
pub const MAX_ADDRESS_LEN: usize = 254;

/// Why a business-contact call failed. No variant carries a value or the database's message
/// (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BusinessContactError {
    /// The address has no `@` or is over [`MAX_ADDRESS_LEN`] characters; nothing was stored.
    #[error("not an email address")]
    BadAddress,
    /// The number is not a 10-digit US number; nothing was stored.
    #[error("not a US phone number")]
    BadPhone,
    /// No mailbox or line has this ID.
    #[error("no such business contact")]
    NotFound,
    /// A phone read under a purpose that may not open it: refused, nothing read, a High alert
    /// raised (SEC-004, SEC-047).
    #[error("this purpose may not read a phone")]
    PhoneRefused,
    /// The caller's connection is inside a transaction; nothing was read or audited (D-752).
    #[error("a business contact read must not run inside the caller's transaction")]
    InTransaction,
    /// A value failed to seal or open, or the vault holds no key (PRIV-002).
    #[error("a business contact value failed to seal or open")]
    Sealed,
    /// The audit row or alert could not be written, so nothing was read.
    #[error("the business contact read could not be audited")]
    Audit,
    /// The database refused or dropped the statement.
    #[error("the business contact database statement failed")]
    Database,
    /// A rotation moved nothing because each mailbox left holds an address another mailbox is
    /// already found by under the active index key: merge or delete one of the pair (PRIV-024).
    #[error("a mailbox shares its sign-in address with another")]
    DuplicateAddress,
}

impl From<VaultError> for BusinessContactError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::InTransaction => Self::InTransaction,
            VaultError::BadPhone => Self::BadPhone,
            VaultError::Sealed => Self::Sealed,
            VaultError::Audit => Self::Audit,
            _ => Self::Database,
        }
    }
}

fn db_fail(error: sqlx::Error) -> BusinessContactError {
    db_err(error).into()
}

/// Why the composition opens a business member's contact (SEC-004, BIZ-037, SEC-047). A later
/// task that sends business notices adds its own purpose here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessPurpose {
    /// The sign-in link email (T280): the address.
    SignInLink,
    /// The person's own new-device code (SEC-004): the address and the phone.
    SignInCode,
    /// The account mailer (D-3186): the address only.
    AccountEmail,
    /// The owner's phone approval page (T397): the address.
    PhoneApproval,
}

impl BusinessPurpose {
    /// Every purpose.
    pub const ALL: [Self; 4] = [
        Self::SignInLink,
        Self::SignInCode,
        Self::AccountEmail,
        Self::PhoneApproval,
    ];

    /// The audit action and read-count label, `vault_read.server.<purpose>` (SEC-047).
    #[must_use]
    pub const fn action(self) -> &'static str {
        match self {
            Self::SignInLink => "vault_read.server.sign_in_link",
            Self::SignInCode => "vault_read.server.sign_in_code",
            Self::AccountEmail => "vault_read.server.account_email",
            Self::PhoneApproval => "vault_read.server.phone_approval",
        }
    }

    /// Whether this purpose may open a texting phone: only the person's own code (SEC-004).
    #[must_use]
    pub const fn opens_phone(self) -> bool {
        matches!(self, Self::SignInCode)
    }
}

/// `address` as [`email_index`] reads a sender: the part inside `<…>` when present, trimmed and
/// lower-cased; refused with no `@` or over [`MAX_ADDRESS_LEN`] characters.
fn bare_address(address: &str) -> Result<String, BusinessContactError> {
    let bare = address
        .rsplit_once('<')
        .map_or(address, |(_, rest)| rest.trim_end_matches('>'))
        .trim()
        .to_lowercase();
    if !bare.contains('@') || bare.chars().count() > MAX_ADDRESS_LEN {
        return Err(BusinessContactError::BadAddress);
    }
    Ok(bare)
}

/// The mailbox IDs whose `email` digest matches one of `digests`, in ID order.
async fn find(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<Vec<Uuid>, BusinessContactError> {
    let versions: Vec<i32> = digests.iter().map(|(v, _)| i32::from(*v)).collect();
    let hashes: Vec<Vec<u8>> = digests.iter().map(|(_, d)| d.to_vec()).collect();
    sqlx::query_scalar!(
        r#"SELECT DISTINCT mailbox_id AS "id!" FROM privacy.business_mailbox_index
           WHERE purpose = 'email' AND (key_version, digest) IN
             (SELECT * FROM unnest($1::int4[], $2::bytea[])) ORDER BY 1"#,
        &versions,
        &hashes,
    )
    .fetch_all(db)
    .await
    .map_err(db_fail)
}

/// The mailbox of a sign-in address: the one its digest finds under any readable index key
/// version, or a new one sealed and indexed under the active versions; `touched_at` moves either
/// way. Runs in the caller's transaction (in a savepoint), race-safe: of two calls minting one
/// address, the loser drops its row and returns the winner's. Returns only the ID (SEC-004,
/// BIZ-037, PRIV-002, PRIV-026).
///
/// # Errors
///
/// [`BusinessContactError::BadAddress`], [`BusinessContactError::Sealed`] or
/// [`BusinessContactError::Database`]. Two transactions each minting two new addresses in
/// opposite order can deadlock on the index's unique key; Postgres aborts one with `Database`,
/// and a deadlock-aborted caller retries its transaction.
pub async fn mailbox_for(
    db: &mut PgConnection,
    vault: &Vault,
    address: &str,
) -> Result<Uuid, BusinessContactError> {
    let bare = bare_address(address)?;
    let digests = email_index(vault, &bare, true);
    let active = *digests.first().ok_or(BusinessContactError::Sealed)?;
    let mut tx = db.begin().await.map_err(db_fail)?;
    let id = match touch(&mut tx, &digests).await? {
        Some(id) => id,
        None => mint(&mut tx, vault, &bare, active, &digests).await?,
    };
    tx.commit().await.map_err(db_fail)?;
    Ok(id)
}

/// Moves `touched_at` of the mailbox one of `digests` finds and returns its ID, in one statement:
/// a mailbox `forget_unused` deleted meanwhile updates no row and finds none, so it is never
/// returned (PRIV-026).
async fn touch(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<Option<Uuid>, BusinessContactError> {
    let versions: Vec<i32> = digests.iter().map(|(v, _)| i32::from(*v)).collect();
    let hashes: Vec<Vec<u8>> = digests.iter().map(|(_, d)| d.to_vec()).collect();
    sqlx::query_scalar!(
        "UPDATE privacy.business_mailbox SET touched_at = clock_timestamp() WHERE id = \
         (SELECT mailbox_id FROM privacy.business_mailbox_index WHERE purpose = 'email' AND \
         (key_version, digest) IN (SELECT * FROM unnest($1::int4[], $2::bytea[])) \
         ORDER BY mailbox_id LIMIT 1) RETURNING id",
        &versions,
        &hashes,
    )
    .fetch_optional(db)
    .await
    .map_err(db_fail)
}

/// Seals and indexes `bare` as a new mailbox; when a simultaneous call indexed it first, deletes
/// this row and returns that mailbox (PRIV-002).
async fn mint(
    db: &mut PgConnection,
    vault: &Vault,
    bare: &str,
    (version, digest): (u16, [u8; 32]),
    digests: &[(u16, [u8; 32])],
) -> Result<Uuid, BusinessContactError> {
    let id = sqlx::query_scalar!(r#"SELECT gen_random_uuid() AS "id!""#)
        .fetch_one(&mut *db)
        .await
        .map_err(db_fail)?;
    let sealed = vault.seal_business(BusinessTable::MailboxAddress, id, bare)?;
    sqlx::query!(
        "INSERT INTO privacy.business_mailbox (id, address) VALUES ($1, $2)",
        id,
        sealed,
    )
    .execute(&mut *db)
    .await
    .map_err(db_fail)?;
    let won = sqlx::query_scalar!(
        "INSERT INTO privacy.business_mailbox_index (purpose, key_version, digest, mailbox_id) \
         VALUES ('email', $1, $2, $3) ON CONFLICT DO NOTHING RETURNING mailbox_id",
        i32::from(version),
        &digest[..],
        id,
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(db_fail)?;
    if won.is_some() {
        return Ok(id);
    }
    sqlx::query!("DELETE FROM privacy.business_mailbox WHERE id = $1", id)
        .execute(&mut *db)
        .await
        .map_err(db_fail)?;
    touch(db, digests)
        .await?
        .ok_or(BusinessContactError::Database)
}

/// The mailbox IDs whose digest of `address` matches under every readable index key version;
/// reads no plaintext and touches nothing, and an unknown or malformed address finds none
/// (PRIV-024, PRIV-026).
///
/// # Errors
///
/// [`BusinessContactError::Database`].
pub async fn mailboxes_of(
    db: &mut PgConnection,
    vault: &Vault,
    address: &str,
) -> Result<Vec<Uuid>, BusinessContactError> {
    match bare_address(address) {
        Ok(bare) => find(db, &email_index(vault, &bare, true)).await,
        Err(_) => Ok(Vec::new()),
    }
}

/// Seals `number`, normalized to `+1` and ten digits, as the texting phone of `line`, replacing
/// any number it held; runs in the caller's transaction (SEC-004, PRIV-002).
///
/// # Errors
///
/// [`BusinessContactError::BadPhone`], [`BusinessContactError::Sealed`] or
/// [`BusinessContactError::Database`].
pub async fn store_staff_phone(
    db: &mut PgConnection,
    vault: &Vault,
    line: Uuid,
    number: &str,
) -> Result<(), BusinessContactError> {
    let phone = normalize_phone(number)?;
    let sealed = vault.seal_business(BusinessTable::StaffPhoneNumber, line, &phone)?;
    sqlx::query!(
        "INSERT INTO privacy.staff_phone (line_id, number) VALUES ($1, $2) \
         ON CONFLICT (line_id) DO UPDATE SET number = EXCLUDED.number, set_at = clock_timestamp()",
        line,
        sealed,
    )
    .execute(db)
    .await
    .map_err(db_fail)?;
    Ok(())
}

/// The sealed value of row `id` of `table`, locked for the caller's transaction.
async fn sealed_of(
    db: &mut PgConnection,
    table: BusinessTable,
    id: Uuid,
) -> Result<Vec<u8>, BusinessContactError> {
    match table {
        BusinessTable::MailboxAddress => sqlx::query_scalar!(
            "SELECT address FROM privacy.business_mailbox WHERE id = $1 FOR UPDATE",
            id,
        )
        .fetch_optional(db)
        .await
        .map_err(db_fail)?,
        BusinessTable::StaffPhoneNumber => sqlx::query_scalar!(
            "SELECT number FROM privacy.staff_phone WHERE line_id = $1 FOR UPDATE",
            id,
        )
        .fetch_optional(db)
        .await
        .map_err(db_fail)?,
    }
    .ok_or(BusinessContactError::NotFound)
}

/// Writes one `ops` audit row for the read of `table` row `id` under `action` (the row as
/// subject, never the value) and counts it for SEC-047 (PRIV-003).
async fn audit_open(
    db: &mut PgConnection,
    action: &'static str,
    table: BusinessTable,
    id: Uuid,
) -> Result<(), BusinessContactError> {
    let (subject_kind, column) = table.place();
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind,
        subject_id: id,
        change: Change::Field(column),
    };
    audit(db, &entry)
        .await
        .map_err(|_| BusinessContactError::Audit)?;
    Ok(count_read(db, action).await?)
}

/// Opens row `id` of `table` for `purpose` on a bare connection, its audit row and count
/// committed before the plaintext is returned (PRIV-003, D-752).
async fn open_one(
    db: &mut PgConnection,
    vault: &Vault,
    (purpose, table): (BusinessPurpose, BusinessTable),
    id: Uuid,
) -> Result<SecretString, BusinessContactError> {
    let mut tx = db.begin().await.map_err(db_fail)?;
    let sealed = sealed_of(&mut tx, table, id).await?;
    let value = vault.open_business(table, id, &sealed)?;
    audit_open(&mut tx, purpose.action(), table, id).await?;
    tx.commit().await.map_err(db_fail)?;
    Ok(value)
}

/// Opens the sign-in address of `mailbox` for `purpose`, for the `server` composition alone: one
/// `vault_read.server.<purpose>` audit row and one read count, committed first; refused inside a
/// transaction with nothing read or audited (SEC-004, BIZ-037, PRIV-003, PRIV-004, SEC-047, D-752).
///
/// # Errors
///
/// [`BusinessContactError::InTransaction`], [`BusinessContactError::NotFound`],
/// [`BusinessContactError::Sealed`], [`BusinessContactError::Audit`] or
/// [`BusinessContactError::Database`].
pub async fn read_business_mailbox(
    db: &mut PgConnection,
    vault: &Vault,
    purpose: BusinessPurpose,
    mailbox: Uuid,
) -> Result<SecretString, BusinessContactError> {
    ensure_bare(db).await?;
    open_one(db, vault, (purpose, BusinessTable::MailboxAddress), mailbox).await
}

/// Opens the texting phone of `line` for `purpose`, as [`read_business_mailbox`]; only
/// [`BusinessPurpose::SignInCode`] may: any other purpose reads nothing and raises one High
/// `privacy.vault_read_unregistered` alert naming the purpose and the line (SEC-004, SEC-047).
///
/// # Errors
///
/// As [`read_business_mailbox`], and [`BusinessContactError::PhoneRefused`].
pub async fn read_staff_phone(
    db: &mut PgConnection,
    vault: &Vault,
    purpose: BusinessPurpose,
    line: Uuid,
) -> Result<SecretString, BusinessContactError> {
    ensure_bare(db).await?;
    if !purpose.opens_phone() {
        let alert = NewAlert {
            kind: UNREGISTERED_ALERT,
            severity: Severity::High,
            subject: Some((purpose.action(), line)),
        };
        raise_alert(db, &alert)
            .await
            .map_err(|_| BusinessContactError::Audit)?;
        return Err(BusinessContactError::PhoneRefused);
    }
    open_one(db, vault, (purpose, BusinessTable::StaffPhoneNumber), line).await
}

/// Deletes every mailbox not in `mailboxes` last touched before `before` (its digests with it) and
/// every staff phone not in `lines` last set before `before`; returns how many rows went. What is
/// already gone is a no-op, so the step is safe to repeat (PRIV-002, D-3262).
///
/// # Errors
///
/// [`BusinessContactError::Database`].
pub async fn forget_unused(
    db: &mut PgConnection,
    (mailboxes, lines): (&[Uuid], &[Uuid]),
    before: Timestamp,
) -> Result<u64, BusinessContactError> {
    let gone = sqlx::query!(
        "DELETE FROM privacy.business_mailbox WHERE touched_at < $2::timestamptz AND NOT (id = ANY($1))",
        mailboxes,
        before.to_sqlx() as _,
    )
    .execute(&mut *db)
    .await
    .map_err(db_fail)?
    .rows_affected();
    let phones = sqlx::query!(
        "DELETE FROM privacy.staff_phone WHERE set_at < $2::timestamptz AND NOT (line_id = ANY($1))",
        lines,
        before.to_sqlx() as _,
    )
    .execute(&mut *db)
    .await
    .map_err(db_fail)?
    .rows_affected();
    Ok(gone + phones)
}

/// Re-seals up to `limit` staff phones under an older `VAULT_KEY`, then up to `limit` mailboxes
/// under an older `VAULT_KEY` or index key version, re-indexing each address under the active
/// one; each move is its own transaction and audits its read as `privacy`'s rotation. A mailbox
/// whose address another mailbox already holds under the active index key is skipped, unmoved and
/// still findable by its old digest, and the rest carry on. Returns how many it moved: run it
/// beside `rotate_batch` until it returns 0 (PRIV-024, D-3517).
///
/// # Errors
///
/// [`BusinessContactError::DuplicateAddress`] when it moved nothing but skipped a duplicate (merge
/// or delete one mailbox of the pair, then run again), [`BusinessContactError::Sealed`],
/// [`BusinessContactError::Audit`] or [`BusinessContactError::Database`].
pub async fn rotate_business_batch(
    db: &mut PgConnection,
    vault: &Vault,
    limit: i64,
) -> Result<u64, BusinessContactError> {
    let lines = sqlx::query_scalar!(
        "SELECT line_id FROM privacy.staff_phone WHERE key_version < $1 ORDER BY line_id LIMIT $2",
        i32::from(vault.seal_version()),
        limit,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(db_fail)?;
    for line in &lines {
        reseal(db, vault, BusinessTable::StaffPhoneNumber, *line).await?;
    }
    let (moved, skipped) = rotate_mailboxes(db, vault, limit).await?;
    let moved = moved + lines.len() as u64;
    if moved == 0 && skipped > 0 {
        return Err(BusinessContactError::DuplicateAddress);
    }
    Ok(moved)
}

/// Moves up to `limit` mailboxes left under an older key, skipping each one whose move finds its
/// address held by another mailbox; returns how many it moved and how many it skipped (PRIV-024).
async fn rotate_mailboxes(
    db: &mut PgConnection,
    vault: &Vault,
    limit: i64,
) -> Result<(u64, u64), BusinessContactError> {
    let mut skipped: Vec<Uuid> = Vec::new();
    let mut moved = 0_u64;
    loop {
        let ids = sqlx::query_scalar!(
            "SELECT id FROM privacy.business_mailbox WHERE key_version < $1 OR id IN \
             (SELECT mailbox_id FROM privacy.business_mailbox_index WHERE key_version < $2) \
             ORDER BY id LIMIT $3",
            i32::from(vault.seal_version()),
            i32::from(vault.index_version()),
            (limit - moved as i64).saturating_add(skipped.len() as i64),
        )
        .fetch_all(&mut *db)
        .await
        .map_err(db_fail)?;
        let fresh: Vec<Uuid> = ids.into_iter().filter(|id| !skipped.contains(id)).collect();
        if fresh.is_empty() || moved as i64 >= limit {
            return Ok((moved, skipped.len() as u64));
        }
        for id in fresh {
            match reseal(db, vault, BusinessTable::MailboxAddress, id).await {
                Ok(()) => moved += 1,
                Err(BusinessContactError::DuplicateAddress) => skipped.push(id),
                Err(other) => return Err(other),
            }
        }
    }
}

/// Opens, audits and re-seals row `id` of `table` under the active key in one transaction, a
/// mailbox's digests rebuilt under the active index key in a savepoint: on `DuplicateAddress`
/// only the move rolls back, and the read's audit row and count still commit (PRIV-003, PRIV-024,
/// SEC-047, D-3518).
async fn reseal(
    db: &mut PgConnection,
    vault: &Vault,
    table: BusinessTable,
    id: Uuid,
) -> Result<(), BusinessContactError> {
    let mut tx = db.begin().await.map_err(db_fail)?;
    let sealed = sealed_of(&mut tx, table, id).await?;
    let plain = vault.open_business(table, id, &sealed)?;
    audit_open(&mut tx, own_grant(Purpose::Rotation).action, table, id).await?;
    let resealed = vault.seal_business(table, id, plain.expose_secret())?;
    if table == BusinessTable::StaffPhoneNumber {
        sqlx::query!(
            "UPDATE privacy.staff_phone SET number = $2 WHERE line_id = $1",
            id,
            resealed,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_fail)?;
    } else {
        let mut inner = tx.begin().await.map_err(db_fail)?;
        if let Err(error) = reindex(&mut inner, vault, id, (&resealed, plain.expose_secret())).await
        {
            inner.rollback().await.map_err(db_fail)?;
            if error == BusinessContactError::DuplicateAddress {
                tx.commit().await.map_err(db_fail)?;
            }
            return Err(error);
        }
        inner.commit().await.map_err(db_fail)?;
    }
    tx.commit().await.map_err(db_fail)
}

/// Writes mailbox `id`'s re-sealed address and replaces its digests with the active one; fails
/// with `DuplicateAddress` (its caller rolls the move back) when another mailbox already holds that
/// digest, so a duplicate address shows up instead of leaving this mailbox unfindable (PRIV-024).
async fn reindex(
    db: &mut PgConnection,
    vault: &Vault,
    id: Uuid,
    (resealed, bare): (&[u8], &str),
) -> Result<(), BusinessContactError> {
    let (version, digest) = *email_index(vault, bare, true)
        .first()
        .ok_or(BusinessContactError::Sealed)?;
    sqlx::query!(
        "UPDATE privacy.business_mailbox SET address = $2 WHERE id = $1",
        id,
        resealed,
    )
    .execute(&mut *db)
    .await
    .map_err(db_fail)?;
    sqlx::query!(
        "DELETE FROM privacy.business_mailbox_index WHERE mailbox_id = $1 AND key_version <> $2",
        id,
        i32::from(version),
    )
    .execute(&mut *db)
    .await
    .map_err(db_fail)?;
    sqlx::query!(
        "INSERT INTO privacy.business_mailbox_index (purpose, key_version, digest, mailbox_id) \
         VALUES ('email', $1, $2, $3) ON CONFLICT (purpose, key_version, digest) DO UPDATE \
         SET mailbox_id = EXCLUDED.mailbox_id \
         WHERE privacy.business_mailbox_index.mailbox_id = EXCLUDED.mailbox_id RETURNING mailbox_id",
        i32::from(version),
        &digest[..],
        id,
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(db_fail)?
    .map(|_| ())
    .ok_or(BusinessContactError::DuplicateAddress)
}

/// The error mapping keeps no value and folds what cannot happen here into `Database`.
#[cfg(test)]
mod tests {
    use super::{BusinessContactError as E, BusinessPurpose, bare_address};
    use crate::models::VaultError;

    #[test]
    fn vault_errors_map_without_a_value() {
        assert_eq!(E::from(VaultError::InTransaction), E::InTransaction);
        assert_eq!(E::from(VaultError::BadPhone), E::BadPhone);
        assert_eq!(E::from(VaultError::Sealed), E::Sealed);
        assert_eq!(E::from(VaultError::Audit), E::Audit);
        assert_eq!(E::from(VaultError::NotFound), E::Database);
        let duplicate = E::DuplicateAddress.to_string();
        assert!(duplicate.contains("shares its sign-in address"));
        assert!(!duplicate.contains('@'), "the message names no address");
    }

    #[test]
    fn an_address_is_bare_lower_case_and_bounded() {
        let wrapped = bare_address("Pat Example <Pat@Example.COM>");
        assert_eq!(wrapped.as_deref(), Ok("pat@example.com"));
        assert_eq!(bare_address("no-at-sign"), Err(E::BadAddress));
        let long = format!("{}@example.com", "a".repeat(250));
        assert_eq!(bare_address(&long), Err(E::BadAddress));
        let only_sign_code = BusinessPurpose::ALL.iter().filter(|p| p.opens_phone());
        assert_eq!(only_sign_code.count(), 1);
    }
}
