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

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use askhava_privacy::api::{PurgeStage, is_held, purge_contact, purge_target};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

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

async fn job_owner(db: &mut PgConnection, job: Uuid) -> Result<Option<Owner>, HandlerError> {
    let row = sqlx::query!(
        "SELECT h.id, h.contact_id FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id WHERE c.job_id = $1 LIMIT 1",
        job,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| DB_FAILED)?;
    Ok(row.map(|r| Owner {
        homeowner: r.id,
        contact: r.contact_id,
    }))
}

/// PRIV-007: true while a request of `homeowner` other than `job`'s is live at `cutoff` (now less
/// 180 days): closed after it, open with a job, or open with no job and answered after it.
async fn other_request_live(
    db: &mut PgConnection,
    homeowner: Uuid,
    job: Option<Uuid>,
    cutoff: Timestamp,
) -> Result<bool, HandlerError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM intake.conversation o WHERE o.homeowner_id = $1
           AND ($2::uuid IS NULL OR o.job_id IS DISTINCT FROM $2)
           AND (o.closed_at > $3 OR (o.closed_at IS NULL AND (o.job_id IS NOT NULL
             OR GREATEST(o.opened_at, (SELECT max(a.answered_at) FROM intake.intake_answer a
               WHERE a.conversation_id = o.id)) > $3)))) AS "live!""#,
        homeowner,
        job,
        cutoff.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB_FAILED)
}

/// PRIV-007: true while any job of `homeowner` has an unreleased hold.
async fn any_job_held(db: &mut PgConnection, homeowner: Uuid) -> Result<bool, HandlerError> {
    let jobs = sqlx::query_scalar!(
        r#"SELECT DISTINCT job_id AS "job!" FROM intake.conversation
           WHERE homeowner_id = $1 AND job_id IS NOT NULL"#,
        homeowner,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| DB_FAILED)?;
    for job in jobs {
        if is_held(db, job).await.map_err(|_| DB_FAILED)? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn free_contact(
    db: &mut PgConnection,
    owner: Owner,
    job: Option<Uuid>,
    now: Timestamp,
) -> Result<Option<Uuid>, HandlerError> {
    if other_request_live(db, owner.homeowner, job, now - CONTACT_DAYS).await?
        || any_job_held(db, owner.homeowner).await?
    {
        return Ok(None);
    }
    Ok(Some(owner.contact))
}

/// PRIV-007: the vault contact of `job`'s homeowner when no other request of hers is live at
/// `now` less 180 days and none of her jobs is held.
///
/// # Errors
/// [`DB_FAILED`] on a database failure.
pub async fn contact_to_purge(
    db: &mut PgConnection,
    job: Uuid,
    now: Timestamp,
) -> Result<Option<Uuid>, HandlerError> {
    match job_owner(db, job).await? {
        Some(owner) => free_contact(db, owner, Some(job), now).await,
        None => Ok(None),
    }
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

async fn clear(db: &mut PgConnection, contact: Option<Uuid>) -> Result<(), HandlerError> {
    match contact {
        Some(contact) => purge_contact(db, contact)
            .await
            .map(|_| ())
            .map_err(|_| HandlerError("intake contact purge failed")),
        None => Ok(()),
    }
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
        PurgeStage::Contact => {
            let contact = contact_to_purge(&mut conn, job, Timestamp::now()).await?;
            clear(&mut conn, contact).await
        }
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
    let row = sqlx::query!(
        "SELECT h.id, h.contact_id FROM intake.conversation c JOIN intake.homeowner h \
         ON h.id = c.homeowner_id WHERE c.id = $1 AND c.job_id IS NULL",
        conversation,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| DB_FAILED)?;
    let Some(r) = row else { return Ok(()) };
    let owner = Owner {
        homeowner: r.id,
        contact: r.contact_id,
    };
    let contact = free_contact(&mut conn, owner, None, Timestamp::now()).await?;
    clear(&mut conn, contact).await
}
