//! The pilot scorecard's request inputs from `intake` (OPS-039, INT-047, INT-041; D-457, D-3753,
//! D-3883; T478).
//!
//! Read-only, codes and counts only, each over a half-open window (`from` counted, `to` not).
//! [`requests_by_source`] counts the requests opened per INT-047 source: a `g` link code, "web"
//! for a web chat with no code, "text" for a request started by text. [`intake_completion`] counts
//! the requests opened, those that reached the card step (a `card.<job>` row of the approval step,
//! or an emergency choice, which skips it) and, for the rest, the step of each one's latest stored
//! answer (its question code up to the first dot, so a trade's questions count as one step;
//! "none" when nothing was answered yet), so a request still in
//! progress counts where it stands now. [`cards_approved`] counts, for the job IDs passed (one
//! trade's or all, from `jobs::api::jobs_by_trade`; `intake` stores no trade, D-3883), the cards
//! approved in the window and those approved with no EDIT first (CARD-007's `intake_answer` rows:
//! `sent.<digest>` and `edit`). No source code reaches a card or routing (D-457).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::IntakeError;

/// The requests opened from one INT-047 source in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCount {
    /// The source: a `g` code, "web" or "text".
    pub source: String,
    /// The requests opened from it.
    pub requests: i64,
}

/// The unfinished requests whose latest answer is one step (OPS-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopCount {
    /// The step's question code, "none" when nothing was answered.
    pub step: String,
    /// The requests stopped there.
    pub requests: i64,
}

/// Intake completion in the window (OPS-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeCompletion {
    /// The requests opened.
    pub started: i64,
    /// Those that reached the card step.
    pub reached_card: i64,
    /// The rest by the step where each stopped, most first, then by step code.
    pub stopped_at: Vec<StopCount>,
}

/// Card approvals for some jobs in the window (OPS-039, INT-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardApprovals {
    /// The cards approved (by SEND or by the auto-send).
    pub approved: i64,
    /// Those approved with no EDIT before.
    pub without_edit: i64,
}

/// INT-047, OPS-039: the requests opened at or after `from` and before `to`, per source, sources
/// in code order; a source with none is absent. Total only: no per-trade split, since a request
/// gets a trade only once routed; a later split would be a job-ID variant over
/// `intake.conversation.job_id` (D-4264).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn requests_by_source(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<SourceCount>, IntakeError> {
    sqlx::query_as!(
        SourceCount,
        r#"SELECT request_source AS source, count(*) AS "requests!" FROM intake.conversation
           WHERE opened_at >= $1 AND opened_at < $2
           GROUP BY request_source ORDER BY request_source"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// OPS-039: the requests opened at or after `from` and before `to`, those that reached the card
/// step and where the rest stopped (module comment).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn intake_completion(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<IntakeCompletion, IntakeError> {
    let rows = sqlx::query!(
        r#"WITH c AS (
             SELECT c.id, EXISTS (SELECT 1 FROM intake.intake_answer a
                 WHERE a.conversation_id = c.id AND (a.question_id LIKE 'card.%'
                   OR (a.question_id = 'emergency_choice' AND a.choice = 'emergency'))) AS reached,
               (SELECT split_part(a.question_id, '.', 1) FROM intake.intake_answer a
                 WHERE a.conversation_id = c.id ORDER BY a.answered_at DESC, a.id DESC LIMIT 1)
                 AS last_step
             FROM intake.conversation c WHERE c.opened_at >= $1 AND c.opened_at < $2)
           SELECT CASE WHEN reached THEN NULL ELSE COALESCE(last_step, 'none') END AS step,
                  count(*) AS "requests!"
           FROM c GROUP BY 1 ORDER BY 2 DESC, 1 NULLS FIRST"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    let mut out = IntakeCompletion {
        started: 0,
        reached_card: 0,
        stopped_at: Vec::new(),
    };
    for r in rows {
        out.started += r.requests;
        match r.step {
            None => out.reached_card += r.requests,
            Some(step) => out.stopped_at.push(StopCount {
                step,
                requests: r.requests,
            }),
        }
    }
    Ok(out)
}

/// OPS-039, INT-041, D-3883: of jobs `jobs`, the cards approved at or after `from` and before `to`
/// and those approved with no EDIT before the approval.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn cards_approved(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
) -> Result<CardApprovals, IntakeError> {
    sqlx::query_as!(
        CardApprovals,
        r#"WITH sent AS (
             SELECT DISTINCT ON (a.conversation_id, a.question_id)
                    a.conversation_id, a.question_id, a.answered_at
             FROM intake.intake_answer a
             WHERE a.question_id = ANY(SELECT 'card.' || replace(j::text, '-', '')
                                       FROM unnest($1::uuid[]) j)
               AND a.choice LIKE 'sent.%' AND a.answered_at >= $2 AND a.answered_at < $3
             ORDER BY a.conversation_id, a.question_id, a.answered_at)
           SELECT count(*) AS "approved!",
                  count(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM intake.intake_answer e
                    WHERE e.conversation_id = s.conversation_id AND e.question_id = s.question_id
                      AND e.choice = 'edit' AND e.answered_at < s.answered_at)) AS "without_edit!"
           FROM sent s"#,
        jobs as _,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| IntakeError::Database)
}
