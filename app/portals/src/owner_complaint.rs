//! Casey's "Count as a complaint" on a check-in PROBLEM report, and the reports he counts from
//! (JOB-016, BIZ-011, BIZ-012, BIZ-029, OPS-005, PRIV-006; D-4423).
//!
//! [`count_problem_as_complaint`] reads the PROBLEM alert (`ops::api::alert_subject`), its job and
//! kept words (`privacy::api::problem_words`, owner session only), the business of the job's latest
//! settled pick (`picks::api::settled_pick`, so a no-show pick still names its business) and runs
//! `businesses::api::count_problem`, which records at most one complaint per job. [`problem_reports`]
//! lists the PROBLEM alerts raised in the last 30 days with job number, kept words, safety mark and
//! whether the job is counted; no homeowner contact field (PRIV-006). Neither sends a text.
//!
//! T712 (D-4423, D-3890): [`owner_problem_reports_page`] renders `owner_problem_reports.html` from
//! those rows: each uncounted one has a form posting the CSRF token, the alert ID and a kind
//! picked from [`PROBLEM_KIND_CHOICES`] (none preselected, no `required`, A11Y-030) to
//! [`OWNER_PROBLEM_COMPLAINT_ROUTE`]; a counted one says so and has no form. A refusal shows the
//! list again with "Error: " in the title and a focused summary linking to the row's choices.
//! `server`'s `owner_portal` serves both fixed paths behind the owner session.

use std::fmt;

use askama::Template;
use askhava_businesses::api::{ComplaintError, Counted, count_problem, counted_jobs};
use askhava_jobs::api::{PROBLEM_ALERT, PROBLEM_SAFETY_ALERT, job_number};
use askhava_ops::api::{AlertSubject, DigestAlert, alert_subject, digest_alerts};
use askhava_picks::api::settled_pick;
use askhava_privacy::api::{OwnerSessionProof, WORDS_KEPT_FOR, problem_words};
use jiff::{Timestamp, tz};
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The alert subject a PROBLEM alert names: the inbound text row.
const INBOX: &str = "messaging.inbox";

/// JOB-016, D-4423: the owner's list of PROBLEM reports (`GET`); a fixed path (D-3890).
pub const OWNER_PROBLEM_REPORTS_ROUTE: &str = "/owner/problem-reports";

/// JOB-016, SEC-006, D-3890: the "Count as a complaint" press (`POST`); the alert ID and kind
/// travel in the form body.
pub const OWNER_PROBLEM_COMPLAINT_ROUTE: &str = "/owner/problem-complaint";

/// JOB-016, D-4423: each complaint kind's stored value (`businesses::api::PROBLEM_KINDS`) and the
/// label the page shows, in the page's order.
pub const PROBLEM_KIND_CHOICES: [(&str, &str); 5] = [
    ("no_show", "No-show"),
    ("conduct", "Conduct"),
    ("work_or_price", "Work or price"),
    ("safety", "Safety"),
    ("other", "Other"),
];

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

/// A press the page refused: why, the alert ID it named (when it was one) and the kind as posted,
/// kept checked on the row (A11Y-012).
#[derive(Debug, Clone, Copy)]
pub struct PressRefused<'a> {
    /// Why nothing was counted; never [`CountRefused::Database`], which is a 500.
    pub why: CountRefused,
    /// The alert ID the form named, when it parsed.
    pub alert: Option<Uuid>,
    /// The kind as posted.
    pub kind: &'a str,
}

/// One listed report as the template shows it: IDs, an Arizona time and the scrubbed words only.
struct ReportRow {
    alert: String,
    number: i64,
    came_in: String,
    words: String,
    safety: bool,
    counted: bool,
    kept: String,
}

#[derive(Template)]
#[template(path = "owner_problem_reports.html")]
struct ReportsView<'a> {
    csrf: &'a str,
    route: &'static str,
    kinds: [(&'static str, &'static str); 5],
    rows: Vec<ReportRow>,
    error: Option<(&'static str, String)>,
}

/// The line a refusal shows (A11Y-011); nothing was counted for any of them.
fn refusal_line(why: CountRefused) -> &'static str {
    match why {
        CountRefused::BadKind => "Choose what kind of complaint this is.",
        CountRefused::NoBusiness => {
            "This job doesn't have a confirmed pick yet, so there's no one to count it against."
        }
        CountRefused::WordsGone => {
            "We delete what homeowners write after 30 days, so this one can't be counted now."
        }
        CountRefused::NotProblem | CountRefused::Database => "We couldn't find that report.",
    }
}

