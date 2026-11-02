//! The fee rules case by case (TD-FEE-01, TD-FEE-02, TD-FEE-27, TD-FEE-28, TD-LIC-17; FEE-001 to
//! FEE-004, FEE-008, FEE-043, FEE-045): the tier edges, range midpoints rounded down, the set
//! tier of a diagnostic-visit quote, hourly quotes and fee-free job types. Fake amounts only.

use crate::api::{Fee, FeeError, FeeJob, FeeQuote, Tier, VisitTier, fee_for};

const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");

/// A job that is not `diagnostic_visit` and not fee-free.
const PRICED: FeeJob = FeeJob {
    diagnostic_visit: false,
    visit_tier: VisitTier::DEFAULT,
    fee_free: false,
};

/// A `diagnostic_visit` job at the default $25 set tier.
const VISIT: FeeJob = FeeJob {
    diagnostic_visit: true,
    ..PRICED
};

fn cents(quote: FeeQuote, job: FeeJob) -> u64 {
    fee_for(quote, job).expect("priced").cents()
}

/// TD-FEE-01 (FEE-001, FEE-002, FEE-008, D-009): W02 to W05 and the tier amounts, tax included.
#[test]
fn td_fee_01_tier_edges() {
    for (amount, fee) in [
        (49_999, 2_500),
        (50_000, 7_500),
        (150_000, 7_500),
        (150_001, 19_900),
        (0, 2_500),
        (u64::MAX, 19_900),
    ] {
        assert_eq!(cents(FeeQuote::Fixed(amount), PRICED), fee, "{amount}");
    }
    assert_eq!(
        [Tier::Small, Tier::Medium, Tier::Large].map(Tier::cents),
        [2_500, 7_500, 19_900]
    );
}

/// TD-FEE-02 (FEE-003, D-033): a range is priced at its midpoint, a half cent rounded down
/// (W31: $1,500.00 to $1,500.01 stays $75), and a reversed range is refused.
#[test]
fn td_fee_02_range_midpoint_rounded_down() {
    let fee = fee_for(
        FeeQuote::Range {
            low: 150_000,
            high: 150_001,
        },
        PRICED,
    );
    assert_eq!(
        fee,
        Ok(Fee::Charged {
            basis: Some(150_000),
            tier: Tier::Medium
        })
    );
    let max = FeeQuote::Range {
        low: u64::MAX,
        high: u64::MAX,
    };
    assert_eq!(fee_for(max, PRICED).map(|f| f.basis()), Ok(Some(u64::MAX)));
    let reversed = FeeQuote::Range { low: 2, high: 1 };
    assert_eq!(fee_for(reversed, PRICED), Err(FeeError::Amounts));
}

/// TD-FEE-02, TD-FEE-27 (FEE-004, PICK-053, D-418): a diagnostic-visit quote pays the set tier,
/// whatever its visit fee (W21, W22, W38), only in a `diagnostic_visit` job; a $199 set tier is
/// refused and $75 passes; every catalog category's stored tier is a valid set tier.
#[test]
fn td_fee_27_diagnostic_visit_set_tier() {
    for visit_fee in [0, 8_900, 15_000, 900_000] {
        let quote = FeeQuote::DiagnosticVisit { visit_fee };
        assert_eq!(cents(quote, VISIT), 2_500);
        assert_eq!(fee_for(quote, PRICED), Err(FeeError::NotDiagnosticVisit));
    }
    assert_eq!(
        VisitTier::from_dollars(199),
        Err(FeeError::VisitTierAboveCap)
    );
    assert_eq!(VisitTier::from_dollars(0), Err(FeeError::NotATier));
    assert_eq!(VisitTier::from_dollars(25), Ok(VisitTier::DEFAULT));
    let seventy_five = VisitTier::from_dollars(75).expect("$75 passes");
    let job = FeeJob {
        visit_tier: seventy_five,
        ..VISIT
    };
    assert_eq!(
        cents(FeeQuote::DiagnosticVisit { visit_fee: 0 }, job),
        7_500
    );
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    for def in &catalog.categories {
        assert!(
            VisitTier::from_dollars(def.trip_fee_tier).is_ok(),
            "{}",
            def.id
        );
    }
}

/// TD-FEE-27 (FEE-003, FEE-043): W34 crosses both lines ($1,550.00, $199); W35 $95 for 3 to 5
/// hours is $285 to $475 (basis $380.00, $25); W36 no hours is the set tier in a
/// `diagnostic_visit` job and goes back for an estimate in any other; an overflow is refused.
#[test]
fn td_fee_27_wide_range_and_hourly() {
    let wide = FeeQuote::Range {
        low: 40_000,
        high: 270_000,
    };
    assert_eq!(fee_for(wide, PRICED).map(|f| f.basis()), Ok(Some(155_000)));
    assert_eq!(cents(wide, PRICED), 19_900);
    let hourly = FeeQuote::Hourly {
        rate: 9_500,
        hours: Some((3, 5)),
    };
    let expect = Fee::Charged {
        basis: Some(38_000),
        tier: Tier::Small,
    };
    assert_eq!(fee_for(hourly, PRICED), Ok(expect));
    assert_eq!(fee_for(hourly, VISIT), Ok(expect));
    let no_hours = FeeQuote::Hourly {
        rate: 12_000,
        hours: None,
    };
    assert_eq!(cents(no_hours, VISIT), 2_500);
    assert_eq!(fee_for(no_hours, PRICED), Err(FeeError::NeedsHours));
    let huge = FeeQuote::Hourly {
        rate: u64::MAX,
        hours: Some((1, 2)),
    };
    assert_eq!(fee_for(huge, PRICED), Err(FeeError::Amounts));
    let reversed = FeeQuote::Hourly {
        rate: 100,
        hours: Some((5, 3)),
    };
    assert_eq!(fee_for(reversed, PRICED), Err(FeeError::Amounts));
}

/// TD-FEE-28, TD-LIC-17 (FEE-045, LIC-015, D-403): the catalog's buyer, pre-listing and
/// refinance inspections and the termite report for a sale or loan pay $0 for every kind of
/// quote; the general condition inspection is charged by its tier.
#[test]
fn td_fee_28_fee_free_job_types_pay_nothing() {
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == "home_inspection")
        .expect("home inspection");
    assert_eq!(def.fee_free_job_types.len(), 4);
    let quotes = [
        FeeQuote::Fixed(45_000),
        FeeQuote::Fixed(900_000),
        FeeQuote::Range {
            low: 40_000,
            high: 270_000,
        },
        FeeQuote::DiagnosticVisit { visit_fee: 0 },
    ];
    for job_type in &def.job_types {
        let fee_free = def.fee_free_job_types.contains(job_type);
        let job = FeeJob { fee_free, ..VISIT };
        for quote in quotes {
            let fee = fee_for(quote, job).expect("priced");
            assert_eq!(matches!(fee, Fee::FeeFree { .. }), fee_free, "{job_type}");
            assert_eq!(fee.cents() == 0, fee_free, "{job_type}");
        }
    }
    let general = def
        .job_types
        .iter()
        .find(|t| t.starts_with("home condition"));
    assert!(general.is_some_and(|t| !def.fee_free_job_types.contains(t)));
    let free = FeeJob {
        fee_free: true,
        ..PRICED
    };
    assert_eq!(
        fee_for(FeeQuote::Fixed(45_000), free),
        Ok(Fee::FeeFree {
            basis: Some(45_000)
        })
    );
}
