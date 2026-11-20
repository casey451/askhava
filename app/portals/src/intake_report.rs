//! The monthly intake accuracy report on the owner portal (OPS-040; T434, D-395, D-492, D-493,
//! D-3155, D-3753, D-3890).
//!
//! [`intake_report_page`] renders `templates/intake_report.html` for one Arizona calendar month:
//! the cards that got no quote and the quotes that said the business needed more information
//! (confirm-on-site ticked or the diagnostic-visit format; a withdrawn quote counts in neither
//! list, D-3155), both from `quotes::api::intake_report_ids`, grouped by the job's category and
//! question set (the latest `jobs.job_facts` row through `jobs::api::report_facts`, the name
//! through `catalog::api::category`, the set's version through `catalog::api::question_bank`), and
//! the latest stored golden-set run (T156, `intake::api::golden_results`) against the target the
//! serving route passes in (`Settings::golden_target_percent`, default 90, D-493). IDs and counts
//! only; no name, contact or note is read. A GET: reads only, writes nothing. The month comes as a
//! `?month=YYYY-MM` query value on the fixed [`OWNER_INTAKE_REPORT_ROUTE`] (D-3890).

use std::collections::{BTreeMap, BTreeSet};

use askama::Template;
use askhava_catalog::api::{category, question_bank};
use askhava_core::api::arizona;
use askhava_intake::api::{GoldenResultRow, golden_results};
use askhava_jobs::api::{ReportFacts, report_facts};
use askhava_quotes::api::intake_report_ids;
use jiff::Timestamp;
use jiff::civil::Date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::owner_portal::OwnerToolError;

/// The report's path (OPS-040); `server` serves it (T305, D-2128), the month as a query value.
pub const OWNER_INTAKE_REPORT_ROUTE: &str = "/owner/intake-report";

/// One category and question set's line of the report (OPS-040).
#[derive(Debug, Default)]
struct Group {
    job_types: BTreeSet<String>,
    no_quote: usize,
    more_info: usize,
}

/// The latest golden-set run as the page words it (OPS-040, D-493).
struct Golden {
    share: i32,
    right: i32,
    total: i32,
    ran_on: Date,
    met: bool,
}

#[derive(Template)]
#[template(path = "intake_report.html")]
struct ReportView {
    month: String,
    month_value: String,
    route: &'static str,
    rows: Vec<(String, String, String, usize, usize)>,
    golden: Option<Golden>,
    target: u32,
}

fn db<E>(_: E) -> OwnerToolError {
    OwnerToolError::Database
}

/// `[start, end)` of the Arizona calendar month starting on `first` (JOB-007, OPS-040).
fn window(first: Date) -> Result<(Timestamp, Timestamp), OwnerToolError> {
    let start = |d: Date| d.to_zoned(arizona()).map(|z| z.timestamp());
    let next = first.checked_add(jiff::Span::new().months(1)).map_err(db)?;
    Ok((start(first).map_err(db)?, start(next).map_err(db)?))
}

/// The latest run of `runs` (newest first) worded against `target` percent (OPS-040, D-493).
fn golden(runs: &[GoldenResultRow], target: u32) -> Option<Golden> {
    runs.first().map(|r| Golden {
        share: r.share_percent(),
        right: r.rows_right,
        total: r.rows_total,
        ran_on: r.ran_at.to_zoned(arizona()).date(),
        met: u32::try_from(r.share_percent()).is_ok_and(|s| s >= target),
    })
}

/// OPS-040: counts the month's jobs into groups keyed by category code (`None` with no facts row).
fn group(
    ids: (&[Uuid], &[Uuid]),
    facts: &BTreeMap<Uuid, ReportFacts>,
) -> BTreeMap<Option<String>, Group> {
    let mut groups: BTreeMap<Option<String>, Group> = BTreeMap::new();
    for (jobs, more) in [(ids.0, false), (ids.1, true)] {
        for job in jobs {
            let f = facts.get(job);
            let g = groups.entry(f.map(|f| f.category.clone())).or_default();
            g.job_types
                .extend(f.into_iter().flat_map(|f| f.job_types.clone()));
            *(if more {
                &mut g.more_info
            } else {
                &mut g.no_quote
            }) += 1;
        }
    }
    groups
}

/// OPS-040: each group's category name and question set, as the table shows them.
async fn rows(
    conn: &mut PgConnection,
    groups: BTreeMap<Option<String>, Group>,
) -> Result<Vec<(String, String, String, usize, usize)>, OwnerToolError> {
    let bank = question_bank().map_err(db)?;
    let mut out = Vec::new();
    for (code, g) in groups {
        let (name, set) = match &code {
            Some(c) => (
                category(conn, c)
                    .await
                    .map_err(db)?
                    .map_or_else(|| c.clone(), |k| k.name),
                bank.sets
                    .get(c)
                    .map_or_else(|| "None".to_owned(), |s| format!("Version {}", s.version)),
            ),
            None => ("Not recorded".to_owned(), "None".to_owned()),
        };
        let types = g.job_types.into_iter().collect::<Vec<_>>().join(", ");
        out.push((name, set, types, g.no_quote, g.more_info));
    }
    Ok(out)
}

/// OPS-040, D-3155, D-493: the report page of the month starting on `first`, as of `now`, the
/// golden-set share shown against `target_percent`.
///
/// # Errors
///
/// [`OwnerToolError::Database`] when a reader fails or the page does not render.
pub async fn intake_report_page(
    conn: &mut PgConnection,
    first: Date,
    target_percent: u32,
    now: Timestamp,
) -> Result<String, OwnerToolError> {
    let ids = intake_report_ids(conn, window(first)?, now)
        .await
        .map_err(db)?;
    let more: Vec<Uuid> = ids.more_info.iter().map(|q| q.job).collect();
    let all: Vec<Uuid> = ids.no_quote_jobs.iter().chain(&more).copied().collect();
    let facts = report_facts(conn, &all).await.map_err(db)?;
    let groups = group((&ids.no_quote_jobs, &more), &facts);
    let runs = golden_results(conn).await.map_err(db)?;
    ReportView {
        month: first.strftime("%B %Y").to_string(),
        month_value: first.strftime("%Y-%m").to_string(),
        route: OWNER_INTAKE_REPORT_ROUTE,
        rows: rows(conn, groups).await?,
        golden: golden(&runs, target_percent),
        target: target_percent,
    }
    .render()
    .map_err(db)
}
