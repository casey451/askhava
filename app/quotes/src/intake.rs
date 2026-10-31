//! Taking in a business's quote: the 24-hour window, the checks a quote passes and the stored
//! record (PICK-001, PICK-002, PICK-007, FEE-043, FEE-044; D-1048 to D-1051).
//!
//! [`parse_quote`] is pure: it turns the form's [`QuoteForm`] into a checked [`Quote`] or a
//! [`Bounce`] for the business. It never computes a fee: the fee preview is `payments`' (T148,
//! D-491). [`submit_quote`] checks the business's own window and the job's status, stores the
//! quote and schedules the every-window-ended check (`crate::handlers`).

use askhava_core::api::Guarded;
use askhava_licensing::api::{LicenseKind, QuotePrice, QuoteRefusal, QuoteSubmission, check_quote};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::gate::final_gate;
use crate::models::{self, QuotesError};

/// How long each business may quote after its own card was sent, every day of the week (PICK-001).
pub const QUOTE_WINDOW: SignedDuration = SignedDuration::from_hours(24);

/// PICK-001, D-042: true while a card sent at `sent_at` still takes a quote at `now`; the window
/// shuts at exactly 24 hours (TD-PICK-01, D-101).
#[must_use]
pub fn window_open(sent_at: Timestamp, now: Timestamp) -> bool {
    now < sent_at.saturating_add(QUOTE_WINDOW).unwrap_or(sent_at)
}

/// A category's or job type's quote format (PICK-053), as `docs/catalog/categories.yaml` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// A low and a high amount.
    Range,
    /// A recurring price and its period.
    Monthly,
    /// Up to 3 named packages with prices.
    Tiers,
    /// One price.
    Firm,
    /// A visit fee plus an estimate on site (FEE-004).
    DiagnosticVisit,
}

impl Format {
    /// The format named by a catalog `quote_format` code, or none.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        let all = [
            Self::Range,
            Self::Monthly,
            Self::Tiers,
            Self::Firm,
            Self::DiagnosticVisit,
        ];
        all.into_iter().find(|f| f.code() == code)
    }

    /// The catalog code, as `quotes.quote.format` stores it.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Range => "range",
            Self::Monthly => "monthly",
            Self::Tiers => "tiers",
            Self::Firm => "firm",
            Self::DiagnosticVisit => "diagnostic_visit",
        }
    }
}

/// The period of a recurring price (D-399).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// Per visit.
    Visit,
    /// Per week.
    Week,
    /// Per month.
    Month,
    /// Per quarter.
    Quarter,
    /// Per year.
    Year,
}

impl Period {
    /// The code `quotes.quote.period` stores.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Visit => "visit",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
        }
    }
}

/// Part of the day of the earliest visit (PICK-040, D-283).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayPart {
    /// Morning.
    Morning,
    /// Afternoon.
    Afternoon,
    /// Evening.
    Evening,
    /// Tonight.
    Tonight,
}

impl DayPart {
    /// The code `quotes.quote.visit_part` stores.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Morning => "morning",
            Self::Afternoon => "afternoon",
            Self::Evening => "evening",
            Self::Tonight => "tonight",
        }
    }
}

/// The price as the business entered it, in cents (PICK-002, PICK-053, FEE-043, FEE-044). Its
/// `Debug` shows tier prices but never tier names, which the business typed (rust brain R6).
#[derive(Clone, PartialEq, Eq)]
pub enum Price {
    /// One price.
    Firm(u64),
    /// Low and high, and the "Range to be confirmed on site" tick.
    Range {
        /// The low amount.
        low: u64,
        /// The high amount.
        high: u64,
        /// The "Range to be confirmed on site" tick (PICK-053).
        on_site: bool,
    },
    /// A recurring price, its period and any one-time setup charge (FEE-044).
    Monthly {
        /// The recurring price.
        cents: u64,
        /// What the price is per.
        period: Period,
        /// Any one-time setup charge, never part of the fee basis (FEE-044).
        setup: Option<u64>,
    },
    /// Up to 3 named packages with prices.
    Tiers(Vec<(String, u64)>),
    /// A visit fee, the estimate given on site.
    VisitFee(u64),
    /// An hourly rate and its estimated hours, low and high (FEE-043).
    Hourly {
        /// The rate per hour.
        rate: u64,
        /// The estimated hours, low and high (equal for one number); none for no estimate.
        hours: Option<(u32, u32)>,
    },
}

