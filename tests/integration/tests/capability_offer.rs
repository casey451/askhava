//! The capability checklist offers what the business's licenses or handyman standing allow and
//! follows the stored category switch (TD-BIZ-46, TD-LIC-27; T368, D-2701, D-3101; BIZ-049,
//! BIZ-004, BIZ-021, LIC-002, LIC-012, LIC-014, LIC-015, LIC-016, LIC-019, LIC-027, LIC-046,
//! LIC-047).
//!
//! As the app role, with the catalog stored: a septage hauler, a certified inspector and a pest
//! attester get their own job types pre-ticked and `save_capability` stores them; a city-list
//! handyman gets faucet replacement and camera install unticked and never gas work (M05, M09); an
//! R-37 business keeps the classes-only offer less the handyman trade; an R-11 business is not
//! offered the handyman trade; a city-list business with no license gets nothing pre-ticked; a
//! category switched off leaves the sign-up list, the checklist and `reapply_license`'s offer, and
//! comes back when switched on. All names and numbers are fake.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Capability, CapabilityAnswers, JobTypeChoice, Known, LargestJob, TradeOffer, checklist_offer,
    create, job_types, offer_for, pre_ticked, reapply_license, save_capability,
    stored_category_list, stored_enabled,
};
use askhava_catalog::api::{Catalog, load, parse, set_category_enabled};
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    Agency, CityRecord, ClassMap, PestAttestation, import_city_list, import_roc_list,
    record_agency_license, record_pest_attestation,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use jiff::civil::date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const ROC: &str = "license_number,class,status,issued_on\n\
ROC900037,R-37,Active,2012-05-01\nROC900011,R-11,Active,2014-02-03\n";

const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7001,Example Fixit LLC,Sample Odd Jobs,Pat Placeholder,2019-03-04\n\
CL-7002,Sample Windows LLC,,Sam Placeholder,2020-06-07\n";

const OWNER: Uuid = Uuid::from_u128(0x0368);

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("license map loads")
}

fn now() -> Timestamp {
    "2026-11-12T17:00:00Z".parse().expect("fake time")
}

/// An app-role connection on a database with the catalog and the day's lists stored.
async fn app(pool: &sqlx::PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    load(&mut conn, &catalog()).await.expect("catalog stored");
    import_roc_list(&mut conn, date(2026, 11, 10), ROC)
        .await
        .expect("ROC list");
    import_city_list(&mut conn, date(2026, 11, 10), CITY)
        .await
        .expect("city list");
    conn
}

/// A new business that chose `trades` at sign-up.
async fn business(conn: &mut PgConnection, trades: &[&str]) -> Uuid {
    let id = create(conn, false, now()).await.expect("business");
    let trades: Vec<String> = trades.iter().map(|t| (*t).to_owned()).collect();
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, gen_random_uuid(), now(), $2)",
    )
    .bind(id)
    .bind(&trades)
    .execute(&mut *conn)
    .await
    .expect("sign-up row");
    id
}

async fn offer(conn: &mut PgConnection, id: Uuid, known: Known<'_>) -> Vec<TradeOffer> {
    offer_for(conn, id, known, (map(), &catalog()))
        .await
        .expect("offer")
}

