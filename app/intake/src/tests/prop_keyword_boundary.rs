//! "Which fits best?" and the keyword license-boundary candidates (TD-INT-05: INT-013, INT-014;
//! TD-INT-06: INT-014, CARD-001; D-104, D-105, D-1003). Fake text only.
//!
//! A licensed job type named in the homeowner's own words stays a candidate whatever trade is read
//! or answered; candidates across trades fall under different licenses, so "Which fits best?" is
//! asked, and only the homeowner's own choice there narrows them.

use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use crate::api::{
    Candidates, FitStep, Opinion, Reading, candidates, job_types_of, keyword_reading,
    second_opinion,
};

const CATALOG: &str = include_str!("../../../../docs/catalog/categories.yaml");

const TRADES: &[&str] = &[
    "plumbing_repair",
    "hvac_repair",
    "electrical",
    "handyman",
    "pool_repair",
];

/// Request phrases, `|`-separated: licensed keywords, trade-only words and words naming nothing.
const PHRASES: &str = "leaking pipe|toilet|clogged drain|faucet|AC|warm air|thermostat|heater|\
    swamp cooler|outlet|breaker|light fixture|panel|wiring|water heater|spa heater|door|drywall|\
    plumber|hvac|odd noise";

/// Replies to "Which fits best?", `|`-separated: numbers, "Not sure", words, nonsense.
const REPLIES: &str = "1|2|3|7|not sure|huh|the furnace|water heater|toilet|outlet|1 or 2";

fn both(trade: &'static str, job_type: Option<&'static str>) -> [Reading<'static>; 2] {
    [Reading { trade, job_type }; 2]
}

fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(t, j)| ((*t).to_owned(), (*j).to_owned()))
        .collect()
}

/// TD-INT-05 (INT-013, D-104): two readings and the keyword trade name three trades, and the
/// question offers all three, readings first.
#[test]
fn td_int_05_three_trades_are_all_offered() {
    let readings = [
        Reading {
            trade: "electrical",
            job_type: None,
        },
        Reading {
            trade: "handyman",
            job_type: None,
        },
    ];
    let Opinion::Ask(q) = second_opinion(readings, &keyword_reading("faucet keeps dripping"))
    else {
        panic!("the readings differ, so the question is asked");
    };
    assert_eq!(q.options, ["electrical", "handyman", "plumbing_repair"]);
}

/// TD-INT-05 (INT-014, D-105, twin cause C2e): "Water heater not getting hot" read as heating/AC
/// and answered as heating/AC keeps the licensed "water heater replacement" and asks "Which fits
/// best?".
#[test]
fn td_int_05_water_heater_read_as_heating_asks_which_fits_best() {
    let kw = keyword_reading("Water heater not getting hot 86403");
    let readings = both("hvac_repair", Some("furnace repair"));
    let Opinion::Ask(q) = second_opinion(readings, &kw) else {
        panic!("the keyword trade differs, so the trade question is asked");
    };
    assert_eq!(q.options, ["hvac_repair", "plumbing_repair"]);
    let got = candidates("hvac_repair", readings, &kw);
    let pairs = [
        ("hvac_repair", "furnace repair"),
        ("plumbing_repair", "water heater replacement"),
    ];
    assert_eq!(got.job_types, owned(&pairs));
    let options = vec![
        "furnace repair".to_owned(),
        "water heater replacement".to_owned(),
    ];
    assert_eq!(got.which_fits, Some(options.clone()));
    assert_eq!(got.fit_step("huh", 1), FitStep::AskAgain(options));
    let water = Candidates {
        job_types: owned(&[("plumbing_repair", "water heater replacement")]),
        which_fits: None,
    };
    assert_eq!(got.fit_step("the water heater", 1), FitStep::Chosen(water));
    assert_eq!(got.fit_step("not sure", 1), FitStep::KeepAll(got.clone()));
    assert_eq!(got.fit_step("huh", 2), FitStep::KeepAll(got.clone()));
}

/// TD-INT-06 (CARD-001, D-105, D-1003): a heater phrase keeps its licensed-only job type even when
/// another job type of the same trade is named, so a drain-only or pump-only license never gets it.
#[test]
fn td_int_06_heater_stays_beside_another_job_of_its_trade() {
    let drain = candidates(
        "plumbing_repair",
        both("plumbing_repair", Some("drain clearing")),
        &keyword_reading("clogged drain and water heater not hot"),
    );
    let pairs = [
        ("plumbing_repair", "drain clearing"),
        ("plumbing_repair", "water heater replacement"),
    ];
    assert_eq!(drain.job_types, owned(&pairs));
    let spa = candidates(
        "pool_repair",
        both("pool_repair", Some("pump repair")),
        &keyword_reading("spa heater broken"),
    );
    let pairs = [
        ("pool_repair", "pump repair"),
        ("pool_repair", "gas pool heater"),
    ];
    assert_eq!(spa.job_types, owned(&pairs));
}

/// INT-012, INT-014, D-103: a lone number and words naming a different option are unusable, so
/// the question is asked again; a number and words naming the same option choose it.
#[test]
fn which_fits_number_and_words_must_agree() {
    let got = candidates(
        "hvac_repair",
        both("hvac_repair", Some("furnace repair")),
        &keyword_reading("Water heater not getting hot"),
    );
    let options = got.which_fits.clone().expect("asked across trades");
    assert_eq!(
        got.fit_step("1 the water heater", 1),
        FitStep::AskAgain(options)
    );
    assert!(matches!(
        got.fit_step("2 the water heater", 1),
        FitStep::Chosen(_)
    ));
    assert_eq!(
        got.fit_step("1 the water heater", 2),
        FitStep::KeepAll(got.clone())
    );
}