impl std::fmt::Debug for Price {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Firm(v) => f.debug_tuple("Firm").field(v).finish(),
            Self::Range { low, high, on_site } => f
                .debug_struct("Range")
                .field("low", low)
                .field("high", high)
                .field("on_site", on_site)
                .finish(),
            Self::Monthly {
                cents,
                period,
                setup,
            } => f
                .debug_struct("Monthly")
                .field("cents", cents)
                .field("period", period)
                .field("setup", setup)
                .finish(),
            Self::Tiers(t) => {
                let cents: Vec<u64> = t.iter().map(|(_, c)| *c).collect();
                f.debug_tuple("Tiers").field(&cents).finish()
            }
            Self::VisitFee(v) => f.debug_tuple("VisitFee").field(v).finish(),
            Self::Hourly { rate, hours } => f
                .debug_struct("Hourly")
                .field("rate", rate)
                .field("hours", hours)
                .finish(),
        }
    }
}

/// What the quote form (or the read-back of a text quote) sends, before any check (PICK-002).
/// Its `Debug` leaves out the notes and original wording, which may hold the contact detail the
/// check refuses (PICK-007, rust brain R6).
#[derive(Clone, PartialEq, Eq)]
pub struct QuoteForm {
    /// The price.
    pub price: Price,
    /// The notes box (PICK-052).
    pub notes: String,
    /// The business's original wording, kept as written (PICK-003).
    pub original: String,
    /// Whether the AI read the price from a text (PICK-006, D-149).
    pub ai_read: bool,
    /// The earliest visit: a date and a part of the day (PICK-040).
    pub earliest_visit: Option<(Date, DayPart)>,
}

impl std::fmt::Debug for QuoteForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuoteForm")
            .field("price", &self.price)
            .field("notes", &"<redacted>")
            .field("original", &"<redacted>")
            .field("ai_read", &self.ai_read)
            .field("earliest_visit", &self.earliest_visit)
            .finish()
    }
}

/// What a quote is checked against: the job's format and the handyman limit (PICK-053, LIC-019).
#[derive(Debug, Clone, Copy)]
pub struct QuoteRules {
    /// The job type's format, else its category's (PICK-053).
    pub format: Format,
    /// The category's `construction_type` flag.
    pub construction_type: bool,
    /// The category's license rule.
    pub license: LicenseKind,
    /// The business quotes under the handyman exemption (its card's handyman flag).
    pub handyman: bool,
    /// The handyman certification box was ticked (LIC-025).
    pub certified: bool,
}

/// Why a quote goes back to the business. No variant carries the text (rust brain R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bounce {
    /// A phone number, link, email, spelled-out number or contact wording (PICK-007).
    Contact,
    /// A phrase on the carrier-restricted list (CARD-010).
    Restricted,
    /// The price does not use the job's quote format (PICK-053).
    WrongFormat,
    /// An hourly rate with no estimated hours outside a site-visit category (FEE-043).
    NeedsHours,
    /// An amount is zero, too large, or a low is not below its high.
    Amounts,
    /// The handyman limit or certification (LIC-019, LIC-025).
    Handyman(QuoteRefusal),
}

impl Bounce {
    /// The line the business reads on the quote form.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Contact => {
                "Please take out any phone number, email, link, or \"call me\" or \"text me\" line, including numbers written out as words. If the homeowner picks your quote, Ask Hava puts you in touch."
            }
            Self::Restricted => {
                "One word in your quote can't be sent in a text. Please reword it and send again."
            }
            Self::WrongFormat => {
                "This job type uses a set price format. Please put your price in the boxes shown."
            }
            Self::NeedsHours => {
                "Please add how many hours you think it will take, or a range like 2 to 3 hours."
            }
            Self::Amounts => {
                "Please check the amounts. Each price must be more than $0, and the low price less than the high one."
            }
            Self::Handyman(QuoteRefusal::NotCertified) => {
                "Please check the handyman certification box before you send this quote."
            }
            Self::Handyman(_) => {
                "Under Arizona's handyman rule, this job has to stay under $1,000, parts and labor together, including the top of your price range. Please bring your quote under $1,000 to send it."
            }
        }
    }
}

/// A checked quote: only [`parse_quote`] makes one (rust brain R2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub(crate) format: Format,
    pub(crate) price: Price,
    pub(crate) form: QuoteForm,
}

