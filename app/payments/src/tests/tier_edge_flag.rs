//! TD-FEE-18 (FEE-033, D-070): 3 of the last 10 bases just under a tier edge flag a business, 2
//! do not, $474.99 does not count, only the last 10 count, and the flag is an answer only: no
//! automatic action and nothing sent to a business. Fake amounts only.

use crate::flags::{FlagError, LAST_BASES, TierEdgeFlag, TierEdgeRule, tier_edge_flag};

const RULE: TierEdgeRule = TierEdgeRule::DEFAULT;

/// Ten bases, newest first: `near` of them just under an edge, the rest well clear of both.
fn bases(near: &[u64]) -> Vec<u64> {
    let mut all = near.to_vec();
    all.resize(LAST_BASES, 30_000);
    all
}

/// TD-FEE-18: the edges of the two windows, $475.00 to $499.99 and $1,425.00 to $1,499.99.
#[test]
fn td_fee_18_the_windows_end_just_under_each_edge() {
    for basis in [47_500, 49_999, 142_500, 149_999] {
        assert!(RULE.near_edge(basis), "{basis} is just under an edge");
    }
    for basis in [47_499, 50_000, 142_499, 150_000, 30_000, 200_000] {
        assert!(!RULE.near_edge(basis), "{basis} is not just under an edge");
    }
}

/// TD-FEE-18: 3 of 10 flag, 2 of 10 do not, and $474.99 does not count.
#[test]
fn td_fee_18_three_of_ten_flag_and_two_do_not() {
    assert_eq!(
        tier_edge_flag(RULE, &bases(&[47_500, 149_999, 49_999])),
        TierEdgeFlag::Flagged { near: 3 }
    );
    assert_eq!(
        tier_edge_flag(RULE, &bases(&[142_500, 49_999])),
        TierEdgeFlag::Clear
    );
    assert_eq!(
        tier_edge_flag(RULE, &bases(&[47_499, 149_999, 49_999])),
        TierEdgeFlag::Clear
    );
}

/// TD-FEE-18: only the last 10 bases count; older ones near an edge change nothing.
#[test]
fn td_fee_18_only_the_last_ten_count() {
    let mut all = bases(&[49_999, 49_999]);
    all.extend([49_999, 49_999]);
    assert_eq!(tier_edge_flag(RULE, &all), TierEdgeFlag::Clear);
    assert_eq!(tier_edge_flag(RULE, &[]), TierEdgeFlag::Clear);
}

/// TD-FEE-18, FEE-033: both numbers are settings with checked ranges.
#[test]
fn td_fee_18_both_numbers_are_settings() {
    let wide = TierEdgeRule::new(10, 2).expect("rule");
    assert!(wide.near_edge(45_000));
    assert!(!RULE.near_edge(45_000));
    assert_eq!(
        tier_edge_flag(wide, &bases(&[45_000, 135_000])),
        TierEdgeFlag::Flagged { near: 2 }
    );
    assert_eq!(TierEdgeRule::new(0, 3), Err(FlagError::Percent));
    assert_eq!(TierEdgeRule::new(51, 3), Err(FlagError::Percent));
    assert_eq!(TierEdgeRule::new(5, 0), Err(FlagError::Count));
    assert_eq!(TierEdgeRule::new(5, 11), Err(FlagError::Count));
    assert_eq!(TierEdgeRule::new(5, 3), Ok(RULE));
}

/// TD-FEE-18: no automatic action and nothing sent: the flag is a pure answer over amounts. Its
/// function takes no connection, business or channel, so it can write or send nothing, and the
/// file holds no database or task call.
#[test]
fn td_fee_18_the_flag_acts_on_nothing_and_sends_nothing() {
    let source = include_str!("../flags.rs");
    for call in ["sqlx", "enqueue", "NewTask", "PgConnection", "messaging"] {
        assert!(!source.contains(call), "flags.rs must not use {call}");
    }
    let flagged = bases(&[49_999; 3]);
    assert_eq!(
        tier_edge_flag(RULE, &flagged),
        tier_edge_flag(RULE, &flagged)
    );
}
