//! A job's catalog category code and remaining candidate job type codes, as append-only narrowing
//! rows in `jobs.job_facts` (INT-018, FEE-005, FEE-045; D-2330, D-3157).
//!
//! The first row is written at card send; a later row may only narrow (the same category, a strict
//! subset of the latest set). The latest row is what the pick reads to fix the introduction fee by
//! job size (T340). Codes only: every fact derived from them is resolved from the catalog then.

use std::collections::BTreeSet;

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

/// A job's latest stored facts (INT-018).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFacts {
    /// The row's per-job sequence number, from 1.
    pub seq: i32,
    /// The catalog category code.
    pub category: String,
    /// The remaining candidate job type codes, sorted, without repeats, never empty.
    pub job_types: Vec<String>,
    /// When the row was written (the transaction's time).
    pub set_at: Timestamp,
}

/// What [`record_job_facts`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// A new row was appended.
    Written,
    /// The latest row already holds this category and set; nothing was written.
    Unchanged,
}

/// Why [`record_job_facts`] wrote nothing. No variant carries a code or the database's message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FactsError {
    /// The category is blank, or the candidate set is empty or holds a blank code.
    #[error("job facts need a category and at least one job type")]
    Empty,
    /// The category differs from the latest row's.
    #[error("job facts cannot change category")]
    OtherCategory,
    /// The set holds a code that is not in the latest row's set.
    #[error("job facts may only narrow the candidate job types")]
    NotASubset,
    /// No job has this ID.
    #[error("no such job")]
    NotFound,
    /// The database refused or dropped the statement.
    #[error("the job facts database statement failed")]
    Database,
}

/// The pure narrowing rule (INT-018, D-2330): `Ok(None)` when `next` equals the latest row,
/// `Ok(Some(set))` (sorted, deduplicated) when it may be appended, otherwise the refusal.
fn narrow(
    latest: Option<&JobFacts>,
    category: &str,
    job_types: &[String],
) -> Result<Option<Vec<String>>, FactsError> {
    let set: BTreeSet<&str> = job_types.iter().map(String::as_str).collect();
    if category.trim().is_empty() || set.is_empty() || set.iter().any(|c| c.trim().is_empty()) {
        return Err(FactsError::Empty);
    }
    let next: Vec<String> = set.into_iter().map(str::to_owned).collect();
    let Some(prev) = latest else {
        return Ok(Some(next));
    };
    if prev.category != category {
        return Err(FactsError::OtherCategory);
    }
    if next.iter().any(|c| !prev.job_types.contains(c)) {
        return Err(FactsError::NotASubset);
    }
    Ok((next != prev.job_types).then_some(next))
}

/// Records job `job`'s category and candidate job types in the caller's transaction `tx` (INT-018,
/// FEE-005, D-2330): locks the `jobs.job` row first, so concurrent calls apply one after the
/// other, then appends a row when it is the first or narrows the latest; the same set again writes
/// nothing; anything else is refused and writes nothing.
///
/// # Errors
///
/// [`FactsError::Empty`], [`FactsError::OtherCategory`], [`FactsError::NotASubset`],
/// [`FactsError::NotFound`] or [`FactsError::Database`].
pub async fn record_job_facts(
    tx: &mut PgConnection,
    job: Uuid,
    category: &str,
    job_types: &[String],
) -> Result<Recorded, FactsError> {
    sqlx::query_scalar!("SELECT id FROM jobs.job WHERE id = $1 FOR UPDATE", job)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| FactsError::Database)?
        .ok_or(FactsError::NotFound)?;
    let latest = job_facts(tx, job).await?;
    let Some(next) = narrow(latest.as_ref(), category, job_types)? else {
        return Ok(Recorded::Unchanged);
    };
    let seq = latest.map_or(1, |l| l.seq + 1);
    sqlx::query!(
        "INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at) \
         VALUES ($1, $2, $3, $4, now())",
        job,
        seq,
        category,
        &next,
    )
    .execute(tx)
    .await
    .map_err(|_| FactsError::Database)?;
    Ok(Recorded::Written)
}

/// Job `job`'s latest facts (the highest sequence number), or `None` before card send (INT-018).
///
/// # Errors
///
/// [`FactsError::Database`].
pub async fn job_facts(conn: &mut PgConnection, job: Uuid) -> Result<Option<JobFacts>, FactsError> {
    let row = sqlx::query!(
        "SELECT seq, category, job_types, set_at FROM jobs.job_facts \
         WHERE job_id = $1 ORDER BY seq DESC LIMIT 1",
        job
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| FactsError::Database)?;
    Ok(row.map(|r| JobFacts {
        seq: r.seq,
        category: r.category,
        job_types: r.job_types,
        set_at: r.set_at.to_jiff(),
    }))
}
