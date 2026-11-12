//! A pest business's identity, its cards and pest control switched off (TD-BIZ-32's pest cases,
//! TD-LIC-22's attestation and switch-off cases; T215, D-446, D-3055, D-3333, D-3335; LIC-048,
//! LIC-047, LIC-012, BIZ-016, CARD-015).
//!
//! As the app role on a fresh database: a pest business activates only once it has attested and
//! a daily city list shows its name (with its code done); its cards come from the attestation
//! alone with no daily state check, and termite and pre-emergent jobs only with the attested
//! category (through `businesses::api::holding` and `licensing::api::eligibility`); with pest
//! control switched off, a pest request closes early with the pointer to the Department's license
//! search and logs no demand. The leaving, saved-card, session, link and name cases are T165's and
//! T228's; the wrong-license report is T389's. All names, numbers and texts are fake.

use askhava_businesses::api::{
    Known, PestIdentity, create, holding, pest_identity, record_pest_match,
};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, NOT_COVERED_KIND, Opened, Outcome, PEST_CONTROL, PEST_OFF_TEMPLATE, Request, Routing,
    TextIn, Why, apply_routing, begin_job, receive_text, recruiting_demand, route, trade_is_open,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::{
    Agency, CityRecord, ClassMap, Eligibility, Job, LicenseKind, NoCardReason, PestAttestation,
    PestRecordCategory, eligibility, import_city_list, record_pest_attestation,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-8101,Example Bug Co LLC,Sample Pest Pros,Pat Placeholder,2020-02-03\n";
const TOWN: &str = "lake_havasu_city";

fn now() -> Timestamp {
    "2026-11-12T17:00:00Z".parse().expect("fake time")
}

async fn app(pool: &sqlx::PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// A pending pest business whose identity code is done (the code step is TD-BIZ-01's).
async fn pest_business(conn: &mut PgConnection) -> Uuid {
    let id = create(conn, false, now()).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades, code_done) \
         VALUES ($1, gen_random_uuid(), now(), ARRAY['pest_control'], true)",
    )
    .bind(id)
    .execute(&mut *conn)
    .await
    .expect("sign-up row");
    id
}

fn names() -> CityRecord {
    CityRecord {
        business_name: "Example Bug Co LLC".to_owned(),
        ..CityRecord::default()
    }
}

fn pest_job(job_type: &str) -> Job {
    Job {
        category: PEST_CONTROL.to_owned(),
        job_type: job_type.to_owned(),
        license: LicenseKind::StateAgency,
        handyman_allowed: false,
        listed: true,
    }
}

/// TD-BIZ-32, TD-LIC-22 (LIC-048, BIZ-016): a pest business stays pending with no attestation
/// and while no city list shows its name; the city-list match plus the attestation activates it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_32_pest_business_activates_on_city_list_match_plus_attestation(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = pest_business(&mut conn).await;
    let names = names();
    let known = Known {
        roc_license: None,
        city_names: Some(&names),
    };
    import_city_list(&mut conn, date(2026, 11, 11), CITY)
        .await
        .expect("city list");
    let (identity, status) = record_pest_match(&mut conn, id, known, now())
        .await
        .expect("no attestation");
    assert_eq!(identity, PestIdentity::NoAttestation);
    assert_eq!(
        status.code(),
        "pending",
        "the city list alone never activates"
    );
    let attested = PestAttestation::new(true, "PM-7001", false, false).expect("attestation");
    record_pest_attestation(&mut conn, id, &attested, Actor::Business(id))
        .await
        .expect("attested");
    let other = CityRecord {
        business_name: "Nobody Listed LLC".to_owned(),
        ..CityRecord::default()
    };
    let unlisted = Known {
        roc_license: None,
        city_names: Some(&other),
    };
    let (identity, status) = record_pest_match(&mut conn, id, unlisted, now())
        .await
        .expect("not listed");
    assert_eq!(identity, PestIdentity::CityListPending);
    assert_eq!(
        status.code(),
        "pending",
        "the attestation alone never activates"
    );
    let (identity, status) = record_pest_match(&mut conn, id, known, now())
        .await
        .expect("matched");
    assert_eq!(identity, PestIdentity::Matched);
    assert_eq!(status.code(), "active");
}

