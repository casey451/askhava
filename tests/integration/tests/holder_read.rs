//! One read of everything a business holds for the license rules (TD-LIC-29; T374, D-2712;
//! LIC-006, LIC-011, LIC-012, LIC-015, LIC-016, LIC-019, LIC-047, LIC-048, BIZ-049, CARD-001).
//!
//! As the app role, on a fresh database: a pest attester with the wood-destroying category, a
//! certified inspector, a septage hauler, an R-37 business and a city-list business that chose
//! Handyman each read back through `businesses::api::holding` with exactly their holder parts; a
//! business with no rows reads as an empty holder; a second license for the same business and
//! agency is refused by the unique index, and each stored license has one audit row naming the
//! agency, never the number. All names and numbers are fake.

use askhava_businesses::api::{BusinessesError, Known, create, holding};
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    Agency, CityListStanding, CityRecord, ClassMap, HolderError, HolderParts, PestAttestation,
    PestRecordCategory, check_license, import_city_list, import_roc_list, record_agency_license,
    record_pest_attestation,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use jiff::civil::date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const ROC: &str = "license_number,class,status,issued_on\nROC900037,R-37,Active,2012-05-01\n";

const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7001,Example Fixit LLC,Sample Odd Jobs,Pat Placeholder,2019-03-04\n";

fn now() -> Timestamp {
    "2026-11-11T17:00:00Z".parse().expect("fake time")
}

/// A new business that chose `trades` at sign-up, on an app-role connection.
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

async fn app(pool: &sqlx::PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn read(conn: &mut PgConnection, id: Uuid, known: Known<'_>) -> HolderParts {
    holding(conn, id, known).await.expect("holding")
}

fn assert_empty_except_agencies(parts: &HolderParts, agencies: &[Agency]) {
    assert!(parts.classes.is_empty(), "no ROC classes");
    let held: Vec<Agency> = parts.holder.agencies.iter().copied().collect();
    assert_eq!(held, agencies);
    assert!(parts.holder.pest.is_none());
    assert!(!parts.holder.handyman);
    assert!(!parts.holder.roc_covers);
    assert_eq!(parts.holder.city_list, CityListStanding::Pending);
}

/// TD-LIC-29 (LIC-012, LIC-047, LIC-048): a pest attester reads back its number and the
/// wood-destroying category only; a homeowner's report pauses it; nothing else is held.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_29_pest_attester_reads_back_its_attestation(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["pest_control"]).await;
    let attested = PestAttestation::new(true, "PM-4242", true, false).expect("attestation");
    record_pest_attestation(&mut conn, id, &attested, Actor::Business(id))
        .await
        .expect("recorded");
    let parts = read(&mut conn, id, Known::default()).await;
    let pest = parts.holder.pest.clone().expect("attested");
    assert_eq!(pest.number(), "PM-4242");
    assert!(pest.holds(PestRecordCategory::WoodDestroying));
    assert!(!pest.holds(PestRecordCategory::WeedControl));
    assert!(!pest.is_paused());
    assert!(parts.holder.agencies.is_empty(), "pest is never an agency");
    assert!(parts.classes.is_empty() && !parts.holder.handyman);
    sqlx::query("UPDATE licensing.business_license SET paused_at = now() WHERE business_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .expect("reported wrong");
    let parts = read(&mut conn, id, Known::default()).await;
    assert!(parts.holder.pest.expect("still attested").is_paused());
}

/// TD-LIC-29 (LIC-015, LIC-016): a certified inspector and a septage hauler each hold exactly
/// their agency; the pest agency and a blank or over-long number are refused as agency records.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_29_inspector_and_hauler_hold_exactly_their_agency(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let inspector = business(&mut conn, &["home_inspection"]).await;
    let hauler = business(&mut conn, &["septic_pumping"]).await;
    let pairs = [
        (inspector, Agency::HomeInspector, "HI-90001"),
        (hauler, Agency::SeptageHauler, "SH 0042"),
    ];
    for (id, agency, number) in pairs {
        record_agency_license(&mut conn, id, (agency, number), Actor::System)
            .await
            .expect("recorded");
        let parts = read(&mut conn, id, Known::default()).await;
        assert_empty_except_agencies(&parts, &[agency]);
    }
    let long = "N".repeat(65);
    for bad in [
        (Agency::PestManagement, "PM-1"),
        (Agency::AlarmBusiness, "  "),
        (Agency::AlarmBusiness, long.as_str()),
    ] {
        let refused = record_agency_license(&mut conn, inspector, bad, Actor::System).await;
        assert_eq!(refused.err(), Some(HolderError::BadRecord));
    }
}

/// TD-LIC-29 (LIC-006, LIC-043, CARD-001): an R-37 business reads its class from the latest ROC
/// list by its number, or by its latest daily check's number; the map covers plumbing for it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_29_r_37_business_reads_its_classes(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["plumbing_repair"]).await;
    import_roc_list(&mut conn, date(2026, 11, 10), ROC)
        .await
        .expect("ROC list");
    let known = Known {
        roc_license: Some("roc900037"),
        city_names: None,
    };
    let parts = read(&mut conn, id, known).await;
    assert_eq!(parts.classes, vec!["R-37".to_owned()]);
    assert_empty_except_agencies(
        &HolderParts {
            classes: Vec::new(),
            ..parts.clone()
        },
        &[],
    );
    let map = ClassMap::shipped().expect("license map");
    let job = parts.for_job(map, "plumbing_repair", "water heater replacement");
    assert!(job.roc_covers, "R-37 covers a water heater");
    assert!(!parts.for_job(map, "electrical", "panel upgrade").roc_covers);
    assert!(
        read(&mut conn, id, Known::default())
            .await
            .classes
            .is_empty()
    );
    check_license(&mut conn, id, "ROC900037")
        .await
        .expect("check");
    let by_check = read(&mut conn, id, Known::default()).await;
    assert_eq!(by_check.classes, parts.classes, "the daily check's number");
}

