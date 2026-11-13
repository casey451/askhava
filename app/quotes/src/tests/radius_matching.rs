//! TD-CARD-13's pure cases (CARD-024, CARD-001, CARD-002, INT-022, INT-017, INT-032; D-1519):
//! radius matching through `rules::card_for_in_reach`, `rules::match_job_held_in_reach` and
//! `rules::coverage_in_reach`, with a Phoenix metro fixture of several towns.
//!
//! The centroids are fixture points near each town's public map location, not the reviewed table;
//! ROC cover comes from the shipped license map. All IDs are generated; no homeowner data.

use std::collections::{BTreeMap, BTreeSet};

use askhava_businesses::api::AccountStatus;
use askhava_catalog::api::{Centroid, miles_between};
use askhava_licensing::api::Holder;
use sqlx::types::Uuid;

use super::matching_examples::{catalog, job, map};
use crate::api::{
    BusinessReach, CardBusiness, CardDecision, CardJob, Coverage, OfferTown, OwnerPauses, Pool,
    Pooled, Reach, Recipient, Skip, rules, within_radius,
};

const PHOENIX: Centroid = Centroid {
    lat: 33.4484,
    lon: -112.0740,
};
const TEMPE: Centroid = Centroid {
    lat: 33.4255,
    lon: -111.9400,
};
const MESA: Centroid = Centroid {
    lat: 33.4152,
    lon: -111.8315,
};
const SCOTTSDALE: Centroid = Centroid {
    lat: 33.4942,
    lon: -111.9261,
};
const GLENDALE: Centroid = Centroid {
    lat: 33.5387,
    lon: -112.1860,
};

const AC: (&str, &str) = ("hvac_repair", "AC not cooling");

/// A job in `town` (state `state`) for the AC candidate.
fn card_job(town: &str, state: &str) -> CardJob {
    CardJob {
        towns: vec![OfferTown {
            code: town.to_owned(),
            state: state.to_owned(),
        }],
        candidates: vec![job(catalog(), AC.0, AC.1)],
        text: String::new(),
    }
}

/// An active, verified business `id` serving `town` with `classes`, ticking the AC job type.
fn business(id: u128, town: &str, classes: &[&str]) -> Pooled {
    Pooled {
        card: CardBusiness {
            id: Uuid::from_u128(id),
            status: AccountStatus::Active,
            verified: true,
            towns: BTreeSet::from([town.to_owned()]),
            ticked: BTreeSet::from([(AC.0.to_owned(), AC.1.to_owned())]),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            licenses: Holder::default(),
        },
        overflow_on: false,
    }
}

