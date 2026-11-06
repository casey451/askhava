//! The just-under-a-tier flag for Casey's monthly review (FEE-033, D-070, D-221).
//!
//! Pure rule: [`tier_edge_flag`] takes a business's fee bases (the newest first, as `payments`
//! fixed them at each pick, FEE-005) and a [`TierEdgeRule`], and answers a [`TierEdgeFlag`]. A
//! flag is only shown to Casey: nothing here changes a fee, a pick or a business, and nothing is
//! sent to the business. The bases never come from `quotes` tables (D-491).

use thiserror::Error;

/// The tier edges a basis can sit just under: $500.00 and $1,500.00, in cents (FEE-001, FEE-033).
pub const TIER_EDGES: [u64; 2] = [50_000, 150_000];
/// How many of the business's latest bases are looked at (FEE-033).
pub const LAST_BASES: usize = 10;

/// A [`TierEdgeRule`] setting outside its range.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum FlagError {
    /// The percent under an edge is not 1 to 50.
    #[error("the percent under a tier edge must be 1 to 50")]
    Percent,
    /// The count is not 1 to 10.
    #[error("the count of quotes near an edge must be 1 to 10")]
    Count,
}

/// The two FEE-033 settings: how far under an edge counts, and how many of the last 10 bases must
/// be that close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierEdgeRule {
    under_percent: u8,
    at_least: u8,
}

impl TierEdgeRule {
    /// D-070: within 5% under an edge, at least 3 of the last 10.
    pub const DEFAULT: Self = Self {
        under_percent: 5,
        at_least: 3,
    };

    /// FEE-033: a rule from the two settings.
    ///
    /// # Errors
    /// [`FlagError::Percent`] outside 1 to 50; [`FlagError::Count`] outside 1 to 10.
    pub fn new(under_percent: u8, at_least: u8) -> Result<Self, FlagError> {
        if !(1..=50).contains(&under_percent) {
            return Err(FlagError::Percent);
        }
        if !(1..=10).contains(&at_least) {
            return Err(FlagError::Count);
        }
        Ok(Self {
            under_percent,
            at_least,
        })
    }

    /// FEE-033: whether `basis_cents` is within the percent under an edge: from $475.00 to
    /// $499.99 or $1,425.00 to $1,499.99 at 5%; the edge itself is not under it.
    #[must_use]
    pub fn near_edge(self, basis_cents: u64) -> bool {
        TIER_EDGES.iter().any(|&edge| {
            let from = edge - edge * u64::from(self.under_percent) / 100;
            (from..edge).contains(&basis_cents)
        })
    }
}

/// What the owner portal shows for one business (FEE-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierEdgeFlag {
    /// Fewer than the rule's count of the last 10 bases sit just under an edge.
    Clear,
    /// Flagged for Casey's monthly review; `near` of the last 10 bases sit just under an edge.
    Flagged {
        /// How many of the last 10 bases sit just under an edge.
        near: usize,
    },
}

/// FEE-033, D-070: the flag for a business whose bases, newest first, are `bases`; only the first
/// [`LAST_BASES`] count. A flag only: no automatic action and nothing sent to the business.
#[must_use]
pub fn tier_edge_flag(rule: TierEdgeRule, bases: &[u64]) -> TierEdgeFlag {
    let near = bases
        .iter()
        .take(LAST_BASES)
        .filter(|&&basis| rule.near_edge(basis))
        .count();
    if near >= usize::from(rule.at_least) {
        TierEdgeFlag::Flagged { near }
    } else {
        TierEdgeFlag::Clear
    }
}
