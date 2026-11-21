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

#[test]
fn asks_follow_the_banks_order() {
    let asks = photo_asks("plumbing_repair");
    assert_eq!(asks.len(), 3);
    assert!(asks[0].contains("water is coming from"));
}

#[test]
fn a_set_with_no_photos_or_unknown_answers_none() {
    assert!(photo_asks("no_such_set").is_empty());
    let bank = bank();
    let empty: Vec<_> = bank
        .sets
        .iter()
        .filter(|(_, s)| s.photos.is_empty())
        .collect();
    assert!(empty.iter().all(|(id, _)| photo_asks(id).is_empty()));
}
