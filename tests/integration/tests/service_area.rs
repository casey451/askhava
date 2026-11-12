//! The service radius at sign-up and the markets data it rests on (TD-BIZ-37: BIZ-058, BIZ-049;
//! TD-INT-37's In cases: INT-048, INT-022; D-1518). Sign-up asks the base ZIP and the radius
//! beside towns served, refuses a blank, 0, a fraction, a number over 100 and a base ZIP that is
//! not 5 digits with accessible errors and every value kept, accepts an out-of-state base with no
//! location, cannot finish without both, stores them per business with audit rows naming the
//! fields only, and reads them back on the check screen. The catalog sync stores the markets with
//! no launch time, and every market town passes INT-022's Arizona check. Fake data only.

use askhava_businesses::api::{
    ServiceArea, ServiceAreaProblem, create, save_service_area, service_area, set_towns,
};
use askhava_catalog::api::{load_embedded, markets, service_area_towns, zip_location};
use askhava_intake::api::{Town, check_towns};
use askhava_ops::api::Actor;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    AreaEntered, CapabilityPage, CapabilityStep, CapabilityValues, PageLanguage, Parts,
    RADIUS_QUESTION, Screen, capability_page, fee_numbers, signup_can_finish, signup_page,
};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// A well-formed ZIP outside the reviewed Arizona table (an out-of-state base).
const OUT_OF_STATE_ZIP: &str = "92363";

fn now() -> Timestamp {
    "2026-11-12T17:00:00Z".parse().expect("fake time")
}

/// A ZIP and town code from the reviewed markets file, so no market is named here.
fn market_zip() -> (String, String) {
    let markets = markets().expect("markets load");
    let town = &markets.markets()[0].towns[0];
    (town.zips[0].clone(), town.code.clone())
}

fn page(area: &AreaEntered<'_>) -> String {
    let parts = Parts {
        texting_on: false,
        language: PageLanguage::En,
        csrf: "fake-csrf",
        fees: fee_numbers(ConfirmSettings::default(), WindowRules::default()),
    };
    signup_page(Screen::ServiceArea(area), &parts).expect("renders")
}

