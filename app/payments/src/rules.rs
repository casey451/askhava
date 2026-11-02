//! Introduction fee by job size for a chosen quote (FEE-001 to FEE-008, FEE-039,
//! FEE-040, FEE-043 to FEE-045, REC-001 to REC-006, REC-010, D-009, D-033, D-399, D-403).
//!
//! Pure: a [`FeeQuote`] and its [`FeeJob`] in, a [`Fee`] out. The fee is always exactly one of
//! the three tiers, $25, $75 or $199, never a percentage of the job and never with tax on top
//! (FEE-002, FEE-008); a fee-free job type pays $0 (FEE-045). [`fee_for`] prices a one-time
//! quote; [`period_fee_for`] (T188) adds the period: the 4-month rule and the recurring cap
//! (REC-002, FEE-039). No input has a setup amount, so a setup charge never reaches a basis
//! (FEE-044, D-1400). [`price_lock_until`] is the early joiner's price lock (FEE-040). T060:
//! [`fee_at_pick`] fixes the fee at the pick from the chosen quote alone as a [`PickedFee`]
//! (FEE-005), in the [`FeeMode`] of the owner's switch, whose [`FeeTerms`] name the card
//! authorization a business must accept before a charge (FEE-007, D-072). Nothing here talks to
//! Stripe or the database.

use jiff::{SignedDuration, Timestamp, ToSpan};

/// The edge of the $75 tier: a basis of $500.00 or more is at least $75 (FEE-001, D-009).
const MEDIUM_FROM_CENTS: u64 = 50_000;
/// The top of the $75 tier, included: a basis over $1,500.00 is $199 (FEE-001, D-009).
const MEDIUM_UP_TO_CENTS: u64 = 150_000;
/// REC-002, REC-006: the one month multiplier for recurring services. Never another number.
pub const RECURRING_MONTHS: u128 = 4;
/// FEE-040, D-197: a business that joins before this long after launch gets the price lock.
const LOCK_JOIN_WINDOW: SignedDuration = SignedDuration::from_hours(90 * 24);
/// FEE-040, D-465: how long the price lock runs, in calendar months (Arizona time).
const LOCK_MONTHS: i64 = 6;

/// The three introduction fees by job size (FEE-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// $25: a basis under $500.00.
    Small,
    /// $75: a basis from $500.00 up to and including $1,500.00.
    Medium,
    /// $199: a basis over $1,500.00.
    Large,
}

impl Tier {
    /// FEE-001, FEE-008: the tier's fee in cents, tax included, nothing added.
    #[must_use]
    pub const fn cents(self) -> u64 {
        match self {
            Self::Small => 2_500,
            Self::Medium => 7_500,
            Self::Large => 19_900,
        }
    }

    /// FEE-001, FEE-002, D-009: the tier of a fee basis in whole cents; edges at $500.00 (in the
    /// $75 tier) and $1,500.00 (still the $75 tier). The basis picks a tier, never a percentage.
    #[must_use]
    pub const fn for_basis(basis_cents: u64) -> Self {
        if basis_cents < MEDIUM_FROM_CENTS {
            Self::Small
        } else if basis_cents <= MEDIUM_UP_TO_CENTS {
            Self::Medium
        } else {
            Self::Large
        }
    }
}

/// The set tier of a `diagnostic_visit` category: $25 or $75, never $199 (FEE-004, D-418).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisitTier(Tier);

impl VisitTier {
    /// The default set tier, $25 (FEE-004, D-399).
    pub const DEFAULT: Self = Self(Tier::Small);

    /// FEE-004, D-418: the category's stored tier in dollars (the catalog's `trip_fee_tier`).
    ///
    /// # Errors
    ///
    /// [`FeeError::VisitTierAboveCap`] for $199, [`FeeError::NotATier`] for any other amount
    /// that is not $25 or $75.
    pub const fn from_dollars(dollars: i32) -> Result<Self, FeeError> {
        match dollars {
            25 => Ok(Self(Tier::Small)),
            75 => Ok(Self(Tier::Medium)),
            199 => Err(FeeError::VisitTierAboveCap),
            _ => Err(FeeError::NotATier),
        }
    }

    /// The tier it stands for.
    #[must_use]
    pub const fn tier(self) -> Tier {
        self.0
    }
}

