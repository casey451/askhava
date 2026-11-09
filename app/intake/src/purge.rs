//! `intake`'s part of the retention purge: on `privacy`'s [`PURGE_KIND`] event it clears the job's
//! homeowner contact (contact stage, 180 days after close) and deletes the job's intake answers
//! (text stage, 3 years after close) (PRIV-007, PRIV-008, D-013, D-491).
//!
//! `intake` knows which homeowner a job belongs to (`intake.conversation.job_id`), so it is the one
//! that calls `privacy::api::purge_contact`, which keeps the phone with its consent records
//! (D-079). The contact belongs to the homeowner, not the job, so it stays while another request
//! of hers is live (open, or closed less than 180 days ago; an open request with no job counts as
//! ended at its last answer) or while any of her jobs has an unreleased `privacy.retention_hold`;
//! the job or request that blocked it clears it at its own purge. A request that never got a job
//! (refused or abandoned) has no `jobs` purge task, so the daily sweep ([`sweep_requests`]) queues
//! [`REQUEST_PURGE_KIND`] 180 days after it ended (D-1799). Consent records and terms acceptances
//! are never deleted here (CON-022, PRIV-010, D-492).
//!
//! [`PURGE_KIND`]: askhava_privacy::api::PURGE_KIND
//!
//! Decide and clear are one transaction under the retention lock order (D-1839, second audit B03,
//! `docs/architecture.md` section 8): (1) the retention advisory key, exclusive
//! (`privacy::api::lock_retention_for_purge`; every hold placement and release takes it shared);
//! (2) the homeowner row (`models::lock_homeowner`, which every request-opening writer takes);
//! (3) the vault contact row (`purge_contact`'s `UPDATE`, a savepoint here). Eligibility is read
//! only after (1) and (2) are held, so a request or hold that committed first is seen and one that
//! comes later waits for the clear. A hold still on its way counts as a hold: an unfinished
//! (pending, running or failed) contact-stage `jobs.purge_due` or `privacy.purge_recheck` task of
//! another of her jobs (the composition places `payments`' dispute holds on those tasks) or any
//! unfinished `businesses.complaint_hold` event (its key names the complaint, not the job), read
//! through `core::api::unfinished`. No outside call is made inside the transaction.

use askhava_businesses::api::COMPLAINT_HOLD_KIND;
use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue, unfinished};
use askhava_jobs::api::PURGE_DUE_KIND;
use askhava_privacy::api::{
    PurgeStage, RECHECK_KIND, any_held, lock_retention_for_purge, purge_contact, purge_target,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection, PgPool};

use crate::models::lock_homeowner;

/// The contact purge of a request that never got a job: payload `{"conversation"}` (PRIV-007).
pub const REQUEST_PURGE_KIND: &str = "intake.request_purge";
const DB_FAILED: HandlerError = HandlerError("intake purge database call failed");
const CONTACT_DAYS: SignedDuration = SignedDuration::from_hours(180 * 24);

/// A homeowner and her vault contact.
#[derive(Debug, Clone, Copy)]
struct Owner {
    homeowner: Uuid,
    contact: Uuid,
}

/// What the contact purge starts from: a job's contact stage or a jobless request.
#[derive(Debug, Clone, Copy)]
enum Target {
    Job(Uuid),
    Request(Uuid),
}

async fn owner_of(db: &mut PgConnection, target: Target) -> Result<Option<Owner>, HandlerError> {
    let (job, request) = match target {
        Target::Job(job) => (Some(job), None),
        Target::Request(request) => (None, Some(request)),
    };
    let row = sqlx::query!(
        "SELECT h.id, h.contact_id FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE (c.job_id = $1 OR (c.id = $2 AND c.job_id IS NULL)) LIMIT 1",
        job,
        request,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| DB_FAILED)?;
    Ok(row.map(|r| Owner {
        homeowner: r.id,
        contact: r.contact_id,
    }))
}

