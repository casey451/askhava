//! TD-LIC-02 (CARD-001, LIC-035): the matching worked examples M01 to M16 of
//! `docs/catalog/matching_examples.yaml`, written before this code (D-110), as parametrized cases
//! through `rules::card_for`, and CARD-003's no cap through `send_cards` on a fresh database.
//!
//! ROC cover comes from the shipped license map, never from the oracle's `map_covers`, so a map
//! or matching mistake fails here. All IDs are generated; no homeowner data.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use askhava_businesses::api::AccountStatus;
use askhava_catalog::api::{Catalog, parse};
use askhava_licensing::api::{Agency, CityListStanding, ClassMap, Holder, Job, LicenseKind};
use jiff::Timestamp;
use serde::Deserialize;
use sqlx::types::Uuid;

use crate::api::{
    CardBusiness, CardDecision, CardJob, OfferTown, Skip, card_holders, rules, send_cards,
};

const EXAMPLES: &str = include_str!("../../../../docs/catalog/matching_examples.yaml");
const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");

#[derive(Deserialize)]
struct Examples {
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    id: String,
    job: ExampleJob,
    business: ExampleBusiness,
    expected: Expected,
}

#[derive(Deserialize)]
struct ExampleJob {
    category: String,
    candidates: Vec<String>,
    town: String,
}

#[derive(Deserialize)]
struct ExampleBusiness {
    roc: Vec<String>,
    agencies: Vec<String>,
    handyman: bool,
    city_list: bool,
    ticked: Vec<String>,
    towns: Vec<String>,
}

#[derive(Deserialize)]
struct Expected {
    outcome: String,
    condition: Option<String>,
}

/// A place name as a catalog town code: `Lake Havasu City` is `lake_havasu_city`.
pub(super) fn code(town: &str) -> String {
    town.to_lowercase().replace(' ', "_")
}

/// The catalog's license rule for one job type of `category` (LIC-001, LIC-043).
pub(super) fn job(catalog: &Catalog, category: &str, job_type: &str) -> Job {
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == category)
        .expect("category in the catalog");
    Job {
        category: category.to_owned(),
        job_type: job_type.to_owned(),
        license: LicenseKind::parse(&def.license.kind).expect("license kind"),
        handyman_allowed: def.handyman_allowed,
        listed: def.job_types.iter().any(|j| j == job_type),
    }
}

pub(super) fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("the shipped license map loads")
}

/// The shipped catalog, parsed once.
pub(super) fn catalog() -> &'static Catalog {
    static CATALOG: LazyLock<Catalog> =
        LazyLock::new(|| parse(CATEGORIES).expect("the catalog parses"));
    &CATALOG
}

fn business(ex: &Example, n: u128) -> CardBusiness {
    let b = &ex.business;
    let agencies = b
        .agencies
        .iter()
        .map(|n| Agency::from_map_name(n).expect("agency"));
    CardBusiness {
        id: Uuid::from_u128(n),
        status: AccountStatus::Active,
        verified: true,
        towns: b.towns.iter().map(|t| code(t)).collect(),
        ticked: b
            .ticked
            .iter()
            .map(|t| (ex.job.category.clone(), t.clone()))
            .collect(),
        classes: b.roc.clone(),
        licenses: Holder {
            agencies: agencies.collect(),
            handyman: b.handyman,
            city_list: if b.city_list {
                CityListStanding::Matched
            } else {
                CityListStanding::Pending
            },
            ..Holder::default()
        },
    }
}

fn card_job(catalog: &Catalog, ex: &ExampleJob) -> CardJob {
    CardJob {
        towns: vec![OfferTown {
            code: code(&ex.town),
            state: "AZ".to_owned(),
        }],
        candidates: ex
            .candidates
            .iter()
            .map(|c| job(catalog, &ex.category, c))
            .collect(),
        text: String::new(),
    }
}

/// TD-LIC-02 (CARD-001, LIC-035): M01 to M16, each through `card_for`. A card with a condition
/// must be a handyman card; `refused_at_intake` (M14) must also get no card here.
#[test]
fn td_lic_02_matching_examples_m01_to_m16() {
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let catalog = catalog();
    let mut ids = BTreeSet::new();
    for ex in &examples.examples {
        let got = rules::card_for(map(), &card_job(catalog, &ex.job), &business(ex, 1));
        let ok = match (
            ex.expected.outcome.as_str(),
            ex.expected.condition.is_some(),
        ) {
            ("card", true) => got == CardDecision::HandymanCard,
            ("card", false) => got == CardDecision::Card,
            ("refused_at_intake", _) => matches!(got, CardDecision::Skip(_)),
            _ => matches!(
                got,
                CardDecision::Skip(Skip::NotCovered(_) | Skip::OutOfTown)
            ),
        };
        assert!(ok, "{}: got {got:?}", ex.id);
        ids.insert(ex.id.clone());
    }
    let want: BTreeSet<String> = (1..=16).map(|n| format!("M{n:02}")).collect();
    assert!(
        want.is_subset(&ids),
        "missing examples: {:?}",
        want.difference(&ids)
    );
}

/// TD-LIC-02 (CARD-001, LIC-035): the M15 business gets the card for its covered job type alone,
/// so the miss comes from the uncovered furnace candidate, and M16's comes from the town.
#[test]
fn td_lic_02_every_candidate_must_be_covered_and_the_town_served() {
    let catalog = catalog();
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let find = |id: &str| {
        examples
            .examples
            .iter()
            .find(|e| e.id == id)
            .expect("example")
    };
    let m15 = find("M15");
    let mut one = card_job(catalog, &m15.job);
    one.candidates.truncate(1);
    assert_eq!(
        rules::card_for(map(), &one, &business(m15, 1)),
        CardDecision::Card
    );
    let m16 = find("M16");
    let got = rules::card_for(map(), &card_job(catalog, &m16.job), &business(m16, 1));
    assert_eq!(got, CardDecision::Skip(Skip::OutOfTown));
}

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

/// TD-LIC-02 (CARD-003): 50 qualified businesses get 50 recorded cards, one unqualified gets
/// none, and a second run records nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_02_fifty_qualified_businesses_get_fifty_cards(mut db: Db) {
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let m01 = examples
        .examples
        .iter()
        .find(|e| e.id == "M01")
        .expect("M01");
    let job = card_job(catalog(), &m01.job);
    let mut all: Vec<CardBusiness> = (1..=50).map(|n| business(m01, n)).collect();
    let mut paused = business(m01, 51);
    paused.status = AccountStatus::Paused;
    all.push(paused);
    let job_id = Uuid::from_u128(0x10b);
    let now = Timestamp::from_second(1_800_000_000).expect("time");
    let chosen = send_cards(&mut db, map(), job_id, &job, &all, now)
        .await
        .expect("send");
    assert_eq!(chosen.len(), 50);
    send_cards(&mut db, map(), job_id, &job, &all, now)
        .await
        .expect("send again");
    let mut want: Vec<Uuid> = all[..50].iter().map(|b| b.id).collect();
    want.sort();
    assert_eq!(card_holders(&mut db, job_id).await.expect("holders"), want);
}
