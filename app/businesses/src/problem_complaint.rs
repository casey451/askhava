//! Casey counts a check-in PROBLEM report as a complaint (JOB-016, BIZ-011, BIZ-012, BIZ-029,
//! OPS-005, SEC-006; D-4423).
//!
//! A PROBLEM reply is not a complaint by itself. [`count_problem`] records one only when Casey
//! presses "Count as a complaint" and picks one of [`PROBLEM_KINDS`]: in one transaction it locks
//! the business row, answers [`Counted::Already`] when the business already has a complaint for
//! the job, else runs `complaints::record_complaint` as built (its transaction becomes a savepoint;
//! the words go only to its safety check and are never stored) and writes one
//! [`PROBLEM_COUNTED_ACTION`] audit row by the owner naming the complaint. [`counted_jobs`] tells
//! the owner portal which PROBLEM jobs are counted already.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::complaints::{ComplaintError, NewComplaint, Recorded, record_complaint};

/// JOB-016, D-4423: the kinds Casey may pick for a counted PROBLEM report.
pub const PROBLEM_KINDS: [&str; 5] = ["no_show", "conduct", "work_or_price", "safety", "other"];
/// SEC-006, D-4423: the audit action of a counted PROBLEM, by the owner, naming the complaint.
pub const PROBLEM_COUNTED_ACTION: &str = "businesses.problem_counted";

const COMPLAINT: &str = "businesses.complaint";

/// What [`count_problem`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counted {
    /// A complaint was recorded (BIZ-011, BIZ-012, BIZ-029).
    Recorded(Recorded),
    /// The business already has a complaint for this job; nothing was written (JOB-016).
    Already,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, ComplaintError> {
    r.map_err(|_| ComplaintError::Database)
}

/// JOB-016, BIZ-011, BIZ-012, BIZ-029, OPS-005, SEC-006, D-4423: counts a PROBLEM report about
/// `business` on `job` as one complaint of `kind`, Casey (`owner`) acting at `now`. `words` (the
/// kept, scrubbed PROBLEM words) go only to the safety check, so a safety word raises
/// `businesses.complaint_safety`. The business row is locked before the check, so two counts at
/// once record one complaint; a job the business already has a complaint for answers
/// [`Counted::Already`] and writes nothing.
///
/// # Errors
/// [`ComplaintError::BadKind`] for a kind outside [`PROBLEM_KINDS`], before anything is written;
/// `Database` or `Audit`.
pub async fn count_problem(
    conn: &mut PgConnection,
    (business, job): (Uuid, Uuid),
    (kind, words): (&str, &str),
    (owner, now): (Uuid, Timestamp),
) -> Result<Counted, ComplaintError> {
    if !PROBLEM_KINDS.contains(&kind) {
        return Err(ComplaintError::BadKind);
    }
    let mut tx = db(conn.begin().await)?;
    db(sqlx::query_scalar!(
        "SELECT id FROM businesses.business WHERE id = $1 FOR UPDATE",
        business
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let already = db(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.complaint WHERE business_id = $1 AND job_id = $2) \
         AS \"already!\"",
        business,
        job
    )
    .fetch_one(&mut *tx)
    .await)?;
    if already {
        return Ok(Counted::Already);
    }
    let new = NewComplaint {
        business,
        job,
        kind,
        words,
    };
    let recorded = record_complaint(&mut tx, &new, now).await?;
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: PROBLEM_COUNTED_ACTION,
        subject_kind: COMPLAINT,
        subject_id: recorded.id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| ComplaintError::Audit)?;
    db(tx.commit().await)?;
    Ok(Counted::Recorded(recorded))
}

/// JOB-016, D-4423: the jobs among `jobs` that have a complaint, each once. Reads only.
///
/// # Errors
/// [`ComplaintError::Database`].
pub async fn counted_jobs(
    conn: &mut PgConnection,
    jobs: &[Uuid],
) -> Result<Vec<Uuid>, ComplaintError> {
    db(sqlx::query_scalar!(
        "SELECT DISTINCT job_id FROM businesses.complaint WHERE job_id = ANY($1)",
        jobs
    )
    .fetch_all(conn)
    .await)
}
