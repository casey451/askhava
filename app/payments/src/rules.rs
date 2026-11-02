//! Introduction fee by job size for a chosen one-time quote (FEE-001 to FEE-004, FEE-008,
//! FEE-043 to FEE-045, D-009, D-033, D-399, D-403).
//!
//! Pure: a [`FeeQuote`] and its [`FeeJob`] in, a [`Fee`] out. The fee is always exactly one of
//! the three tiers, $25, $75 or $199, never a percentage of the job and never with tax on top
//! (FEE-002, FEE-008); a fee-free job type pays $0 (FEE-045). Recurring quotes (the 4-month rule,
//! the $75 cap and the setup charge left out of the basis, REC-002, FEE-039, FEE-044) are T188's;
//! this module has no recurring input and no setup amount, so no setup charge can reach a basis
//! here (D-1400). Nothing here talks to Stripe or the database.

/// The edge of the $75 tier: a basis of $500.00 or more is at least $75 (FEE-001, D-009).
const MEDIUM_FROM_CENTS: u64 = 50_000;
/// The top of the $75 tier, included: a basis over $1,500.00 is $199 (FEE-001, D-009).
const MEDIUM_UP_TO_CENTS: u64 = 150_000;

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
}

/// FEE-003, D-033: the midpoint of `low` to `high`, a half cent rounded down; never overflows.
const fn midpoint(low: u64, high: u64) -> u64 {
    low / 2 + high / 2 + (low % 2 + high % 2) / 2
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
