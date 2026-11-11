//! One quote format per category or job type, and the quote form's price fields (PICK-053,
//! REC-010; D-441, D-2333, D-1407).
//!
//! [`format_for`] reads a job type's format from the stored catalog (`job_type_formats`, else the
//! category's `quote_format`), so every quote for a job type uses the same one. [`parse_price`]
//! turns the form's [`PriceFields`] for that format into a [`Price`]: a range job's form has no
//! visit-fee field, and a per-visit recurring price needs its [`VisitSchedule`] (one of the five
//! frequencies `payments::rules::Period` converts) or "Other: enter the monthly amount", which
//! makes it a monthly price. [`visit_schedule_of`] reads the stored schedule (migration 0157);
//! `edit_withdraw::save_quote` stores it with the quote. No fee code here (D-491).

use askhava_catalog::api::Category;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::{Format, Period, Price};
use crate::models::QuotesError;

/// PICK-053: the format every quote for `job_type` in `category` uses: the job type's own
/// (`job_type_formats`), else `firm` for a one-time job type of a recurring category, else the
/// category's; none for an unknown code.
#[must_use]
pub fn format_for(category: &Category, job_type: &str) -> Option<Format> {
    let own = category.job_types.iter().find(|j| j.name == job_type);
    if let Some(code) = own.and_then(|j| j.quote_format.as_deref()) {
        return Format::from_code(code);
    }
    if category.recurring && own.is_some_and(|j| j.one_time) {
        return Some(Format::Firm);
    }
    Format::from_code(&category.quote_format)
}

/// REC-010, D-2333: how often a per-visit price's visits come; the five frequencies
/// `payments::rules::Period` converts to a monthly amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitSchedule {
    /// Every week.
    Weekly,
    /// Every two weeks.
    EveryTwoWeeks,
    /// Every month.
    Monthly,
    /// Every three months.
    Quarterly,
    /// Once a year.
    Yearly,
}

impl VisitSchedule {
    /// Every schedule, in the order the form offers them.
    pub const ALL: [Self; 5] = [
        Self::Weekly,
        Self::EveryTwoWeeks,
        Self::Monthly,
        Self::Quarterly,
        Self::Yearly,
    ];

    /// The code the form posts and `quotes.quote.visit_schedule` stores.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Weekly => "weekly",
            Self::EveryTwoWeeks => "every_two_weeks",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Yearly => "yearly",
        }
    }

    /// The schedule named by `code`, or none.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.code() == code)
    }

    /// The form's label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Weekly => "Weekly",
            Self::EveryTwoWeeks => "Every two weeks",
            Self::Monthly => "Monthly",
            Self::Quarterly => "Every 3 months",
            Self::Yearly => "Once a year",
        }
    }
}

/// The schedule choice that turns a per-visit price into a monthly one (D-1407).
pub const OTHER_SCHEDULE: &str = "other";

/// The price fields as the quote form posts them; empty when left blank. `Debug` leaves out
/// the tier names, which the business typed (rust brain R6).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PriceFields {
    /// A firm price, a recurring price or a visit fee.
    pub amount: String,
    /// A range's low.
    pub low: String,
    /// A range's high.
    pub high: String,
    /// The "Range to be confirmed on site" tick.
    pub on_site: bool,
    /// A recurring price's period: `month`, `week`, `quarter`, `year` or `visit`.
    pub per: String,
    /// A per-visit price's schedule code, or [`OTHER_SCHEDULE`].
    pub schedule: String,
    /// A recurring price's one-time setup charge (FEE-044).
    pub setup: String,
    /// Up to 3 package names and their prices.
    pub tiers: [(String, String); 3],
}

impl std::fmt::Debug for PriceFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PriceFields")
            .field("amount", &self.amount)
            .field("per", &self.per)
            .field("schedule", &self.schedule)
            .finish_non_exhaustive()
    }
}

/// A price field the form names when it is missing or unreadable (A11Y-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceProblem {
    /// The price (or visit fee) is missing or not an amount.
    Amount,
    /// A range's low or high is missing or not an amount.
    Range,
    /// A recurring price has no period.
    Per,
    /// A per-visit price has no visit schedule (REC-010, D-2333).
    Schedule,
    /// No package has both a name and a price, or one has only one of them.
    Tiers,
    /// The setup charge is not an amount.
    Setup,
}

impl PriceProblem {
    /// The form element the message points at.
    #[must_use]
    pub const fn field(self) -> &'static str {
        match self {
            Self::Amount => "amount",
            Self::Range => "low",
            Self::Per => "per",
            Self::Schedule => "schedule",
            Self::Tiers => "tier1_name",
            Self::Setup => "setup",
        }
    }

    /// The line the business reads by the field and in the summary.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Amount => "Enter your price in dollars, like 250 or 250.00.",
            Self::Range => "Enter a low and a high price in dollars.",
            Self::Per => {
                "Choose what the price covers: one week, one month, 3 months, one year or one visit."
            }
            Self::Schedule => {
                "Choose how often you'll visit, or choose Other and enter the monthly amount."
            }
            Self::Tiers => "Give each package a name and a price in dollars.",
            Self::Setup => "Enter the setup charge in dollars, or leave it empty.",
        }
    }
}

