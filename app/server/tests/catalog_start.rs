//! TD-LIC-31 (LIC-001, LIC-002, LIC-003; T400, D-3100): the web process stores the catalog file at
//! start-up, so every reader follows the stored switch. Fake data only; the owner id is made up.

use askhava_catalog::api::{enabled_categories, is_enabled, parse, set_category_enabled};
use askhava_core::api::MIGRATOR;
use askhava_ops::api::Actor;
use askhava_server::catalog_start::store_catalog;
use sqlx::PgPool;
use sqlx::types::Uuid;

const OWNER: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0400);
const FILE: &str = include_str!("../../../docs/catalog/categories.yaml");

/// LIC-003: with no stored row a category reads as off, before the step runs.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_category_with_no_row_is_off(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    assert!(!is_enabled(&mut conn, "handyman").await.expect("read"));
    assert!(
        enabled_categories(&mut conn)
            .await
            .expect("read")
            .is_empty()
    );
}

/// LIC-001: on an empty database the step stores the file; the on set is the file's on set and a
/// category the file has off reads as off.
#[sqlx::test(migrator = "MIGRATOR")]
async fn the_step_stores_the_file(pool: PgPool) {
    store_catalog(&pool).await.expect("store");
    let mut conn = pool.acquire().await.expect("conn");
    let mut expected: Vec<String> = parse(FILE)
        .expect("file")
        .categories
        .into_iter()
        .filter(|c| c.enabled)
        .map(|c| c.id)
        .collect();
    expected.sort();
    assert!(!expected.is_empty());
    assert!(is_enabled(&mut conn, &expected[0]).await.expect("read"));
    assert!(!is_enabled(&mut conn, "junk_removal").await.expect("read"));
    let stored = enabled_categories(&mut conn).await.expect("read");
    assert!(expected.iter().all(|c| stored.contains(c)));
    assert!(!stored.iter().any(|c| c == "junk_removal"));
}

/// LIC-002: a category the owner switched off stays off after the step runs again (a restart).
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_restart_keeps_the_owners_switch(pool: PgPool) {
    store_catalog(&pool).await.expect("first");
    let mut conn = pool.acquire().await.expect("conn");
    let done = set_category_enabled(&mut conn, "handyman", false, Actor::Owner(OWNER)).await;
    assert_eq!(done, Ok(true));
    store_catalog(&pool).await.expect("second");
    assert!(!is_enabled(&mut conn, "handyman").await.expect("read"));
}

/// Two web processes starting at once both succeed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn two_runs_at_once_both_succeed(pool: PgPool) {
    let (a, b) = tokio::join!(store_catalog(&pool), store_catalog(&pool));
    assert!(a.is_ok() && b.is_ok());
    let mut conn = pool.acquire().await.expect("conn");
    assert!(
        !enabled_categories(&mut conn)
            .await
            .expect("read")
            .is_empty()
    );
}

/// SEC-002: a failure is an error naming no value (here the schema is missing).
#[sqlx::test]
async fn a_failed_load_is_an_error_naming_nothing(pool: PgPool) {
    let err = store_catalog(&pool).await.expect_err("no schema");
    assert_eq!(err.to_string(), "the catalog file could not be stored");
}

/// The binary's web path calls the step after the pool and before it serves; `migrate` and
/// `bootstrap-owner` return before it.
#[test]
fn the_web_path_stores_the_catalog_before_serving() {
    let main = include_str!("../src/main.rs");
    let at = |needle: &str| {
        main.find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"))
    };
    let pool = at("app_pool(&settings)");
    let step = at("catalog_start::store_catalog(&pool).await?");
    let serve = at("axum::serve(");
    assert!(pool < step && step < serve);
    assert!(at("return run_migrations") < step);
    assert!(at("return bootstrap_owner") < step);
    assert_eq!(main.matches("store_catalog").count(), 1);
}
