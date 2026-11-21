//! Re-keying the suppression list to the active `VAULT_INDEX_KEY` version, and retiring an older
//! version (PRIV-024, CON-035, CON-013, D-079, D-4418, D-4748).
//!
//! `record` lists a number under the index key versions configured at that moment, so a number
//! that opted out while only v1 existed has a v1 row only and v1 could never be dropped
//! (`check_suppression_versions`). The order for an index key is: add the new version to
//! `VAULT_INDEX_KEY`; run `privacy::api::rotate_batch` and [`rekey_suppression_batch`] until both
//! return 0; call [`retire_index_version`] for the old version; only then drop it from the key.
//!
//! A number's rows are tied together by their `event_id`: `record` and the batch write one event
//! on every row of a number, so a row under an older version whose event has no row under the
//! active version is a number still to move. The batch reads the kept vault phone (D-079) through
//! `privacy::api::phone_for_index`, computes the number's digest under every configured version
//! and, holding T701's per-number lock (`still_unlisted`, the same advisory lock `record` takes),
//! writes the missing rows with the event and time of the number's earliest row. A listing whose
//! phone no contact holds is counted in one warning and left, never guessed. No phone or digest is
//! logged, audited or put in an error. Not closed (D-5014): START lifts a listing without that
//! lock, so a START racing the batch can leave the number listed until she texts START again.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_privacy::api::{IndexPurpose, Vault, phone_for_index};
use secrecy::ExposeSecret;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;

use crate::models::put_suppression;
use crate::optout::{listing_key, still_unlisted};

/// The audit action of a retired index key version; the row's subject ID is the version number
/// as a UUID (`Uuid::from_u128`), never a digest.
pub const INDEX_VERSION_RETIRED_ACTION: &str = "messaging.index_version_retired";

/// Why a re-keying batch or a retirement stopped; it names counts and version numbers only, never
/// a number or a digest (PRIV-016, D-4748).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RekeyError {
    /// The version asked to retire is the active one.
    #[error("VAULT_INDEX_KEY version {0} is the active one and is never retired (D-4748)")]
    Active(u16),
    /// `count` numbers listed under `version` lack their listing under the `active` version.
    #[error(
        "{count} opt-outs held under VAULT_INDEX_KEY version {version} are not yet listed under \
         active version {active}; run the re-keying batch to 0 first (D-4748)"
    )]
    Unlisted {
        /// The version asked to retire.
        version: u16,
        /// The active version.
        active: u16,
        /// How many rows under `version` have no active-version row.
        count: u64,
    },
    /// A listing's kept phone could not be read from the vault.
    #[error("a listed number's vault phone could not be read (D-4748)")]
    Vault,
    /// The suppression list could not be read or written.
    #[error("the suppression list could not be re-keyed (D-4748)")]
    Database,
}

fn database<E>(_: E) -> RekeyError {
    RekeyError::Database
}

/// Lists up to `limit` numbers held under an older index key version, with no row under the
/// active one, under every configured version too, and returns how many it moved; run it beside
/// `rotate_batch` until both return 0 (PRIV-024, CON-035, CON-013, D-4748). Each number is its
/// own transaction under its opt-out lock. `db` must be outside any transaction (the vault read
/// audits and commits first, PRIV-003). Listings no contact's phone matches are skipped, so they
/// never stall the run, and their count is logged once.
///
/// # Errors
///
/// [`RekeyError::Vault`] or [`RekeyError::Database`].
pub async fn rekey_suppression_batch(
    db: &mut PgConnection,
    vault: &Vault,
    limit: usize,
) -> Result<usize, RekeyError> {
    let active = i16::try_from(vault.index_version()).map_err(database)?;
    let page = i64::try_from(limit).map_err(database)?;
    let (mut moved, mut left) = (0, 0_usize);
    let mut after = (0_i16, Vec::new());
    while moved < limit {
        let rows = candidates(db, active, &after, page).await?;
        let Some(last) = rows.last() else { break };
        after = last.clone();
        for row in &rows {
            if moved == limit {
                break;
            }
            let held = u16::try_from(row.0)
                .ok()
                .zip(<[u8; 32]>::try_from(&row.1[..]).ok());
            let step = match held {
                Some(held) => rekey_one(db, vault, held).await?,
                None => None,
            };
            match step {
                Some(wrote) => moved += usize::from(wrote),
                None => left += 1,
            }
        }
    }
    if left > 0 {
        tracing::warn!(
            left,
            "opt-out listings whose phone no contact holds stay under an older index key version"
        );
    }
    Ok(moved)
}