/// A typed dollar amount in cents: digits with an optional `$`, commas and two decimals.
fn dollars(text: &str) -> Option<u64> {
    let t = text.trim().trim_start_matches('$').replace(',', "");
    let (whole, part) = t.split_once('.').unwrap_or((&t, "00"));
    let digits = |s: &str, max: usize| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit())
    };
    if !digits(whole, 8) || !digits(part, 2) || part.len() != 2 {
        return None;
    }
    Some(whole.parse::<u64>().ok()? * 100 + part.parse::<u64>().ok()?)
}

/// PICK-053, REC-010, D-2333: the price `fields` give for a job in `format`, with the visit
/// schedule of a per-visit price. A range job reads only low and high (no visit fee); "Other"
/// makes the amount a monthly price with no schedule (D-1407).
///
/// # Errors
///
/// The first [`PriceProblem`]; amounts are checked again by `parse_quote`.
pub fn parse_price(
    format: Format,
    fields: &PriceFields,
) -> Result<(Price, Option<VisitSchedule>), PriceProblem> {
    let amount = || dollars(&fields.amount).ok_or(PriceProblem::Amount);
    let price = match format {
        Format::Firm => Price::Firm(amount()?),
        Format::DiagnosticVisit => Price::VisitFee(amount()?),
        Format::Range => Price::Range {
            low: dollars(&fields.low).ok_or(PriceProblem::Range)?,
            high: dollars(&fields.high).ok_or(PriceProblem::Range)?,
            on_site: fields.on_site,
        },
        Format::Tiers => Price::Tiers(tiers(fields)?),
        Format::Monthly => return monthly(fields, amount()?),
    };
    Ok((price, None))
}

/// The recurring price: its period, and for a per-visit price the required schedule.
fn monthly(
    fields: &PriceFields,
    cents: u64,
) -> Result<(Price, Option<VisitSchedule>), PriceProblem> {
    let setup = match fields.setup.trim() {
        "" => None,
        s => Some(dollars(s).ok_or(PriceProblem::Setup)?),
    };
    let periods = [
        Period::Week,
        Period::Month,
        Period::Quarter,
        Period::Year,
        Period::Visit,
    ];
    let period = periods
        .into_iter()
        .find(|p| p.code() == fields.per)
        .ok_or(PriceProblem::Per)?;
    let (period, schedule) = match (period, fields.schedule.as_str()) {
        (Period::Visit, OTHER_SCHEDULE) => (Period::Month, None),
        (Period::Visit, code) => {
            let s = VisitSchedule::from_code(code).ok_or(PriceProblem::Schedule)?;
            (Period::Visit, Some(s))
        }
        (other, _) => (other, None),
    };
    let price = Price::Monthly {
        cents,
        period,
        setup,
    };
    Ok((price, schedule))
}

/// The packages with a name and a price; a half-filled one or none at all is a problem.
fn tiers(fields: &PriceFields) -> Result<Vec<(String, u64)>, PriceProblem> {
    let mut out = Vec::new();
    for (name, price) in &fields.tiers {
        match (name.trim(), price.trim()) {
            ("", "") => {}
            (n, p) if !n.is_empty() => {
                out.push((n.to_owned(), dollars(p).ok_or(PriceProblem::Tiers)?));
            }
            _ => return Err(PriceProblem::Tiers),
        }
    }
    if out.is_empty() {
        return Err(PriceProblem::Tiers);
    }
    Ok(out)
}

/// REC-010: stores the quote's visit schedule (none clears it), in the caller's transaction.
pub(crate) async fn store_schedule(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    schedule: Option<VisitSchedule>,
) -> Result<(), QuotesError> {
    sqlx::query!(
        "UPDATE quotes.quote SET visit_schedule = $3 WHERE job_id = $1 AND business_id = $2",
        job,
        business,
        schedule.map(VisitSchedule::code)
    )
    .execute(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(())
}

/// REC-010, D-2333: the stored visit schedule of `business`'s quote on `job`, for the pick's
/// pricing (T341); none for no quote or a price that is not per visit.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn visit_schedule_of(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
) -> Result<Option<VisitSchedule>, QuotesError> {
    let code = sqlx::query_scalar!(
        "SELECT visit_schedule FROM quotes.quote WHERE job_id = $1 AND business_id = $2",
        job,
        business
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?
    .flatten();
    Ok(code.as_deref().and_then(VisitSchedule::from_code))
}
