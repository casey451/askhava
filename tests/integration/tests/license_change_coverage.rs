//! T585 (D-4052, D-4064): a license change that drops job types reports coverage (OPS-006), and
//! a lapsed ROC license never shows city-list lines (LIC-024, LIC-026, LIC-038, PICK-041).
//!
//! A class change on an active business counts the trades and towns it served before and serves
//! now in the same transaction, so a requested trade it leaves with no active business alerts at
//! once and one still held by 2 raises nothing. On an ROC category a business whose latest check
//! did not show Active gets no-license facts with no city line (a handyman still gets LIC-024's
//! line); one with no check row keeps today's facts; a no-license category keeps its city line.
//! Fake businesses, names and license numbers only.

use askhava_businesses::api::{
    Capability, CapabilityAnswers, JobTypeChoice, LargestJob, checklist_offer, create, pre_ticked,
    reapply_license, save_capability,
};
use askhava_catalog::api::{Catalog, parse};
use askhava_licensing::api::{
    CardFacts, CardLanguage, CityRecord, ClassMap, HANDYMAN_CARD_LINE, NoLicenseFacts, card_facts,
    no_license_lines,
};
use askhava_ops::api::{Actor, COVERAGE_ALERT};
use jiff::Timestamp;
use jiff::civil::Date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const TOWN: &str = "lake_havasu_city";
const ELECTRICAL: &str = "electrical";

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("license map loads")
}

fn now() -> Timestamp {
    "2026-11-10T17:00:00Z".parse().expect("fake time")
}

fn list_day() -> Date {
    "2026-11-10".parse().expect("fake date")
}

/// An active R-11 business in [`TOWN`] with every electrical job type and window cleaning ticked,
/// made active directly so nothing is reported before the class change.
async fn r11_business(db: &mut PgConnection) -> Uuid {
    let id = create(db, false, now()).await.expect("business");
    let offer = checklist_offer(map(), &catalog(), &["R-11"]);
    let mut ticked = pre_ticked(&offer);
    ticked.push(JobTypeChoice::new(
        "window_cleaning",
        "inside and outside windows",
    ));
    let answers = CapabilityAnswers {
        ticked,
        towns: vec![TOWN.to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours: false,
    };
    save_capability(db, id, &offer, &answers, (Actor::System, now()))
        .await
        .expect("saved");
    sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("active");
    id
}

/// The catalog stored and one electrical request this Arizona month, so an electrical count under
/// 2 alerts (OPS-006, D-459); `report_edit` counts at the real clock.
async fn setup(db: &mut PgConnection) {
    askhava_catalog::api::load(db, &catalog())
        .await
        .expect("catalog stored");
    sqlx::query(
        "INSERT INTO ops.trade_request (trade, month, requests) VALUES ($1, \
         date_trunc('month', now() AT TIME ZONE 'America/Phoenix')::date, 1)",
    )
    .bind(ELECTRICAL)
    .execute(&mut *db)
    .await
    .expect("request");
}

/// R-11 dropped from business `id`'s classes.
async fn drop_r11(db: &mut PgConnection, id: Uuid) {
    let classes: (&[&str], &[&str]) = (&["R-11"], &[]);
    let change = reapply_license(db, id, (map(), &catalog()), classes, (list_day(), now()))
        .await
        .expect("re-applied");
    assert!(!change.removed.is_empty() && !change.paused);
}

async fn scalar(db: &mut PgConnection, sql: &'static str, code: &str) -> Option<i64> {
    sqlx::query_scalar(sql)
        .bind(code)
        .fetch_optional(db)
        .await
        .expect("read")
}

async fn alerts(db: &mut PgConnection, scope: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert a JOIN ops.coverage c ON c.id = a.subject_id \
         WHERE a.kind = $1 AND c.scope = $2 AND a.resolved_at IS NULL",
    )
    .bind(COVERAGE_ALERT)
    .bind(scope)
    .fetch_one(db)
    .await
    .expect("alerts")
}

const COUNT: &str = "SELECT active::bigint FROM ops.coverage WHERE scope = 'trade' AND code = $1";

/// OPS-006 (TD-OPS-01's shape, D-4052): a class change that leaves the requested electrical trade
/// with no active business records 0 active for it and alerts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_class_change_leaving_a_trade_bare_alerts(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    setup(&mut db).await;
    let id = r11_business(&mut db).await;
    drop_r11(&mut db, id).await;
    assert_eq!(scalar(&mut db, COUNT, ELECTRICAL).await, Some(0));
    assert_eq!(alerts(&mut db, "trade").await, 1);
}