/// The chosen one-time quote as the fee sees it, amounts in whole cents (PICK-002, FEE-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeQuote {
    /// One price.
    Fixed(u64),
    /// A price range, low to high.
    Range {
        /// The low amount.
        low: u64,
        /// The high amount.
        high: u64,
    },
    /// A rate per hour and the estimated hours, low and high (equal for one number), or none.
    Hourly {
        /// The rate per hour.
        rate: u64,
        /// The estimated whole hours, or none for no estimate.
        hours: Option<(u32, u32)>,
    },
    /// A visit fee plus an estimate on site; the visit fee never sets the fee (FEE-004).
    DiagnosticVisit {
        /// The visit fee as written (may be $0).
        visit_fee: u64,
    },
}

/// What the fee needs from the job's category and job type (FEE-004, FEE-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeJob {
    /// The job's quote format is `diagnostic_visit` (PICK-053).
    pub diagnostic_visit: bool,
    /// The category's set tier for a diagnostic-visit quote.
    pub visit_tier: VisitTier,
    /// The job type is one of the catalog's `fee_free_job_types` (FEE-045).
    pub fee_free: bool,
}

/// The introduction fee by job size for a chosen quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fee {
    /// Charged at a tier; `basis` is none for a diagnostic-visit quote's set tier.
    Charged {
        /// The fee basis in whole cents, rounded down (D-033).
        basis: Option<u64>,
        /// The tier charged.
        tier: Tier,
    },
    /// A fee-free job type: $0, recorded as "fee-free job type" (FEE-045).
    FeeFree {
        /// The fee basis the quote would have had.
        basis: Option<u64>,
    },
}

impl Fee {
    /// FEE-008, FEE-045: the amount charged in cents: exactly the tier, or 0 for fee-free.
    #[must_use]
    pub const fn cents(&self) -> u64 {
        match self {
            Self::Charged { tier, .. } => tier.cents(),
            Self::FeeFree { .. } => 0,
        }
    }

    /// The fee basis in whole cents, none for a diagnostic-visit quote.
    #[must_use]
    pub const fn basis(&self) -> Option<u64> {
        match self {
            Self::Charged { basis, .. } | Self::FeeFree { basis } => *basis,
        }
    }
}

/// Why a quote has no fee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeeError {
    /// An hourly quote with no hours outside a `diagnostic_visit` job goes back for an estimate
    /// (FEE-043).
    #[error("an hourly quote needs estimated hours in this job")]
    NeedsHours,
    /// A diagnostic-visit quote outside a `diagnostic_visit` job (FEE-004, PICK-053).
    #[error("a diagnostic-visit quote is not taken in this job")]
    NotDiagnosticVisit,
    /// A low above its high, or a rate times hours too large to hold.
    #[error("the quote amounts do not make a price")]
    Amounts,
    /// A diagnostic-visit set tier of $199 (FEE-004, D-418).
    #[error("a diagnostic-visit set tier can never be above $75")]
    VisitTierAboveCap,
    /// A set tier that is not one of the fees.
    #[error("not an introduction fee tier")]
    NotATier,
    /// A recurring-service quote with no period goes back to the business to state one
    /// (REC-010, D-159).
    #[error("a recurring-service quote needs a period")]
    NeedsPeriod,
    /// A period on a quote in a category that is not recurring (D-037).
    #[error("a period is not taken in a category that is not recurring")]
    PeriodNotRecurring,
    /// A period on an hourly or diagnostic-visit quote: only a price or a range recurs.
    #[error("only a fixed price or a range can have a period")]
    NotRecurringPrice,
}

/// How often a recurring price is stated; a per-visit price with a weekly visit schedule is
/// `Weekly` (REC-004, REC-010, D-010, D-159). Only these five are taken: a per-visit price on
/// any other schedule is converted by the caller to a monthly amount and passed as `Monthly`
/// (D-1407).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// Per week, or per visit with weekly visits: × 52 ÷ 12 a month.
    Weekly,
    /// Every two weeks: × 26 ÷ 12 a month.
    EveryTwoWeeks,
    /// Per month.
    Monthly,
    /// Per quarter: ÷ 3 a month.
    Quarterly,
    /// Per year: ÷ 12 a month.
    Yearly,
}

