//! The 4-month rule, the recurring cap and the absence of any early-joiner price lock (TD-FEE-02,
//! TD-FEE-03, TD-FEE-21, TD-FEE-35, TD-FEE-41; REC-001 to REC-006, REC-010, REC-011, FEE-001,
//! FEE-005, FEE-039).
//! Every worked example with a period runs through `period_fee_for` to the cent. All data fake.

use std::collections::BTreeMap;

use proptest::prelude::*;
use serde::Deserialize;

use crate::api::{
    Fee, FeeError, FeeJob, FeeQuote, Period, PeriodJob, RECURRING_MONTHS, RecurringCap, Service,
    VisitTier, period_fee_for,
};

const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");
const EXAMPLES: &str = include_str!("../../../../docs/catalog/worked_examples.yaml");

#[derive(Deserialize)]
struct Examples {
    fee_rules: Rules,
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Rules {
    recurring_months: u128,
    recurring_fee_cap: i32,
}

#[derive(Deserialize)]
struct Example {
    id: String,
    category: String,
    job_type: Option<String>,
    quote: BTreeMap<String, String>,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    fee_basis: Option<String>,
    fee: u64,
}

fn hundredths(text: &str) -> u64 {
    let (whole, part) = text.split_once('.').unwrap_or((text, "0"));
    let part = format!("{part:0<2}");
    whole.parse::<u64>().expect("whole") * 100 + part[..2].parse::<u64>().expect("cents")
}

fn period(text: &str) -> Period {
    match text {
        "weekly" => Period::Weekly,
        "every_two_weeks" => Period::EveryTwoWeeks,
        "monthly" => Period::Monthly,
        "quarterly" => Period::Quarterly,
        "yearly" => Period::Yearly,
        other => panic!("unknown period {other:?}"),
    }
}

/// A plain job with the given service and cap.
fn job(service: Service, cap: RecurringCap) -> PeriodJob {
    let job = FeeJob {
        diagnostic_visit: false,
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
    };
    PeriodJob { job, service, cap }
}

/// The row's fee from the catalog's flags (REC-001); the setup charge is never read (FEE-044).
fn row_fee(example: &Example, catalog: &askhava_catalog::api::Catalog, cap: RecurringCap) -> Fee {
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == example.category)
        .expect("category");
    let one_time = example
        .job_type
        .as_ref()
        .is_some_and(|t| def.one_time_job_types.contains(t));
    let q = &example.quote;
    let quote = match q["type"].as_str() {
        "fixed" => FeeQuote::Fixed(hundredths(&q["amount"])),
        "range" => FeeQuote::Range {
            low: hundredths(&q["low"]),
            high: hundredths(&q["high"]),
        },
        other => panic!("{}: quote type {other:?}", example.id),
    };
    let service = Service::of(def.recurring, one_time);
    period_fee_for(quote, q.get("period").map(|p| period(p)), job(service, cap)).expect("priced")
}

/// TD-FEE-03, TD-FEE-21, TD-FEE-02 (REC-002 to REC-005, REC-011, FEE-039): every worked example
/// with a period, and W32, match to the cent (W11 to W20, W30, W33, W37, W18's range, W30's
/// round-down); with the cap removed W20 and W33 pay $199, so the cap is what holds them at $75.
#[test]
fn td_fee_03_every_recurring_worked_example_matches() {
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let mut checked = 0;
    for example in &examples.examples {
        if !example.quote.contains_key("period") && example.id != "W32" {
            continue;
        }
        let fee = row_fee(example, &catalog, RecurringCap::DEFAULT);
        let id = &example.id;
        assert_eq!(
            fee.basis(),
            example.expected.fee_basis.as_deref().map(hundredths),
            "{id}"
        );
        assert_eq!(fee.cents(), example.expected.fee * 100, "{id}");
        let uncapped = row_fee(
            example,
            &catalog,
            RecurringCap::from_dollars(199).expect("cap"),
        );
        let want = if ["W20", "W33"].contains(&id.as_str()) {
            19_900
        } else {
            fee.cents()
        };
        assert_eq!(uncapped.cents(), want, "{id} without the cap");
        checked += 1;
    }
    assert_eq!(checked, 14);
}

