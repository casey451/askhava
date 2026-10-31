//! "Which fits best?" and the keyword license-boundary candidates (TD-INT-05: INT-013, INT-014;
//! TD-INT-06: INT-014, CARD-001; D-104, D-105, D-1003). Fake text only.
//!
//! A licensed job type named in the homeowner's own words stays a candidate whatever trade is read
//! or answered; candidates across trades fall under different licenses, so "Which fits best?" is
//! asked, and only the homeowner's own choice there narrows them.

use proptest::prelude::*;

use crate::api::{
    Candidates, FitStep, Opinion, Reading, candidates, job_types_of, keyword_reading,
    second_opinion,
};

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
/// and answered as heating/AC keeps the plumbing candidate and asks "Which fits best?".
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
        ("plumbing_repair", "plumbing_repair_other"),
    ];
    assert_eq!(got.job_types, owned(&pairs));
    let options = vec![
        "furnace repair".to_owned(),
        "plumbing_repair_other".to_owned(),
    ];
    assert_eq!(got.which_fits, Some(options.clone()));
    assert_eq!(got.fit_step("huh", 1), FitStep::AskAgain(options));
    let water = Candidates {
        job_types: owned(&[("plumbing_repair", "plumbing_repair_other")]),
        which_fits: None,
    };
    assert_eq!(got.fit_step("the water heater", 1), FitStep::Chosen(water));
    assert_eq!(got.fit_step("not sure", 1), FitStep::KeepAll(got.clone()));
    assert_eq!(got.fit_step("huh", 2), FitStep::KeepAll(got.clone()));
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
            .chain(job_types_of(trade).into_iter().map(Some))
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

proptest! {
    /// TD-INT-06 (INT-014, CARD-001, D-105): a licensed keyword job type stays a candidate, so
    /// only a license covering it gets the card, unless the homeowner chose another job type from
    /// a "Which fits best?" that offered it; candidates across trades are never sent unasked.
    #[test]
    fn td_int_06_licensed_keyword_never_reaches_a_license_without_it(
        (words, readings, trade, replies) in request()
    ) {
        let kw = keyword_reading(&words);
        let c = candidates(trade, readings, &kw);
        let spans = c.job_types.iter().any(|(t, _)| t != &c.job_types[0].0);
        prop_assert!(!spans || c.which_fits.is_some(), "{words}: asked before a card");
        let offered = c.which_fits.clone().unwrap_or_default();
        let (last, chose) = settle(&c, replies);
        for (t, j) in &kw.boundary {
            let kept = last.job_types.contains(&((*t).to_owned(), (*j).to_owned()));
            prop_assert!(kept || (chose && offered.iter().any(|o| o == j)), "{words}: {j}");
        }
        for t in &kw.boundary_trades {
            let kept = last.job_types.iter().any(|(had, _)| had == t);
            let other = format!("{t}_other");
            let shown = offered.iter().any(|o| *o == other || job_types_of(t).contains(&o.as_str()));
            prop_assert!(kept || (chose && shown), "{words}: {t}");
        }
        if chose {
            prop_assert_eq!(last.job_types.len(), 1);
            prop_assert!(offered.contains(&last.job_types[0].1), "{words}: chosen from the list");
        }
    }
}