impl Period {
    /// REC-004, D-010: the monthly amount is the price × the first ÷ the second.
    const fn per_month(self) -> (u128, u128) {
        match self {
            Self::Weekly => (52, 12),
            Self::EveryTwoWeeks => (26, 12),
            Self::Monthly => (1, 1),
            Self::Quarterly => (1, 3),
            Self::Yearly => (1, 12),
        }
    }
}

/// What the job's category and job type make of a period (REC-001, REC-010, FEE-039, D-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// The category is not recurring: a period is an error.
    NotRecurring,
    /// A recurring category's job type listed in its `one_time_job_types`: no period is face
    /// value, a period is 4 months, and it is never capped (D-265).
    OneTimeJobType,
    /// Any other job type in a recurring category: a period is required and the fee is capped.
    RecurringJobType,
}

impl Service {
    /// REC-001, REC-010: from the category's `recurring` flag and whether the job type is one of
    /// its `one_time_job_types`.
    #[must_use]
    pub const fn of(category_recurring: bool, one_time_job_type: bool) -> Self {
        match (category_recurring, one_time_job_type) {
            (false, _) => Self::NotRecurring,
            (true, true) => Self::OneTimeJobType,
            (true, false) => Self::RecurringJobType,
        }
    }
}

/// The highest tier a recurring job type pays, a setting: $75 by default (FEE-039, D-196).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecurringCap(Tier);

impl RecurringCap {
    /// FEE-039, D-196: the default cap, $75.
    pub const DEFAULT: Self = Self(Tier::Medium);

    /// FEE-039: the cap setting in dollars; it is always one of the fees.
    ///
    /// # Errors
    ///
    /// [`FeeError::NotATier`] for an amount that is not $25, $75 or $199.
    pub const fn from_dollars(dollars: i32) -> Result<Self, FeeError> {
        match dollars {
            25 => Ok(Self(Tier::Small)),
            75 => Ok(Self(Tier::Medium)),
            199 => Ok(Self(Tier::Large)),
            _ => Err(FeeError::NotATier),
        }
    }
}

/// The job as a quote with a period sees it (REC-001, FEE-039).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodJob {
    /// The one-time fee inputs: format, set tier and fee-free flag.
    pub job: FeeJob,
    /// What the category and job type make of a period.
    pub service: Service,
    /// The recurring cap setting.
    pub cap: RecurringCap,
}

/// What a business that joined early is charged at most (FEE-040, D-197, D-261, D-288, D-465).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockDates {
    /// When Ask Hava launched.
    pub launch: Timestamp,
    /// The business's join date (kept through a waitlist, D-175).
    pub joined: Timestamp,
    /// When its first trade was switched on.
    pub first_trade_on: Timestamp,
    /// For a business that waited on its city-list match, its activation; none otherwise.
    pub activated: Option<Timestamp>,
}

/// FEE-003, D-033: the midpoint of `low` to `high`, a half cent rounded down; never overflows.
const fn midpoint(low: u64, high: u64) -> u64 {
    low / 2 + high / 2 + (low % 2 + high % 2) / 2
}

/// REC-002, REC-004, REC-005, D-010, D-033: 4 months of a fixed price or of a range's midpoint,
/// worked exactly and rounded down to the cent once, at the end.
fn four_months(quote: FeeQuote, period: Period) -> Result<u64, FeeError> {
    let (sum, halves) = match quote {
        FeeQuote::Fixed(amount) => (u128::from(amount), 1),
        FeeQuote::Range { low, high } if low <= high => (u128::from(low) + u128::from(high), 2),
        FeeQuote::Range { .. } => return Err(FeeError::Amounts),
        FeeQuote::Hourly { .. } | FeeQuote::DiagnosticVisit { .. } => {
            return Err(FeeError::NotRecurringPrice);
        }
    };
    let (times, per) = period.per_month();
    u64::try_from(sum * times * RECURRING_MONTHS / (per * halves)).map_err(|_| FeeError::Amounts)
}

