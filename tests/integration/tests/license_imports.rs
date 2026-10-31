//! The daily ROC and city list imports and the daily license check on a fresh database (CARD-015,
//! LIC-006, LIC-007, LIC-010, LIC-027).
//!
//! Proves, as the app role, that the daily import reads a fixture list and records each row with
//! its source and list date; that planted malformed rows (a short row, an unknown status, a bad
//! date, an unclosed quote, a blank number) are skipped and counted without stopping the import;
//! that a changed header records nothing; that a second run for the same day records nothing new;
//! that the class and issue year read back from the state record; that a card goes only to a
//! business whose latest daily check was Active, pest excepted (LIC-048); that the city list matches
//! a business's own names and drops every contact column; and that imported rows cannot be
//! rewritten. All names and numbers are fake.

use askhava_integration::set_app_role;
use askhava_licensing::api::{
    CityListStanding, CityRecord, DailyCheck, ImportError, ImportSummary, check_license,
    city_standing, daily_check_allows_card, import_city_list, import_roc_list, parse_roc_list,
    read_back,
};
use jiff::civil::date;
use sqlx::PgPool;
use sqlx::pool::PoolConnection;
use sqlx::types::Uuid;

const ROC_DAY_1: &str = "license_number,class,status,issued_on\n\
ROC900001,cr-37,Active,2009-04-01\n\
ROC900001,R-39,Active,2015-06-30\n\
ROC900002,R-11,Suspended,2020-01-15\n\
ROC900003,R-37,Active,\n\
ROC900003,R-37,Active,\n\
ROC900004,R-37\n\
ROC900005,R-37,Pending,2020-01-01\n\
ROC900006,R-37,Active,2020-13-40\n\
\"ROC900007,R-37,Active,\n\
,R-37,Active,2020-01-01\n";

const ROC_DAY_2: &str =
    "license_number,class,status,issued_on\nROC900001,CR-37,Expired,2009-04-01\n";

const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-1001,\"Example Pools, LLC\",Sample Pool Care,Pat Placeholder,2018-02-03\n\
,Fake Window Washers,,,\n\
CL-1003,,No Business Name,,\n";

const BUSINESS: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0036_0001);

async fn app(pool: &PgPool) -> PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// The import records good rows with source and date, and skips and counts every malformed row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn daily_roc_import_records_rows_and_counts_malformed_ones(pool: PgPool) {
    let mut conn = app(&pool).await;
    let day = date(2026, 10, 30);
    let done = import_roc_list(&mut conn, day, ROC_DAY_1)
        .await
        .expect("import");
    let want = ImportSummary {
        recorded: 4,
        skipped: 6,
        already_imported: false,
    };
    assert_eq!(done, want, "one repeat and five malformed rows skipped");
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT source, list_date::text, class_code FROM licensing.agency_record ORDER BY class_code",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("rows");
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|(s, d, _)| s == "roc" && d == "2026-10-30"));
    let again = import_roc_list(&mut conn, day, ROC_DAY_1)
        .await
        .expect("again");
    assert_eq!(
        again,
        ImportSummary {
            already_imported: true,
            ..want
        },
        "nothing new the same day"
    );
    let bad = import_roc_list(&mut conn, date(2026, 10, 31), "number,class\nROC1,R-37\n").await;
    assert_eq!(
        bad.err(),
        Some(ImportError::BadHeader),
        "a changed layout records nothing"
    );
    let parsed = parse_roc_list(ROC_DAY_1).expect("parse");
    assert_eq!((parsed.rows.len(), parsed.skipped), (5, 5));
}

/// The class and issue year come from the state record, never typed (LIC-006, LIC-027).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn read_back_gives_classes_and_since_year_from_the_record(pool: PgPool) {
    let mut conn = app(&pool).await;
    assert_eq!(
        read_back(&mut conn, "ROC900001").await,
        Err(ImportError::NoList)
    );
    import_roc_list(&mut conn, date(2026, 10, 30), ROC_DAY_1)
        .await
        .expect("import");
    let back = read_back(&mut conn, " ROC900001 ")
        .await
        .expect("read")
        .expect("listed");
    assert_eq!(back.classes, ["CR-37", "R-39"]);
    assert_eq!(back.since_year, Some(2009));
    let undated = read_back(&mut conn, "ROC900003")
        .await
        .expect("read")
        .expect("listed");
    assert_eq!(undated.since_year, None);
    assert_eq!(read_back(&mut conn, "ROC999999").await, Ok(None));
}

