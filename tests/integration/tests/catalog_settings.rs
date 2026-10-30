//! The catalog loaded into its tables, and category switches (TD-LIC-01; LIC-001 to LIC-005, AI-017).
//!
//! Proves, as the app role on a fresh database, that every category's fields, job types, license
//! rule and fee tier load; that garage doors and junk removal are off and estate cleanouts and
//! donation pickup on; that a category not in the file is not stored; that the eight not-offered
//! kinds are known and cannot be switched on; that a licensed category with no resolved class
//! cannot be switched on; that only the owner switches, and each real switch is one audit row;
//! that loading again keeps the owner's switch but switches off, with a logged row, a category
//! that became unsafe or left the file. Fake data only.

use askhava_catalog::api::{
    CatalogError, category, enabled_categories, is_enabled, is_not_offered, load, load_embedded,
    parse, set_category_enabled,
};
use askhava_integration::set_app_role;
use askhava_ops::api::{Actor, export_audit};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const FILE: &str = include_str!("../../../docs/catalog/categories.yaml");
const OWNER: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0021);

async fn loaded(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    load_embedded(&mut conn).await.expect("load");
    conn
}

async fn switch_rows(conn: &mut PgConnection) -> usize {
    let rows = export_audit(conn, 0, 100).await.expect("export");
    rows.iter()
        .filter(|r| r.action == "catalog.category_switched")
        .count()
}

/// TD-LIC-01, LIC-001: a category loads with its job types, license rule and fee tier.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_category_loads_with_its_fields(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    let plumbing = category(&mut conn, "plumbing_repair")
        .await
        .expect("read")
        .expect("stored");
    assert_eq!(
        (plumbing.license_kind.as_str(), plumbing.license_classes),
        ("roc", vec!["R-37".to_owned()])
    );
    assert_eq!(
        (plumbing.trip_fee_tier, plumbing.recurring, plumbing.enabled),
        (25, false, true)
    );
    assert_eq!(plumbing.job_types.len(), 4);
    assert_eq!(
        (
            plumbing.after_hours_emergency,
            plumbing.handyman_allowed,
            plumbing.construction_type
        ),
        (true, true, true)
    );
    assert_eq!(plumbing.quote_format, "diagnostic_visit");
    assert!(plumbing.keywords.contains(&"faucet".to_owned()));
    assert_eq!(plumbing.agency, None);
    let faucet = &plumbing.job_types[0];
    assert_eq!(
        (
            faucet.name.as_str(),
            faucet.kind.as_deref(),
            faucet.quote_format.as_deref()
        ),
        ("drain clearing", Some("repair"), Some("firm"))
    );
    let leak = plumbing
        .job_types
        .iter()
        .find(|j| j.name == "leak repair")
        .expect("job type");
    assert_eq!(
        (leak.kind.as_deref(), leak.quote_format.as_deref()),
        (Some("repair"), None)
    );
    let pool_cleaning = category(&mut conn, "pool_cleaning")
        .await
        .expect("read")
        .expect("stored");
    assert!(pool_cleaning.recurring);
    let one_time: Vec<_> = pool_cleaning
        .job_types
        .iter()
        .filter(|j| j.one_time)
        .collect();
    assert_eq!(one_time.len(), 1);
    assert_eq!(one_time[0].name, "green pool cleanup");
    assert!(
        pool_cleaning
            .job_types
            .iter()
            .all(|j| !j.fee_free && j.kind.is_none())
    );
    assert_eq!(pool_cleaning.quote_format, "monthly");
    let pest = category(&mut conn, "pest_control")
        .await
        .expect("read")
        .expect("stored");
    assert_eq!(pest.license_kind, "state_agency");
    assert!(pest.license_classes.is_empty());
    assert_eq!(
        pest.agency.as_deref(),
        Some("AZ Department of Agriculture (pest management)")
    );
    let inspection = category(&mut conn, "home_inspection")
        .await
        .expect("read")
        .expect("stored");
    assert_eq!(
        inspection.job_types.iter().filter(|j| j.fee_free).count(),
        4
    );
    assert!(
        category(&mut conn, "window_tint")
            .await
            .expect("read")
            .is_none()
    );
}

/// TD-LIC-01, LIC-003, LIC-005: garage doors and junk removal are off; cleanouts and donation pickup on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn garage_doors_and_junk_removal_are_off(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    for code in ["garage_doors", "junk_removal", "window_tint"] {
        assert!(!is_enabled(&mut conn, code).await.expect("read"), "{code}");
    }
    for code in ["estate_cleanout", "donation_pickup"] {
        assert!(is_enabled(&mut conn, code).await.expect("read"), "{code}");
    }
    let on = enabled_categories(&mut conn).await.expect("list");
    assert_eq!(on.len(), 37);
}

