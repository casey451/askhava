//! OPS-046's first-six-weeks email panel and OPS-047's figures per pushed trade, as plain view
//! structs for T158's scorecard page (T622, D-490, D-4280, D-4639).
//!
//! This module computes no input itself: every count comes from another crate's reader. The email
//! panel is `messaging::api::email_days` (T531) shown only while the window lasts: from the
//! Arizona day of the sending subdomain's first send for `messaging::api::WINDOW_DAYS` days. Its
//! deferral alert is open when the Arizona day before now (the day the daily check
//! `messaging::api::check_deferrals` judges) has deferrals above 5% of its sends; the panel never
//! calls that check, because a page read changes nothing (SEC-026). The OPS-047 figures take one
//! trade's job IDs from `quotes::api::jobs_by_trade` (T477), then the median quote minutes and
//! each business's quotes from `quotes::api`, that business's picks in the window from
//! `picks::api::pick_numbers`, refund requests by reason from `payments::api::refund_reasons`,
//! and not-a-fit answers by reason from `ops::api::not_a_fit_counts` (T479) for the Arizona month
//! of the window's last second. Counts, code names and record IDs only, never a contact detail.

use askhava_messaging::api::{EmailDay, email_days};
use askhava_ops::api::{NotAFitReason, not_a_fit_counts};
use askhava_payments::api::refund_reasons;
use askhava_picks::api::pick_numbers;
use askhava_quotes::api::{jobs_by_trade, median_quote_minutes, quotes_per_business};
use jiff::tz::{TimeZone, offset};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::session::PortalsError;

const DB: PortalsError = PortalsError::Database;
const DAY: SignedDuration = SignedDuration::from_hours(24);

/// OPS-046's first-six-weeks email panel: each Arizona day so far, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailPanel {
    /// Each day's sends, deferrals, bounces and complaints; a day's
    /// [`EmailDay::deferrals_too_high`] marks it over the line.
    pub days: Vec<EmailDay>,
    /// Whether the deferral alert is open: the day before today had deferrals above 5% of sends.
    pub alert_open: bool,
}

/// OPS-046, D-490, D-4639: the email panel at `now`; `None` before the first send and from the
/// day after the six-week window ends, when the page shows no panel. Reads only.
///
/// # Errors
///
/// [`PortalsError::Database`].
pub async fn email_panel(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Option<EmailPanel>, PortalsError> {
    let days = email_days(db, now).await.map_err(|_| DB)?;
    let arizona = TimeZone::fixed(offset(-7));
    let today = now.to_zoned(arizona.clone()).date();
    if days.last().is_none_or(|d| d.day != today) {
        return Ok(None);
    }
    let yesterday = (now - DAY).to_zoned(arizona).date();
    let alert_open = days
        .iter()
        .any(|d| d.day == yesterday && d.deferrals_too_high());
    Ok(Some(EmailPanel { days, alert_open }))
}

/// One business's quotes and picks for OPS-047's quotes per pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusinessQuotes {
    /// The business, by ID.
    pub business: Uuid,
    /// Its quotes on the trade's jobs received in the window.
    pub quotes: i64,
    /// Its picks made in the window (`picks::api::pick_numbers`, every trade).
    pub picks: u32,
}

impl BusinessQuotes {
    /// OPS-047: quotes per pick in tenths, rounded half up; `None` with no pick.
    #[must_use]
    pub fn quotes_per_pick_tenths(&self) -> Option<i64> {
        let picks = i64::from(self.picks);
        (picks > 0).then(|| (self.quotes * 20 + picks) / (picks * 2))
    }
}

/// OPS-047's figures for one pushed trade in a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeFigures {
    /// The trade's catalog code name.
    pub trade: String,
    /// The median "minutes this quote took", to the second; `None` when no quote gave it.
    pub median_quote_minutes: Option<SignedDuration>,
    /// Each business that quoted, by business ID.
    pub quotes_per_pick: Vec<BusinessQuotes>,
    /// Not-a-fit answers by reason, in the page's reason order; a reason with none left out.
    pub not_a_fit: Vec<(NotAFitReason, i64)>,
    /// Refund requests by reason code name, by name; empty with none.
    pub refund_reasons: Vec<(String, i64)>,
}

/// OPS-047, D-490, D-4639: the figures of each of `pushed` (trade code names, in that order) for
/// quotes, picks and refund requests at or after `from` and before `to`; a pushed trade with no
/// job gets empty figures. Reads only.
///
/// # Errors
///
/// [`PortalsError::Database`].
pub async fn trade_figures(
    db: &mut PgConnection,
    pushed: &[&str],
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<TradeFigures>, PortalsError> {
    let trades = jobs_by_trade(db, to).await.map_err(|_| DB)?;
    let last_second = to - SignedDuration::from_secs(1);
    let answers = not_a_fit_counts(db, last_second).await.map_err(|_| DB)?;
    let mut figures = Vec::with_capacity(pushed.len());
    for trade in pushed {
        let jobs = trades
            .iter()
            .find(|t| t.trade == *trade)
            .map_or(&[][..], |t| t.jobs.as_slice());
        let mut not_a_fit: Vec<_> = answers
            .iter()
            .filter(|a| a.trade == *trade)
            .map(|a| (a.reason, a.count))
            .collect();
        not_a_fit.sort_by_key(|(reason, _)| NotAFitReason::ALL.iter().position(|r| r == reason));
        figures.push(TradeFigures {
            trade: (*trade).to_owned(),
            median_quote_minutes: median_quote_minutes(db, jobs, from, to)
                .await
                .map_err(|_| DB)?,
            quotes_per_pick: business_quotes(db, jobs, (from, to)).await?,
            not_a_fit,
            refund_reasons: refund_reasons(db, jobs, from, to)
                .await
                .map_err(|_| DB)?
                .unwrap_or_default(),
        });
    }
    Ok(figures)
}

/// OPS-047: each business's quotes on `jobs` in the window with its picks in the window.
async fn business_quotes(
    db: &mut PgConnection,
    jobs: &[Uuid],
    window: (Timestamp, Timestamp),
) -> Result<Vec<BusinessQuotes>, PortalsError> {
    let counts = quotes_per_business(db, jobs, window.0, window.1)
        .await
        .map_err(|_| DB)?
        .unwrap_or_default();
    let mut rows = Vec::with_capacity(counts.len());
    for (business, quotes) in counts {
        let numbers = pick_numbers(db, business, window).await.map_err(|_| DB)?;
        rows.push(BusinessQuotes {
            business,
            quotes,
            picks: numbers.picks,
        });
    }
    Ok(rows)
}