/// TD-BIZ-37: the screen asks both questions with labels and hints and no error at first.
#[test]
fn td_biz_37_signup_asks_base_zip_and_radius() {
    let html = page(&AreaEntered::default());
    assert!(html.contains(r#"<label for="base-zip">Base ZIP code</label>"#));
    assert!(html.contains(&format!(r#"<label for="radius">{RADIUS_QUESTION}</label>"#)));
    assert!(html.contains(r#"aria-describedby="base-zip-hint""#));
    assert!(!html.contains("error-summary") && !html.contains("aria-invalid"));
    assert!(html.contains("<title>How far do you travel for jobs?"));
}

/// TD-BIZ-37: blank, 0, a fraction, over 100 and a malformed ZIP are refused.
#[test]
fn td_biz_37_bad_values_are_refused() {
    use ServiceAreaProblem::{BaseZip, Radius};
    for radius in ["", " ", "0", "2.5", "101", "-5", "+5", "ten", "1000"] {
        assert_eq!(
            ServiceArea::parse("86403", radius),
            Err(vec![Radius]),
            "{radius:?}"
        );
    }
    for zip in ["", "8640", "864031", "86403-1234", "8640a"] {
        assert_eq!(ServiceArea::parse(zip, "25"), Err(vec![BaseZip]), "{zip:?}");
    }
    assert_eq!(ServiceArea::parse("", ""), Err(vec![BaseZip, Radius]));
    for radius in ["1", "100", " 25 "] {
        assert!(ServiceArea::parse(" 86403 ", radius).is_ok());
    }
}

/// TD-BIZ-37, A11Y-010 to A11Y-013: a refusal shows an "Error: " title, a focused summary linking
/// to each field, the message by each field and both typed values kept; a carried base ZIP or
/// radius is never posted a second time as a hidden field.
#[test]
fn td_biz_37_errors_are_accessible_and_values_kept() {
    let problems = ServiceArea::parse("8640", "2.5").expect_err("refused");
    let carried = [
        ("town", "fake_town".to_owned()),
        ("base_zip", "00000".to_owned()),
        ("radius", "99".to_owned()),
    ];
    let html = page(&AreaEntered {
        base_zip: "8640",
        radius: "2.5",
        problems: &problems,
        carried: &carried,
    });
    assert!(html.contains("<title>Error: How far do you travel for jobs?"));
    assert!(html.contains(r#"<div id="error-summary" role="alert" tabindex="-1" autofocus>"#));
    assert!(html.contains(r##"<a href="#base-zip">"##) && html.contains(r##"<a href="#radius">"##));
    assert!(html.contains(r#"id="base-zip-error""#) && html.contains(r#"id="radius-error""#));
    assert!(html.contains(r#"value="8640" aria-invalid="true""#));
    assert!(html.contains(r#"value="2.5" aria-invalid="true""#));
    assert!(html.contains(r#"<input type="hidden" name="town" value="fake_town">"#));
    assert_eq!(
        html.matches(r#"name="base_zip""#).count(),
        1,
        "base ZIP posted once"
    );
    assert_eq!(
        html.matches(r#"name="radius""#).count(),
        1,
        "radius posted once"
    );
}

/// TD-BIZ-37, BIZ-030: the check screen reads the base ZIP and radius back, and every capability
/// screen carries them.
#[test]
fn td_biz_37_check_screen_reads_them_back() {
    let pairs: Vec<(String, String)> = [
        ("town", "fake_town"),
        ("largest", "none"),
        ("recurring", "no"),
        ("after_hours", "no"),
        ("base_zip", "86403"),
        ("radius", "25"),
    ]
    .map(|(a, b)| (a.to_owned(), b.to_owned()))
    .into();
    let values = CapabilityValues::from_pairs(&pairs);
    let towns = [("fake_town".to_owned(), "Fake Town".to_owned())];
    let at = |step| CapabilityPage {
        step,
        offer: &[],
        towns: &towns,
        values: &values,
        problems: &[],
        csrf: "fake-csrf",
    };
    let check = capability_page(&at(CapabilityStep::Check)).expect("renders");
    assert!(check.contains("<li>Base ZIP code: 86403</li>"));
    assert!(check.contains("<li>Miles you travel for a job: 25</li>"));
    for step in [CapabilityStep::All, CapabilityStep::Largest] {
        let html = capability_page(&at(step)).expect("renders");
        assert!(
            html.contains(r#"name="base_zip" value="86403""#)
                && html.contains(r#"name="radius" value="25""#)
        );
    }
}

async fn audit_fields(db: &mut PgConnection, id: Uuid) -> Vec<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT field FROM ops.audit_log WHERE subject_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(db)
    .await
    .expect("audit rows")
    .into_iter()
    .flatten()
    .collect()
}

/// TD-BIZ-37: stored per business with audit rows naming the fields only; sign-up finishes only
/// with towns and a service area.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_37_stored_per_business_and_needed_to_finish(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (zip, town) = market_zip();
    let (a, b) = (
        create(&mut db, false, now()).await.expect("a"),
        create(&mut db, false, now()).await.expect("b"),
    );
    assert!(!signup_can_finish(&mut db, a).await.expect("read"));
    set_towns(&mut db, a, &[town.as_str()], Actor::Business(a))
        .await
        .expect("towns");
    assert!(
        !signup_can_finish(&mut db, a).await.expect("read"),
        "towns alone"
    );
    let area = ServiceArea::parse(&zip, "25").expect("good");
    save_service_area(&mut db, b, &area, (Actor::Business(b), now()))
        .await
        .expect("saved");
    assert!(
        !signup_can_finish(&mut db, b).await.expect("read"),
        "service area alone"
    );
    save_service_area(&mut db, a, &area, (Actor::Business(a), now()))
        .await
        .expect("saved");
    assert!(signup_can_finish(&mut db, a).await.expect("read"));
    let wider = ServiceArea::parse(&zip, "60").expect("good");
    save_service_area(&mut db, a, &wider, (Actor::Business(a), now()))
        .await
        .expect("changed");
    let stored = service_area(&mut db, a).await.expect("read").expect("row");
    assert_eq!(
        (stored.area.base_zip(), stored.area.radius_miles()),
        (zip.as_str(), 60)
    );
    assert_eq!(stored.location, zip_location(&zip).expect("table"));
    assert!(stored.location.is_some());
    assert_eq!(
        service_area(&mut db, b)
            .await
            .expect("read")
            .expect("row")
            .area
            .radius_miles(),
        25
    );
    let fields = audit_fields(&mut db, a).await;
    assert_eq!(fields.iter().filter(|f| *f == "base_zip").count(), 2);
    assert_eq!(fields.iter().filter(|f| *f == "service_radius").count(), 2);
    let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE row_to_json(audit_log)::text LIKE '%' || $1 || '%'")
        .bind(&zip)
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(leaked, 0, "no audit row holds the ZIP");
}

/// TD-BIZ-37, BIZ-058: an out-of-state base ZIP is accepted and stored with no location.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_37_out_of_state_base_has_no_location(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = create(&mut db, false, now()).await.expect("business");
    let area = ServiceArea::parse(OUT_OF_STATE_ZIP, "40").expect("well formed");
    save_service_area(&mut db, id, &area, (Actor::System, now()))
        .await
        .expect("saved");
    let stored = service_area(&mut db, id).await.expect("read").expect("row");
    assert_eq!(stored.area.base_zip(), OUT_OF_STATE_ZIP);
    assert_eq!(stored.location, None);
    assert!(
        save_service_area(&mut db, Uuid::nil(), &area, (Actor::System, now()))
            .await
            .is_err()
    );
}

/// BIZ-058: the table refuses a radius outside 1 to 100 and a bad ZIP, and the app role may
/// update a row but never delete one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_37_the_table_holds_the_rules(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = create(&mut db, false, now()).await.expect("business");
    let insert = "INSERT INTO businesses.service_area VALUES ($1, $2, $3, now())";
    for (zip, miles) in [("86403", 0_i16), ("86403", 101), ("8640", 5)] {
        let r = sqlx::query(insert)
            .bind(id)
            .bind(zip)
            .bind(miles)
            .execute(&mut *db)
            .await;
        assert!(r.is_err(), "{zip} {miles}");
    }
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    sqlx::query(insert)
        .bind(id)
        .bind("86403")
        .bind(10_i16)
        .execute(&mut *db)
        .await
        .expect("insert");
    let update = "UPDATE businesses.service_area SET radius_miles = 20 WHERE business_id = $1";
    sqlx::query(update)
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("update");
    let delete = "DELETE FROM businesses.service_area WHERE business_id = $1";
    assert!(
        sqlx::query(delete)
            .bind(id)
            .execute(&mut *db)
            .await
            .is_err()
    );
}

/// TD-INT-37: the sync stores every market with no launch time and the centroids, twice safely,
/// and the app role cannot set a launch time, by update or by insert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_37_sync_stores_markets_with_no_launch(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("load");
    load_embedded(&mut db).await.expect("load again");
    let towns = service_area_towns().expect("towns");
    let (markets, launched): (i64, i64) =
        sqlx::query_as("SELECT count(*), count(launched_at) FROM catalog.market")
            .fetch_one(&mut *db)
            .await
            .expect("markets");
    assert!(markets >= 1);
    assert_eq!(launched, 0);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.market_town")
        .fetch_one(&mut *db)
        .await
        .expect("towns");
    assert_eq!(usize::try_from(stored).expect("count"), towns.len());
    let centroids: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.zcta_centroid")
        .fetch_one(&mut *db)
        .await
        .expect("centroids");
    assert!(centroids > 400);
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let launch = sqlx::query("UPDATE catalog.market SET launched_at = now()")
        .execute(&mut *db)
        .await;
    assert!(
        launch.is_err(),
        "a launch is an owner action, never the app's"
    );
    let insert_launched = sqlx::query(
        "INSERT INTO catalog.market (id, name, state, time_zone, identity_sources, launched_at)
         VALUES ('fake-market', 'Fake', 'AZ', 'America/Phoenix', ARRAY['city'], now())",
    )
    .execute(&mut *db)
    .await;
    assert!(
        insert_launched.is_err(),
        "the app role cannot insert a market with a launch time"
    );
}

/// TD-INT-37, INT-022: every market town, read from the markets file, passes the Arizona check.
#[test]
fn td_int_37_every_market_town_passes_the_arizona_check() {
    let towns = service_area_towns().expect("towns");
    assert!(!towns.is_empty());
    let checked: Vec<Town<'_>> = towns
        .iter()
        .map(|t| Town {
            name: &t.name,
            state: &t.state,
        })
        .collect();
    assert_eq!(check_towns(&checked), Ok(()));
}
