//! The daily consent sweep, `intake.consent_sweep` (CON-022, CON-035, CON-036, PRIV-024; D-079,
//! D-2208, D-3155, D-3753, D-3844, D-3967, D-4280, D-4387, D-4418).
//!
//! CON-022 keeps a number's consent records, its STOP and START events (`messaging.optout_event`)
//! and its message logs (a HELP is its inbox row and its reply's outbox row) 5 years after the last
//! message to that number, and no longer. Once a day this sweep lists the numbers whose last message
//! is strictly older than 1,827 days on the database clock (`messaging::api::due_numbers`) and the
//! contacts waiting in `intake.consent_phone_wait`, and for each, in one READ COMMITTED transaction
//! ([`sweep_number`]): re-checks the suppression list under the per-number opt-out lock
//! (`messaging::api::still_unlisted`, its very first statement), reads whether the number is still
//! past the 5 years (`intake.consent_phone_due`, migration 0196), deletes its old opt-out events and
//! consent records through the two `SECURITY DEFINER` functions and then its message log, writes one
//! `ops` audit row of IDs only per deleted row, and, as its last write, clears the locked vault phone
//! of a number that answered "still unlisted" (`privacy::api::clear_consent_phone`). A listed number
//! keeps its phone, its suppression row and the opt-out event that row points to, since PRIV-024's
//! key rotation rebuilds the suppression digest from the phone (D-079).
//!
//! The listing key is built before the transaction from the contact's homeowner-phone digests in
//! `privacy.contact_index` ([`due_number`]; no plaintext read, D-4280). A contact missing a digest
//! under a configured `VAULT_INDEX_KEY` version (a rotation not yet run for it) has no key: its rows
//! still go, its phone is kept, and it waits in `intake.consent_phone_wait` until a later run, after
//! the rotation, clears it (D-4387). An opt-out by any channel that commits before the re-check keeps
//! the phone; one that waits on the lock commits after the clear and lists the number by digest with
//! no phone behind it, still refused every text (accepted in D-4418). The phone clear takes the
//! contact row with `NOWAIT`: when another transaction holds it, the whole number's transaction rolls
//! back and the number is left for the next day ([`SweepStep::Busy`]). No outside call is made inside
//! the transaction.

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use askhava_messaging::api::{
    ListingKey, delete_message_log, due_numbers, listing_key, purge_optout_events, still_unlisted,
};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_privacy::api::{PhoneClear, Vault, clear_consent_phone, phone_digests};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::IntakeError;

/// The daily consent sweep's task kind (CON-022, D-3844).
pub const CONSENT_SWEEP_KIND: &str = "intake.consent_sweep";

/// The audit action of each row the sweep deletes, its subject the row's table and ID (CON-022).
pub const CONSENT_PURGED: &str = "consent_purged";

/// How many numbers of each list one run takes; the rest go the next day.
const BATCH: i64 = 500;

const DAY: SignedDuration = SignedDuration::from_hours(24);

const FAILED: HandlerError = HandlerError("intake consent sweep database call failed");

/// A due number as the sweep found it before its transaction: the contact, its homeowner-phone
/// digests as `privacy.contact_index` holds them, and the listing key built from them, `None` when a
/// configured index key version has no digest for it (CON-022, D-4280, D-4418). No `Debug`: it holds
/// blind-index digests (PRIV-002).
pub struct DueNumber {
    contact: Uuid,
    digests: Vec<(u16, [u8; 32])>,
    key: Option<ListingKey>,
}

/// What [`sweep_number`] did with one number (CON-022, D-4418).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepStep {
    /// Its rows past the 5 years are gone and so is its vault phone.
    Cleared,
    /// Its rows past the 5 years are gone; its phone is kept (listed, no key yet, no phone, or not due).
    Kept,
    /// Another transaction held its contact row: nothing changed, the next run tries again.
    Busy,
}

/// Reads contact `contact`'s homeowner-phone digests and builds its listing key under `vault`'s
/// configured index key versions (`None` without a vault or with a version missing), outside any
/// transaction and with no plaintext read (CON-022, PRIV-024, D-4280, D-4418).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn due_number(
    db: &mut PgConnection,
    vault: Option<&Vault>,
    contact: Uuid,
) -> Result<DueNumber, IntakeError> {
    let digests = phone_digests(db, contact)
        .await
        .map_err(|_| IntakeError::Database)?;
    let key = vault.and_then(|v| listing_key(v, &digests));
    Ok(DueNumber {
        contact,
        digests,
        key,
    })
}

/// The sweep's step for one number, in one READ COMMITTED transaction whose first statement is the
/// suppression re-check (CON-022, CON-035, CON-036, D-079, D-3753, D-4387, D-4418): the old opt-out
/// events and consent records, then the message log, each deleted row audited by ID, then the vault
/// phone of a number still unlisted and past the 5 years, cleared last. A held contact row rolls
/// everything back ([`SweepStep::Busy`]).
///
/// # Errors
///
/// [`IntakeError::Database`] or [`IntakeError::Audit`]; nothing is kept then.
pub async fn sweep_number(
    db: &mut PgConnection,
    due: &DueNumber,
) -> Result<SweepStep, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let unlisted = match &due.key {
        Some(key) => still_unlisted(&mut tx, key)
            .await
            .map_err(|_| IntakeError::Database)?,
        None => false,
    };
    let phone_due = take_phone_due(&mut tx, due.contact).await?;
    purge_rows(&mut tx, due).await?;
    let mut step = SweepStep::Kept;
    if phone_due && !due.digests.is_empty() {
        if due.key.is_none() {
            wait_for_key(&mut tx, due.contact).await?;
        } else if unlisted {
            match clear_consent_phone(&mut tx, due.contact, &due.digests).await {
                Ok(PhoneClear::Busy) => return Ok(SweepStep::Busy),
                Ok(PhoneClear::Cleared) => step = SweepStep::Cleared,
                Ok(PhoneClear::Kept) => {}
                Err(_) => return Err(IntakeError::Database),
            }
        }
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(step)
}