impl Quote {
    /// The format stored: an hourly quote with hours is a range, without hours a site visit.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The low and high a range or an hourly quote stands for: rate times hours (FEE-043).
    #[must_use]
    pub fn range(&self) -> Option<(u64, u64)> {
        match self.price {
            Price::Range { low, high, .. } => Some((low, high)),
            Price::Hourly {
                rate,
                hours: Some((lo, hi)),
            } => Some((rate.checked_mul(lo.into())?, rate.checked_mul(hi.into())?)),
            _ => None,
        }
    }
}

/// PICK-002, PICK-007, PICK-053, FEE-043, FEE-044, LIC-019: checks `form` against `rules`.
///
/// # Errors
///
/// The first [`Bounce`] found: text first, then format, amounts and the handyman limit.
pub fn parse_quote(form: QuoteForm, rules: &QuoteRules) -> Result<Quote, Bounce> {
    let tiers = match &form.price {
        Price::Tiers(t) => t.iter().map(|(name, _)| name.as_str()).collect(),
        _ => Vec::new(),
    };
    for text in [form.notes.as_str(), form.original.as_str()]
        .into_iter()
        .chain(tiers)
    {
        text_bounce(text)?;
    }
    let format = format_of(&form.price, rules.format)?;
    let price = form.price.clone();
    let quote = Quote {
        format,
        price,
        form,
    };
    let top = top_amount(&quote)?;
    let submission = QuoteSubmission {
        construction_type: rules.construction_type,
        license: rules.license,
        handyman: rules.handyman,
        price: top.map_or(QuotePrice::TripFee, QuotePrice::Fixed),
        certified: rules.certified,
    };
    check_quote(&submission).map_err(Bounce::Handyman)?;
    Ok(quote)
}

/// PICK-007: a text bounces when the card gate (CARD-008) would cut any word of it, or when it
/// holds a carrier-restricted phrase (CARD-010); one gate decides both, so cards and quotes agree.
pub(crate) fn text_bounce(text: &str) -> Result<(), Bounce> {
    if askhava_catalog::api::restricted_match(text).is_some() {
        return Err(Bounce::Restricted);
    }
    let words = |t: &str| -> Vec<String> {
        let parts = t.split(|c: char| !c.is_alphanumeric());
        parts
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    if words(&final_gate(text)) == words(text) {
        Ok(())
    } else {
        Err(Bounce::Contact)
    }
}

/// PICK-053, FEE-043: the stored format, which must be the job's. An hourly quote with hours is
/// a range, taken in a `range` or `diagnostic_visit` job; without hours it is a site visit in a
/// `diagnostic_visit` job and goes back for hours in any other (D-1050).
fn format_of(price: &Price, job: Format) -> Result<Format, Bounce> {
    let given = match price {
        Price::Hourly { hours: None, .. } if job == Format::DiagnosticVisit => job,
        Price::Hourly { hours: None, .. } => return Err(Bounce::NeedsHours),
        Price::Hourly { .. } if matches!(job, Format::Range | Format::DiagnosticVisit) => {
            return Ok(Format::Range);
        }
        Price::Hourly { .. } => return Err(Bounce::WrongFormat),
        Price::Range { .. } => Format::Range,
        Price::VisitFee(_) => Format::DiagnosticVisit,
        Price::Firm(_) => Format::Firm,
        Price::Monthly { .. } => Format::Monthly,
        Price::Tiers(_) => Format::Tiers,
    };
    if given == job {
        Ok(given)
    } else {
        Err(Bounce::WrongFormat)
    }
}

/// The largest amount a quote can come to: $10 million, in cents.
const MAX_CENTS: u64 = 1_000_000_000;

/// The amount the handyman limit judges (LIC-019: the price, the top of a range or of a tier set;
/// a setup charge never counts, FEE-044), or none for a visit fee. Refuses a zero or too-large
/// amount, a range low not below its high, more hours low than high, or no tier or more than 3.
fn top_amount(quote: &Quote) -> Result<Option<u64>, Bounce> {
    let ok = |v: u64| {
        (v > 0 && v <= MAX_CENTS)
            .then_some(v)
            .ok_or(Bounce::Amounts)
    };
    if let Price::Monthly { setup: Some(s), .. } = quote.price {
        ok(s)?;
    }
    match &quote.price {
        Price::Firm(c) | Price::Monthly { cents: c, .. } => ok(*c).map(Some),
        Price::VisitFee(c)
        | Price::Hourly {
            rate: c,
            hours: None,
        } => ok(*c).map(|_| None),
        Price::Range { .. } | Price::Hourly { .. } => {
            let (low, high) = quote.range().ok_or(Bounce::Amounts)?;
            let hourly = matches!(quote.price, Price::Hourly { .. });
            let ordered = if hourly {
                ok(low)? <= ok(high)?
            } else {
                ok(low)? < ok(high)?
            };
            ordered.then_some(Some(high)).ok_or(Bounce::Amounts)
        }
        Price::Tiers(t) if (1..=3).contains(&t.len()) => {
            let top = t.iter().try_fold(0, |m, (_, c)| ok(*c).map(|c| m.max(c)));
            top.map(Some)
        }
        Price::Tiers(_) => Err(Bounce::Amounts),
    }
}

/// What [`submit_quote`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submitted {
    /// Stored (a second quote from the same business is an edit, PICK-051).
    Saved,
    /// The business holds no card for this job.
    NoCard,
    /// Its 24 hours have ended (PICK-001).
    WindowClosed,
    /// The job takes no quote now: the picked business has tapped Confirm, or it closed (D-043).
    NotTaking,
    /// The business already quoted and the job has had a pick, so its quote can't be edited
    /// (PICK-051); a first quote from another business is still taken (D-043).
    EditClosed,
}

