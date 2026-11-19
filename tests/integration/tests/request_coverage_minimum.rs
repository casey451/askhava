//! TD-LIC-34 (T679, T710; LIC-046, INT-017, OPS-006; D-4360, D-4421): a launch-open category
//! takes homeowner requests only with at least `MIN_ACTIVE_BUSINESSES` (1) active businesses,
//! counted category-wide. PostgreSQL, fake IDs and no homeowner data.
//!
//! `quotes::api::request_coverage` reads the active count through
//! `businesses::api::active_in_category` and applies `licensing::api::request_intake`: a category
//! whose only businesses are paused or removed, a "Coming later" category and an off category are
//! `Coverage::NotCovered`; one active business serving the job's town gives the coverage count.
//! Paused and removed businesses are not counted; a business in another town gets no card.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, PauseReason, add_owner, apply, create, request_welcome,
    save_business_name, set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_integration::set_app_role;
use askhava_licensing::api::{ClassMap, import_city_list};
use askhava_ops::api::Actor;
use askhava_quotes::api::{CardFacts, Coverage, MIN_ACTIVE_BUSINESSES, request_coverage};
use jiff::Timestamp;
use jiff::civil::date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
const OTHER_TOWN: &str = "kingman";
/// A launch-open category with no license, so any active business ticking it covers the job.
const CLEAN: (&str, &str) = ("house_cleaning", "deep cleaning");
/// The catalog's off category (D-404).
const OFF: (&str, &str) = ("junk_removal", "junk haul-away");
/// The fake city list: each business below is stored under one of these names, so the
/// no-license categories match it (LIC-011).
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-6791,Sample Tidy One LLC,,,2019-03-04\nCL-6792,Sample Tidy Two LLC,,,2019-03-04\n\
CL-6793,Sample Tidy Three LLC,,,2019-03-04\nCL-6794,Sample Tidy Four LLC,,,2019-03-04\n";
const NAMES: [&str; 4] = ["One", "Two", "Three", "Four"];
/// A catalog with the cleaning category enabled but "Coming later".
const LATER_CATS: &str = "categories:
  - {id: house_cleaning, enabled: true, launch_open: false, job_types: [deep cleaning]}
";

fn at() -> Timestamp {
    "2026-11-02T10:00:00-07:00".parse().expect("time")
}

fn shipped() -> &'static ClassMap {
    ClassMap::shipped().expect("shipped map")
}

/// The catalog and the fake city list; then the app role, as production runs.
async fn setup(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    import_city_list(db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// The `n`th (0 to 3) active, welcomed business, on the city list, serving `town` and ticking
/// `job` (category, job type).
async fn active_business(db: &mut PgConnection, n: usize, town: &str, job: (&str, &str)) -> Uuid {
    let b = create(db, false, at()).await.expect("business");
    add_owner(db, b, Uuid::new_v4(), at()).await.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, at())
        .await
        .expect("active");
    request_welcome(db, b, at()).await.expect("welcome");
    welcome_sent(db, b, at()).await.expect("sent");
    let name = format!("Sample Tidy {} LLC", NAMES[n]);
    save_business_name(db, b, (&name, None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[town], actor).await.expect("towns");
    let choice = JobTypeChoice::new(job.0, job.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
    b
}

/// The coverage for a request for `job` in the town, with `map`.
async fn coverage(db: &mut PgConnection, map: &ClassMap, job: (&str, &str)) -> Coverage {
    let facts = CardFacts {
        category: job.0.to_owned(),
        candidates: vec![job.1.to_owned()],
        emergency: false,
    };
    request_coverage(db, map, ((TOWN, None), &facts), None)
        .await
        .expect("coverage")
}

/// TD-LIC-34, LIC-046, INT-017: a paused and a removed business are not counted, so the request
/// is not covered; one active business serving the job's town opens it and the count is its card;
/// an active one in another town adds no card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_34_paused_or_removed_only_is_not_covered_one_active_gives_the_count(mut db: Db) {
    assert_eq!(MIN_ACTIVE_BUSINESSES, 1);
    setup(&mut db).await;
    let paused = active_business(&mut db, 1, TOWN, CLEAN).await;
    let pause = Event::Pause(PauseReason::Owner);
    apply(&mut db, paused, pause, Actor::System, at())
        .await
        .expect("paused");
    let removed = active_business(&mut db, 2, TOWN, CLEAN).await;
    apply(&mut db, removed, Event::Remove, Actor::System, at())
        .await
        .expect("removed");
    let none = coverage(&mut db, shipped(), CLEAN).await;
    assert_eq!(none, Coverage::NotCovered);
    active_business(&mut db, 0, TOWN, CLEAN).await;
    let one = coverage(&mut db, shipped(), CLEAN).await;
    assert_eq!(one, Coverage::Send(1));
    active_business(&mut db, 3, OTHER_TOWN, CLEAN).await;
    let still = coverage(&mut db, shipped(), CLEAN).await;
    assert_eq!(still, Coverage::Send(1));
}

/// TD-LIC-34, LIC-046: a "Coming later" category and an off one take no request, however many
/// active businesses tick it; the same two businesses under the shipped map give the count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_34_coming_later_and_off_categories_are_not_covered(mut db: Db) {
    setup(&mut db).await;
    for (n, job) in [CLEAN, CLEAN, OFF, OFF].into_iter().enumerate() {
        active_business(&mut db, n, TOWN, job).await;
    }
    let open = coverage(&mut db, shipped(), CLEAN).await;
    assert_eq!(open, Coverage::Send(2));
    let later = ClassMap::load("commercial_only: []\nclasses: []\n", LATER_CATS).expect("map");
    let waitlist = coverage(&mut db, &later, CLEAN).await;
    assert_eq!(waitlist, Coverage::NotCovered);
    let off = coverage(&mut db, shipped(), OFF).await;
    assert_eq!(off, Coverage::NotCovered);
}
