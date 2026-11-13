//! The Confirm gate's version floor and wording check (FEE-007, FEE-015, D-072, D-3397): no
//! database; `required_version` keeps a mode switch above every card-save version, and
//! `authorized` needs both the version the terms need and the current wording's text.

use crate::models::AUTHORIZATION_VERSION;
use crate::pick_fee::{authorized, required_version};
use crate::rules::{FeeMode, FeeTerms};

const W2: &str = "Fake authorization wording, second edition.";
const W3: &str = "Fake authorization wording, third edition.";

#[test]
fn required_version_is_the_larger_of_stored_and_the_card_save_version() {
    assert_eq!(
        required_version(AUTHORIZATION_VERSION - 1),
        AUTHORIZATION_VERSION
    );
    assert_eq!(required_version(0), AUTHORIZATION_VERSION);
    assert_eq!(
        required_version(AUTHORIZATION_VERSION),
        AUTHORIZATION_VERSION
    );
    assert_eq!(
        required_version(AUTHORIZATION_VERSION + 1),
        AUTHORIZATION_VERSION + 1
    );
    assert_eq!(required_version(i32::MAX), i32::MAX);
    let top = FeeTerms::new(FeeMode::Tiers, required_version(i32::MAX));
    assert_eq!(top.switched(FeeMode::Flat), None);
}

#[test]
fn a_switch_after_the_floor_is_above_every_card_save_version() {
    for stored in 1..=AUTHORIZATION_VERSION {
        let terms = FeeTerms::new(FeeMode::Tiers, required_version(stored));
        let next = terms.switched(FeeMode::Flat).expect("the version can rise");
        assert!(next.authorization_version() > AUTHORIZATION_VERSION);
    }
}

#[test]
fn no_rows_is_never_authorized() {
    assert!(!authorized(&[], 1, W2));
}

#[test]
fn wording_2_then_switch_3_then_wording_3_needs_the_new_text() {
    let mut rows: Vec<(i32, &str)> = vec![(2, W2)];
    assert!(authorized(&rows, 2, W2));
    let required = FeeTerms::new(FeeMode::Tiers, 2)
        .switched(FeeMode::Flat)
        .expect("the version can rise")
        .authorization_version();
    assert_eq!(required, 3);
    assert!(!authorized(&rows, required, W2));
    rows.push((3, W2));
    assert!(authorized(&rows, required, W2));
    assert!(!authorized(&rows, required, W3));
    rows.push((3, W3));
    assert!(authorized(&rows, required, W3));
}

#[test]
fn the_new_text_below_the_required_version_is_not_enough() {
    let rows: Vec<(i32, &str)> = vec![(2, W2), (2, W3)];
    assert!(!authorized(&rows, 3, W3));
}

#[test]
fn the_text_is_compared_byte_for_byte() {
    let rows: Vec<(i32, &str)> = vec![(3, "Fake authorization wording, third edition. ")];
    assert!(!authorized(&rows, 3, W3));
}