/// Her requests as one statement reads them under the locks.
struct Standing {
    /// PRIV-007: a request other than `job`'s is live at the cutoff.
    live: bool,
    /// Every job of hers.
    jobs: Vec<Uuid>,
}

/// PRIV-007: in one statement, whether a request of `homeowner` other than `job`'s is live at
/// `cutoff` (now less 180 days: closed after it, open with a job, or open with no job and answered
/// after it), and all of her jobs.
async fn standing(
    db: &mut PgConnection,
    homeowner: Uuid,
    job: Option<Uuid>,
    cutoff: Timestamp,
) -> Result<Standing, HandlerError> {
    let row = sqlx::query!(
        r#"SELECT EXISTS (SELECT 1 FROM intake.conversation o WHERE o.homeowner_id = $1
           AND ($2::uuid IS NULL OR o.job_id IS DISTINCT FROM $2)
           AND (o.closed_at > $3 OR (o.closed_at IS NULL AND (o.job_id IS NOT NULL
             OR GREATEST(o.opened_at, (SELECT max(a.answered_at) FROM intake.intake_answer a
               WHERE a.conversation_id = o.id)) > $3)))) AS "live!",
           ARRAY(SELECT DISTINCT c.job_id FROM intake.conversation c
             WHERE c.homeowner_id = $1 AND c.job_id IS NOT NULL) AS "jobs!""#,
        homeowner,
        job,
        cutoff.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB_FAILED)?;
    Ok(Standing {
        live: row.live,
        jobs: row.jobs,
    })
}