/// OPS-006 (D-4052): a class change that leaves 2 active businesses in the trade and in the town
/// counts them and raises no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_class_change_leaving_2_in_trade_and_town_is_quiet(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    setup(&mut db).await;
    let id = r11_business(&mut db).await;
    r11_business(&mut db).await;
    r11_business(&mut db).await;
    drop_r11(&mut db, id).await;
    assert_eq!(scalar(&mut db, COUNT, ELECTRICAL).await, Some(2));
    let town = "SELECT active::bigint FROM ops.coverage WHERE scope = 'town' AND code = $1";
    assert_eq!(scalar(&mut db, town, TOWN).await, Some(3));
    assert_eq!(
        alerts(&mut db, "trade").await + alerts(&mut db, "town").await,
        0
    );
}

/// A city list naming "Sample Lake Pipes" since 2019, and that business's own names.
async fn city_list(db: &mut PgConnection) -> CityRecord {
    askhava_catalog::api::load(db, &catalog())
        .await
        .expect("catalog stored");
    let list = Uuid::from_u128(0x585_0001);
    sqlx::query("INSERT INTO licensing.list_import (id, source, list_date, rows_recorded, rows_skipped) VALUES ($1, 'city', '2026-11-01', 1, 0)").bind(list).execute(&mut *db).await.expect("list");
    sqlx::query("INSERT INTO licensing.city_license_entry (import_id, source, list_date, business_name, issued_on) VALUES ($1, 'city', '2026-11-01', 'SAMPLE LAKE PIPES', '2019-06-01')").bind(list).execute(&mut *db).await.expect("entry");
    CityRecord {
        business_name: "Sample Lake Pipes".to_owned(),
        trade_name: None,
        owner_name: None,
        license_number: None,
    }
}

/// Business `id`'s fake ROC check with `status` on November `day`.
async fn check(db: &mut PgConnection, id: Uuid, status: &str, day: i32) {
    sqlx::query("INSERT INTO licensing.license_check (business_id, source, license_number, list_date, status, checked_at) VALUES ($1, 'roc', '399999', make_date(2026, 11, $3), $2, make_timestamptz(2026, 11, $3, 9, 0, 0, 'America/Phoenix'))").bind(id).bind(status).bind(day).execute(&mut *db).await.expect("check");
}

async fn facts(db: &mut PgConnection, id: Uuid, code: &str, names: &CityRecord) -> CardFacts {
    let read = card_facts(db, id, code, Some(names)).await.expect("read");
    read.expect("a catalog category")
}

/// The city facts a no-license card gets from the list above (LIC-038).
fn city_card() -> CardFacts {
    CardFacts::NoLicense {
        facts: NoLicenseFacts {
            handyman: false,
            city_since_year: Some(2019),
            insurance: None,
        },
        city_listed: true,
    }
}

/// LIC-038, LIC-024, PICK-041 (D-4064): a lapsed ROC check gives no-license facts with no city
/// line, and a handyman's card still leads with LIC-024's line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_lapsed_roc_check_gives_no_city_line(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let names = city_list(&mut db).await;
    for status in ["inactive", "not_listed"] {
        let id = Uuid::new_v4();
        check(&mut db, id, "active", 10).await;
        check(&mut db, id, status, 11).await;
        let CardFacts::NoLicense {
            mut facts,
            city_listed,
        } = facts(&mut db, id, "plumbing_repair", &names).await
        else {
            panic!("licensed card on a lapse");
        };
        assert!(!city_listed);
        assert_eq!(facts.city_since_year, None);
        facts.handyman = true;
        let lines = no_license_lines(&facts, CardLanguage::English).expect("lines");
        assert_eq!(lines[0].text, HANDYMAN_CARD_LINE);
    }
}

/// D-4064, TD-PICK-62: with no ROC check row an ROC category keeps today's city facts, and a
/// no-license category gives its city line even after a lapse.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_check_row_and_no_license_kinds_keep_the_city_line(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let names = city_list(&mut db).await;
    let unchecked = Uuid::new_v4();
    let roc = facts(&mut db, unchecked, "plumbing_repair", &names).await;
    assert_eq!(roc, city_card());
    let lapsed = Uuid::new_v4();
    check(&mut db, lapsed, "inactive", 10).await;
    let windows = facts(&mut db, lapsed, "window_cleaning", &names).await;
    assert_eq!(windows, city_card());
}