/// JOB-016, SEC-006, PRIV-006, A11Y-011, A11Y-012, A11Y-030, D-4423: the problem reports page for
/// `reports` (newest first, as [`problem_reports`] lists them) with `refused`'s summary, linked to
/// the refused row's first choice when that row is listed. No homeowner contact is shown.
///
/// # Errors
/// The template's render error.
pub fn owner_problem_reports_page(
    csrf: &str,
    reports: &[ProblemReport],
    refused: Option<PressRefused<'_>>,
) -> askama::Result<String> {
    let refused_alert = refused.and_then(|r| r.alert);
    let rows: Vec<ReportRow> = reports
        .iter()
        .map(|r| ReportRow {
            alert: r.alert.to_string(),
            number: r.number,
            came_in: arizona(r.raised_at),
            words: r.words.clone(),
            safety: r.safety,
            counted: r.counted,
            kept: match refused {
                Some(p) if refused_alert == Some(r.alert) => p.kind.to_owned(),
                _ => String::new(),
            },
        })
        .collect();
    let error = refused.map(|p| {
        let open = p
            .alert
            .filter(|a| reports.iter().any(|r| r.alert == *a && !r.counted));
        let anchor = open.map_or_else(
            || "#problem-reports".to_owned(),
            |a| format!("#kind-{a}-{}", PROBLEM_KIND_CHOICES[0].0),
        );
        (refusal_line(p.why), anchor)
    });
    ReportsView {
        csrf,
        route: OWNER_PROBLEM_COMPLAINT_ROUTE,
        kinds: PROBLEM_KIND_CHOICES,
        rows,
        error,
    }
    .render()
}

/// `at` in Arizona time (MST all year), as the owner pages show times.
fn arizona(at: Timestamp) -> String {
    at.to_zoned(tz::TimeZone::fixed(tz::offset(-7)))
        .strftime("%b %-d, %-I:%M %p")
        .to_string()
}

#[cfg(test)]
mod page_tests {
    //! JOB-016, PRIV-006, A11Y-030: the page's rows, form and refusal summary.

    use super::*;

    fn report(counted: bool) -> ProblemReport {
        ProblemReport {
            alert: Uuid::from_u128(0x712),
            raised_at: Timestamp::UNIX_EPOCH,
            number: 1042,
            words: "the sample tap still drips".to_owned(),
            safety: true,
            counted,
        }
    }

    /// JOB-016: an uncounted report has the form with every kind, none checked; a counted one
    /// says so with no form; an empty list says there are none.
    #[test]
    fn rows_show_the_form_only_while_uncounted() {
        let open = owner_problem_reports_page("t", &[report(false)], None).expect("page");
        assert!(open.contains("Count as a complaint</button>"));
        assert!(open.contains("Safety word"));
        assert!(!open.contains(" checked>"), "no kind preselected");
        for (_, label) in PROBLEM_KIND_CHOICES {
            assert!(open.contains(label), "{label}");
        }
        let done = owner_problem_reports_page("t", &[report(true)], None).expect("page");
        assert!(done.contains("Counted as a complaint"));
        assert!(!done.contains("<form"));
        let none = owner_problem_reports_page("t", &[], None).expect("page");
        assert!(none.contains("No problem reports in the last 30 days."));
    }

    /// A11Y-011, A11Y-012, A11Y-030: a refusal titles the page "Error: ", links its line to the
    /// row's first choice and keeps the posted kind checked.
    #[test]
    fn a_refusal_links_to_the_row_and_keeps_the_kind() {
        let refused = PressRefused {
            why: CountRefused::NoBusiness,
            alert: Some(Uuid::from_u128(0x712)),
            kind: "conduct",
        };
        let page = owner_problem_reports_page("t", &[report(false)], Some(refused)).expect("page");
        assert!(page.contains("<title>Error: "));
        assert!(page.contains(&format!(
            "href=\"#kind-{}-no_show\"",
            Uuid::from_u128(0x712)
        )));
        assert!(page.contains(r#"value="conduct" checked"#));
        let gone = PressRefused {
            alert: None,
            ..refused
        };
        let page = owner_problem_reports_page("t", &[], Some(gone)).expect("page");
        assert!(page.contains(r##"href="#problem-reports""##));
    }
}
