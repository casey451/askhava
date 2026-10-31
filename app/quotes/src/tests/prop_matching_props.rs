//! TD-LIC-02 property tests (CARD-001, CARD-002, CARD-003, LIC-035; rust brain R17): for generated
//! businesses and jobs, a card goes exactly when the business is active, verified, serves an
//! Arizona offer town (its own or a neighbouring one), its classes cover every candidate job type
//! as the license map says and it ticked one; any number of qualified businesses all get one.
//!
//! The oracle reads the license map per class and job type and whether the generated status is
//! the active one, never `rules`. Towns and IDs are public place names and generated values; no homeowner data.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_licensing::api::Holder;
use proptest::prelude::*;
use sqlx::types::Uuid;

use super::matching_examples::{catalog, job, map};
use crate::api::{CardBusiness, CardDecision, CardJob, OfferTown, Skip, rules};

const CATEGORY: &str = "hvac_repair";
const CLASSES: [&str; 5] = [
    "R-39",
    "R-39R Air Conditioning and Refrigeration",
    "R-39R Warm Air Heating, Evaporative Cooling and Ventilating",
    "R-39R Evaporative Cooling and Ventilators",
    "R-11",
];
const TOWNS: [&str; 3] = ["lake_havasu_city", "kingman", "needles"];
/// Every status but `Active`, named so a new status must be placed here or there (CARD-016).
const NOT_ACTIVE: [AccountStatus; 5] = [
    AccountStatus::Pending,
    AccountStatus::Paused,
    AccountStatus::Waitlist,
    AccountStatus::NeedsReview,
    AccountStatus::Removed,
];

/// `Active` when `active`, else the `other`th status of [`NOT_ACTIVE`].
fn status(active: bool, other: usize) -> AccountStatus {
    if active {
        AccountStatus::Active
    } else {
        NOT_ACTIVE[other % NOT_ACTIVE.len()]
    }
}

/// The first five hvac job types of the catalog.
fn job_types() -> Vec<String> {
    let catalog = catalog();
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == CATEGORY)
        .expect("hvac");
    def.job_types.iter().take(5).cloned().collect()
}

fn pick<T: Clone>(all: &[T], mask: u8) -> Vec<T> {
    all.iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .map(|(_, v)| v.clone())
        .collect()
}

/// The job: candidates by mask (at least one), offered in Lake Havasu City, then Kingman
/// (a neighbouring Arizona town) and Needles, CA, when their bits are set.
fn card_job(candidates: u8, offer: u8) -> CardJob {
    let towns = [(TOWNS[0], "AZ"), (TOWNS[1], "AZ"), (TOWNS[2], "CA")];
    let offered = pick(&towns, offer | 1);
    let catalog = catalog();
    CardJob {
        towns: offered
            .into_iter()
            .map(|(code, state)| OfferTown {
                code: code.to_owned(),
                state: state.to_owned(),
            })
            .collect(),
        candidates: pick(&job_types(), candidates | 1)
            .iter()
            .map(|jt| job(catalog, CATEGORY, jt))
            .collect(),
        text: String::new(),
    }
}

fn business(
    id: u128,
    status: AccountStatus,
    verified: bool,
    towns: u8,
    classes: u8,
    ticked: u8,
) -> CardBusiness {
    CardBusiness {
        id: Uuid::from_u128(id),
        status,
        verified,
        towns: pick(&TOWNS, towns).into_iter().map(str::to_owned).collect(),
        ticked: pick(&job_types(), ticked)
            .into_iter()
            .map(|jt| (CATEGORY.to_owned(), jt))
            .collect(),
        classes: pick(&CLASSES, classes)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        licenses: Holder::default(),
    }
}

