//! The twin's six "qualified business left out" warnings as regression tests (TD-INT-03: INT-013,
//! AI-011, CARD-001; `docs/twin-warnings.md` section 3, D-102, D-105). Fake text only.
//!
//! Each time both AI readings named the same wrong trade, the keyword trade differed, and the
//! homeowner chose the right trade; the keyword job type of that trade must be kept, never
//! replaced by `<trade>_other`, so an AC-only license and businesses that did not tick "other"
//! still get the card.

use crate::api::{
    Candidates, Opinion, Reading, TradeStep, candidates, job_types_of, keyword_reading,
    second_opinion, two_trades,
};

const CATALOG: &str = include_str!("../../../../docs/catalog/categories.yaml");

/// The four requests behind the six warnings (1044, 1247, 1268, 1284; 1247 and 1268 each left out
/// two businesses): (message, wrong trade, wrong job type, the answer, chosen trade, kept job type).
const REQUESTS: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "Fix sticking door 86406",
        "plumbing_repair",
        "plumbing_repair_other",
        "handyman",
        "handyman",
        "door repair",
    ),
    (
        "AC blowing warm air 86405",
        "electrical",
        "light fixture",
        "it's the AC",
        "hvac_repair",
        "AC not cooling",
    ),
    (
        "AC blowing warm air 86405",
        "electrical",
        "light fixture",
        "2",
        "hvac_repair",
        "AC not cooling",
    ),
    (
        "Heater makes a loud rattle 86406",
        "plumbing_repair",
        "leak repair",
        "2",
        "hvac_repair",
        "furnace repair",
    ),
];

fn wrong(trade: &'static str, job: &'static str) -> [Reading<'static>; 2] {
    [Reading {
        trade,
        job_type: Some(job),
    }; 2]
}

/// TD-INT-03 (AI-011, INT-013, CARD-001, D-102): the keyword job type of the chosen trade is kept.
#[test]
fn td_int_03_chosen_trade_keeps_its_keyword_job_type() {
    for &(message, bad_trade, bad_job, reply, right, kept) in REQUESTS {
        let kw = keyword_reading(message);
        let readings = wrong(bad_trade, bad_job);
        let Opinion::Ask(q) = second_opinion(readings, &kw) else {
            panic!("{message}: the keyword trade differs, so the question is asked");
        };
        assert_eq!(q.options, [bad_trade, right], "{message}");
        assert_eq!(
            q.step(q.read(reply), 1),
            TradeStep::Chosen(right.to_owned()),
            "{reply}"
        );
        let got = candidates(right, readings, &kw);
        let want = Candidates {
            job_types: vec![(right.to_owned(), kept.to_owned())],
            which_fits: None,
        };
        assert_eq!(got, want, "{message}: never <trade>_other");
    }
}

/// TD-INT-03 (D-102): with no job type of the chosen trade, `<trade>_other` is stored and
/// "Which fits best?" lists that trade's job types.
#[test]
fn td_int_03_no_job_type_of_the_trade_asks_which_fits_best() {
    let kw = keyword_reading("hvac guy needed 86403");
    let got = candidates("hvac_repair", wrong("electrical", "panel upgrade"), &kw);
    assert_eq!(
        got.job_types,
        [("hvac_repair".to_owned(), "hvac_repair_other".to_owned())]
    );
    let list: Vec<String> = job_types_of("hvac_repair")
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(got.which_fits, Some(list));
}

/// TD-INT-03 (D-105, CARD-001): a wrong trade answer keeps the licensed keyword job type, so the
/// candidates span two trades and "Which fits best?" is asked instead of routing outside a license.
#[test]
fn td_int_03_license_boundary_job_types_stay_after_a_wrong_answer() {
    let kw = keyword_reading("Heater makes a loud rattle 86406");
    let got = candidates(
        "plumbing_repair",
        wrong("plumbing_repair", "leak repair"),
        &kw,
    );
    let pairs = [
        ("plumbing_repair", "leak repair"),
        ("hvac_repair", "furnace repair"),
    ];
    let want: Vec<(String, String)> = pairs
        .iter()
        .map(|(t, j)| ((*t).to_owned(), (*j).to_owned()))
        .collect();
    assert_eq!(got.job_types, want);
    assert_eq!(
        got.which_fits,
        Some(vec!["leak repair".to_owned(), "furnace repair".to_owned()])
    );
}

/// TD-INT-03 (CARD-001): every job type the keyword rules name is a catalog job type, so the
/// license map can decide coverage for it.
#[test]
fn td_int_03_keyword_job_types_are_catalog_job_types() {
    for trade in ["plumbing_repair", "hvac_repair", "electrical", "handyman"] {
        let block = CATALOG
            .split("\n  - id: ")
            .find(|b| b.starts_with(&format!("{trade}\n")))
            .expect("category in the catalog");
        let line = block
            .lines()
            .find(|l| l.trim_start().starts_with("job_types:"))
            .expect("job types");
        for job in job_types_of(trade) {
            assert!(line.contains(job), "{trade}: {job} not in the catalog");
        }
    }
}

/// TD-INT-03 (CARD-001, D-105; R-37 in `docs/catalog/categories.yaml`): a water heater is
/// plumbing and a pool or spa heater is pool repair; the longest phrase wins, so "heater" inside
/// them never adds the furnace candidate or a second trade, while "heater" alone stays heating.
#[test]
fn td_int_03_water_and_spa_heaters_never_read_as_a_furnace() {
    let water = keyword_reading("Water heater leaking 86403");
    assert_eq!(water.trades, ["plumbing_repair"]);
    assert_eq!(water.job_types, [("plumbing_repair", "leak repair")]);
    assert_eq!(two_trades("Water heater leaking 86403"), None);
    let got = candidates(
        "plumbing_repair",
        wrong("plumbing_repair", "leak repair"),
        &water,
    );
    assert!(
        got.job_types.iter().all(|(t, _)| t != "hvac_repair"),
        "no furnace candidate: {got:?}"
    );
    for words in ["the pool heater won't light", "Spa heater is broken"] {
        let kw = keyword_reading(words);
        assert_eq!(kw.trades, ["pool_repair"], "{words}");
        assert!(kw.boundary.is_empty(), "{words}");
    }
    let both = keyword_reading("water heater and the heater in the hall");
    assert_eq!(both.trades, ["plumbing_repair", "hvac_repair"]);
    assert_eq!(keyword_reading("heater rattles").trades, ["hvac_repair"]);
}
