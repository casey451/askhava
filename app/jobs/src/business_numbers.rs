//! One business's picked-to-finished counts for the portal's "Your numbers" panel (BIZ-025;
//! D-250, D-143; T451).
//!
//! `jobs` stores no check-in reply and `jobs.job` names no business, so [`finished_counts`] takes
//! the `(job, connected_at)` pairs `picks::api::pick_numbers` lists for one business and reads each
//! job's `jobs.status_history`. For each pair it finds the move into Connected that pair belongs
//! to (the last one at or before `connected_at`; none recorded, `connected_at` itself) and then the
//! first move out of Connected after it, so a later reconnect under another business's pick is
//! never credited to this one. That move is classed as `rules::connected_move` maps the replies
//! (JOB-015, D-143): to Done is DONE, to Quotes ready is NO (a day-3 NO under email contact or the
//! 72-hour no-call return), to Closed is the third figure (NO, PROBLEM, a timeout or a request).
//! Each pair counts once, in the Arizona month of that move out, never the month of the pick.

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;
use crate::status::JobStatus;

/// One recorded status move of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    /// The job moved.
    pub job: Uuid,
    /// The job's version after the move (rows are ordered by it).
    pub version: i32,
    /// The status it left.
    pub from: JobStatus,
    /// The status it entered.
    pub to: JobStatus,
    /// When.
    pub at: Timestamp,
}

/// The panel's picked-to-finished figures: DONE, NO and the third (closed) figure (D-250).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Finished {
    /// Moved from Connected to Done.
    pub done: u32,
    /// Moved from Connected back to Quotes ready.
    pub no: u32,
    /// Moved from Connected to Closed.
    pub other: u32,
}

/// D-250: the first move out of Connected (the one status that `takes_check_ins`) that belongs to the connection at `connected_at` of
/// `job`, from `moves` ordered by version.
#[must_use]
pub fn move_out(moves: &[Move], job: Uuid, connected_at: Timestamp) -> Option<Move> {
    let of_job = || moves.iter().filter(move |m| m.job == job);
    let into = of_job()
        .filter(|m| m.to.takes_check_ins() && m.at <= connected_at)
        .map(|m| m.version)
        .max();
    of_job()
        .find(|m| m.from.takes_check_ins() && into.map_or(m.at >= connected_at, |v| m.version > v))
        .copied()
}

/// Which picked-to-finished figure a move out of Connected counts in (D-250, D-143), from
/// [`JobStatus::finished_figure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Figure {
    /// To Done: DONE.
    Done,
    /// Back to Quotes ready: NO.
    No,
    /// To Closed: the third figure.
    Other,
}

/// D-250, D-143: adds the move out of Connected into `out` to `counts` by where it went.
pub fn tally(counts: &mut Finished, out: JobStatus) {
    match out.finished_figure() {
        Some(Figure::Done) => counts.done += 1,
        Some(Figure::No) => counts.no += 1,
        Some(Figure::Other) => counts.other += 1,
        None => {}
    }
}

/// BIZ-025, D-250: the DONE, NO and third figures for one business in `[from, to)` from its
/// `(job, connected_at)` pairs, each counted in the month of its move out of Connected.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn finished_counts(
    db: &mut PgConnection,
    connected: &[(Uuid, Timestamp)],
    (from, to): (Timestamp, Timestamp),
) -> Result<Finished, JobsError> {
    let moves = connected_moves(db, connected).await?;
    let mut counts = Finished::default();
    for &(job, at) in connected {
        if let Some(out) = move_out(&moves, job, at)
            && out.at >= from
            && out.at < to
        {
            tally(&mut counts, out.to);
        }
    }
    Ok(counts)
}

/// Every move into or out of Connected of the pairs' jobs, ordered by job and version.
async fn connected_moves(
    db: &mut PgConnection,
    connected: &[(Uuid, Timestamp)],
) -> Result<Vec<Move>, JobsError> {
    let jobs: Vec<Uuid> = connected.iter().map(|(job, _)| *job).collect();
    let code = JobStatus::Connected.code();
    let rows = sqlx::query!(
        "SELECT job_id, version, from_status, to_status, at FROM jobs.status_history
          WHERE job_id = ANY($1) AND (from_status = $2 OR to_status = $2)
          ORDER BY job_id, version",
        &jobs,
        code,
    )
    .fetch_all(db)
    .await
    .map_err(|_| JobsError::Database)?;
    let moves = rows.into_iter().filter_map(|r| {
        Some(Move {
            job: r.job_id,
            version: r.version,
            from: JobStatus::from_code(&r.from_status)?,
            to: JobStatus::from_code(&r.to_status)?,
            at: r.at.to_jiff(),
        })
    });
    Ok(moves.collect())
}
