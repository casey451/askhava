//! Every photo id the question bank lists resolves to its English ask, at most 3 per set and in the
//! bank's order (INT-044, D-432, D-5251). Reads the compiled-in files; fake data only.

use crate::photo_asks::photo_asks;
use crate::question_bank::QuestionBank;

const BANK: &str = include_str!("../../../../docs/catalog/question_bank.yaml");

fn bank() -> QuestionBank {
    serde_saphyr::from_str(BANK).expect("bank parses")
}

#[test]
fn every_listed_photo_id_resolves_to_a_non_empty_ask() {
    let mut listed = 0;
    for (id, set) in &bank().sets {
        let asks = photo_asks(id);
        assert_eq!(
            asks.len(),
            set.photos.len(),
            "set {id}: an unknown photo id"
        );
        assert!(asks.iter().all(|a| !a.trim().is_empty()), "set {id}");
        listed += asks.len();
    }
    assert!(listed > 0);
}

#[test]
fn at_most_three_asks_per_set() {
    assert!(bank().sets.keys().all(|id| photo_asks(id).len() <= 3));
}

/// The part of a trade file this test reads, parsed on its own terms (not through the loader).
#[derive(serde::Deserialize)]
struct Trade {
    photos_to_request: Vec<Def>,
}

#[derive(serde::Deserialize)]
struct Def {
    id: String,
    ask: String,
}

fn trade_asks(kb_trade: &str) -> Vec<Def> {
    let path = format!(
        "{}/../../docs/catalog/trade_kb/trades/{kb_trade}.yaml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("trade file reads");
    serde_saphyr::from_str::<Trade>(&text)
        .expect("trade file parses")
        .photos_to_request
}

#[test]
fn asks_follow_the_banks_order_id_by_id() {
    let mut checked = 0;
    for (id, set) in &bank().sets {
        let Some(kb) = set.kb_trade.as_deref() else {
            continue;
        };
        let defs = trade_asks(kb);
        let want: Vec<&str> = set
            .photos
            .iter()
            .map(|p| {
                let def = defs.iter().find(|d| &d.id == p);
                def.unwrap_or_else(|| panic!("set {id}: no ask for {p}"))
                    .ask
                    .as_str()
            })
            .collect();
        assert_eq!(photo_asks(id), want, "set {id}");
        checked += want.len();
    }
    assert!(checked > 0);
}

#[test]
fn plumbing_repair_has_three_distinct_asks_in_order() {
    let asks = photo_asks("plumbing_repair");
    assert_eq!(asks.len(), 3);
    assert!(asks[0].contains("water is coming from"));
    assert!(asks[0] != asks[1] && asks[1] != asks[2] && asks[0] != asks[2]);
}

#[test]
fn an_unknown_set_answers_none() {
    assert!(photo_asks("no_such_set").is_empty());
}

#[test]
fn a_set_without_photos_answers_none() {
    // In the current bank every set without photos also has no kb_trade, so this is the only path.
    let bank = bank();
    let mut none = 0;
    for (id, _) in bank.sets.iter().filter(|(_, s)| s.photos.is_empty()) {
        assert!(photo_asks(id).is_empty(), "set {id}");
        none += 1;
    }
    assert!(none > 0);
}