/// TD-INT-06 regression (once found by the property): a spa heater with plumbing words, read as
/// plumbing and answered as pool repair, keeps every licensed candidate after the replies 1, 1.
#[test]
fn td_int_06_spa_heater_with_plumbing_words_answered_as_pool() {
    let words = "spa heater and leaking pipe and toilet";
    let readings = both("plumbing_repair", None);
    check(words, readings, "pool_repair", ["1", "1"]).expect("licensed candidates kept");
}

/// TD-INT-05 (INT-014, D-102, D-105): with no job type of the answered trade, the options list
/// that trade's job types and then the licensed candidate of another trade.
#[test]
fn td_int_05_no_job_type_named_still_offers_the_licensed_candidate() {
    let got = candidates(
        "hvac_repair",
        both("hvac_repair", None),
        &keyword_reading("toilet"),
    );
    let mut options: Vec<String> = job_types_of("hvac_repair")
        .into_iter()
        .map(str::to_owned)
        .collect();
    options.push("toilet repair".to_owned());
    assert_eq!(got.which_fits, Some(options));
}

/// Any one of a `|`-separated list.
fn pick(list: &'static str) -> impl Strategy<Value = &'static str> {
    prop::sample::select(list.split('|').collect::<Vec<_>>())
}

/// The catalog job types of `trade` (`job_types:` in `docs/catalog/categories.yaml`), so readings
/// may carry job types the keyword rules do not list (pool "pump repair").
fn catalog_job_types(trade: &str) -> Vec<&'static str> {
    let block = CATALOG
        .split("\n  - id: ")
        .find(|b| b.starts_with(&format!("{trade}\n")))
        .expect("category in the catalog");
    let line = block
        .lines()
        .find(|l| l.trim_start().starts_with("job_types:"))
        .expect("job types");
    let list = line.split_once('[').and_then(|(_, r)| r.split_once(']'));
    list.expect("a job type list").0.split(", ").collect()
}

/// One request: its phrases, the two readings, the trade read or answered and two replies.
fn request() -> impl Strategy<
    Value = (
        String,
        [Reading<'static>; 2],
        &'static str,
        [&'static str; 2],
    ),
> {
    let reading = prop::sample::select(TRADES).prop_flat_map(|trade| {
        let jobs: Vec<Option<&'static str>> = std::iter::once(None)
            .chain(catalog_job_types(trade).into_iter().map(Some))
            .collect();
        prop::sample::select(jobs).prop_map(move |job_type| Reading { trade, job_type })
    });
    let words = prop::collection::vec(pick(PHRASES), 1..4);
    let reply = || pick(REPLIES);
    (
        words,
        reading.clone(),
        reading,
        prop::sample::select(TRADES),
        reply(),
        reply(),
    )
        .prop_map(|(w, a, b, t, r1, r2)| (w.join(" and "), [a, b], t, [r1, r2]))
}

/// The candidates after up to two replies, and whether the homeowner chose a job type.
fn settle(c: &Candidates, replies: [&str; 2]) -> (Candidates, bool) {
    match c.fit_step(replies[0], 1) {
        FitStep::Chosen(x) => (x, true),
        FitStep::KeepAll(x) => (x, false),
        FitStep::AskAgain(_) => match c.fit_step(replies[1], 2) {
            FitStep::Chosen(x) => (x, true),
            FitStep::KeepAll(x) => (x, false),
            again @ FitStep::AskAgain(_) => panic!("a second reply never re-asks: {again:?}"),
        },
    }
}

/// TD-INT-06 for one request: each licensed job type the words name, its own (trade, job type),
/// is still a candidate after the replies, unless the homeowner chose another job type from a
/// "Which fits best?" that offered it; candidates across trades are never sent unasked.
fn check(
    words: &str,
    readings: [Reading<'_>; 2],
    trade: &str,
    replies: [&str; 2],
) -> Result<(), TestCaseError> {
    let kw = keyword_reading(words);
    let c = candidates(trade, readings, &kw);
    let spans = c.job_types.iter().any(|(t, _)| t != &c.job_types[0].0);
    prop_assert!(
        !spans || c.which_fits.is_some(),
        "{words}: asked before a card"
    );
    let offered = c.which_fits.clone().unwrap_or_default();
    let (last, chose) = settle(&c, replies);
    for (t, j) in kw.boundary.iter().chain(&kw.licensed_only) {
        let kept = last.job_types.contains(&((*t).to_owned(), (*j).to_owned()));
        prop_assert!(
            kept || (chose && offered.iter().any(|o| o == j)),
            "{words}: {j}"
        );
    }
    if chose {
        prop_assert_eq!(last.job_types.len(), 1);
        prop_assert!(
            offered.contains(&last.job_types[0].1),
            "{words}: chosen from the list"
        );
    }
    Ok(())
}

proptest! {
    /// TD-INT-06 (INT-014, CARD-001, D-105, D-1003): a licensed keyword job type never reaches a
    /// license that doesn't cover it unless the homeowner chose another job type in "Which fits
    /// best?" (see [`check`]).
    #[test]
    fn td_int_06_licensed_keyword_never_reaches_a_license_without_it(
        (words, readings, trade, replies) in request()
    ) {
        check(&words, readings, trade, replies)?;
    }
}
