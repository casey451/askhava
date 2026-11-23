//! TD-LIC-32 (T653, D-4310; LIC-013, LIC-015, LIC-034, CARD-015, INT-017, WORD-003): a BTR number
//! typed at sign-up is stored `typed_unchecked` and gives no card and no license line.
//!
//! On PostgreSQL, fake data only: `record_typed_agency_number` stores the typed number unchecked;
//! the holder does not count it, so a home-inspection job and alarm installs and monitoring (an
//! R-67 holder included) give no card; `card_facts` prints no LIC-034 line; the home-inspection
//! category counts zero businesses for INT-017. An `agency_record` row, as a BTR source would
//! write it, still gives the card and the line dated that row's check day. Migration 0212 moves an
//! existing inspector `agency_record` row to `typed_unchecked`. The writer refuses pest, septage
//! hauler, a blank and a 65-character number, and a second row for the same agency.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, set_job_types,
    set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::MIGRATOR;
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    Agency, CardFacts, ClassMap, Eligibility, HolderError, HolderInputs, Issuer, Job, LicenseKind,
    card_facts, eligibility, holder_for, import_roc_list, record_agency_license,
    record_typed_agency_number,
};
use askhava_ops::api::Actor;
use askhava_quotes::api::{CardFacts as Request, Coverage, request_coverage};
use jiff::Timestamp;
use jiff::civil::date;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const TOWN: &str = "lake_havasu_city";
const INSPECTION: (&str, &str) = (
    "home_inspection",
    "home condition inspection (no sale or refinance)",
);
const ALARM: &str = "alarm_systems";
const ROC: &str = "license_number,class,status,issued_on\nROC906767,R-67,Active,2015-02-01\n";
/// The check day an `agency_record` row carries, Arizona time (D-4310).
const CHECKED: &str = "2026-10-20T10:00:00-07:00";

fn at() -> Timestamp {
    "2026-11-02T10:00:00-07:00".parse().expect("time")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("shipped map")
}

/// The catalog, then the app role, as production runs.
async fn setup(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// An active, welcomed business serving the town and ticking `job` (category, job type).
async fn active_business(db: &mut PgConnection, job: (&str, &str)) -> Uuid {
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
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(job.0, job.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
    b
}

fn job(category: &str, job_type: &str, license: LicenseKind) -> Job {
    Job {
        category: category.to_owned(),
        job_type: job_type.to_owned(),
        license,
        handyman_allowed: false,
        listed: true,
    }
}

/// LIC-015, LIC-013, CARD-015: whether `business` gets the inspection card and each alarm card.
async fn cards(db: &mut PgConnection, business: Uuid) -> Vec<Eligibility> {
    let inputs = HolderInputs {
        roc_license: Some("ROC906767"),
        ..HolderInputs::default()
    };
    let parts = holder_for(db, business, inputs).await.expect("holder");
    let inspect = job(INSPECTION.0, INSPECTION.1, LicenseKind::StateAgency);
    let mut out = vec![eligibility(&inspect, &parts.holder)];
    for kind in ["alarm install", "alarm monitoring setup"] {
        let holder = parts.for_job(map(), ALARM, kind);
        assert!(holder.roc_covers, "R-67 covers {kind}");
        out.push(eligibility(
            &job(ALARM, kind, LicenseKind::RocAndStateAgency),
            &holder,
        ));
    }
    out
}

/// INT-017: the coverage of a home-inspection request in the town.
async fn coverage(db: &mut PgConnection) -> Coverage {
    let facts = Request {
        category: INSPECTION.0.to_owned(),
        candidates: vec![INSPECTION.1.to_owned()],
        emergency: false,
    };
    request_coverage(db, map(), ((TOWN, None), &facts), None)
        .await
        .expect("coverage")
}

async fn verification(db: &mut PgConnection, business: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT verification FROM licensing.business_license WHERE business_id = $1 \
         ORDER BY agency",
    )
    .bind(business)
    .fetch_all(db)
    .await
    .expect("rows")
}

/// TD-LIC-32 (LIC-013, LIC-015, LIC-034, CARD-015, INT-017): a typed BTR number is stored
/// unchecked, gives no inspection or alarm card (R-67 included), no LIC-034 line and no INT-017
/// count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_32_a_typed_btr_number_gives_no_card_line_or_count(mut db: Db) {
    setup(&mut db).await;
    import_roc_list(&mut db, date(2026, 11, 1), ROC)
        .await
        .expect("ROC list");
    let b = active_business(&mut db, INSPECTION).await;
    let actor = Actor::Business(b);
    for agency in [Agency::HomeInspector, Agency::AlarmBusiness] {
        record_typed_agency_number(&mut db, b, (agency, "  HI-5678 "), actor)
            .await
            .expect("typed");
    }
    assert_eq!(verification(&mut db, b).await, ["typed_unchecked"; 2]);
    let numbers: Vec<String> =
        sqlx::query_scalar("SELECT license_number FROM licensing.business_license")
            .fetch_all(&mut *db)
            .await
            .expect("numbers");
    assert_eq!(numbers, ["HI-5678"; 2], "trimmed");
    assert!(
        cards(&mut db, b)
            .await
            .iter()
            .all(|e| *e != Eligibility::Card),
        "LIC-013, LIC-015: no card on an unchecked number"
    );
    for code in [INSPECTION.0, ALARM] {
        let facts = card_facts(&mut db, b, code, None).await.expect("facts");
        assert!(
            !matches!(facts, Some(CardFacts::Licensed(_))),
            "LIC-034: no check line for {code}"
        );
    }
    assert_eq!(coverage(&mut db).await, Coverage::NotCovered, "INT-017");
}