const ROC_MIXED: &str = "license_number,class,status,issued_on\n\
ROC900037,R-37,Active,2012-05-01\nROC900038,R-37,Suspended,2015-01-01\n\
ROC900039,R-37,Active,2016-01-01\nROC900039,R-11,Expired,2016-01-01\n\
ROC900040,R-11,Active,2018-06-01\n";

/// D-2743 (LIC-007, CARD-015): an Inactive class is never held: a number with only an Inactive
/// class holds no class and covers nothing, and a mixed number holds only its Active class.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_inactive_roc_class_is_never_held(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["plumbing_repair"]).await;
    import_roc_list(&mut conn, date(2026, 11, 10), ROC_MIXED)
        .await
        .expect("ROC list");
    let known = |number: &'static str| Known {
        roc_license: Some(number),
        city_names: None,
    };
    let inactive = read(&mut conn, id, known("ROC900038")).await;
    assert!(inactive.classes.is_empty(), "a suspended class is not held");
    let map = ClassMap::shipped().expect("license map");
    let job = inactive.for_job(map, "plumbing_repair", "water heater replacement");
    assert!(!job.roc_covers);
    let mixed = read(&mut conn, id, known("ROC900039")).await;
    assert_eq!(mixed.classes, vec!["R-37".to_owned()]);
}

/// D-2744 (CARD-015): once the business has a daily check, its checked number wins over a
/// different number from the caller; before any check the caller's number is read.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_daily_checked_number_wins_over_the_callers(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["plumbing_repair"]).await;
    import_roc_list(&mut conn, date(2026, 11, 10), ROC_MIXED)
        .await
        .expect("ROC list");
    let known = Known {
        roc_license: Some("ROC900037"),
        city_names: None,
    };
    let unchecked = read(&mut conn, id, known).await;
    assert_eq!(unchecked.classes, vec!["R-37".to_owned()], "no check yet");
    check_license(&mut conn, id, "ROC900040")
        .await
        .expect("check");
    let checked = read(&mut conn, id, known).await;
    assert_eq!(
        checked.classes,
        vec!["R-11".to_owned()],
        "the checked number"
    );
}

/// TD-LIC-29 (LIC-011, LIC-019): a city-list business that chose Handyman reads as a matched
/// handyman with nothing else.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_29_city_list_handyman_reads_matched_and_handyman(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["handyman", "house_cleaning"]).await;
    let names = CityRecord {
        business_name: "Example Fixit LLC".to_owned(),
        ..CityRecord::default()
    };
    let known = Known {
        roc_license: None,
        city_names: Some(&names),
    };
    assert_eq!(
        read(&mut conn, id, known).await.holder.city_list,
        CityListStanding::Pending,
        "no city list yet"
    );
    import_city_list(&mut conn, date(2026, 11, 10), CITY)
        .await
        .expect("city list");
    let parts = read(&mut conn, id, known).await;
    assert!(parts.holder.handyman);
    assert_eq!(parts.holder.city_list, CityListStanding::Matched);
    assert!(parts.classes.is_empty() && parts.holder.agencies.is_empty());
    assert!(parts.holder.pest.is_none());
}

/// TD-LIC-29: a business with no rows, no sign-up row and no list reads as an empty holder; an
/// unknown business is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_29_no_rows_read_as_an_empty_holder(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = create(&mut conn, false, now()).await.expect("business");
    let known = Known {
        roc_license: Some("ROC900037"),
        city_names: None,
    };
    assert_empty_except_agencies(&read(&mut conn, id, known).await, &[]);
    let unknown = holding(&mut conn, Uuid::from_u128(0x7e57), Known::default()).await;
    assert_eq!(unknown.err(), Some(BusinessesError::UnknownBusiness));
}

/// TD-LIC-29 (LIC-047): a second attestation or agency license for the same business and agency
/// is refused by the unique index and replaces nothing; each stored one has one audit row that
/// names the agency, never the number (PRIV-028).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_29_a_second_license_for_one_agency_is_refused(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = business(&mut conn, &["pest_control", "home_inspection"]).await;
    let first = PestAttestation::new(true, "PM-1001", false, true).expect("first");
    let second = PestAttestation::new(true, "PM-2002", true, true).expect("second");
    let actor = Actor::Business(id);
    record_pest_attestation(&mut conn, id, &first, actor)
        .await
        .expect("first");
    let again = record_pest_attestation(&mut conn, id, &second, actor).await;
    assert_eq!(again.err(), Some(HolderError::AlreadyRecorded));
    let inspector = (Agency::HomeInspector, "HI-1");
    record_agency_license(&mut conn, id, inspector, actor)
        .await
        .expect("inspector");
    let again = record_agency_license(&mut conn, id, (Agency::HomeInspector, "HI-2"), actor).await;
    assert_eq!(again.err(), Some(HolderError::AlreadyRecorded));
    let parts = read(&mut conn, id, Known::default()).await;
    let pest = parts.holder.pest.expect("kept");
    assert_eq!(pest.number(), "PM-1001", "the first attestation stands");
    assert!(!pest.holds(PestRecordCategory::WoodDestroying));
    let fields: Vec<String> = sqlx::query_scalar(
        "SELECT field FROM ops.audit_log WHERE action = 'licensing.license_recorded' ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("audit rows");
    assert_eq!(fields, ["pest_management", "home_inspector"]);
}