/// TD-LIC-01, LIC-004: the eight not-offered kinds are known by any spelling and never switch on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_not_offered_kinds_are_known(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    for kind in [
        "childcare",
        "Home care",
        "massage",
        "mobile_hair_and_nails",
        "mobile vets",
        "catering",
        "towing",
        "property management",
    ] {
        assert!(
            is_not_offered(&mut conn, kind).await.expect("read"),
            "{kind}"
        );
    }
    assert!(!is_not_offered(&mut conn, "home watch").await.expect("read"));
    assert!(
        !is_not_offered(&mut conn, "plumbing repair")
            .await
            .expect("read")
    );
    assert_eq!(
        set_category_enabled(&mut conn, "no_such_category", true, Actor::Owner(OWNER)).await,
        Err(CatalogError::UnknownCategory)
    );
}

/// TD-LIC-01, LIC-002: turning a category off and on is a logged settings change, once per change.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn switching_a_category_is_logged(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    let owner = Actor::Owner(OWNER);
    assert_eq!(
        set_category_enabled(&mut conn, "handyman", false, owner).await,
        Ok(true)
    );
    assert!(!is_enabled(&mut conn, "handyman").await.expect("read"));
    assert_eq!(
        set_category_enabled(&mut conn, "handyman", false, owner).await,
        Ok(false)
    );
    assert_eq!(switch_rows(&mut conn).await, 1, "a repeat writes nothing");
    assert_eq!(
        set_category_enabled(&mut conn, "handyman", true, owner).await,
        Ok(true)
    );
    assert_eq!(switch_rows(&mut conn).await, 2);
    let rows = export_audit(&mut conn, 0, 100).await.expect("export");
    let row = &rows[0];
    assert_eq!(
        (
            row.actor_id,
            row.from_status.as_deref(),
            row.to_status.as_deref()
        ),
        (Some(OWNER), Some("on"), Some("off"))
    );
}

/// TD-LIC-01, LIC-003, AI-017: a ROC category with no resolved class and a non-owner stay refused;
/// loading again keeps the owner's switch.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_unresolved_class_and_a_non_owner_cannot_switch_on(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    let owner = Actor::Owner(OWNER);
    assert_eq!(
        set_category_enabled(&mut conn, "garage_doors", true, owner).await,
        Err(CatalogError::LicenseUnresolved)
    );
    assert_eq!(
        set_category_enabled(&mut conn, "junk_removal", true, Actor::System).await,
        Err(CatalogError::OwnerOnly)
    );
    assert_eq!(switch_rows(&mut conn).await, 0);
    assert_eq!(
        set_category_enabled(&mut conn, "junk_removal", true, owner).await,
        Ok(true)
    );
    load_embedded(&mut conn).await.expect("load again");
    assert!(
        is_enabled(&mut conn, "junk_removal").await.expect("read"),
        "a reload keeps the switch"
    );
}

async fn reload_with(
    conn: &mut PgConnection,
    edit: impl FnOnce(&mut askhava_catalog::api::Catalog),
) {
    let mut catalog = parse(FILE).expect("parses");
    edit(&mut catalog);
    load(conn, &catalog).await.expect("reload");
}

/// LIC-003, AI-017: a reload that empties a ROC category's classes switches it off and logs it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_reload_with_no_classes_switches_a_roc_category_off(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    assert!(
        is_enabled(&mut conn, "plumbing_repair")
            .await
            .expect("read")
    );
    reload_with(&mut conn, |c| {
        let def = c
            .categories
            .iter_mut()
            .find(|d| d.id == "plumbing_repair")
            .expect("listed");
        def.license.classes = vec![];
    })
    .await;
    assert!(
        !is_enabled(&mut conn, "plumbing_repair")
            .await
            .expect("read")
    );
    assert_eq!(switch_rows(&mut conn).await, 1);
    let rows = export_audit(&mut conn, 0, 100).await.expect("export");
    assert_eq!(
        (
            rows[0].actor_id,
            rows[0].from_status.as_deref(),
            rows[0].to_status.as_deref()
        ),
        (None, Some("on"), Some("off"))
    );
}

/// LIC-004: a kind newly on the not-offered list is switched off by a reload, and a kind taken off
/// the list is no longer reported as not offered.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_reload_applies_a_changed_not_offered_list(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    reload_with(&mut conn, |c| {
        c.not_offered_at_launch.retain(|n| n != "towing");
        c.not_offered_at_launch.push("Handyman".to_owned());
    })
    .await;
    assert!(!is_enabled(&mut conn, "handyman").await.expect("read"));
    assert!(is_not_offered(&mut conn, "handyman").await.expect("read"));
    assert!(!is_not_offered(&mut conn, "towing").await.expect("read"));
    assert_eq!(switch_rows(&mut conn).await, 1);
}

/// LIC-003: a category dropped from the file is switched off and logged, and its code reads as off.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_category_dropped_from_the_file_is_switched_off(pool: PgPool) {
    let mut conn = loaded(&pool).await;
    reload_with(&mut conn, |c| c.categories.retain(|d| d.id != "handyman")).await;
    assert!(!is_enabled(&mut conn, "handyman").await.expect("read"));
    assert_eq!(switch_rows(&mut conn).await, 1);
    assert_eq!(enabled_categories(&mut conn).await.expect("list").len(), 36);
}