/// A reach for a job at `job` with each `(id, base, radius)`.
fn reach(job: Option<Centroid>, bases: &[(u128, Centroid, u8)]) -> Reach {
    Reach {
        job,
        businesses: bases
            .iter()
            .map(|(id, base, radius_miles)| {
                let r = BusinessReach {
                    base: *base,
                    radius_miles: *radius_miles,
                };
                (Uuid::from_u128(*id), r)
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

fn ids(recipients: &[Recipient]) -> Vec<u128> {
    recipients.iter().map(|r| r.business.as_u128()).collect()
}

/// The Phoenix metro fixture's recipients and coverage for a job in Phoenix at `job_at`.
fn metro(job_at: Option<Centroid>, radii: &[(u128, Centroid, u8)]) -> (Vec<u128>, Coverage) {
    let pool = [
        business(1, "phoenix", &["R-39"]),
        business(2, "tempe", &["R-39"]),
        business(3, "mesa", &["R-39"]),
        business(4, "scottsdale", &["R-39"]),
        business(5, "glendale", &["R-39"]),
    ];
    let pauses = OwnerPauses::default();
    let pool = Pool {
        businesses: &pool,
        pauses: &pauses,
    };
    let job = card_job("phoenix", "AZ");
    let reach = reach(job_at, radii);
    let got = rules::match_job_held_in_reach(map(), &job, &pool, &[], &reach);
    let count = rules::coverage_in_reach(map(), &job, &pool, &reach);
    (ids(&got), count)
}

/// TD-CARD-13 (CARD-024): a Tempe business whose radius reaches the Phoenix job gets the card
/// across the town line; a Mesa business whose radius stops short does not; the Phoenix business
/// gets it by its town whatever its radius; the coverage count equals the recipients.
#[test]
fn td_card_13_phoenix_metro_card_crosses_a_town_line_and_one_does_not() {
    assert!(miles_between(TEMPE, PHOENIX) < 10.0);
    assert!(miles_between(MESA, PHOENIX) > 5.0);
    let radii = [(1, PHOENIX, 1), (2, TEMPE, 10), (3, MESA, 5)];
    let (got, count) = metro(Some(PHOENIX), &radii);
    assert_eq!(got, vec![1, 2]);
    assert_eq!(count, Coverage::Send(got.len()));
}

/// TD-CARD-13 (CARD-024): two businesses in different towns with overlapping radii both get it.
#[test]
fn td_card_13_overlapping_radii_both_get_the_card() {
    let radii = [(4, SCOTTSDALE, 15), (5, GLENDALE, 15), (2, TEMPE, 15)];
    let (got, count) = metro(Some(PHOENIX), &radii);
    assert_eq!(got, vec![1, 2, 4, 5]);
    assert_eq!(count, Coverage::Send(4));
}

/// TD-CARD-13 (CARD-024, CARD-001): a job with no location, and businesses with no reach (a base
/// ZIP with no centroid is left out of `Reach`), match by towns only; so does the empty `Reach`
/// every function without `_in_reach` passes.
#[test]
fn td_card_13_no_location_means_towns_only() {
    let radii = [(2, TEMPE, 100), (3, MESA, 100)];
    assert_eq!(metro(None, &radii), (vec![1], Coverage::Send(1)));
    assert_eq!(metro(Some(PHOENIX), &[]), (vec![1], Coverage::Send(1)));
    let job = card_job("phoenix", "AZ");
    let tempe = business(2, "tempe", &["R-39"]).card;
    assert_eq!(
        rules::card_for(map(), &job, &tempe),
        CardDecision::Skip(Skip::OutOfTown)
    );
}

/// TD-CARD-13 (CARD-024): the boundary is inclusive: a distance equal to the radius is in reach;
/// the fixture points just inside and just outside 10 miles of the base fall on either side.
#[test]
fn td_card_13_boundary_is_inclusive() {
    assert!(within_radius(10.0, 10));
    assert!(!within_radius(10.000_001, 10));
    let base = Centroid {
        lat: 33.45,
        lon: -112.07,
    };
    let degrees_per_mile = 1.0 / 3958.8 * (180.0 / std::f64::consts::PI);
    let at = |miles: f64| Centroid {
        lat: base.lat + miles * degrees_per_mile,
        lon: base.lon,
    };
    let inside = reach(Some(at(9.999_99)), &[(7, base, 10)]);
    let outside = reach(Some(at(10.000_01)), &[(7, base, 10)]);
    assert!(inside.reaches(Uuid::from_u128(7)));
    assert!(!outside.reaches(Uuid::from_u128(7)));
}

/// TD-CARD-13 (CARD-024, CARD-001, LIC-043): a business in reach without the license for the
/// candidate job type, or with none ticked, does not get the card; the license rule still decides.
#[test]
fn td_card_13_in_reach_still_needs_the_license_and_a_tick() {
    let job = card_job("phoenix", "AZ");
    let reach = reach(Some(PHOENIX), &[(2, TEMPE, 20), (3, TEMPE, 20)]);
    let unlicensed = business(2, "tempe", &["R-11"]).card;
    assert!(matches!(
        rules::card_for_in_reach(map(), &job, &unlicensed, &reach),
        CardDecision::Skip(Skip::NotCovered(_))
    ));
    let mut unticked = business(3, "tempe", &["R-39"]).card;
    unticked.ticked.clear();
    assert_eq!(
        rules::card_for_in_reach(map(), &job, &unticked, &reach),
        CardDecision::Skip(Skip::NothingTicked)
    );
    let mut paused = business(3, "tempe", &["R-39"]).card;
    paused.status = AccountStatus::Paused;
    assert_eq!(
        rules::card_for_in_reach(map(), &job, &paused, &reach),
        CardDecision::Skip(Skip::NotActive)
    );
}

/// TD-CARD-13 (CARD-024, INT-022): only Arizona towns are matched; a radius never reaches a job
/// whose own town is outside Arizona.
#[test]
fn td_card_13_only_arizona_jobs_are_reached_by_radius() {
    let job = card_job("needles", "CA");
    let reach = reach(Some(PHOENIX), &[(2, TEMPE, 50)]);
    let tempe = business(2, "tempe", &["R-39"]).card;
    assert_eq!(
        rules::card_for_in_reach(map(), &job, &tempe, &reach),
        CardDecision::Skip(Skip::OutOfTown)
    );
}

/// TD-CARD-13 (CARD-024, CARD-016): the owner pauses still apply to a business in reach.
#[test]
fn td_card_13_owner_pause_still_applies_in_reach() {
    let pool = [business(2, "tempe", &["R-39"])];
    let pauses = OwnerPauses {
        all: true,
        ..OwnerPauses::default()
    };
    let pool = Pool {
        businesses: &pool,
        pauses: &pauses,
    };
    let reach = reach(Some(PHOENIX), &[(2, TEMPE, 20)]);
    let job = card_job("phoenix", "AZ");
    assert!(rules::match_job_held_in_reach(map(), &job, &pool, &[], &reach).is_empty());
}

/// TD-CARD-13 (D-1519): no geocoding crate is in the dependency tree; distances come only from
/// the reviewed centroid table and `catalog::api::miles_between`.
#[test]
fn td_card_13_no_geocoding_crate_in_the_dependency_tree() {
    let lock = include_str!("../../../../Cargo.lock");
    let names: Vec<&str> = lock
        .lines()
        .filter_map(|l| l.strip_prefix("name = \""))
        .map(|n| n.trim_end_matches('"'))
        .collect();
    assert!(names.len() > 10);
    for name in names {
        assert!(
            !(name.starts_with("geo") || name.contains("haversine") || name.contains("geocod")),
            "{name}"
        );
    }
}