/// FEE-003, FEE-043, D-033: the fee basis of a priced quote, or none for a set-tier visit.
fn basis_of(quote: FeeQuote, job: FeeJob) -> Result<Option<u64>, FeeError> {
    let range = |low: u64, high: u64| {
        if low <= high {
            Ok(Some(midpoint(low, high)))
        } else {
            Err(FeeError::Amounts)
        }
    };
    match quote {
        FeeQuote::Fixed(amount) => Ok(Some(amount)),
        FeeQuote::Range { low, high } => range(low, high),
        FeeQuote::Hourly {
            rate,
            hours: Some((lo, hi)),
        } => {
            let times = |h: u32| rate.checked_mul(u64::from(h)).ok_or(FeeError::Amounts);
            range(times(lo)?, times(hi)?)
        }
        FeeQuote::Hourly { hours: None, .. } if !job.diagnostic_visit => Err(FeeError::NeedsHours),
        FeeQuote::DiagnosticVisit { .. } if !job.diagnostic_visit => {
            Err(FeeError::NotDiagnosticVisit)
        }
        FeeQuote::Hourly { hours: None, .. } | FeeQuote::DiagnosticVisit { .. } => Ok(None),
    }
}

/// FEE-001 to FEE-004, FEE-008, FEE-043, FEE-045, D-009, D-033, D-399, D-403: the introduction
/// fee by job size of a chosen one-time quote. A range (and an hourly quote with hours, read as
/// rate times the low to rate times the high hours) is priced at its midpoint rounded down to the
/// cent; a diagnostic-visit quote, and an hourly quote with no hours, pays the category's set
/// tier in a `diagnostic_visit` job only; a fee-free job type pays $0 whatever the quote.
///
/// # Errors
///
/// [`FeeError::NeedsHours`], [`FeeError::NotDiagnosticVisit`] or [`FeeError::Amounts`] for a
/// quote the job does not take.
pub fn fee_for(quote: FeeQuote, job: FeeJob) -> Result<Fee, FeeError> {
    let basis = basis_of(quote, job)?;
    if job.fee_free {
        return Ok(Fee::FeeFree { basis });
    }
    let tier = basis.map_or(job.visit_tier.tier(), Tier::for_basis);
    Ok(Fee::Charged { basis, tier })
}

/// FEE-039, FEE-044, REC-002 to REC-006, REC-010, D-037, D-159, D-196, D-265: the introduction
/// fee by job size of a chosen quote and its period. With no period: a recurring job type goes
/// back for one, any other is [`fee_for`]'s one-time face value. With a period: an error outside
/// a recurring category, else the tier of 4 months of the monthly price, a recurring job type
/// never above `cap`; a fee-free job type still pays $0.
///
/// # Errors
///
/// [`FeeError::NeedsPeriod`], [`FeeError::PeriodNotRecurring`], [`FeeError::NotRecurringPrice`],
/// [`FeeError::Amounts`], or [`fee_for`]'s errors with no period.
pub fn period_fee_for(
    quote: FeeQuote,
    period: Option<Period>,
    job: PeriodJob,
) -> Result<Fee, FeeError> {
    let Some(period) = period else {
        if job.service == Service::RecurringJobType {
            return Err(FeeError::NeedsPeriod);
        }
        return fee_for(quote, job.job);
    };
    if job.service == Service::NotRecurring {
        return Err(FeeError::PeriodNotRecurring);
    }
    let basis = four_months(quote, period)?;
    if job.job.fee_free {
        return Ok(Fee::FeeFree { basis: Some(basis) });
    }
    let tier = Tier::for_basis(basis);
    let tier = if job.service == Service::RecurringJobType {
        tier.min(job.cap.0)
    } else {
        tier
    };
    Ok(Fee::Charged {
        basis: Some(basis),
        tier,
    })
}

/// FEE-040, D-197, D-261, D-288, D-465: when the early joiner's price lock ends, or none for a
/// business that joined 90 days or more after launch. The lock runs 6 calendar months (Arizona
/// time) from the latest of the join date, the first trade's switch-on and any activation.
#[must_use]
pub fn price_lock_until(dates: LockDates) -> Option<Timestamp> {
    let window_end = dates.launch.checked_add(LOCK_JOIN_WINDOW).ok()?;
    if dates.joined >= window_end {
        return None;
    }
    let start = dates
        .joined
        .max(dates.first_trade_on)
        .max(dates.activated.unwrap_or(dates.joined));
    let end = askhava_core::api::arizona_time(start).checked_add(LOCK_MONTHS.months());
    end.ok().map(|at| at.timestamp())
}