/// PICK-001, PICK-002, D-042, D-043: stores `quote` from `business` on `job` at `now` when its own
/// card's 24 hours are still open and the job takes quotes (an edit only before any pick,
/// PICK-051), confirmed unless the AI read it
/// (PICK-006, D-149), and schedules the every-window-ended check (TXT-042, JOB-025). One
/// transaction; a Confirm committed after the status read orders as just after this quote (D-1051).
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`]; nothing is kept on an error.
pub async fn submit_quote(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    quote: &Quote,
    now: Timestamp,
) -> Result<Submitted, QuotesError> {
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    let Some(sent_at) = models::card_sent_at(&mut tx, job, business).await? else {
        return Ok(Submitted::NoCard);
    };
    if !window_open(sent_at, now) {
        return Ok(Submitted::WindowClosed);
    }
    let status = askhava_jobs::api::current(&mut tx, job)
        .await
        .map_err(|_| QuotesError::Job)?;
    if !status.status.takes_quotes() {
        return Ok(Submitted::NotTaking);
    }
    let (edit, picked) = models::edit_facts(&mut tx, job, business).await?;
    if edit && (picked || !status.status.takes_quote_edits()) {
        return Ok(Submitted::EditClosed);
    }
    models::upsert_quote(&mut tx, (job, business), quote, now).await?;
    crate::handlers::schedule(&mut tx, job).await?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(Submitted::Saved)
}

/// PICK-006, D-149: the business confirmed its AI-read quote on the read-back screen (T148), so
/// the homeowner may now see it. Moved on when there is no such quote, it is withdrawn, or the
/// job no longer takes quotes (`JobStatus::takes_quotes`, D-043, D-1049).
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`].
pub async fn confirm_quote(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Guarded<()>, QuotesError> {
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    let status = askhava_jobs::api::current(&mut tx, job)
        .await
        .map_err(|_| QuotesError::Job)?;
    if !status.status.takes_quotes() {
        return Ok(Guarded::MovedOn);
    }
    let done = models::confirm_quote(&mut tx, job, business).await?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(done)
}

/// JOB-025, D-1051: `picks` marks the quote of a failed pick unavailable at `at`; it is no longer
/// choosable at the every-window-ended check, and no quote on the job can be edited after it
/// (PICK-051). Moved on when there is no such quote or it is already marked.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn mark_unavailable(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    at: Timestamp,
) -> Result<Guarded<()>, QuotesError> {
    models::mark_unavailable(db, job, business, at).await
}

/// TD-PICK-02: the businesses whose quote on `job` the homeowner's page may show: confirmed by
/// the business and not withdrawn (PICK-002, PICK-006); an unconfirmed quote never appears.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn shown_quotes(db: &mut PgConnection, job: Uuid) -> Result<Vec<Uuid>, QuotesError> {
    models::shown_quotes(db, job).await
}