/// TD-LIC-32 (LIC-034, CARD-015, WORD-003): an `agency_record` row, as a BTR source writes it,
/// gives the card and the line dated that row's check day, never the sign-up day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_32_an_agency_record_row_still_gives_the_card_and_its_check_day(mut db: Db) {
    setup(&mut db).await;
    import_roc_list(&mut db, date(2026, 11, 1), ROC)
        .await
        .expect("ROC list");
    let b = active_business(&mut db, INSPECTION).await;
    for agency in [Agency::HomeInspector, Agency::AlarmBusiness] {
        record_agency_license(&mut db, b, (agency, "HI-5512"), Actor::System)
            .await
            .expect("agency record");
    }
    sqlx::query("UPDATE licensing.business_license SET recorded_at = $2::timestamptz WHERE business_id = $1")
        .bind(b)
        .bind(CHECKED)
        .execute(&mut *db)
        .await
        .expect("check day");
    assert_eq!(cards(&mut db, b).await, [Eligibility::Card; 3]);
    let facts = card_facts(&mut db, b, INSPECTION.0, None)
        .await
        .expect("facts");
    let Some(CardFacts::Licensed(lines)) = facts else {
        panic!("LIC-034: a licensed card");
    };
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].issuer, Issuer::Agency(Agency::HomeInspector));
    assert_eq!(lines[0].number, "HI-5512");
    assert_eq!(
        lines[0].checked_on,
        date(2026, 10, 20),
        "the row's check day"
    );
    assert_eq!(coverage(&mut db).await, Coverage::Send(1), "INT-017");
}

/// TD-LIC-32 (D-4310): migration 0212 moves an existing inspector and alarm `agency_record` row to
/// `typed_unchecked`, keeps the septage hauler's and the pest attestation, and refuses
/// `typed_unchecked` on any other agency.
#[sqlx::test(migrations = false)]
async fn td_lic_32_the_migration_moves_typed_rows_to_unchecked(pool: PgPool) {
    MIGRATOR
        .run_to(211, &pool)
        .await
        .expect("migrations before 0212");
    let ids: Vec<Uuid> = (1..=4).map(|n| Uuid::from_u128(0x653_0000 + n)).collect();
    let rows = [
        ("home_inspector", "HI-1", "agency_record"),
        ("alarm_business", "AB-1", "agency_record"),
        ("septage_hauler", "SH-1", "agency_record"),
        ("pest_management", "PM-1", "self_attested"),
    ];
    for (id, (agency, number, verification)) in ids.iter().zip(rows) {
        sqlx::query(
            "INSERT INTO licensing.business_license \
             (business_id, agency, license_number, verification) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(agency)
        .bind(number)
        .bind(verification)
        .execute(&pool)
        .await
        .expect("row before 0212");
    }
    MIGRATOR.run(&pool).await.expect("migration 0212");
    let mut db = pool.acquire().await.expect("db");
    let after = [
        "typed_unchecked",
        "typed_unchecked",
        "agency_record",
        "self_attested",
    ];
    for (id, want) in ids.iter().zip(after) {
        assert_eq!(verification(&mut db, *id).await, [want]);
    }
    let refused = sqlx::query(
        "UPDATE licensing.business_license SET verification = 'typed_unchecked' \
         WHERE business_id = $1",
    )
    .bind(ids[2])
    .execute(&mut *db)
    .await;
    assert!(refused.is_err(), "typed_unchecked is BTR only");
}

/// TD-LIC-32 (LIC-015, PRIV-028): `record_typed_agency_number` refuses pest, septage hauler, a
/// blank and a 65-character number, and a second row for the same agency; its one audit row names
/// the agency, never the number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_32_the_typed_writer_refuses_bad_records(mut db: Db) {
    setup(&mut db).await;
    let b = active_business(&mut db, INSPECTION).await;
    let actor = Actor::Business(b);
    let long = "H".repeat(65);
    let bad = [
        (Agency::PestManagement, "PM-1"),
        (Agency::SeptageHauler, "SH-1"),
        (Agency::HomeInspector, "   "),
        (Agency::HomeInspector, long.as_str()),
    ];
    for record in bad {
        let refused = record_typed_agency_number(&mut db, b, record, actor).await;
        assert_eq!(
            refused.err(),
            Some(HolderError::BadRecord),
            "{:?}",
            record.0
        );
    }
    let fits = "H".repeat(64);
    record_typed_agency_number(&mut db, b, (Agency::HomeInspector, &fits), actor)
        .await
        .expect("64 characters fit");
    let again =
        record_typed_agency_number(&mut db, b, (Agency::HomeInspector, "HI-2"), actor).await;
    assert_eq!(again.err(), Some(HolderError::AlreadyRecorded));
    assert_eq!(verification(&mut db, b).await, ["typed_unchecked"]);
    let fields: Vec<String> = sqlx::query_scalar(
        "SELECT field FROM ops.audit_log WHERE action = 'licensing.license_recorded' ORDER BY id",
    )
    .fetch_all(&mut *db)
    .await
    .expect("audit rows");
    assert_eq!(fields, ["home_inspector"]);
}