/// FEE-040, D-465: the amount charged at `at` for a fee whose current amount is `current` and
/// whose amount on the join date was `on_join`: never more than `on_join` while the lock runs.
#[must_use]
pub fn locked_cents(
    lock_until: Option<Timestamp>,
    at: Timestamp,
    current: u64,
    on_join: u64,
) -> u64 {
    match lock_until {
        Some(until) if at < until => current.min(on_join),
        Some(_) | None => current,
    }
}

/// How the introduction fee is set, an owner-portal switch (FEE-007, D-072).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeMode {
    /// The fee is the tier of the chosen quote's basis (FEE-001).
    Tiers,
    /// The fee is one flat tier per job type, whatever the quote (FEE-007).
    Flat,
}

impl FeeMode {
    /// The code name logged in the audit row of a switch (PRIV-028).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Tiers => "tiers",
            Self::Flat => "flat",
        }
    }
}

/// FEE-007, D-072: the fee mode in force and the card-authorization version a business must have
/// accepted before Ask Hava may charge it under that mode. Every mode change raises the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeTerms {
    mode: FeeMode,
    authorization_version: i32,
}

impl FeeTerms {
    /// The terms as stored by the owner-portal setting (the mode and its required version).
    #[must_use]
    pub const fn new(mode: FeeMode, authorization_version: i32) -> Self {
        Self {
            mode,
            authorization_version,
        }
    }

    /// The fee mode in force.
    #[must_use]
    pub const fn mode(self) -> FeeMode {
        self.mode
    }

    /// The lowest authorization version a business must have accepted to be charged.
    #[must_use]
    pub const fn authorization_version(self) -> i32 {
        self.authorization_version
    }

    /// FEE-007, D-072: the terms after switching to `to`, with the next authorization version, so
    /// every business accepts again before its next charge; none when `to` is the mode in force
    /// or the version cannot rise.
    #[must_use]
    pub fn switched(self, to: FeeMode) -> Option<Self> {
        if to == self.mode {
            return None;
        }
        let version = self.authorization_version.checked_add(1)?;
        Some(Self::new(to, version))
    }
}

/// FEE-007, D-072: a job type's flat fee: the owner's edit, else its category's set tier.
#[must_use]
pub fn flat_tier(set_tier: VisitTier, edited: Option<Tier>) -> Tier {
    edited.unwrap_or(set_tier.tier())
}

/// The introduction fee by job size fixed at the homeowner's pick (FEE-005); only
/// [`fee_at_pick`] makes one, and nothing changes it afterwards: not an AI flag, not an edit to
/// another quote, not the final bill or a PROBLEM reply (FEE-006, JOB-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickedFee {
    fee: Fee,
    mode: FeeMode,
}

impl PickedFee {
    /// The fee fixed at the pick.
    #[must_use]
    pub const fn fee(&self) -> Fee {
        self.fee
    }

    /// The fee mode in force at the pick.
    #[must_use]
    pub const fn mode(&self) -> FeeMode {
        self.mode
    }

    /// FEE-006: the amount charged at Confirm, in cents.
    #[must_use]
    pub const fn cents(&self) -> u64 {
        self.fee.cents()
    }
}

/// FEE-005, FEE-007, D-009, D-072: fixes the fee at the pick from the chosen quote alone (its
/// amounts and period, the job's category and job type, the fee mode); no AI flag and no other
/// quote is an input. In flat mode a charged fee becomes the job type's [`flat_tier`]
/// (`flat_edit`, else the category's set tier); a fee-free job type still pays $0 (FEE-045).
///
/// # Errors
///
/// [`period_fee_for`]'s errors: the chosen quote must still be one the job takes.
pub fn fee_at_pick(
    chosen: FeeQuote,
    period: Option<Period>,
    job: PeriodJob,
    mode: FeeMode,
    flat_edit: Option<Tier>,
) -> Result<PickedFee, FeeError> {
    let fee = match (period_fee_for(chosen, period, job)?, mode) {
        (Fee::Charged { basis, .. }, FeeMode::Flat) => Fee::Charged {
            basis,
            tier: flat_tier(job.job.visit_tier, flat_edit),
        },
        (fee, FeeMode::Tiers | FeeMode::Flat) => fee,
    };
    Ok(PickedFee { fee, mode })
}
