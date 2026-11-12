//! The handyman limits (TD-LIC-08, TD-LIC-09, TD-LIC-10; AI-012, LIC-019 to LIC-025, LIC-036):
//! every worked example in `docs/catalog/worked_examples.yaml` through `check_quote`, the
//! certification box, the card line, the gas list against the catalog file, the request-text
//! blocks and the two-jobs case that shows no split-job check exists (LIC-023, D-2407). Every business, address and price is fake.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::api::{
    CityListStanding, Eligibility, HANDYMAN_CARD_LINE, Holder, Job, JobRequest, LicenseKind,
    NeverHandyman, NoCardReason, QuotePrice, QuoteRefusal, QuoteSubmission, check_quote,
    eligibility_for_request, never_handyman,
};
use crate::handyman::parse_gas;

const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");
const EXAMPLES: &str = include_str!("../../../../docs/catalog/worked_examples.yaml");
const GAS: &str = include_str!("../../../../docs/catalog/gas_keywords.yaml");

#[derive(Deserialize)]
struct Examples {
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    id: String,
    category: String,
    business: String,
    quote: BTreeMap<String, String>,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    outcome: String,
}

/// "999.99" or "6" as hundredths.
fn hundredths(text: &str) -> u64 {
    let (whole, part) = text.split_once('.').unwrap_or((text, "0"));
    let part = format!("{part:0<2}");
    whole.parse::<u64>().expect("whole") * 100 + part[..2].parse::<u64>().expect("cents")
}

/// The worked example's quote as the portal form would hand it over; hourly with hours is a range
/// (FEE-043), hourly without hours (W36) and `trip_fee` are site-visit quotes; any other type panics.
fn price(quote: &BTreeMap<String, String>) -> QuotePrice {
    let cents = |key: &str| hundredths(&quote[key]);
    match quote["type"].as_str() {
        "fixed" => QuotePrice::Fixed(cents("amount")),
        "range" => QuotePrice::Range {
            low: cents("low"),
            high: cents("high"),
        },
        "hourly" if quote.contains_key("hours_high") => QuotePrice::Range {
            low: cents("rate") * cents("hours_low") / 100,
            high: cents("rate") * cents("hours_high") / 100,
        },
        "trip_fee" | "hourly" => QuotePrice::TripFee,
        other => panic!("worked example quote type {other:?} has no QuotePrice mapping"),
    }
}

fn submission(example: &Example, certified: bool) -> QuoteSubmission {
    let catalog = askhava_catalog::api::parse(CATEGORIES).expect("catalog parses");
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == example.category)
        .expect("category");
    QuoteSubmission {
        construction_type: def.construction_type,
        license: LicenseKind::parse(&def.license.kind).expect("kind"),
        handyman: example.business == "handyman_exemption",
        price: price(&example.quote),
        certified,
    }
}

/// TD-LIC-08 (LIC-019, LIC-020, LIC-025, D-035): W25 $999.99 accepted, W26 $1,000.00 refused, W27
/// a range judged by its top, W29 a $1,200 car detail allowed, W40 an hourly top refused; every
/// other worked example passes the limit.
#[test]
fn worked_examples_hold_the_handyman_limit() {
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    for id in ["W25", "W26", "W27", "W29"] {
        assert!(examples.examples.iter().any(|e| e.id == id), "{id} missing");
    }
    for example in &examples.examples {
        let outcome = check_quote(&submission(example, true));
        if example.expected.outcome == "refused" {
            assert_eq!(
                outcome,
                Err(QuoteRefusal::OverHandymanLimit),
                "{}",
                example.id
            );
        } else {
            assert_eq!(outcome, Ok(()), "{}", example.id);
        }
    }
}

fn handyman_quote(price: QuotePrice, certified: bool) -> QuoteSubmission {
    QuoteSubmission {
        construction_type: true,
        license: LicenseKind::None,
        handyman: true,
        price,
        certified,
    }
}

/// TD-LIC-08 (LIC-019, LIC-025, LIC-036, D-186): one cent either side of the limit, the top of a
/// range, and a handyman quote without the certification box refused by the rule function.
#[test]
fn certification_box_and_limit_edges() {
    assert_eq!(
        check_quote(&handyman_quote(QuotePrice::Fixed(99_999), true)),
        Ok(())
    );
    assert_eq!(
        check_quote(&handyman_quote(QuotePrice::Fixed(100_000), true)),
        Err(QuoteRefusal::OverHandymanLimit)
    );
    let range = QuotePrice::Range {
        low: 70_000,
        high: 110_000,
    };
    assert_eq!(
        check_quote(&handyman_quote(range, true)),
        Err(QuoteRefusal::OverHandymanLimit)
    );
    for price in [QuotePrice::Fixed(5_000), QuotePrice::TripFee] {
        assert_eq!(
            check_quote(&handyman_quote(price, false)),
            Err(QuoteRefusal::NotCertified)
        );
    }
    assert_eq!(
        check_quote(&handyman_quote(QuotePrice::TripFee, true)),
        Ok(())
    );
}

/// TD-LIC-08 (LIC-024): the handyman card line, word for word.
#[test]
fn handyman_card_line_is_the_spec_line() {
    assert_eq!(
        HANDYMAN_CARD_LINE,
        "Not a licensed contractor. Only for jobs under $1,000 total with no permit."
    );
}

fn request<'a>(text: &'a str, candidates: &'a [String]) -> JobRequest<'a> {
    JobRequest { text, candidates }
}