/// TD-BIZ-32, TD-LIC-22 (LIC-048, LIC-012, LIC-047, CARD-015): with no daily state check on
/// record a pest attester gets general pest cards; termite and pre-emergent cards only with the
/// attested category; a business that never attested gets no pest card even on the city list.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_22_pest_cards_follow_the_attestation_with_no_daily_check(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    import_city_list(&mut conn, date(2026, 11, 11), CITY)
        .await
        .expect("city list");
    let names = names();
    let known = Known {
        roc_license: None,
        city_names: Some(&names),
    };
    let general = pest_business(&mut conn).await;
    let termites = pest_business(&mut conn).await;
    let never = pest_business(&mut conn).await;
    let plain = PestAttestation::new(true, "PM-7002", false, false).expect("plain");
    let wood = PestAttestation::new(true, "PM-7003", true, false).expect("wood");
    for (id, attested) in [(general, &plain), (termites, &wood)] {
        record_pest_attestation(&mut conn, id, attested, Actor::Business(id))
            .await
            .expect("attested");
    }
    let map = ClassMap::shipped().expect("license map");
    let card = |parts: &askhava_licensing::api::HolderParts, job_type: &str| {
        eligibility(
            &pest_job(job_type),
            &parts.for_job(map, PEST_CONTROL, job_type),
        )
    };
    let parts = holding(&mut conn, general, known).await.expect("general");
    assert_eq!(card(&parts, "general pest service"), Eligibility::Card);
    assert_eq!(card(&parts, "scorpions"), Eligibility::Card);
    let missing = |c| Eligibility::NoCard(NoCardReason::PestCategoryMissing(c));
    assert_eq!(
        card(&parts, "termites"),
        missing(PestRecordCategory::WoodDestroying)
    );
    assert_eq!(
        card(&parts, "pre-emergent weed control"),
        missing(PestRecordCategory::WeedControl)
    );
    let parts = holding(&mut conn, termites, known).await.expect("termites");
    assert_eq!(card(&parts, "termites"), Eligibility::Card);
    let parts = holding(&mut conn, never, known).await.expect("never");
    let refused = Eligibility::NoCard(NoCardReason::AgencyMissing(Agency::PestManagement));
    assert_eq!(card(&parts, "general pest service"), refused);
    assert_eq!(
        pest_identity(&mut conn, never, known).await.expect("never"),
        PestIdentity::NoAttestation
    );
    let checks: i64 = sqlx::query_scalar("SELECT count(*) FROM licensing.license_check")
        .fetch_one(&mut *conn)
        .await
        .expect("checks");
    assert_eq!(checks, 0, "no daily state check was needed");
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn text(contact: Uuid) -> TextIn<'static> {
    TextIn {
        inbox_id: Uuid::from_u128(0x4_5000 + contact.as_u128()),
        contact,
        inbound: Inbound {
            words: "scorpions in the garage again",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now(),
    }
}

/// TD-LIC-22 (LIC-012, D-402): with pest control switched off, a pest request closes early with
/// the pointer to the Department's license search; no demand row and no owner email task.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_22_switched_off_pest_request_closes_early_with_the_pointer(
    mut db: sqlx::pool::PoolConnection<sqlx::Postgres>,
) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let owner = Actor::Owner(Uuid::from_u128(0x0e1));
    askhava_catalog::api::set_category_enabled(&mut db, PEST_CONTROL, false, owner)
        .await
        .expect("switched off");
    let msg = text(Uuid::from_u128(0x7101));
    let handled = receive_text(&mut db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let conversation = handled.conversation.expect("request");
    let start = JobStart {
        conversation,
        words: msg.inbound.words,
        emergency: false,
        trade: PEST_CONTROL,
        at: now(),
    };
    let Opened::Created(job) = begin_job(&mut db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let req = Request {
        town: TOWN,
        area: &[TOWN],
        trade: PEST_CONTROL,
        trade_open: trade_is_open(&mut db, PEST_CONTROL).await.expect("open"),
        words: msg.inbound.words,
        move_states: None,
        job_types: &[],
    };
    let outcome = route(&req);
    assert_eq!(outcome, Outcome::NotCovered(Why::PestSwitchedOff));
    let routing = Routing {
        conversation,
        msg: &msg,
        language: askhava_messaging::api::Language::En,
        spanish_on: false,
        label: "pest control",
        job_type: None,
    };
    let queued = apply_routing(&mut db, &secret(), (&req, outcome), &routing)
        .await
        .expect("apply")
        .expect("the pointer was queued");
    assert_eq!(queued.template, PEST_OFF_TEMPLATE);
    let status = askhava_jobs::api::current(&mut db, job).await.expect("job");
    assert_eq!(status.status.code(), "closed_early");
    let demand = recruiting_demand(&mut db, conversation)
        .await
        .expect("rows");
    assert!(demand.is_empty(), "recruiting cannot fill it");
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(NOT_COVERED_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(tasks, 0, "no owner email for a switched-off trade");
    let body = askhava_messaging::api::render(PEST_OFF_TEMPLATE, routing.language, false, &[])
        .expect("render")
        .body;
    assert!(body.contains("Arizona Department of Agriculture's license list"));
    assert!(!body.contains('$') && !body.to_lowercase().contains("fee"));
}
