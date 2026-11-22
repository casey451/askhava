//! Casey's "Count as a complaint" on a check-in PROBLEM report, and the reports he counts from
//! (JOB-016, BIZ-011, BIZ-012, BIZ-029, OPS-005, PRIV-006; D-4423).
//!
//! [`count_problem_as_complaint`] reads the PROBLEM alert (`ops::api::alert_subject`), its job and
//! kept words (`privacy::api::problem_words`, owner session only), the business of the job's latest
//! settled pick (`picks::api::settled_pick`, so a no-show pick still names its business) and runs
//! `businesses::api::count_problem`, which records at most one complaint per job. [`problem_reports`]
//! lists the PROBLEM alerts raised in the last 30 days with job number, kept words, safety mark and
//! whether the job is counted; no homeowner contact field (PRIV-006). Neither sends a text nor
//! serves a page (T712 serves the page, T713 the homeowner's BIZ-011 text).

use std::fmt;

use askhava_businesses::api::{ComplaintError, Counted, count_problem, counted_jobs};
use askhava_jobs::api::{PROBLEM_ALERT, PROBLEM_SAFETY_ALERT, job_number};
use askhava_ops::api::{AlertSubject, DigestAlert, alert_subject, digest_alerts};
use askhava_picks::api::settled_pick;
use askhava_privacy::api::{OwnerSessionProof, WORDS_KEPT_FOR, problem_words};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The alert subject a PROBLEM alert names: the inbound text row.
const INBOX: &str = "messaging.inbox";

/// Why counting a PROBLEM report failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum CountRefused {
    /// No alert, or an alert that is not a PROBLEM report naming an inbound text (JOB-016).
    #[error("not a PROBLEM report")]
    NotProblem,
    /// The report's kept words were deleted after TXT-020's 30 days.
    #[error("the PROBLEM words are gone")]
    WordsGone,
    /// The job has no settled pick, so no business to count against.
    #[error("the job has no settled pick")]
    NoBusiness,
    /// The kind is not one of `businesses::api::PROBLEM_KINDS`.
    #[error("not a complaint kind")]
    BadKind,
    /// A database call, audit row or alert failed.
    #[error("complaint count database call failed")]
    Database,
}

impl From<ComplaintError> for CountRefused {
    fn from(e: ComplaintError) -> Self {
        match e {
            ComplaintError::BadKind => Self::BadKind,
            _ => Self::Database,
        }
    }
}

/// JOB-016, BIZ-011, BIZ-012, BIZ-029, OPS-005, D-4423: Casey (`owner`, on `proof`) counts the
/// PROBLEM report of alert `alert` as one complaint of `kind` at `now`, against the business of
/// the job's latest settled pick. A second count of the same job answers [`Counted::Already`].
///
/// # Errors
/// [`CountRefused::NotProblem`], `WordsGone`, `NoBusiness`, `BadKind` (each writes nothing) or
/// `Database`.
pub async fn count_problem_as_complaint(
    conn: &mut PgConnection,
    proof: &OwnerSessionProof,
    (alert, kind): (Uuid, &str),
    (owner, now): (Uuid, Timestamp),
) -> Result<Counted, CountRefused> {
    let subject = alert_subject(conn, alert)
        .await
        .map_err(|_| CountRefused::Database)?;
    let inbox = subject
        .as_ref()
        .and_then(problem_inbox)
        .ok_or(CountRefused::NotProblem)?;
    let kept = problem_words(conn, proof, inbox)
        .await
        .map_err(|_| CountRefused::Database)?
        .ok_or(CountRefused::WordsGone)?;
    let pick = settled_pick(conn, kept.job)
        .await
        .map_err(|_| CountRefused::Database)?
        .ok_or(CountRefused::NoBusiness)?;
    let business_job = (pick.business, kept.job);
    let counted = count_problem(conn, business_job, (kind, &kept.words), (owner, now)).await?;
    Ok(counted)
}

/// The inbox row a PROBLEM alert names; `None` for any other alert.
fn problem_inbox(a: &AlertSubject) -> Option<Uuid> {
    let problem = a.kind == PROBLEM_ALERT || a.kind == PROBLEM_SAFETY_ALERT;
    let inbox = a.subject_kind.as_deref() == Some(INBOX);
    a.subject_id.filter(|_| problem && inbox)
}

/// One PROBLEM report as Casey's owner portal lists it: no homeowner contact (PRIV-006). `Debug`
/// leaves the words out (PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct ProblemReport {
    /// The PROBLEM alert, the ID "Count as a complaint" takes.
    pub alert: Uuid,
    /// When the alert was raised.
    pub raised_at: Timestamp,
    /// The job's number.
    pub number: i64,
    /// The kept, scrubbed words.
    pub words: String,
    /// True for a `jobs.problem_report_safety` alert (OPS-005).
    pub safety: bool,
    /// True when the job already has a complaint (JOB-016).
    pub counted: bool,
}

impl fmt::Debug for ProblemReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProblemReport")
            .field("alert", &self.alert)
            .field("number", &self.number)
            .field("safety", &self.safety)
            .field("counted", &self.counted)
            .finish_non_exhaustive()
    }
}

/// JOB-016, PRIV-006, TXT-020, D-4423: every PROBLEM alert raised in the 30 days before `now`,
/// newest first, with its job number, kept words, safety mark and whether its job is counted. An
/// alert resolved inside the window but raised before it is left out, and so is one whose words
/// are no longer kept. Reads only.
///
/// # Errors
/// [`CountRefused::Database`].
pub async fn problem_reports(
    conn: &mut PgConnection,
    proof: &OwnerSessionProof,
    now: Timestamp,
) -> Result<Vec<ProblemReport>, CountRefused> {
    let from = now - WORDS_KEPT_FOR;
    let kinds = [PROBLEM_ALERT, PROBLEM_SAFETY_ALERT];
    let alerts = digest_alerts(conn, &kinds, from, now)
        .await
        .map_err(|_| CountRefused::Database)?;
    let mut found = Vec::new();
    for a in alerts.iter().rev().filter(|a| a.raised_at >= from) {
        if let Some(row) = report(conn, proof, a).await? {
            found.push(row);
        }
    }
    let jobs: Vec<Uuid> = found.iter().map(|(job, _)| *job).collect();
    let counted = counted_jobs(conn, &jobs).await?;
    Ok(found
        .into_iter()
        .map(|(job, mut row)| {
            row.counted = counted.contains(&job);
            row
        })
        .collect())
}

/// One listed report with its job, or `None` when its words are no longer kept.
async fn report(
    conn: &mut PgConnection,
    proof: &OwnerSessionProof,
    a: &DigestAlert,
) -> Result<Option<(Uuid, ProblemReport)>, CountRefused> {
    let Some((INBOX, inbox)) = a.subject.as_ref().map(|(k, id)| (k.as_str(), *id)) else {
        return Ok(None);
    };
    let kept = problem_words(conn, proof, inbox)
        .await
        .map_err(|_| CountRefused::Database)?;
    let Some(kept) = kept else {
        return Ok(None);
    };
    let number = job_number(conn, kept.job)
        .await
        .map_err(|_| CountRefused::Database)?;
    let row = ProblemReport {
        alert: a.id,
        raised_at: a.raised_at,
        number,
        words: kept.words,
        safety: a.kind == PROBLEM_SAFETY_ALERT,
        counted: false,
    };
    Ok(Some((kept.job, row)))
}
