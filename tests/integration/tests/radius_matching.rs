//! Radius matching and the portal's service-area edit on a fresh database (T249; TD-CARD-13's
//! database case: CARD-024, CARD-001, CARD-002; TD-BIZ-38: BIZ-010, BIZ-058, PRIV-028; D-1519,
//! D-1521). Fake IDs, ZIP codes from the reviewed Arizona centroid table and no homeowner data.
//!
//! Each business's reach is read back through `businesses::api::service_area` (its base ZIP's
//! centroid from the reviewed table, or none) and the job's location through
//! `catalog::api::zip_location`, as `quotes`' card pool fills them; `send_job_cards_in_reach`
//! then records the cards. One case builds the pool through the real `quotes::api::card_pool`
//! from stored businesses (base ZIP, radius, towns, ticks, welcome), as `intake`'s sender does.

use std::collections::{BTreeMap, BTreeSet};

use askhava_businesses::api::{
    AccountStatus, Acting, Checks, Event, JobTypeChoice, Roles, ServiceArea, ServiceAreaProblem,
    add_member, add_owner, apply, create, request_welcome, save_service_area, service_area,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::{Catalog, load_embedded, parse, zip_location};
use askhava_integration::set_app_role;
use askhava_jobs::api::create as create_job;
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_portals::api::{
    AreaChangeError, AreaSettings, BUSINESS_SESSION_LIFE, BusinessPrincipal, CookieJar, Realm,
    business_principal, change_service_area, service_area_settings_page, start_session,
};
use askhava_quotes::api::{
    BusinessReach, CardBusiness, CardFacts, CardJob, Coverage, OfferTown, OwnerPauses, Pool,
    Pooled, Reach, card_holders, card_pool, coverage_in_reach, send_job_cards,
    send_job_cards_in_reach,
};
use jiff::Timestamp;
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const CATEGORIES: &str = include_str!("../../../docs/catalog/categories.yaml");
const TOWN: &str = "lake_havasu_city";
const AC: (&str, &str) = ("hvac_repair", "AC not cooling");

fn at(minute: i8) -> Timestamp {
    format!("2026-11-02T10:{minute:02}:00-07:00")
        .parse()
        .expect("time")
}

fn ac_job(catalog: &Catalog) -> Job {
    let def = catalog
        .categories
        .iter()
        .find(|c| c.id == AC.0)
        .expect("category");
    Job {
        category: AC.0.to_owned(),
        job_type: AC.1.to_owned(),
        license: LicenseKind::parse(&def.license.kind).expect("kind"),
        handyman_allowed: def.handyman_allowed,
        listed: def.job_types.iter().any(|j| j == AC.1),
    }
}

/// An active, verified R-39 business `id` serving `town` and ticking the AC job type.
fn pooled(id: Uuid, town: &str) -> Pooled {
    Pooled {
        card: CardBusiness {
            id,
            status: AccountStatus::Active,
            verified: true,
            towns: BTreeSet::from([town.to_owned()]),
            ticked: BTreeSet::from([(AC.0.to_owned(), AC.1.to_owned())]),
            classes: vec!["R-39".to_owned()],
            licenses: Holder::default(),
        },
        overflow_on: false,
    }
}

/// A stored business with base ZIP `zip` and `radius` miles.
async fn business_at(db: &mut PgConnection, zip: &str, radius: &str) -> Uuid {
    let b = create(db, false, at(0)).await.expect("business");
    let area = ServiceArea::parse(zip, radius).expect("area");
    save_service_area(db, b, &area, (Actor::Business(b), at(0)))
        .await
        .expect("saved");
    b
}

/// Each business's reach as the card pool reads it: the stored base centroid and radius.
async fn reach_of(db: &mut PgConnection, job_zip: &str, ids: &[Uuid]) -> Reach {
    let mut businesses = BTreeMap::new();
    for id in ids {
        let stored = service_area(db, *id).await.expect("read").expect("area");
        if let Some(base) = stored.location {
            let radius_miles = stored.area.radius_miles();
            businesses.insert(*id, BusinessReach { base, radius_miles });
        }
    }
    Reach {
        job: zip_location(job_zip).expect("table"),
        businesses,
    }
}

/// TD-CARD-13 (CARD-024, CARD-001, CARD-002): a Kingman business whose 100-mile radius reaches a
/// Lake Havasu City job gets its card through `send_job_cards_in_reach`; a Kingman one with 20
/// miles does not; a town business whose base ZIP has no centroid gets it by its town; the
/// coverage count equals the cards sent; `send_job_cards` (no reach) sends to the town only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_13_send_job_cards_reaches_a_business_in_another_town(mut db: Db) {
    let far = business_at(&mut db, "86401", "100").await;
    let short = business_at(&mut db, "86401", "20").await;
    let out_of_table = business_at(&mut db, "92363", "50").await;
    let reach = reach_of(&mut db, "86403", &[far, short, out_of_table]).await;
    assert!(reach.job.is_some());
    assert!(!reach.businesses.contains_key(&out_of_table));
    let pool = [
        pooled(far, "kingman"),
        pooled(short, "kingman"),
        pooled(out_of_table, TOWN),
    ];
    let pauses = OwnerPauses::default();
    let pool = Pool {
        businesses: &pool,
        pauses: &pauses,
    };
    let catalog = parse(CATEGORIES).expect("catalog");
    let job = CardJob {
        towns: vec![OfferTown {
            code: TOWN.to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![ac_job(&catalog)],
        text: String::new(),
    };
    let map = ClassMap::shipped().expect("map");
    let towns_only = create_job(&mut db, at(1), false, Actor::System)
        .await
        .expect("job");
    let sent = send_job_cards(&mut db, map, towns_only, &job, &pool, at(2)).await;
    let only: Vec<Uuid> = sent.expect("sent").iter().map(|r| r.business).collect();
    assert_eq!(only, vec![out_of_table]);
    let id = create_job(&mut db, at(1), false, Actor::System)
        .await
        .expect("job");
    let sent = send_job_cards_in_reach(&mut db, map, id, (&job, &pool, &reach), at(2))
        .await
        .expect("sent");
    let mut want = vec![far, out_of_table];
    want.sort();
    assert_eq!(card_holders(&mut db, id).await.expect("holders"), want);
    let count = coverage_in_reach(&job, &pool, &reach).expect("count");
    assert_eq!(count, Coverage::Send(sent.len()));
}

/// An active, welcomed business serving only `town`, ticking the AC job type, with base ZIP `zip`
/// and `radius` miles, as the card pool reads it from the database.
async fn stored_business(db: &mut PgConnection, town: &str, (zip, radius): (&str, &str)) -> Uuid {
    let b = business_at(db, zip, radius).await;
    add_owner(db, b, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, at(0))
        .await
        .expect("active");
    request_welcome(db, b, at(0)).await.expect("welcome");
    welcome_sent(db, b, at(0)).await.expect("sent");
    let actor = Actor::Business(b);
    set_towns(db, b, &[town], actor).await.expect("towns");
    let choice = JobTypeChoice::new(AC.0, AC.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
    b
}

/// TD-CARD-13 through the real `quotes::api::card_pool` (CARD-024, D-1519): a business serving
/// only Kingman (base 86401) with 100 miles is pooled, with its reach, for a Lake Havasu City job
/// at 86403; one with 20 miles is not; with no ZIP the job is towns only and neither is pooled.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_13_card_pool_reaches_a_business_serving_another_town(mut db: Db) {
    load_embedded(&mut db).await.expect("catalog");
    let far = stored_business(&mut db, "kingman", ("86401", "100")).await;
    let short = stored_business(&mut db, "kingman", ("86401", "20")).await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let facts = CardFacts {
        category: AC.0.to_owned(),
        candidates: vec![AC.1.to_owned()],
        emergency: false,
    };
    let pool = card_pool(
        &mut db,
        ((TOWN, Some("86403")), &facts),
        String::new(),
        None,
    )
    .await
    .expect("pool");
    let pooled: Vec<Uuid> = pool.businesses.iter().map(|p| p.card.id).collect();
    assert_eq!(pooled, vec![far]);
    assert!(pool.reach.job.is_some());
    assert!(pool.reach.reaches(far) && !pool.reach.reaches(short));
    let pool = card_pool(&mut db, ((TOWN, None), &facts), String::new(), None)
        .await
        .expect("pool");
    assert!(pool.businesses.is_empty());
    assert!(pool.reach.job.is_none() && pool.reach.businesses.is_empty());
}

/// A business with its owner signed in, and a second member with `roles` signed in too.
async fn signed_in(
    db: &mut PgConnection,
    roles: Roles,
) -> (Uuid, BusinessPrincipal, BusinessPrincipal) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(0x2490_0001), at(0))
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let other = add_member(db, by, Uuid::from_u128(0x2490_0002), roles, at(0))
        .await
        .expect("member");
    let owner = principal(db, business, owner).await;
    let other = principal(db, business, other).await;
    (business, owner, other)
}

