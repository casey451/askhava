//! The owner flag inputs from `jobs` (OPS-031; D-3880, D-3881, D-4037, D-4053, T523).
//!
//! [`record_check_in_answer`] writes one `ops` audit row per applied check-in answer (action
//! [`CHECK_IN_ANSWER`], subject the job, field code `<day3|day14>_<answer>`, a code name only,
//! PRIV-028) in the caller's transaction. [`connected_answers`] reads, for given job IDs and a
//! month, the jobs that became Connected in it and those of them whose connection drew a day-3 NO
//! or OTHER: a row counts for a connection only when written at or after that move into Connected
//! and before the job's next one, in any month (D-4053). Counts and IDs only; the 30% and the two
//! months are T524's settings.

use askhava_ops::api::{Actor, AuditEntry, Change, audit, audit_subjects};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::followups::Answer;
use crate::models::JobsError;
use crate::rules::CheckIn;

/// The audit action of one recorded check-in answer (OPS-031).
pub const CHECK_IN_ANSWER: &str = "jobs.check_in_answer";
/// The field codes OPS-031 counts.
const NO_OR_OTHER: [&str; 2] = ["day3_no", "day3_other"];

/// One month's connected jobs and those that answered day-3 NO or OTHER (OPS-031).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectedAnswers {
    /// The jobs that moved into Connected in the month, each once, in ID order.
    pub connected: Vec<Uuid>,
    /// Of those, the jobs with a `day3_no` or `day3_other` row for such a connection.
    pub no_or_other: Vec<Uuid>,
}

/// The field code of `answer` to check-in `check` (OPS-031).
fn field_code(check: CheckIn, answer: Answer) -> &'static str {
    const CODES: [[&str; 6]; 2] = [
        [
            "day3_done",
            "day3_no",
            "day3_problem",
            "day3_later",
            "day3_ongoing",
            "day3_other",
        ],
        [
            "day14_done",
            "day14_no",
            "day14_problem",
            "day14_later",
            "day14_ongoing",
            "day14_other",
        ],
    ];
    let column = match answer {
        Answer::Done => 0,
        Answer::No => 1,
        Answer::Problem => 2,
        Answer::Later => 3,
        Answer::Ongoing => 4,
        Answer::Other => 5,
    };
    CODES[usize::from(check == CheckIn::Day14)][column]
}

/// OPS-031, D-4053: one [`CHECK_IN_ANSWER`] row for `answer` to `check` on job `job`, in the
/// caller's transaction, so it commits or rolls back with the answer it records.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn record_check_in_answer(
    db: &mut PgConnection,
    job: Uuid,
    (check, answer): (CheckIn, Answer),
    actor: Actor,
) -> Result<(), JobsError> {
    let entry = AuditEntry {
        actor,
        action: CHECK_IN_ANSWER,
        subject_kind: "jobs.job",
        subject_id: job,
        change: Change::Field(field_code(check, answer)),
    };
    audit(db, &entry)
        .await
        .map(drop)
        .map_err(|_| JobsError::Database)
}

/// OPS-031, D-4053: of `jobs`, those that moved into Connected at or after `from` and before
/// `to`, and of those the ones whose connection drew a `day3_no` or `day3_other` row (read through
/// `ops::api::audit_subjects`) before the job's next move into Connected.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn connected_answers(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
) -> Result<ConnectedAnswers, JobsError> {
    let spans = sqlx::query!(
        r#"SELECT job_id AS "job!", at AS "at!: jiff_sqlx::Timestamp",
                  next AS "next?: jiff_sqlx::Timestamp"
           FROM (SELECT job_id, at, lead(at) OVER (PARTITION BY job_id ORDER BY at, id) AS next
                 FROM jobs.status_history WHERE to_status = 'connected' AND job_id = ANY($1)) c
           WHERE at >= $2 AND at < $3 ORDER BY job_id, at"#,
        jobs,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| JobsError::Database)?;
    let rows = audit_subjects(db, CHECK_IN_ANSWER, from, Timestamp::MAX)
        .await
        .map_err(|_| JobsError::Database)?;
    let mut out = ConnectedAnswers::default();
    for s in spans {
        let (at, next) = (s.at.to_jiff(), s.next.map(|n| n.to_jiff()));
        let answered = rows.iter().any(|r| {
            r.subject_id == s.job
                && r.field.as_deref().is_some_and(|f| NO_OR_OTHER.contains(&f))
                && r.at >= at
                && next.is_none_or(|n| r.at < n)
        });
        if out.connected.last() != Some(&s.job) {
            out.connected.push(s.job);
        }
        if answered && out.no_or_other.last() != Some(&s.job) {
            out.no_or_other.push(s.job);
        }
    }
    Ok(out)
}
