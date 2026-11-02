//! Property tests of the fee rules (TD-FEE-01, TD-FEE-02, TD-FEE-27, TD-FEE-28; FEE-001 to
//! FEE-004, FEE-008, FEE-043, FEE-045; rust brain R17): the fee is always exactly $25, $75 or
//! $199, never falls as the basis rises, a range's basis lies between its ends, an hourly quote
//! is its rate-times-hours range, a visit fee never moves the fee and a fee-free job pays $0.

use proptest::prelude::*;

use crate::api::{Fee, FeeJob, FeeQuote, Tier, VisitTier, fee_for};

/// The largest amount a quote can come to: $10 million, in cents (as `quotes` checks).
const MAX_CENTS: u64 = 1_000_000_000;

const PRICED: FeeJob = FeeJob {
    diagnostic_visit: false,
    visit_tier: VisitTier::DEFAULT,
    fee_free: false,
};

fn charged(quote: FeeQuote) -> Fee {
    fee_for(quote, PRICED).expect("priced")
}

proptest! {
    /// TD-FEE-01 (FEE-001, FEE-002, FEE-008): the charged amount is exactly one tier and never
    /// decreases as the basis rises.
    #[test]
    fn td_fee_01_fee_is_a_tier_and_monotone(a in 0..=MAX_CENTS, b in 0..=MAX_CENTS) {
        let (low, high) = (a.min(b), a.max(b));
        let fee_low = charged(FeeQuote::Fixed(low)).cents();
        let fee_high = charged(FeeQuote::Fixed(high)).cents();
        prop_assert!([2_500, 7_500, 19_900].contains(&fee_low));
        prop_assert!(fee_low <= fee_high);
        prop_assert!(fee_high <= Tier::Large.cents());
    }

    /// TD-FEE-02 (FEE-003, D-033, D-441): a range's basis lies between its ends and is its
    /// midpoint rounded down to the cent.
    #[test]
    fn td_fee_02_range_basis_between_ends(a in 0..=MAX_CENTS, b in 0..=MAX_CENTS) {
        let (low, high) = (a.min(b), a.max(b));
        let basis = charged(FeeQuote::Range { low, high }).basis().expect("basis");
        prop_assert!(low <= basis && basis <= high);
        prop_assert_eq!(basis, (low + high) / 2);
    }

    /// TD-FEE-27 (FEE-043): an hourly quote with hours is the range rate times low to rate
    /// times high hours; a diagnostic-visit quote's visit fee never changes the fee (FEE-004).
    #[test]
    fn td_fee_27_hourly_is_a_range_and_visit_fee_never_counts(
        rate in 1..=100_000_u64,
        lo in 0..=200_u32,
        extra in 0..=200_u32,
        visit_fee in 0..=MAX_CENTS,
    ) {
        let hours = Some((lo, lo + extra));
        let hourly = charged(FeeQuote::Hourly { rate, hours });
        let range = FeeQuote::Range {
            low: rate * u64::from(lo),
            high: rate * u64::from(lo + extra),
        };
        prop_assert_eq!(hourly, charged(range));
        let visit = FeeJob { diagnostic_visit: true, ..PRICED };
        let fee = fee_for(FeeQuote::DiagnosticVisit { visit_fee }, visit).expect("set tier");
        prop_assert_eq!(fee, Fee::Charged { basis: None, tier: Tier::Small });
    }

    /// TD-FEE-28 (FEE-045): a fee-free job type pays $0 whatever the amount.
    #[test]
    fn td_fee_28_fee_free_is_always_zero(amount in 0..=MAX_CENTS) {
        let job = FeeJob { fee_free: true, ..PRICED };
        let fee = fee_for(FeeQuote::Fixed(amount), job).expect("priced");
        prop_assert_eq!(fee, Fee::FeeFree { basis: Some(amount) });
        prop_assert_eq!(fee.cents(), 0);
    }
}
