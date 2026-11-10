//! The K2b worked examples through the fee rules (TD-FEE-01, TD-FEE-02, TD-FEE-27, TD-FEE-28;
//! FEE-034): every one-time row of `docs/catalog/worked_examples.yaml` gives its fee basis and
//! fee to the cent. The recurring rows (a `period`: the 4-month rule, the $75 cap and the setup
//! charge, REC-002, FEE-039, FEE-044) are T188's and the refused rows (the handyman limit,
//! LIC-019) are `licensing`'s `tests::handyman`; both lists are pinned here, so a new row cannot
//! slip past every test (D-1400). All rows are fake.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::api::{Fee, FeeJob, FeeQuote, VisitTier, fee_for};

const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");
const EXAMPLES: &str = include_str!("../../../../docs/catalog/worked_examples.yaml");

/// Rows with a recurring period: T188's.
const RECURRING: [&str; 35] = [
    "W11", "W12", "W13", "W14", "W15", "W16", "W17", "W18", "W19", "W20", "W30", "W33", "W37",
    "W41", "W42", "W43", "W44", "W45", "W46", "W47", "W48", "W49", "W50", "W51", "W52", "W53",
    "W54", "W55", "W56", "W57", "W58", "W59", "W60", "W61", "W62",
];
/// Rows refused by the handyman limit: `licensing`'s.
const REFUSED: [&str; 3] = ["W26", "W27", "W40"];

#[derive(Deserialize)]
struct Examples {
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    id: String,
    category: String,
    situation: String,
    job_type: Option<String>,
    quote: BTreeMap<String, String>,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    outcome: String,
    fee_basis: Option<String>,
    fee: u64,
    waived: u64,
}

/// "999.99" or "6" as hundredths.
fn hundredths(text: &str) -> u64 {
    let (whole, part) = text.split_once('.').unwrap_or((text, "0"));
    let part = format!("{part:0<2}");
    whole.parse::<u64>().expect("whole") * 100 + part[..2].parse::<u64>().expect("cents")
}

fn hours(text: &str) -> u32 {
    text.parse().expect("whole hours")
}

/// The row's quote as the fee sees it; any other type panics.
fn quote(row: &BTreeMap<String, String>) -> FeeQuote {
    let cents = |key: &str| hundredths(&row[key]);
    match row["type"].as_str() {
        "fixed" => FeeQuote::Fixed(cents("amount")),
        "range" => FeeQuote::Range {
            low: cents("low"),
            high: cents("high"),
        },
        "hourly" => FeeQuote::Hourly {
            rate: cents("rate"),
            hours: row
                .get("hours_low")
                .map(|lo| (hours(lo), hours(&row["hours_high"]))),
        },
        "trip_fee" => FeeQuote::DiagnosticVisit {
            visit_fee: cents("amount"),
        },
        other => panic!("worked example quote type {other:?} has no FeeQuote mapping"),
    }
}

/// The row's job from the catalog: its format, set tier and fee-free job types.
fn job(example: &Example, catalog: &askhava_catalog::api::Catalog) -> FeeJob {
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == example.category)
        .expect("category");
    let job_type = example.job_type.as_ref();
    let format = job_type
        .and_then(|t| def.job_type_formats.get(t))
        .unwrap_or(&def.quote_format);
    FeeJob {
        diagnostic_visit: format == "diagnostic_visit",
        visit_tier: VisitTier::from_dollars(def.trip_fee_tier).expect("set tier"),
        fee_free: job_type.is_some_and(|t| def.fee_free_job_types.contains(t)),
    }
}

/// Checks one row: its basis to the cent, and the fee (or, for a free first win, the waived tier;
/// a repeat customer's $0 is FEE-013's, not these rules').
fn check(example: &Example, fee: Fee) {
    let id = &example.id;
    let want = &example.expected;
    assert_eq!(
        fee.basis(),
        want.fee_basis.as_deref().map(hundredths),
        "{id}"
    );
    assert_eq!(
        matches!(fee, Fee::FeeFree { .. }),
        want.outcome == "fee_free",
        "{id}"
    );
    let dollars = fee.cents() / 100;
    match example.situation.as_str() {
        "new_customer" => assert_eq!(dollars, want.fee, "{id}"),
        "free_first_win" => assert_eq!(dollars, want.waived, "{id}"),
        "repeat_customer" => assert_eq!(want.fee, 0, "{id}"),
        other => panic!("{id}: unknown situation {other:?}"),
    }
}

/// TD-FEE-01, TD-FEE-02, TD-FEE-27, TD-FEE-28 (FEE-001 to FEE-004, FEE-008, FEE-034, FEE-043,
/// FEE-045): every one-time, not-refused worked example matches to the cent; the others are
/// exactly the pinned recurring and refused lists.
#[test]
fn td_fee_01_every_worked_example_matches() {
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let (mut recurring, mut refused, mut checked) = (BTreeSet::new(), BTreeSet::new(), 0);
    for example in &examples.examples {
        if example.quote.contains_key("period") {
            recurring.insert(example.id.as_str());
        } else if example.expected.outcome == "refused" {
            refused.insert(example.id.as_str());
        } else {
            let fee = fee_for(quote(&example.quote), job(example, &catalog)).expect("priced");
            check(example, fee);
            checked += 1;
        }
    }
    assert_eq!(recurring, BTreeSet::from(RECURRING));
    assert_eq!(refused, BTreeSet::from(REFUSED));
    assert_eq!(checked, 24);
}