/// PRIV-007, D-1839: true while a hold-producing task is unfinished: any complaint hold event, or
/// a contact-stage purge task (where `payments`' dispute holds are placed) of any of `jobs` but
/// `own` (the task that handed this purge on has placed its own).
async fn hold_on_its_way(
    db: &mut PgConnection,
    jobs: &[Uuid],
    own: Option<Uuid>,
) -> Result<bool, HandlerError> {
    let complaint = format!("{COMPLAINT_HOLD_KIND}:");
    if unfinished(db, COMPLAINT_HOLD_KIND, &complaint)
        .await
        .map_err(|_| DB_FAILED)?
    {
        return Ok(true);
    }
    for job in jobs.iter().filter(|job| Some(**job) != own) {
        let due = format!("{PURGE_DUE_KIND}:{job}:{}", PurgeStage::Contact.code());
        let recheck = format!("{RECHECK_KIND}:{job}:");
        for (kind, prefix) in [(PURGE_DUE_KIND, due), (RECHECK_KIND, recheck)] {
            if unfinished(db, kind, &prefix).await.map_err(|_| DB_FAILED)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// PRIV-007, D-1839: with (1) and (2) held, whether `owner`'s contact may go: no other request of
/// hers live at `now` less 180 days, no job of hers held, no hold-producing task unfinished.
async fn free_under_locks(
    db: &mut PgConnection,
    owner: Owner,
    job: Option<Uuid>,
    now: Timestamp,
) -> Result<bool, HandlerError> {
    let seen = standing(db, owner.homeowner, job, now - CONTACT_DAYS).await?;
    if seen.live || any_held(db, &seen.jobs).await.map_err(|_| DB_FAILED)? {
        return Ok(false);
    }
    Ok(!hold_on_its_way(db, &seen.jobs, job).await?)
}

/// PRIV-007, D-1839: the contact purge of `target` at `now`, decided and cleared in one
/// transaction under the retention lock order; true when the contact was cleared (or nothing was
/// left to clear). Safe to run twice and to run beside request writers and hold events.
async fn purge_owner_contact(
    db: &mut PgConnection,
    target: Target,
    now: Timestamp,
) -> Result<bool, HandlerError> {
    let mut tx = db.begin().await.map_err(|_| DB_FAILED)?;
    lock_retention_for_purge(&mut tx)
        .await
        .map_err(|_| DB_FAILED)?;
    let Some(owner) = owner_of(&mut tx, target).await? else {
        return Ok(false);
    };
    lock_homeowner(&mut tx, owner.homeowner)
        .await
        .map_err(|_| DB_FAILED)?;
    let job = match target {
        Target::Job(job) => Some(job),
        Target::Request(_) => None,
    };
    if !free_under_locks(&mut tx, owner, job, now).await? {
        return Ok(false);
    }
    purge_contact(&mut tx, owner.contact)
        .await
        .map_err(|_| HandlerError("intake contact purge failed"))?;
    tx.commit().await.map_err(|_| DB_FAILED)?;
    Ok(true)
}

/// PRIV-008: deletes the intake answers of `job`'s requests; returns how many went.
///
/// # Errors
/// [`DB_FAILED`] on a database failure.
pub async fn purge_answers(db: &mut PgConnection, job: Uuid) -> Result<u64, HandlerError> {
    let gone = sqlx::query!(
        "DELETE FROM intake.intake_answer WHERE conversation_id IN \
         (SELECT id FROM intake.conversation WHERE job_id = $1)",
        job,
    )
    .execute(db)
    .await
    .map_err(|_| DB_FAILED)?;
    Ok(gone.rows_affected())
}

/// The [`PURGE_KIND`](askhava_privacy::api::PURGE_KIND) handler; safe to run twice.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database or vault failure.
pub async fn run_purge(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    match stage {
        PurgeStage::Photos => Ok(()),
        PurgeStage::Text => purge_answers(&mut conn, job).await.map(|_| ()),
        PurgeStage::Contact => purge_owner_contact(&mut conn, Target::Job(job), Timestamp::now())
            .await
            .map(|_| ()),
    }
}

/// PRIV-007, D-1799: queues [`REQUEST_PURGE_KIND`] for every request with no job that has ended
/// (closed, or open with its last answer at or before `now` less 180 days), due 180 days after it
/// ended; the dedupe key is the request and its end, so a request that ends again is queued again.
/// Returns how many tasks were new.
///
/// # Errors
/// [`TaskError::Db`].
pub async fn sweep_requests(db: &mut PgConnection, now: Timestamp) -> Result<usize, TaskError> {
    let rows = sqlx::query!(
        r#"SELECT id, ended AS "ended!: jiff_sqlx::Timestamp" FROM (SELECT c.id, c.closed_at,
             COALESCE(c.closed_at, GREATEST(c.opened_at, (SELECT max(a.answered_at)
               FROM intake.intake_answer a WHERE a.conversation_id = c.id))) AS ended
           FROM intake.conversation c WHERE c.job_id IS NULL) r
           WHERE closed_at IS NOT NULL OR ended <= $1"#,
        (now - CONTACT_DAYS).to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| TaskError::Db)?;
    let mut queued = 0;
    for row in rows {
        let ended = row.ended.to_jiff();
        let payload = json!({ "conversation": row.id });
        let mut task = NewTask::new(REQUEST_PURGE_KIND, payload, ended + CONTACT_DAYS);
        let key = format!("{REQUEST_PURGE_KIND}:{}:{}", row.id, ended.as_second());
        task.dedupe_key = Some(key);
        queued += usize::from(enqueue(db, &task).await?.is_some());
    }
    Ok(queued)
}

/// The composition's step on `jobs`' daily sweep: [`sweep_requests`] at the wall clock.
///
/// # Errors
/// A fixed [`HandlerError`] when the database call fails.
pub async fn run_request_sweep(pool: PgPool, _task: Claimed) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    sweep_requests(&mut conn, Timestamp::now())
        .await
        .map(|_| ())
        .map_err(|_| DB_FAILED)
}

/// The [`REQUEST_PURGE_KIND`] handler (PRIV-007): clears the contact of a request that still has
/// no job once no other request of the homeowner is live and none of her jobs is held; safe to run
/// twice.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database or vault failure.
pub async fn run_request_purge(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let conversation = task.payload["conversation"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(HandlerError("request purge payload is not a request"))?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let target = Target::Request(conversation);
    purge_owner_contact(&mut conn, target, Timestamp::now())
        .await
        .map(|_| ())
}