/// TD-LIC-09 (AI-012, D-157): the catalog file holds at least AI-012's words, and the D-157 cases
/// hit the gas list; "gasket" is not "gas".
#[test]
fn gas_list_holds_ai_012_words_and_catches_d_157_cases() {
    let phrases = parse_gas(GAS).expect("gas list parses");
    let required = [
        "gas",
        "propane",
        "natural gas",
        "pilot",
        "pilot light",
        "won't light",
        "furnace",
        "gas line",
        "gas dryer",
        "gas stove",
        "gas range",
        "propano",
        "piloto",
    ];
    for word in required {
        let word: Vec<String> = word.split(' ').map(str::to_owned).collect();
        assert!(phrases.contains(&word), "{word:?} missing");
    }
    let none: [String; 0] = [];
    for text in [
        "pool heater won't light",
        "Pool heater won\u{2019}t light",
        "pilot keeps going out",
        "el piloto no prende",
    ] {
        assert_eq!(
            never_handyman(&request(text, &none)),
            Some(NeverHandyman::Gas),
            "{text}"
        );
    }
    assert_eq!(
        never_handyman(&request("replace the faucet gasket", &none)),
        None
    );
}

/// TD-LIC-09 (AI-012): a malformed or empty gas file is refused, so the rule fails closed.
#[test]
fn malformed_gas_file_is_refused() {
    assert!(parse_gas("decision: D-157\nen: []\nes: []\n").is_none());
    assert!(parse_gas("en: [gas]\nes: [gas]\nextra: 1\n").is_none());
    assert!(parse_gas("not yaml: [").is_none());
}

/// TD-LIC-09 (LIC-021, LIC-022): smoke alarms, fire sprinklers and permit-type candidates block
/// a handyman card whatever the AI read; a lawn sprinkler and a drywall patch do not.
#[test]
fn never_handyman_job_types_block_the_handyman_card() {
    let cases = [
        ("hardwired smoke alarm", Some(NeverHandyman::SmokeAlarm)),
        ("fire sprinklers", Some(NeverHandyman::Sprinkler)),
        ("water heater replacement", Some(NeverHandyman::PermitType)),
        ("panel upgrade", Some(NeverHandyman::PermitType)),
        ("new circuit", Some(NeverHandyman::PermitType)),
        ("re-roof", Some(NeverHandyman::PermitType)),
        ("sprinkler repair", None),
        ("drywall patch", None),
    ];
    for (candidate, expected) in cases {
        let candidates = vec!["drywall patch".to_owned(), candidate.to_owned()];
        assert_eq!(
            never_handyman(&request("fix it", &candidates)),
            expected,
            "{candidate}"
        );
    }
    let job = Job {
        category: "handyman".to_owned(),
        job_type: "drywall patch".to_owned(),
        license: LicenseKind::None,
        handyman_allowed: true,
        listed: true,
    };
    let holder = Holder {
        handyman: true,
        city_list: CityListStanding::Matched,
        ..Holder::default()
    };
    let clean = vec!["drywall patch".to_owned()];
    let gas = vec!["drywall patch".to_owned(), "gas line".to_owned()];
    assert_eq!(
        eligibility_for_request(&job, &holder, &request("hole in wall", &clean)),
        Eligibility::HandymanCard
    );
    assert_eq!(
        eligibility_for_request(&job, &holder, &request("hole in wall", &gas)),
        Eligibility::NoCard(NoCardReason::RefusedJobType)
    );
}

/// TD-LIC-09 (LIC-021, LIC-022, D-2637): smoke detectors, fire sprinklers and a wall removal
/// written only in the request text block a handyman whatever the candidates; a lawn sprinkler
/// does not.
#[test]
fn request_text_alone_blocks_smoke_sprinkler_and_permit_work() {
    let door = vec!["door repair".to_owned()];
    let drywall = vec!["drywall patch".to_owned()];
    let cases = [
        (
            "Install hardwired smoke detectors by the bedroom door",
            &door,
            Some(NeverHandyman::SmokeAlarm),
        ),
        (
            "fire sprinkler head leaking by the garage door",
            &door,
            Some(NeverHandyman::Sprinkler),
        ),
        (
            "remove the wall between kitchen and den, then drywall",
            &drywall,
            Some(NeverHandyman::PermitType),
        ),
        ("lawn sprinkler head broke by the fence", &door, None),
    ];
    for (text, candidates, expected) in cases {
        assert_eq!(
            never_handyman(&request(text, candidates)),
            expected,
            "{text}"
        );
    }
}

/// TD-LIC-10 (LIC-023, LIC-019, D-2407, D-2408): no split-job check. Two handyman jobs of one
/// business at one address on the same day, sharing a job type, $600 and $500, are each accepted
/// on their own price; the single-job edges stay TD-LIC-08's.
#[test]
fn each_handyman_job_is_judged_only_by_its_own_price() {
    for cents in [60_000, 50_000] {
        assert_eq!(
            check_quote(&handyman_quote(QuotePrice::Fixed(cents), true)),
            Ok(()),
            "{cents}"
        );
    }
    assert_eq!(
        check_quote(&handyman_quote(QuotePrice::Fixed(99_999), true)),
        Ok(())
    );
    assert_eq!(
        check_quote(&handyman_quote(QuotePrice::Fixed(100_000), true)),
        Err(QuoteRefusal::OverHandymanLimit)
    );
}
