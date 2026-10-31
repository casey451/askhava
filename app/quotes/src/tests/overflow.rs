//! TD-LIC-03 (CARD-004) and the pure cases of TD-CARD-05 (CARD-016) and TD-INT-25, TD-INT-29
//! (INT-017, INT-032): "jobs nobody else covers", the owner pauses and the coverage count,
//! through `rules::match_job`, `rules::card_under` and `rules::coverage`.
//!
//! ROC cover comes from the shipped license map. All IDs are generated and the towns are public
//! place names; no homeowner data.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_licensing::api::Holder;
use sqlx::types::Uuid;

use super::matching_examples::{catalog, job, map};
use crate::api::{
    CardBusiness, CardDecision, CardJob, Coverage, OfferTown, OwnerPauses, Pool, Pooled, Recipient,
    Skip, rules,
};

const HAVASU: &str = "lake_havasu_city";
const KINGMAN: &str = "kingman";

/// A job offered in Lake Havasu City and Kingman with the given (category, job type) candidates.
fn card_job(candidates: &[(&str, &str)]) -> CardJob {
    CardJob {
        towns: [HAVASU, KINGMAN]
            .iter()
            .map(|t| OfferTown {
                code: (*t).to_owned(),
                state: "AZ".to_owned(),
            })
            .collect(),
        candidates: candidates
            .iter()
            .map(|(c, j)| job(catalog(), c, j))
            .collect(),
        text: String::new(),
    }
}

/// An active, verified business serving Lake Havasu City with `classes`, ticking `ticked`.
fn pooled(id: u128, classes: &[&str], ticked: &[(&str, &str)], overflow_on: bool) -> Pooled {
    Pooled {
        card: CardBusiness {
            id: Uuid::from_u128(id),
            status: AccountStatus::Active,
            verified: true,
            towns: BTreeSet::from([HAVASU.to_owned()]),
            ticked: ticked
                .iter()
                .map(|(c, j)| ((*c).to_owned(), (*j).to_owned()))
                .collect(),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            licenses: Holder::default(),
        },
        overflow_on,
    }
}

fn ids(recipients: &[Recipient]) -> Vec<u128> {
    recipients.iter().map(|r| r.business.as_u128()).collect()
}

const AC: (&str, &str) = ("hvac_repair", "AC not cooling");

/// TD-LIC-03 (CARD-004): nobody ticked the job type, so only the licensed businesses with "jobs
/// nobody else covers" on get it: not one with it off, not an unlicensed one, not a paused one.
#[test]
fn td_lic_03_nobody_ticked_goes_to_licensed_overflow_businesses_only() {
    let mut paused = pooled(4, &["R-39"], &[], true);
    paused.card.status = AccountStatus::Paused;
    let pool = [
        pooled(1, &["R-39"], &[], true),
        pooled(2, &["R-39"], &[], false),
        pooled(3, &["R-11"], &[], true),
        paused,
    ];
    let pauses = OwnerPauses::default();
    let got = rules::match_job(
        map(),
        &card_job(&[AC]),
        &Pool {
            businesses: &pool,
            pauses: &pauses,
        },
    );
    assert_eq!(ids(&got), vec![1]);
}

/// TD-LIC-03 (CARD-004): once one business ticked the job type and gets the card, overflow does
/// not apply.
#[test]
fn td_lic_03_one_business_ticked_so_overflow_does_not_apply() {
    let pool = [
        pooled(1, &["R-39"], &[], true),
        pooled(2, &["R-39"], &[AC], false),
    ];
    let pauses = OwnerPauses::default();
    let got = rules::match_job(
        map(),
        &card_job(&[AC]),
        &Pool {
            businesses: &pool,
            pauses: &pauses,
        },
    );
    assert_eq!(ids(&got), vec![2]);
}

/// TD-CARD-05 (CARD-016): the "all" switch, the job's trade switch or the switch of every offer
/// town a business serves gives it no card; a paused town it does not need changes nothing.
#[test]
fn td_card_05_owner_pause_switches_send_nothing() {
    let pool = [pooled(1, &["R-39"], &[AC], false)];
    let job = card_job(&[AC]);
    let send = |pauses: OwnerPauses| {
        ids(&rules::match_job(
            map(),
            &job,
            &Pool {
                businesses: &pool,
                pauses: &pauses,
            },
        ))
    };
    let codes = |c: &str| BTreeSet::from([c.to_owned()]);
    assert!(
        send(OwnerPauses {
            all: true,
            ..OwnerPauses::default()
        })
        .is_empty()
    );
    assert!(
        send(OwnerPauses {
            trades: codes(AC.0),
            ..OwnerPauses::default()
        })
        .is_empty()
    );
    assert!(
        send(OwnerPauses {
            towns: codes(HAVASU),
            ..OwnerPauses::default()
        })
        .is_empty()
    );
    assert_eq!(
        send(OwnerPauses {
            towns: codes(KINGMAN),
            ..OwnerPauses::default()
        }),
        vec![1]
    );
    let all = OwnerPauses {
        all: true,
        ..OwnerPauses::default()
    };
    assert_eq!(
        rules::card_under(map(), &job, &all, (&pool[0].card, false)),
        CardDecision::Skip(Skip::OwnerPaused)
    );
}

/// TD-INT-29, TD-INT-25 (INT-017, INT-032, CARD-001): the count is the match's; a request needing
/// plumbing and electrical with nobody licensed for both is a SPLIT offer and sends no partial
/// card; with no business in town at all it is not covered.
#[test]
fn td_int_29_coverage_counts_the_match_and_offers_split() {
    let pipe = ("plumbing_repair", "faucet replacement");
    let wire = ("electrical", "outlet repair");
    let pauses = OwnerPauses::default();
    let five: Vec<Pooled> = (1..=5)
        .map(|n| pooled(n, &["R-39"], &[AC], false))
        .collect();
    let cover = |job: &CardJob, pool: &[Pooled]| {
        rules::coverage(
            map(),
            job,
            &Pool {
                businesses: pool,
                pauses: &pauses,
            },
        )
    };
    assert_eq!(cover(&card_job(&[AC]), &five), Coverage::Send(5));
    assert_eq!(cover(&card_job(&[AC]), &five[..1]), Coverage::Send(1));
    let parts = [
        pooled(6, &["R-37"], &[pipe], true),
        pooled(7, &["R-11"], &[wire], true),
    ];
    let both = card_job(&[pipe, wire]);
    assert!(
        rules::match_job(
            map(),
            &both,
            &Pool {
                businesses: &parts,
                pauses: &pauses
            }
        )
        .is_empty()
    );
    assert_eq!(cover(&both, &parts), Coverage::Split);
    assert_eq!(cover(&card_job(&[AC]), &parts), Coverage::NotCovered);
}
