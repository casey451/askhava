//! CARD-023's reader of a job's intake answers for the business job page (T745; D-4748 (5a),
//! D-5553; PRIV-001, PICK-022, INT-041).
//!
//! [`job_answers`] finds the request that opened a job through `intake.request_job` (or its
//! `conversation.job_id`), reads that job's own trade-question answers through
//! `order::job_bank_answers` (the request's first job `bank.<question ID>`, a later job
//! `bank.<job, 32 hex>.<question ID>`), keeps the latest answer to each question and returns them
//! in the order of the question set in `docs/catalog/question_bank.yaml`. Only bank questions are
//! returned, each as its stored code (an option number, `not_sure` or `text`); the contact
//! questions of step 11 (her name, street address and any on-site contact, INT-041) are never in
//! the bank and never returned. Her words are not stored, so none can be returned. A job no
//! request opened reads none. The `server` crate turns the codes into wording (T746).

use std::collections::BTreeMap;

use askhava_catalog::api::{QuestionSet, question_bank};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::conversation::{ADDRESS_CONTACT, NAME_ADDRESS_ASKED};
use crate::models::IntakeError;
use crate::order::job_bank_answers;

/// The step-11 contact questions (INT-041), never returned whatever a row holds (PRIV-001).
const CONTACT: [&str; 2] = [ADDRESS_CONTACT, NAME_ADDRESS_ASKED];

/// One intake answer of a job (CARD-023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobAnswer {
    /// The question bank's question ID, e.g. `fixture` or `details`.
    pub question: String,
    /// The stored code: the option number, `not_sure` or `text` (her words are never stored).
    pub value: String,
}

/// CARD-023, PRIV-001, PICK-022, D-5553: the latest answer to each trade question of `job`, in
/// question-bank order, from the request that opened it, open or closed; none for a job no
/// request opened. No contact question is ever returned.
///
/// # Errors
///
/// [`IntakeError::Database`] or [`IntakeError::Catalog`].
pub async fn job_answers(db: &mut PgConnection, job: Uuid) -> Result<Vec<JobAnswer>, IntakeError> {
    let Some(request) = request_of(db, job).await? else {
        return Ok(Vec::new());
    };
    let stored = job_bank_answers(db, (request, job)).await?;
    let latest: BTreeMap<String, String> = stored
        .into_iter()
        .filter(|(id, _)| !CONTACT.contains(&id.as_str()))
        .collect();
    let bank = question_bank().map_err(|_| IntakeError::Catalog)?;
    Ok(best_set(bank.sets.values(), &latest).map_or_else(Vec::new, |set| in_order(set, &latest)))
}

/// The request that opened `job`: its `intake.request_job` row, else the request whose first job
/// it is (D-5553).
async fn request_of(db: &mut PgConnection, job: Uuid) -> Result<Option<Uuid>, IntakeError> {
    sqlx::query_scalar!(
        r#"SELECT r.id AS "id!" FROM (SELECT conversation_id AS id FROM intake.request_job
           WHERE job_id = $1 UNION SELECT id FROM intake.conversation WHERE job_id = $1) r
           LIMIT 1"#,
        job
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// The question set that holds the most of the answered question IDs (the job's set; the first
/// by category code on a tie), or `None` when nothing was answered.
fn best_set<'a>(
    sets: impl Iterator<Item = &'a QuestionSet>,
    latest: &BTreeMap<String, String>,
) -> Option<&'a QuestionSet> {
    let held = |set: &QuestionSet| {
        set.questions
            .iter()
            .filter(|q| latest.contains_key(&q.id))
            .count()
    };
    let mut best: Option<(&QuestionSet, usize)> = None;
    for set in sets {
        let count = held(set);
        if count > 0 && best.is_none_or(|(_, top)| count > top) {
            best = Some((set, count));
        }
    }
    best.map(|(set, _)| set)
}

/// The answers to `set`'s questions in the set's order.
fn in_order(set: &QuestionSet, latest: &BTreeMap<String, String>) -> Vec<JobAnswer> {
    set.questions
        .iter()
        .filter_map(|q| {
            latest.get(&q.id).map(|value| JobAnswer {
                question: q.id.clone(),
                value: value.clone(),
            })
        })
        .collect()
}