/// Up to `page` rows, as version and digest in that order after `after`, held under a version
/// other than `active` whose opt-out event has no row under `active` (PRIV-024, D-5012).
async fn candidates(
    db: &mut PgConnection,
    active: i16,
    after: &(i16, Vec<u8>),
    page: i64,
) -> Result<Vec<(i16, Vec<u8>)>, RekeyError> {
    let rows = sqlx::query!(
        "SELECT s.index_version, s.digest FROM messaging.suppression s \
         WHERE s.index_version <> $1 AND (s.index_version, s.digest) > ($2, $3) \
         AND NOT EXISTS (SELECT 1 FROM messaging.suppression a \
           WHERE a.index_version = $1 AND a.event_id = s.event_id) \
         ORDER BY s.index_version, s.digest LIMIT $4",
        active,
        after.0,
        &after.1,
        page
    )
    .fetch_all(db)
    .await
    .map_err(database)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.index_version, r.digest))
        .collect())
}

/// Re-keys the number whose digest under an older version is `held`: `Some(true)` when rows were
/// written, `Some(false)` when the number was lifted or listed in full meanwhile, `None` when no
/// contact's kept phone has that digest (left as it is, never guessed; D-079, D-4748).
async fn rekey_one(
    db: &mut PgConnection,
    vault: &Vault,
    held: (u16, [u8; 32]),
) -> Result<Option<bool>, RekeyError> {
    let phone = phone_for_index(db, vault, held)
        .await
        .map_err(|_| RekeyError::Vault)?;
    let Some(phone) = phone else { return Ok(None) };
    let digests = vault.blind_index(IndexPurpose::HomeownerPhone, phone.expose_secret());
    let Some(key) = listing_key(vault, &digests).filter(|_| digests.contains(&held)) else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(database)?;
    if still_unlisted(&mut tx, &key).await.map_err(database)? {
        return Ok(Some(false));
    }
    let wrote = write_rows(&mut tx, &digests).await?;
    tx.commit().await.map_err(database)?;
    Ok(Some(wrote))
}

/// Inside the number's lock, gives every configured version's row of the number with `digests`
/// the event and time of its earliest row, adding the missing rows; `false` when the number has
/// no row left (START lifted it) or nothing changed (CON-035, CON-015, D-4748). The rows are
/// locked first, so a START already deleting them is waited for and then seen.
async fn write_rows(
    tx: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<bool, RekeyError> {
    let versions: Vec<i16> = digests.iter().map(|(v, _)| v.cast_signed()).collect();
    let hashes: Vec<Vec<u8>> = digests.iter().map(|(_, d)| d.to_vec()).collect();
    let rows = sqlx::query!(
        "SELECT s.since, s.event_id FROM messaging.suppression s \
         JOIN UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON s.index_version = k.v AND s.digest = k.h \
         ORDER BY s.since, s.event_id FOR UPDATE OF s",
        &versions,
        &hashes
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(database)?;
    let Some(first) = rows.first() else {
        return Ok(false);
    };
    let added = put_suppression(&mut *tx, digests, first.event_id, first.since.to_jiff())
        .await
        .map_err(database)?;
    let aligned = sqlx::query!(
        "UPDATE messaging.suppression s SET event_id = $3, since = $4 \
         FROM UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         WHERE s.index_version = k.v AND s.digest = k.h AND s.event_id <> $3",
        &versions,
        &hashes,
        first.event_id,
        first.since as _
    )
    .execute(&mut *tx)
    .await
    .map_err(database)?
    .rows_affected();
    Ok(added + aligned > 0)
}

/// Removes every suppression row held under index key `version`, in one transaction with one
/// audit row, and returns how many it removed; the operator then drops the version from
/// `VAULT_INDEX_KEY` and `check_suppression_versions` passes (PRIV-024, CON-035, CON-013,
/// D-4748). Refused for the active version, and while any row under `version` has no row of the
/// same opt-out under the active version: nothing is removed then.
///
/// # Errors
///
/// [`RekeyError::Active`], [`RekeyError::Unlisted`] or [`RekeyError::Database`].
pub async fn retire_index_version(
    db: &mut PgConnection,
    vault: &Vault,
    version: u16,
) -> Result<u64, RekeyError> {
    let active = vault.index_version();
    if version == active {
        return Err(RekeyError::Active(version));
    }
    let old = i16::try_from(version).map_err(database)?;
    let mut tx = db.begin().await.map_err(database)?;
    let removed = sqlx::query!(
        "DELETE FROM messaging.suppression s WHERE s.index_version = $1 \
         AND EXISTS (SELECT 1 FROM messaging.suppression a \
           WHERE a.index_version = $2 AND a.event_id = s.event_id)",
        old,
        active.cast_signed()
    )
    .execute(&mut *tx)
    .await
    .map_err(database)?
    .rows_affected();
    let count = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM messaging.suppression WHERE index_version = $1",
        old
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(database)?;
    if count > 0 {
        return Err(RekeyError::Unlisted {
            version,
            active,
            count: count.cast_unsigned(),
        });
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: INDEX_VERSION_RETIRED_ACTION,
        subject_kind: "messaging.suppression",
        subject_id: Uuid::from_u128(u128::from(version)),
        change: Change::Action,
    };
    audit(&mut tx, &entry).await.map_err(database)?;
    tx.commit().await.map_err(database)?;
    Ok(removed)
}
