//! The intake golden run (OPS-040, INT-040, D-395, D-493): every row of
//! `docs/catalog/intake_golden.yaml` is routed with no human and the share that reaches the right
//! trade, job type and first question is stored once per catalog version in
//! `intake.golden_result`, which T210's monthly report reads through `intake::api`.
//!
//! The run uses the AI-off path (keyword rules, [`HIGH_CONFIDENCE`]) because the build has no real
//! model; a row is right only when [`decide`] goes on with no question at the row's trade and job
//! type and the trade's set opens with the row's question. [`catalog_version`] is a fingerprint of
//! the compiled-in catalog, question bank and golden set, so a new build with a changed file runs
//! once (the worker calls [`run_golden`] at start-up). `test_set.csv` uses the knowledge base's
//! own trade and job-type codes, which map to no catalog job type yet, so it is not run here
//! (D-1271). The 90% target is the report's; nothing here blocks on it.

use askhava_catalog::api::{QuestionBank, question_bank};
use jiff::Timestamp;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;

use crate::matching::keyword_reading;
use crate::models::{self, GoldenResultRow, IntakeError};
use crate::routing_confidence::{Decision, HIGH_CONFIDENCE, decide};

const GOLDEN: &str = include_str!("../../../docs/catalog/intake_golden.yaml");
const CATEGORIES: &str = include_str!("../../../docs/catalog/categories.yaml");
const BANK: &str = include_str!("../../../docs/catalog/question_bank.yaml");

/// One golden row (fake text only).
#[derive(Debug, Clone, Deserialize)]
pub struct GoldenRow {
    /// `<trade>-<nn>`.
    pub id: String,
    /// The catalog category ID it must reach.
    pub trade: String,
    /// The job type it must reach.
    pub job_type: String,
    /// The ID of the first question asked.
    pub question: String,
    /// `en` or `es`.
    pub lang: String,
    /// The fake request.
    pub text: String,
}

#[derive(Deserialize)]
struct GoldenFile {
    rows: Vec<GoldenRow>,
}

/// The compiled-in golden rows (OPS-040).
///
/// # Errors
///
/// [`IntakeError::Catalog`] when the file does not parse.
pub fn golden_rows() -> Result<Vec<GoldenRow>, IntakeError> {
    serde_saphyr::from_str::<GoldenFile>(GOLDEN)
        .map(|f| f.rows)
        .map_err(|_| IntakeError::Catalog)
}

/// The catalog version: 16 hex of the SHA256 of the compiled-in catalog, question bank and
/// golden set (OPS-040).
#[must_use]
pub fn catalog_version() -> String {
    let mut hash = Sha256::new();
    for file in [CATEGORIES, BANK, GOLDEN] {
        hash.update(file.len().to_be_bytes());
        hash.update(file.as_bytes());
    }
    hash.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// OPS-040, INT-040: the row reaches its trade and job type with no question and the trade's set
/// opens with its question (the first question with no `ask_if`).
#[must_use]
pub fn reaches(row: &GoldenRow, bank: &QuestionBank) -> bool {
    let routed = matches!(
        decide(&keyword_reading(&row.text), None, HIGH_CONFIDENCE),
        Decision::GoOn { trade, job_type } if trade == row.trade && job_type == row.job_type
    );
    let first = bank
        .sets
        .get(&row.trade)
        .and_then(|s| s.questions.iter().find(|q| q.ask_if.is_none()));
    routed && first.is_some_and(|q| q.id == row.question)
}

/// The run's counts for `version` (OPS-040).
///
/// # Errors
///
/// [`IntakeError::Catalog`] when the bank or the golden set does not load.
pub fn score(version: &str, at: Timestamp) -> Result<GoldenResultRow, IntakeError> {
    let bank = question_bank().map_err(|_| IntakeError::Catalog)?;
    let rows = golden_rows()?;
    let right = rows.iter().filter(|r| reaches(r, &bank)).count();
    let count = |n: usize| i32::try_from(n).map_err(|_| IntakeError::Catalog);
    Ok(GoldenResultRow {
        catalog_version: version.to_owned(),
        rows_total: count(rows.len())?,
        rows_right: count(right)?,
        ran_at: at,
    })
}

/// OPS-040, D-493: runs the golden set for `version` unless it already has a stored run, and
/// returns the new run; `None` when the version was already run.
///
/// # Errors
///
/// [`IntakeError::Catalog`] or [`IntakeError::Database`].
pub async fn run_golden_for(
    db: &mut PgConnection,
    version: &str,
    at: Timestamp,
) -> Result<Option<GoldenResultRow>, IntakeError> {
    let row = score(version, at)?;
    Ok(models::insert_golden(db, &row).await?.then_some(row))
}

/// [`run_golden_for`] the compiled-in [`catalog_version`]; the worker calls it at start-up.
///
/// # Errors
///
/// As [`run_golden_for`].
pub async fn run_golden(
    db: &mut PgConnection,
    at: Timestamp,
) -> Result<Option<GoldenResultRow>, IntakeError> {
    run_golden_for(db, &catalog_version(), at).await
}

/// Every stored run, newest first, for T210's report (OPS-040).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn golden_results(db: &mut PgConnection) -> Result<Vec<GoldenResultRow>, IntakeError> {
    models::golden_rows(db).await
}

impl GoldenResultRow {
    /// The share that reached the right job type with no human, in whole percent rounded down.
    #[must_use]
    pub fn share_percent(&self) -> i32 {
        self.rows_right * 100 / self.rows_total.max(1)
    }
}
