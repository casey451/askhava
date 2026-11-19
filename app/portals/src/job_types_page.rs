//! The business's Job types page after sign-up (T375; BIZ-004, BIZ-049, BIZ-050, SEC-004; D-2730).
//!
//! [`job_types_page`] renders `templates/job_types.html` (it extends `business_layout.html`, so it
//! shows the business-only "Get help" link, D-3155): the checklist as the sign-up capability step
//! shows it, from T368's offer (`businesses::api::offer_for`, licensed trades first), the stored
//! ticks checked, one save button, and after a save the result in the status region. A job type a
//! license change removed is not in the offer, so it is never shown. [`shown_offer`] leaves out the
//! recurring rows for a business that answered "no" to recurring work, as saving the capability
//! step does; [`within`] says whether every posted tick is a shown row. The route
//! [`JOB_TYPES_ROUTE`] is served by `server::business_portal` (owner or "Confirm and billing").

use std::collections::BTreeSet;

use askama::Template;
use askhava_businesses::api::{JobTypeChoice, TradeOffer};

/// BIZ-004: the Job types page's route; reachable by URL, no served page links to it yet (D-3890).
pub const JOB_TYPES_ROUTE: &str = "/b/job-types";

/// What the last save did, shown in the page's status region (BIZ-004, BIZ-050).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobTypesOutcome {
    /// The ticks were stored.
    Saved,
    /// The ticks were stored and a `no_covered_job_type` pause was lifted (D-2730).
    Resumed,
    /// A tick was outside the offer, so nothing was stored.
    NotSaved,
}

impl JobTypesOutcome {
    /// The line the status region shows.
    #[must_use]
    pub fn line(self) -> &'static str {
        match self {
            Self::Saved => "Saved. We'll send you the jobs checked below.",
            Self::Resumed => "Saved. You're back on, so new jobs will start coming again.",
            Self::NotSaved => {
                "Nothing was saved. One of the jobs you checked isn't one we can send you. \
                 Look over the list below and save again."
            }
        }
    }
}

/// What the page shows: the offer, the ticks to show checked, the last save's result and the
/// session's CSRF token (SEC-042).
pub struct JobTypesPage<'a> {
    /// The rows offered, from [`shown_offer`].
    pub offer: &'a [TradeOffer],
    /// The ticks shown checked: the stored ones, or the posted ones after a refused save.
    pub ticked: &'a BTreeSet<JobTypeChoice>,
    /// The last save's result, if the page follows one.
    pub outcome: Option<JobTypesOutcome>,
    /// The session's CSRF token.
    pub csrf: &'a str,
}

#[derive(Template)]
#[template(path = "job_types.html")]
struct JobTypesView<'a> {
    page: &'a JobTypesPage<'a>,
}

impl JobTypesView<'_> {
    /// Whether `c` shows checked.
    fn ticked(&self, c: &JobTypeChoice) -> bool {
        self.page.ticked.contains(c)
    }

    /// The form value of a job type, "category:job type".
    fn value(c: &JobTypeChoice) -> String {
        format!("{}:{}", c.category, c.job_type)
    }

    /// Whether no row of `trade` needs a license, so its legend says so.
    fn no_license(trade: &TradeOffer) -> bool {
        trade.rows.iter().all(|r| !r.licensed)
    }
}

/// BIZ-004, BIZ-049, BIZ-050, A11Y-012: the Job types page over `templates/job_types.html`.
///
/// # Errors
/// The template's render error.
pub fn job_types_page(page: &JobTypesPage<'_>) -> Result<String, askama::Error> {
    JobTypesView { page }.render()
}

/// BIZ-049, REC-002: `offer` as the page shows it: with `recurring` false (the business answered
/// "no" to recurring work) the recurring rows are left out, and a trade left with none with them.
#[must_use]
pub fn shown_offer(offer: Vec<TradeOffer>, recurring: bool) -> Vec<TradeOffer> {
    let mut shown = offer;
    for trade in &mut shown {
        trade.rows.retain(|r| recurring || !r.recurring);
    }
    shown.retain(|t| !t.rows.is_empty());
    shown
}

/// BIZ-004, LIC-043: whether every one of `ticks` is a row of `offer`.
#[must_use]
pub fn within(offer: &[TradeOffer], ticks: &[JobTypeChoice]) -> bool {
    let rows: BTreeSet<&JobTypeChoice> = offer
        .iter()
        .flat_map(|t| t.rows.iter().map(|r| &r.choice))
        .collect();
    ticks.iter().all(|c| rows.contains(c))
}

/// BIZ-004: a posted `job_type` value, "category:job type", as a choice; `None` without a colon.
#[must_use]
pub fn posted_choice(value: &str) -> Option<JobTypeChoice> {
    let (category, job_type) = value.split_once(':')?;
    Some(JobTypeChoice::new(category, job_type))
}