/// TD-FEE-03 (REC-006): the one month multiplier is 4, in code and in the worked examples'
/// rules, and the code's default cap is the examples' $75 (D-196).
#[test]
fn td_fee_03_four_months_and_the_75_cap_everywhere() {
    let rules = serde_saphyr::from_str::<Examples>(EXAMPLES)
        .expect("parse")
        .fee_rules;
    assert_eq!(RECURRING_MONTHS, 4);
    assert_eq!(rules.recurring_months, RECURRING_MONTHS);
    let cap = RecurringCap::from_dollars(rules.recurring_fee_cap).expect("cap");
    assert_eq!(cap, RecurringCap::DEFAULT);
}

/// The basis and the cents charged, at the default $75 cap unless given.
fn priced(quote: FeeQuote, period: Option<Period>, service: Service) -> Result<Pair, FeeError> {
    priced_at(quote, period, service, RecurringCap::DEFAULT)
}

type Pair = (Option<u64>, u64);

fn priced_at(
    q: FeeQuote,
    p: Option<Period>,
    s: Service,
    cap: RecurringCap,
) -> Result<Pair, FeeError> {
    period_fee_for(q, p, job(s, cap)).map(|f| (f.basis(), f.cents()))
}

/// REC-003, TD-FEE-03 (REC-010, D-037, D-159): the $150-a-month pool account is $600.00 and
/// $75; $40 a visit with weekly visits is $173.33 a month, basis $693.33, $75; a recurring-service
/// quote with no period goes back to the business; a period in a non-recurring category is an
/// error; a one-time job with no period is face value (W32).
#[test]
fn td_fee_03_periods_by_service() {
    let (monthly, weekly) = (Some(Period::Monthly), Some(Period::Weekly));
    let recurring = Service::of(true, false);
    let pool = priced(FeeQuote::Fixed(15_000), monthly, recurring);
    assert_eq!(pool, Ok((Some(60_000), 7_500)));
    let visit = priced(FeeQuote::Fixed(4_000), weekly, recurring);
    assert_eq!(visit, Ok((Some(69_333), 7_500)));
    let none = priced(FeeQuote::Fixed(4_000), None, recurring);
    assert_eq!(none, Err(FeeError::NeedsPeriod));
    let other = Service::of(false, false);
    let typed = priced(FeeQuote::Fixed(4_000), monthly, other);
    assert_eq!(typed, Err(FeeError::PeriodNotRecurring));
    let one_time = priced(FeeQuote::Fixed(30_000), None, Service::of(true, true));
    assert_eq!(one_time, Ok((Some(30_000), 2_500)));
    let plain = priced(FeeQuote::Fixed(30_000), None, other);
    assert_eq!(plain, Ok((Some(30_000), 2_500)));
}

/// TD-FEE-35, TD-FEE-21 (FEE-039, D-265): a one-time job type quoted "per month" over $1,500.00
/// pays $199, a recurring job type $75, a one-time job with no period over $1,500.00 $199; a
/// lower cap setting lowers the fee with no code change.
#[test]
fn td_fee_35_the_cap_follows_recurring_job_types_only() {
    let (quote, monthly) = (FeeQuote::Fixed(40_000), Some(Period::Monthly));
    let typed = priced(quote, monthly, Service::OneTimeJobType);
    assert_eq!(typed, Ok((Some(160_000), 19_900)));
    let recurring = priced(quote, monthly, Service::RecurringJobType);
    assert_eq!(recurring, Ok((Some(160_000), 7_500)));
    let face = priced(FeeQuote::Fixed(160_000), None, Service::OneTimeJobType);
    assert_eq!(face, Ok((Some(160_000), 19_900)));
    let small = RecurringCap::from_dollars(25).expect("cap");
    let lower = priced_at(quote, monthly, Service::RecurringJobType, small);
    assert_eq!(lower, Ok((Some(160_000), 2_500)));
}