fn roc(number: &'static str) -> Known<'static> {
    Known {
        roc_license: Some(number),
        city_names: None,
    }
}

fn set(category: &str, job_types: &[&str]) -> BTreeSet<JobTypeChoice> {
    job_types
        .iter()
        .map(|t| JobTypeChoice::new(category, t))
        .collect()
}

fn trades(offer: &[TradeOffer]) -> Vec<&str> {
    offer.iter().map(|t| t.category.as_str()).collect()
}

/// The unticked rows of `offer`.
fn extras(offer: &[TradeOffer]) -> BTreeSet<JobTypeChoice> {
    let rows = offer.iter().flat_map(|t| &t.rows);
    rows.filter(|r| !r.licensed)
        .map(|r| r.choice.clone())
        .collect()
}

/// Saves `ticked` from `offer` and returns what is stored.
async fn save(
    conn: &mut PgConnection,
    id: Uuid,
    offer: &[TradeOffer],
    ticked: Vec<JobTypeChoice>,
) -> BTreeSet<JobTypeChoice> {
    let answers = CapabilityAnswers {
        ticked,
        towns: vec!["lake_havasu_city".to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours: false,
    };
    save_capability(conn, id, offer, &answers, (Actor::Business(id), now()))
        .await
        .expect("saved");
    job_types(conn, id)
        .await
        .expect("read")
        .into_iter()
        .collect()
}

/// Pre-ticks exactly `expected`, offers no handyman trade, and stores the pre-ticked rows.
async fn pre_ticks_and_stores(
    conn: &mut PgConnection,
    id: Uuid,
    expected: BTreeSet<JobTypeChoice>,
) {
    let offer = offer(conn, id, Known::default()).await;
    let ticked = pre_ticked(&offer);
    assert_eq!(ticked.iter().cloned().collect::<BTreeSet<_>>(), expected);
    assert!(!trades(&offer).contains(&"handyman"));
    assert_eq!(save(conn, id, &offer, ticked).await, expected);
}

/// TD-BIZ-46 (LIC-015, LIC-016, BIZ-049, LIC-027): a septage hauler and a certified inspector get
/// their category's job types pre-ticked, and saving stores them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_46_agency_licensees_get_their_job_types(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let hauler = business(&mut conn, &["septic_pumping"]).await;
    let inspector = business(&mut conn, &["home_inspection"]).await;
    let licenses = [
        (hauler, Agency::SeptageHauler, "SH 0368"),
        (inspector, Agency::HomeInspector, "HI-0368"),
    ];
    for (id, agency, number) in licenses {
        record_agency_license(&mut conn, id, (agency, number), Actor::System)
            .await
            .expect("recorded");
    }
    let septic = set("septic_pumping", &["tank pumping", "tank inspection"]);
    pre_ticks_and_stores(&mut conn, hauler, septic).await;
    let inspections = set(
        "home_inspection",
        &[
            "home condition inspection (no sale or refinance)",
            "buyer inspection",
            "pre-listing inspection",
            "refinance inspection",
            "termite report for a sale or loan",
        ],
    );
    pre_ticks_and_stores(&mut conn, inspector, inspections).await;
}

/// TD-BIZ-46 (LIC-012, LIC-047): a pest attester gets general pest service and scorpions
/// pre-ticked; termites only with the wood-destroying organism record category.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_46_pest_attester_gets_its_record_categories(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let plain = business(&mut conn, &["pest_control"]).await;
    let wood = business(&mut conn, &["pest_control"]).await;
    for (id, wdo) in [(plain, false), (wood, true)] {
        let attested = PestAttestation::new(true, "PM-0368", wdo, false).expect("attestation");
        record_pest_attestation(&mut conn, id, &attested, Actor::Business(id))
            .await
            .expect("recorded");
    }
    let general = ["general pest service", "scorpions"];
    pre_ticks_and_stores(&mut conn, plain, set("pest_control", &general)).await;
    let with_termites = set(
        "pest_control",
        &["general pest service", "scorpions", "termites"],
    );
    pre_ticks_and_stores(&mut conn, wood, with_termites).await;
}

/// TD-BIZ-46 (LIC-014, LIC-019, M05, M09): a city-list handyman gets faucet replacement and
/// camera install unticked, never gas work and nothing pre-ticked; saving them stores them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_46_city_list_handyman_gets_handyman_extras(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["handyman"]).await;
    let names = CityRecord {
        business_name: "Example Fixit LLC".to_owned(),
        ..CityRecord::default()
    };
    let known = Known {
        roc_license: None,
        city_names: Some(&names),
    };
    let offer = offer(&mut conn, id, known).await;
    assert!(pre_ticked(&offer).is_empty());
    let unticked = extras(&offer);
    let faucet = JobTypeChoice::new("plumbing_repair", "faucet replacement");
    let camera = JobTypeChoice::new("security_cameras", "camera install");
    assert!(unticked.contains(&faucet) && unticked.contains(&camera));
    assert!(trades(&offer).contains(&"handyman"));
    let gas = |c: &JobTypeChoice| c.job_type.contains("gas");
    assert!(
        !unticked.iter().any(gas),
        "no gas line work or gas pool heater"
    );
    assert!(!trades(&offer).contains(&"electrical"));
    let stored = save(&mut conn, id, &offer, vec![faucet.clone(), camera.clone()]).await;
    assert_eq!(stored, [faucet, camera].into_iter().collect());
}