/// A card goes only after an Active latest daily check; pest is attested instead (CARD-015, LIC-048).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_needs_latest_daily_check_active(pool: PgPool) {
    let mut conn = app(&pool).await;
    let check = check_license(&mut conn, BUSINESS, "ROC900001").await;
    assert_eq!(
        check,
        Err(ImportError::NoList),
        "no list, no check recorded"
    );
    assert!(
        !daily_check_allows_card(&mut conn, BUSINESS, "pool_repair")
            .await
            .expect("allow")
    );
    assert!(
        daily_check_allows_card(&mut conn, BUSINESS, "pest_control")
            .await
            .expect("pest")
    );
    import_roc_list(&mut conn, date(2026, 10, 29), ROC_DAY_1)
        .await
        .expect("day 1");
    let first = check_license(&mut conn, BUSINESS, "ROC900001").await;
    assert_eq!(first, Ok(DailyCheck::Active));
    assert!(
        daily_check_allows_card(&mut conn, BUSINESS, "pool_repair")
            .await
            .expect("allow")
    );
    let other = Uuid::from_u128(BUSINESS.as_u128() + 1);
    assert_eq!(
        check_license(&mut conn, other, "ROC900002").await,
        Ok(DailyCheck::Inactive)
    );
    assert_eq!(
        check_license(&mut conn, other, "ROC999999").await,
        Ok(DailyCheck::NotListed)
    );
    assert!(
        !daily_check_allows_card(&mut conn, other, "pool_repair")
            .await
            .expect("allow")
    );
    import_roc_list(&mut conn, date(2026, 10, 30), ROC_DAY_2)
        .await
        .expect("day 2");
    assert_eq!(
        check_license(&mut conn, BUSINESS, "ROC900001").await,
        Ok(DailyCheck::Inactive)
    );
    assert!(
        !daily_check_allows_card(&mut conn, BUSINESS, "pool_repair")
            .await
            .expect("allow")
    );
    let dates: Vec<String> = sqlx::query_scalar(
        "SELECT list_date::text FROM licensing.license_check WHERE business_id = $1 ORDER BY checked_at",
    )
    .bind(BUSINESS)
    .fetch_all(&mut *conn)
    .await
    .expect("checks");
    assert_eq!(dates, ["2026-10-29", "2026-10-30"]);
}

/// The city list keeps names for matching, skips a row without a business name (LIC-010, LIC-011).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn city_list_matches_own_names_for_verification(pool: PgPool) {
    let mut conn = app(&pool).await;
    let us = CityRecord {
        business_name: "Sample Pool Care".to_owned(),
        ..CityRecord::default()
    };
    assert_eq!(
        city_standing(&mut conn, &us).await,
        Ok(CityListStanding::Pending)
    );
    let done = import_city_list(&mut conn, date(2026, 10, 30), CITY)
        .await
        .expect("import");
    assert_eq!((done.recorded, done.skipped), (2, 1));
    assert_eq!(
        city_standing(&mut conn, &us).await,
        Ok(CityListStanding::Matched)
    );
    let stranger = CityRecord {
        business_name: "Unlisted Test Co".to_owned(),
        ..us.clone()
    };
    assert_eq!(
        city_standing(&mut conn, &stranger).await,
        Ok(CityListStanding::Pending)
    );
    let bad = import_city_list(&mut conn, date(2026, 10, 31), "name\nX\n").await;
    assert_eq!(bad.err(), Some(ImportError::BadHeader));
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns WHERE table_schema = 'licensing' \
         AND column_name ~ '(phone|email|address)'",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("columns");
    assert!(cols.is_empty(), "no contact column is stored: {cols:?}");
}

/// Imported rows and checks are append-only for the app role.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn imported_rows_cannot_be_rewritten(pool: PgPool) {
    let mut conn = app(&pool).await;
    import_roc_list(&mut conn, date(2026, 10, 30), ROC_DAY_1)
        .await
        .expect("import");
    for sql in [
        "UPDATE licensing.agency_record SET status = 'active'",
        "DELETE FROM licensing.list_import",
        "DELETE FROM licensing.license_check",
    ] {
        assert!(sqlx::query(sql).execute(&mut *conn).await.is_err(), "{sql}");
    }
}
