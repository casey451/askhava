//! "Not a fit for us" on the business job page (T479; PICK-057, OPS-039, A11Y-007, A11Y-009,
//! D-3753, D-3883).
//!
//! Every job card page offers the business one tap per reason: not my trade, not my town, too big
//! or too small, and other ([`NotAFitChoice`], shown by `business_actions::job_details_page_choice`
//! as one form whose four buttons each send their reason). The `POST` goes to
//! [`NOT_A_FIT_ROUTE`] (`{object}` the job, `{token}` the card's Quote link token, so the door
//! probe can fill it; T306 serves it, checks the link and passes the proven business and job).
//! [`answer_not_a_fit`] stores the answer through `ops::api::record_not_a_fit` under the job's
//! trade from `quotes::api::job_trade`, only for a business that holds a card for the job, and
//! changes nothing else: no routing, no fee. A second answer by the same business for the same job
//! adds nothing. [`not_a_fit_page`] renders `not_a_fit.html`: the thanks, the "already told us"
//! line, or a refused submit's error with the four reasons again. IDs and codes only.

use askama::Template;
use askhava_ops::api::{NotAFit, NotAFitReason, record_not_a_fit};
use askhava_quotes::api::job_trade;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// PICK-057, D-3883: the answer's `POST` route; `{object}` is the job's ID and `{token}` the
/// card's Quote link token. T306 serves it.
pub const NOT_A_FIT_ROUTE: &str = "/b/not-a-fit/{object}/{token}";

/// The form field holding the reason code.
pub const NOT_A_FIT_FIELD: &str = "reason";

/// PICK-057: what the page says before the reasons; the answer changes no routing or fee.
pub const NOT_A_FIT_LINE: &str = "If this job isn't for you, tap a reason. Your answer doesn't change which jobs we send you or what you pay.";

/// A11Y-030: the message when a submit names no known reason.
pub const NOT_A_FIT_ERROR: &str = "Tap one of the reasons below.";

/// PICK-057: the button wording of each reason.
#[must_use]
pub const fn reason_label(reason: NotAFitReason) -> &'static str {
    match reason {
        NotAFitReason::NotMyTrade => "Not my trade",
        NotAFitReason::NotMyTown => "Not my town",
        NotAFitReason::TooBigOrSmall => "Too big or too small",
        NotAFitReason::Other => "Other reason",
    }
}

/// PICK-057: the four reasons as (code, label), in the order the page offers them.
#[must_use]
pub fn reasons() -> [(&'static str, &'static str); 4] {
    NotAFitReason::ALL.map(|r| (r.code(), reason_label(r)))
}

/// PICK-057: the reason a submitted form names, or `None` for no or an unknown reason (R1).
#[must_use]
pub fn parse_reason(raw: Option<&str>) -> Option<NotAFitReason> {
    raw.and_then(NotAFitReason::from_code)
}

/// PICK-057: [`NOT_A_FIT_ROUTE`] filled with the job and the card's Quote link token.
#[must_use]
pub fn not_a_fit_action(job: Uuid, token: &str) -> String {
    NOT_A_FIT_ROUTE
        .replace("{object}", &job.to_string())
        .replace("{token}", token)
}

/// PICK-057: the form the job page and a refused submit show: where it posts and its CSRF token.
#[derive(Debug, Clone, Copy)]
pub struct NotAFitChoice<'a> {
    /// The filled [`NOT_A_FIT_ROUTE`] ([`not_a_fit_action`]).
    pub action: &'a str,
    /// The form's CSRF token.
    pub csrf: &'a str,
}

impl NotAFitChoice<'_> {
    /// The four reasons as (code, label).
    #[must_use]
    pub fn reasons(&self) -> [(&'static str, &'static str); 4] {
        reasons()
    }

    /// What the page says before the reasons.
    #[must_use]
    pub const fn line(&self) -> &'static str {
        NOT_A_FIT_LINE
    }
}

/// What [`answer_not_a_fit`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAFitOutcome {
    /// The answer was stored.
    Saved,
    /// The business already answered for this job; nothing was written.
    AlreadyAnswered,
    /// The business holds no card for the job; nothing was written.
    NoCard,
}

/// Why an answer could not be stored. Carries no value (PRIV-016, R3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotAFitError {
    /// A database statement failed.
    #[error("the not-a-fit answer could not be stored")]
    Database,
}

/// PICK-057, OPS-039: stores `business`'s answer `reason` on `job` at `now`, under the job's trade,
/// when the business holds a card for the job; writes nothing else, so no routing or fee changes.
///
/// # Errors
///
/// [`NotAFitError::Database`].
pub async fn answer_not_a_fit(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    reason: NotAFitReason,
    now: Timestamp,
) -> Result<NotAFitOutcome, NotAFitError> {
    let trade = job_trade(db, (job, business))
        .await
        .map_err(|_| NotAFitError::Database)?;
    let Some(trade) = trade else {
        return Ok(NotAFitOutcome::NoCard);
    };
    let answer = NotAFit {
        job,
        business,
        trade: &trade,
        reason,
    };
    let stored = record_not_a_fit(db, &answer, now)
        .await
        .map_err(|_| NotAFitError::Database)?;
    Ok(if stored {
        NotAFitOutcome::Saved
    } else {
        NotAFitOutcome::AlreadyAnswered
    })
}

/// What the not-a-fit page shows after a submit.
#[derive(Debug, Clone, Copy)]
pub enum NotAFitState<'a> {
    /// The answer was stored: thanks, naming the reason.
    Saved(NotAFitReason),
    /// The business had already answered for this job.
    AlreadyAnswered,
    /// No known reason was sent: the error and the four reasons again (A11Y-030).
    Refused(NotAFitChoice<'a>),
}

#[derive(Template)]
#[template(path = "not_a_fit.html")]
struct NotAFitView<'a> {
    job_number: &'a str,
    job_link: &'a str,
    saved: Option<&'static str>,
    choice: Option<NotAFitChoice<'a>>,
    error: &'static str,
}

/// PICK-057, A11Y-007, A11Y-009, A11Y-030: the page after a not-a-fit submit on job `job_number`,
/// with a link back to the job page at `job_link`.
///
/// # Errors
/// The template's render error.
pub fn not_a_fit_page(
    state: NotAFitState<'_>,
    job_number: &str,
    job_link: &str,
) -> Result<String, askama::Error> {
    let (saved, choice) = match state {
        NotAFitState::Saved(reason) => (Some(reason_label(reason)), None),
        NotAFitState::AlreadyAnswered => (None, None),
        NotAFitState::Refused(choice) => (None, Some(choice)),
    };
    NotAFitView {
        job_number,
        job_link,
        saved,
        choice,
        error: NOT_A_FIT_ERROR,
    }
    .render()
}
