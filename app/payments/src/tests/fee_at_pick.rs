//! The fee fixed at the pick and the fee-mode terms, case by case (TD-FEE-04 unit cases,
//! TD-FEE-06 rule cases; FEE-005, FEE-007, FEE-045, D-009, D-072): the chosen quote alone sets the
//! tier, whatever the AI read; flat mode takes the job type's edit or its category's set tier;
//! every mode change raises the authorization version. Fake amounts only.

use proptest::prelude::*;

use crate::api::{
    Fee, FeeError, FeeJob, FeeMode, FeeQuote, FeeTerms, Period, PeriodJob, RecurringCap, Service,
    Tier, VisitTier, fee_at_pick, flat_tier, period_fee_for,
};

/// A one-time job that is not `diagnostic_visit` and not fee-free, in a category whose set tier
/// is `visit_tier`.
const fn job(visit_tier: VisitTier, fee_free: bool) -> PeriodJob {
    PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier,
            fee_free,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    }
}

const PRICED: PeriodJob = job(VisitTier::DEFAULT, false);

/// A chosen quote as the pick sees it, with the AI's major flag beside it: the flag is part of
/// the request's AI reading, never an input of [`fee_at_pick`] (D-009).
fn picked(quote: FeeQuote, _ai_major: bool) -> Fee {
    fee_at_pick(quote, None, PRICED, FeeMode::Tiers, None)
        .expect("fee")
        .fee()
}

/// TD-FEE-04 (FEE-005, D-009): the tier comes from the chosen quote; the AI's major flag set or
/// unset gives the same fee.
#[test]
fn td_fee_04_the_chosen_quote_alone_sets_the_tier() {
    let small = FeeQuote::Fixed(49_999);
    assert_eq!(picked(small, true), picked(small, false));
    assert_eq!(picked(small, true).cents(), 2_500);
    let large = FeeQuote::Range {
        low: 150_000,
        high: 150_004,
    };
    assert_eq!(picked(large, false), picked(large, true));
    assert_eq!(picked(large, true).cents(), 19_900);
}

proptest! {
    /// TD-FEE-04 (FEE-005): in tiers mode the fee fixed at the pick is exactly the chosen
    /// quote's [`period_fee_for`] fee, whatever the AI flag.
    #[test]
    fn td_fee_04_tiers_mode_is_the_chosen_quote_fee(amount in 1_u64..1_000_000, major: bool) {
        let chosen = FeeQuote::Fixed(amount);
        let fixed = fee_at_pick(chosen, None, PRICED, FeeMode::Tiers, None).expect("fee");
        prop_assert_eq!(fixed.fee(), period_fee_for(chosen, None, PRICED).expect("fee"));
        prop_assert_eq!(fixed.fee(), picked(chosen, major));
        prop_assert_eq!(fixed.mode(), FeeMode::Tiers);
    }
}

/// TD-FEE-06 (FEE-007, D-072): flat mode charges the job type's edited tier, else its category's
/// set tier, whatever the quote; it keeps the basis and the quote's own refusals.
#[test]
fn td_fee_06_flat_mode_defaults_to_the_set_tier() {
    let medium = VisitTier::from_dollars(75).expect("tier");
    let quote = FeeQuote::Fixed(900_000);
    let flat = fee_at_pick(quote, None, job(medium, false), FeeMode::Flat, None).expect("fee");
    assert_eq!(
        flat.fee(),
        Fee::Charged {
            basis: Some(900_000),
            tier: Tier::Medium
        }
    );
    assert_eq!((flat.cents(), flat.mode()), (7_500, FeeMode::Flat));
    let edited = fee_at_pick(quote, None, PRICED, FeeMode::Flat, Some(Tier::Large));
    assert_eq!(edited.expect("fee").cents(), 19_900);
    assert_eq!(flat_tier(VisitTier::DEFAULT, None), Tier::Small);
    let hourly = FeeQuote::Hourly {
        rate: 9_000,
        hours: None,
    };
    let refused = fee_at_pick(hourly, None, PRICED, FeeMode::Flat, None);
    assert_eq!(refused, Err(FeeError::NeedsHours));
    let period = fee_at_pick(quote, Some(Period::Monthly), PRICED, FeeMode::Flat, None);
    assert_eq!(period, Err(FeeError::PeriodNotRecurring));
}

/// TD-FEE-06 (FEE-007, FEE-045): a fee-free job type pays $0 in flat mode too.
#[test]
fn td_fee_06_fee_free_stays_free_in_flat_mode() {
    let free = fee_at_pick(
        FeeQuote::Fixed(40_000),
        None,
        job(VisitTier::DEFAULT, true),
        FeeMode::Flat,
        Some(Tier::Large),
    );
    assert_eq!(
        free.expect("fee").fee(),
        Fee::FeeFree {
            basis: Some(40_000)
        }
    );
}

/// TD-FEE-06 (FEE-007, D-072): each mode change raises the authorization version; choosing the
/// mode in force, or a version that cannot rise, changes nothing.
#[test]
fn td_fee_06_every_switch_needs_a_new_authorization() {
    let start = FeeTerms::new(FeeMode::Tiers, 1);
    assert_eq!(start.switched(FeeMode::Tiers), None);
    let flat = start.switched(FeeMode::Flat).expect("switched");
    assert_eq!(
        (flat.mode(), flat.authorization_version()),
        (FeeMode::Flat, 2)
    );
    let back = flat.switched(FeeMode::Tiers).expect("switched");
    assert_eq!(
        (back.mode(), back.authorization_version()),
        (FeeMode::Tiers, 3)
    );
    assert_eq!(
        FeeTerms::new(FeeMode::Flat, i32::MAX).switched(FeeMode::Tiers),
        None
    );
    assert_eq!(
        (FeeMode::Tiers.code(), FeeMode::Flat.code()),
        ("tiers", "flat")
    );
}