/// FEE-039, FEE-045: the quotes a period cannot take, a cap that is not a fee, and a fee-free
/// job type with a period.
#[test]
fn period_errors_and_fee_free() {
    let (monthly, recurring) = (Some(Period::Monthly), Service::RecurringJobType);
    let hourly = FeeQuote::Hourly {
        rate: 5_000,
        hours: Some((2, 3)),
    };
    let visit = FeeQuote::DiagnosticVisit { visit_fee: 0 };
    for quote in [hourly, visit] {
        assert_eq!(
            priced(quote, monthly, recurring),
            Err(FeeError::NotRecurringPrice)
        );
    }
    let upside_down = FeeQuote::Range { low: 2, high: 1 };
    assert_eq!(
        priced(upside_down, monthly, recurring),
        Err(FeeError::Amounts)
    );
    let huge = FeeQuote::Fixed(u64::MAX);
    assert_eq!(
        priced(huge, Some(Period::Weekly), recurring),
        Err(FeeError::Amounts)
    );
    assert_eq!(RecurringCap::from_dollars(50), Err(FeeError::NotATier));
    let mut free = job(recurring, RecurringCap::DEFAULT);
    free.job.fee_free = true;
    let fee = period_fee_for(FeeQuote::Fixed(15_000), monthly, free);
    assert_eq!(
        fee,
        Ok(Fee::FeeFree {
            basis: Some(60_000)
        })
    );
}

const RULES_RS: &str = include_str!("../rules.rs");
const API_RS: &str = include_str!("../api.rs");

/// TD-FEE-41 (FEE-001, FEE-005, FEE-039, D-1494): there is no early-joiner price lock. The fee
/// rules and the public surface name no lock type or function and no fee input is a date, so a
/// business that joined 1 day after launch and one that joined 400 days after, picking the same
/// quote, pay the same published tier: $25, $75, $199, or the $75 recurring cap.
#[test]
fn td_fee_41_no_early_joiner_price_lock() {
    let gone =
        "LockDates price_lock_until locked_cents LOCK_JOIN_WINDOW LOCK_MONTHS Timestamp joined";
    for name in gone.split(' ') {
        assert!(!RULES_RS.contains(name), "rules.rs names {name}");
        assert!(!API_RS.contains(name), "api.rs names {name}");
    }
    let (one, recurring) = (Service::OneTimeJobType, Service::RecurringJobType);
    let cases = [
        (FeeQuote::Fixed(30_000), None, one, 2_500),
        (FeeQuote::Fixed(100_000), None, one, 7_500),
        (FeeQuote::Fixed(200_000), None, one, 19_900),
        (
            FeeQuote::Fixed(40_000),
            Some(Period::Monthly),
            recurring,
            7_500,
        ),
    ];
    for (quote, period, service, cents) in cases {
        let one_day = priced(quote, period, service).expect("priced");
        let four_hundred_days = priced(quote, period, service).expect("priced");
        assert_eq!(one_day.1, cents);
        assert_eq!(four_hundred_days, one_day);
    }
}

proptest! {
    /// TD-FEE-03 (REC-002): monthly m has basis 4m; TD-FEE-02 (REC-005): a recurring range's
    /// basis lies between 4 months of its ends.
    #[test]
    fn prop_four_months_of_monthly(m in 0u64..1_000_000_000, extra in 0u64..1_000_000) {
        let recurring = job(Service::RecurringJobType, RecurringCap::DEFAULT);
        let fee = period_fee_for(FeeQuote::Fixed(m), Some(Period::Monthly), recurring);
        prop_assert_eq!(fee.map(|f| f.basis()), Ok(Some(4 * m)));
        let range = FeeQuote::Range { low: m, high: m + extra };
        let fee = period_fee_for(range, Some(Period::Monthly), recurring).expect("priced");
        let basis = fee.basis().expect("basis");
        prop_assert!(4 * m <= basis && basis <= 4 * (m + extra));
    }
}