async fn principal(db: &mut PgConnection, business: Uuid, member: Uuid) -> BusinessPrincipal {
    let session = (Realm::Business, member);
    let jar = start_session(db, CookieJar::new(), session, BUSINESS_SESSION_LIFE, at(0))
        .await
        .expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(member)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let read = business_principal(db, &jar, at(1)).await.expect("read");
    read.expect("signed in")
}

/// `business`'s audit rows in order: (action, `field` column, every column as JSON).
async fn audit_fields(
    db: &mut PgConnection,
    business: Uuid,
) -> Vec<(String, Option<String>, Value)> {
    let rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT action, field, row_to_json(audit_log)::text FROM ops.audit_log \
         WHERE subject_id = $1 ORDER BY id",
    )
    .bind(business)
    .fetch_all(db)
    .await
    .expect("audit");
    rows.into_iter()
        .map(|(a, f, row)| (a, f, serde_json::from_str(&row).expect("json")))
        .collect()
}

/// PRIV-028: no column of an audit row holds the new radius or base ZIP.
fn holds_no_value(row: &Value) {
    let columns = row.as_object().expect("columns");
    for (name, value) in columns {
        let text = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        assert!(text != "30" && !text.contains("86406"), "{name}: {text}");
    }
}

/// TD-BIZ-38 (BIZ-010, BIZ-058, PRIV-028): the portal changes the base ZIP and radius with
/// sign-up's checks (a refusal stores nothing and the page shows it in the error state); a member
/// without "Confirm and billing" cannot; the change is audited by field name only, never the
/// value, and changes no other business's record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_38_portal_changes_base_zip_and_radius(mut db: Db) {
    let neighbour = business_at(&mut db, "86404", "15").await;
    let quoter = Roles::new(true, false).expect("roles");
    let (business, owner, other) = signed_in(&mut db, quoter).await;
    let before = audit_fields(&mut db, business).await.len();
    let refused = change_service_area(&mut db, &owner, ("8640", "101"), at(2)).await;
    let problems = vec![ServiceAreaProblem::BaseZip, ServiceAreaProblem::Radius];
    assert_eq!(refused, Err(AreaChangeError::Refused(problems.clone())));
    assert_eq!(service_area(&mut db, business).await.expect("read"), None);
    let page = AreaSettings {
        base_zip: "8640",
        radius: "101",
        problems: &problems,
        saved: false,
    };
    let html = service_area_settings_page(&page, "fake-csrf").expect("page");
    assert!(html.contains("<title>Error: "));
    assert!(html.contains("id=\"error-summary\" role=\"alert\""));
    assert!(html.contains("value=\"8640\"") && html.contains("value=\"101\""));
    assert!(html.contains("id=\"area-saved\" role=\"status\"></p>"));
    let denied = change_service_area(&mut db, &other, ("86406", "30"), at(2)).await;
    assert_eq!(denied, Err(AreaChangeError::NotAllowed));
    let saved = change_service_area(&mut db, &owner, (" 86406 ", "30"), at(3)).await;
    assert_eq!(saved.as_ref().map(ServiceArea::base_zip), Ok("86406"));
    let stored = service_area(&mut db, business)
        .await
        .expect("read")
        .expect("area");
    assert_eq!(stored.area.radius_miles(), 30);
    assert!(stored.location.is_some());
    let rows = audit_fields(&mut db, business).await;
    let new = &rows[before..];
    let mut fields: Vec<&str> = new.iter().filter_map(|r| r.1.as_deref()).collect();
    fields.sort_unstable();
    assert_eq!(fields, vec!["base_zip", "service_radius"]);
    for (action, _, row) in new {
        assert_eq!(action, "businesses.field_changed");
        holds_no_value(row);
    }
    let theirs = service_area(&mut db, neighbour)
        .await
        .expect("read")
        .expect("area");
    assert_eq!(
        (theirs.area.base_zip(), theirs.area.radius_miles()),
        ("86404", 15)
    );
}