/// TD-BIZ-46 (BIZ-049, LIC-043): an R-37 business's offer is the classes-only offer less the
/// handyman trade; an R-11 business is not offered the handyman trade.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_46_roc_offers_are_unchanged_but_for_handyman(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let plumber = business(&mut conn, &["plumbing_repair"]).await;
    let got = offer(&mut conn, plumber, roc("ROC900037")).await;
    let mut before = checklist_offer(map(), &catalog(), &["R-37"]);
    before.retain(|t| t.category != "handyman");
    assert_eq!(got, before);
    assert!(
        pre_ticked(&got)
            .iter()
            .all(|c| c.category == "plumbing_repair")
    );
    assert!(!pre_ticked(&got).is_empty());
    let electrician = business(&mut conn, &["electrical"]).await;
    let got = offer(&mut conn, electrician, roc("ROC900011")).await;
    assert!(!trades(&got).contains(&"handyman"));
    assert_eq!(got[0].category, "electrical");
    assert!(got[0].rows.iter().all(|r| r.licensed));
}

/// TD-BIZ-46 (LIC-011, BIZ-049): a city-list business with no license gets no pre-ticked row,
/// only no-license extras, and no handyman trade.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_46_city_list_business_gets_nothing_pre_ticked(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["window_cleaning"]).await;
    let names = CityRecord {
        business_name: "Sample Windows LLC".to_owned(),
        ..CityRecord::default()
    };
    let known = Known {
        roc_license: None,
        city_names: Some(&names),
    };
    let offer = offer(&mut conn, id, known).await;
    assert!(pre_ticked(&offer).is_empty());
    let windows = JobTypeChoice::new("window_cleaning", "inside and outside windows");
    assert!(extras(&offer).contains(&windows));
    assert!(!trades(&offer).contains(&"handyman"));
    assert!(!trades(&offer).contains(&"plumbing_repair"));
}

/// Switches `code` on or off as the owner.
async fn switch(conn: &mut PgConnection, code: &str, on: bool) {
    let changed = set_category_enabled(conn, code, on, Actor::Owner(OWNER)).await;
    assert_eq!(changed, Ok(true), "{code} switched");
}

/// The sign-up list for plumbing and pool cleaning by the stored switch.
async fn listed(conn: &mut PgConnection) -> Vec<(String, &'static str)> {
    let enabled = stored_enabled(conn).await.expect("stored switch");
    stored_category_list(map(), &enabled, &["plumbing_repair", "pool_cleaning"])
}

/// TD-LIC-27 (LIC-002, LIC-046, BIZ-021): a switched-off category leaves the sign-up list, the
/// checklist and `reapply_license`'s offer; switched on again it comes back "Open now".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_27_switched_off_category_leaves_every_offer(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["electrical"]).await;
    let both = vec![
        ("plumbing_repair".to_owned(), "Open now"),
        ("pool_cleaning".to_owned(), "Open now"),
    ];
    assert_eq!(listed(&mut conn).await, both);
    switch(&mut conn, "plumbing_repair", false).await;
    switch(&mut conn, "pool_cleaning", false).await;
    assert_eq!(listed(&mut conn).await, vec![]);
    let off = offer(&mut conn, id, roc("ROC900011")).await;
    assert!(!trades(&off).contains(&"pool_cleaning"));
    let classes: (&[&str], &[&str]) = (&["R-11"], &["R-11", "R-37"]);
    let when = (date(2026, 11, 11), now());
    let maps = (map(), &catalog());
    let change = reapply_license(&mut conn, id, maps, classes, when)
        .await
        .expect("re-applied");
    assert!(
        change.offered.is_empty(),
        "a switched-off trade is never offered"
    );
    switch(&mut conn, "plumbing_repair", true).await;
    switch(&mut conn, "pool_cleaning", true).await;
    assert_eq!(listed(&mut conn).await, both);
    let on = offer(&mut conn, id, roc("ROC900011")).await;
    assert!(trades(&on).contains(&"pool_cleaning"));
    let change = reapply_license(&mut conn, id, maps, classes, when)
        .await
        .expect("re-applied");
    assert!(!change.offered.is_empty());
    assert!(
        change
            .offered
            .iter()
            .all(|c| c.category == "plumbing_repair")
    );
}