/// Whether the number is past CON-022's 5 years (`intake.consent_phone_due`, read before any delete),
/// and drops its wait row, which [`wait_for_key`] writes again if it still has no key (D-4387).
async fn take_phone_due(tx: &mut PgConnection, contact: Uuid) -> Result<bool, IntakeError> {
    let due = sqlx::query_scalar!(r#"SELECT intake.consent_phone_due($1) AS "due!""#, contact)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| IntakeError::Database)?;
    sqlx::query!(
        "DELETE FROM intake.consent_phone_wait WHERE contact_id = $1",
        contact
    )
    .execute(tx)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(due)
}

/// Lists the contact in `intake.consent_phone_wait`, so a later run clears its kept phone once a
/// rotation gave it a digest under every configured version (D-4387, D-4418).
async fn wait_for_key(tx: &mut PgConnection, contact: Uuid) -> Result<(), IntakeError> {
    sqlx::query!(
        "INSERT INTO intake.consent_phone_wait (contact_id, since) VALUES ($1, now())",
        contact
    )
    .execute(tx)
    .await
    .map(drop)
    .map_err(|_| IntakeError::Database)
}

/// Deletes the number's CON-022 rows past the 5 years, both purge functions before the message log
/// (their guard reads it), and audits each deleted row by table and ID only (CON-022, CON-036).
async fn purge_rows(tx: &mut PgConnection, due: &DueNumber) -> Result<(), IntakeError> {
    let events = purge_optout_events(tx, due.contact, &due.digests)
        .await
        .map_err(|_| IntakeError::Database)?;
    let consent = sqlx::query_scalar!(
        r#"SELECT intake.purge_expired_consent($1) AS "id!""#,
        due.contact
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| IntakeError::Database)?;
    let log = delete_message_log(tx, due.contact)
        .await
        .map_err(|_| IntakeError::Database)?;
    let gone = [
        ("messaging.optout_event", events),
        ("intake.consent_record", consent),
        ("messaging.outbox", log.outbox),
        ("messaging.inbox", log.inbox),
    ];
    for (kind, ids) in gone {
        for id in ids {
            let entry = AuditEntry {
                actor: Actor::System,
                action: CONSENT_PURGED,
                subject_kind: kind,
                subject_id: id,
                change: Change::Action,
            };
            audit(tx, &entry).await.map_err(|_| IntakeError::Audit)?;
        }
    }
    Ok(())
}

/// One run of the sweep: every due number and every waiting contact, up to [`BATCH`] of each, one
/// transaction per number ([`sweep_number`]); returns how many phones it cleared (CON-022, D-4418).
/// A number left [`SweepStep::Busy`] keeps its rows and is found again the next day.
///
/// # Errors
///
/// [`IntakeError::Database`] or [`IntakeError::Audit`]; numbers done before it stay done.
pub async fn sweep_consent(
    db: &mut PgConnection,
    vault: Option<&Vault>,
) -> Result<u64, IntakeError> {
    let mut contacts = due_numbers(db, BATCH)
        .await
        .map_err(|_| IntakeError::Database)?;
    contacts.extend(
        sqlx::query_scalar!(
            "SELECT contact_id FROM intake.consent_phone_wait ORDER BY contact_id LIMIT $1",
            BATCH
        )
        .fetch_all(&mut *db)
        .await
        .map_err(|_| IntakeError::Database)?,
    );
    contacts.sort_unstable();
    contacts.dedup();
    let mut cleared = 0;
    for contact in contacts {
        let due = due_number(db, vault, contact).await?;
        if sweep_number(db, &due).await? == SweepStep::Cleared {
            cleared += 1;
        }
    }
    Ok(cleared)
}

/// Queues the next day's sweep, one per UTC day (CON-022, JOB-006).
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_consent_sweep(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let mut task = NewTask::new(CONSENT_SWEEP_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!(
        "{CONSENT_SWEEP_KIND}:{}",
        run_at.as_second() / 86_400
    ));
    enqueue(db, &task).await.map(drop)
}

/// The [`CONSENT_SWEEP_KIND`] handler at `now` (the worker's clock): queues the next day's run
/// first, so a run that fails for good still leaves it, then [`sweep_consent`] (CON-022). Without
/// the vault keys (dev and CI) no listing key is built, so rows go and every phone is kept.
///
/// # Errors
///
/// A fixed [`HandlerError`] when a database call fails.
pub async fn run_consent_sweep_at(
    pool: PgPool,
    vault: Option<Arc<Vault>>,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_consent_sweep(&mut db, now)
        .await
        .map_err(|_| FAILED)?;
    sweep_consent(&mut db, vault.as_deref())
        .await
        .map(drop)
        .map_err(|_| FAILED)
}