/// The TD-LIC-02 oracle, written from CARD-001, CARD-002 and INT-022; `active` is whether the
/// business was given the `Active` status.
fn oracle(job: &CardJob, b: &CardBusiness, active: bool) -> bool {
    let classes: Vec<&str> = b.classes.iter().map(String::as_str).collect();
    let serves = job
        .towns
        .iter()
        .any(|t| t.state == "AZ" && b.towns.contains(&t.code));
    let covered = job
        .candidates
        .iter()
        .all(|c| map().covers(&classes, CATEGORY, &c.job_type));
    let ticked = job.candidates.iter().any(|c| {
        b.ticked
            .contains(&(CATEGORY.to_owned(), c.job_type.clone()))
    });
    active && b.verified && serves && covered && ticked
}

proptest! {
    /// TD-LIC-02 (CARD-001, CARD-002, LIC-035): a card goes exactly when the oracle says so, and
    /// never as a handyman card to a business that is not a handyman.
    #[test]
    fn td_lic_02_card_exactly_when_qualified(
        candidates in 0u8..32, offer in 0u8..8, active: bool, other in 0usize..5,
        verified: bool, towns in 0u8..8, classes in 0u8..32, ticked in 0u8..32,
    ) {
        let job = card_job(candidates, offer);
        let b = business(1, status(active, other), verified, towns, classes, ticked);
        let got = rules::card_for(map(), &job, &b);
        prop_assert_ne!(got, CardDecision::HandymanCard);
        prop_assert_eq!(got == CardDecision::Card, oracle(&job, &b, active));
    }

    /// TD-LIC-02 (CARD-002): a business serving only the neighbouring Arizona town gets the card by
    /// the same rule as one in the job's own town; one serving only a town outside Arizona never.
    #[test]
    fn td_lic_02_neighbouring_towns_follow_the_same_rule(
        candidates in 0u8..32, classes in 0u8..32, ticked in 0u8..32,
    ) {
        let job = card_job(candidates, 0b110);
        let active = AccountStatus::Active;
        let own = business(1, active, true, 0b001, classes, ticked);
        let neighbour = business(2, active, true, 0b010, classes, ticked);
        let outside = business(3, active, true, 0b100, classes, ticked);
        let decide = |b: &CardBusiness| rules::card_for(map(), &job, b);
        prop_assert_eq!(decide(&own), decide(&neighbour));
        prop_assert_eq!(decide(&outside), CardDecision::Skip(Skip::OutOfTown));
    }

    /// TD-LIC-02 (CARD-003): n qualified businesses among unqualified ones get exactly n cards,
    /// whatever n; there is no cap.
    #[test]
    fn td_lic_02_no_cap_on_recipients(n in 0u128..120, unqualified in 0u128..10) {
        let job = card_job(0, 0);
        let active = AccountStatus::Active;
        let mut all: Vec<CardBusiness> =
            (0..n).map(|i| business(i, active, true, 1, 1, 1)).collect();
        all.extend((0..unqualified).map(|i| business(1000 + i, active, false, 1, 1, 1)));
        let got = rules::recipients(map(), &job, &all);
        prop_assert_eq!(got.len() as u128, n);
        let ids: BTreeSet<Uuid> = got.iter().map(|r| r.business).collect();
        prop_assert_eq!(ids.len() as u128, n);
    }
}

/// TD-LIC-02 (CARD-003): exactly 50 qualified businesses get 50 cards.
#[test]
fn td_lic_02_fifty_qualified_get_fifty_cards() {
    let job = card_job(0, 0);
    let all: Vec<CardBusiness> = (0..50)
        .map(|i| business(i, AccountStatus::Active, true, 1, 1, 1))
        .collect();
    assert_eq!(rules::recipients(map(), &job, &all).len(), 50);
}

/// CARD-001, CARD-016: only `Active` receives cards; every other status, named, is skipped first.
#[test]
fn only_the_active_status_receives_cards() {
    assert!(AccountStatus::Active.receives_cards());
    let job = card_job(0, 0);
    for s in NOT_ACTIVE {
        assert!(!s.receives_cards(), "{s:?}");
        let b = business(1, s, true, 1, 1, 1);
        let got = rules::card_for(map(), &job, &b);
        assert_eq!(got, CardDecision::Skip(Skip::NotActive), "{s:?}");
    }
    let every = AccountStatus::ALL.len();
    assert_eq!(NOT_ACTIVE.len() + 1, every);
}
