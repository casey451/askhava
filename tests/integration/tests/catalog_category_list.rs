//! The site's category list: every stored category with its plural display name, its switch and
//! license kind, and the kinds not offered (TD-LIC-01; WEB-015, WEB-023, D-3879).
//!
//! Proves, as the app role on a fresh database loaded with the compiled-in catalog, that every
//! category is listed with a non-empty plural, that a switched-off category reads off and a
//! switched-on one on, that the not-offered names are listed, and that a category with an empty
//! plural makes the list an error rather than a guessed name. Fake data only.

use askhava_catalog::api::{
    CatalogError, CategoryList, category_list, category_list_from, load_embedded, parse,
};
use askhava_integration::set_app_role;
use sqlx::PgPool;

const FILE: &str = include_str!("../../../docs/catalog/categories.yaml");

async fn listed(pool: &PgPool) -> CategoryList {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    load_embedded(&mut conn).await.expect("load");
    category_list(&mut conn).await.expect("list")
}

/// TD-LIC-01, WEB-015: every category of the compiled-in file has a non-empty plural.
#[test]
fn every_category_of_the_file_has_a_plural() {
    let catalog = parse(FILE).expect("parse");
    assert!(!catalog.categories.is_empty());
    for def in &catalog.categories {
        assert!(!def.plural.trim().is_empty(), "{} has no plural", def.id);
    }
}

/// TD-LIC-01, WEB-015, WEB-023: the list holds every stored category, each with its plural.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_list_holds_every_category_with_its_plural(pool: PgPool) {
    let list = listed(&pool).await;
    let catalog = parse(FILE).expect("parse");
    assert_eq!(list.categories.len(), catalog.categories.len());
    assert!(list.categories.iter().all(|c| !c.plural.is_empty()));
    let codes: Vec<&str> = list.categories.iter().map(|c| c.code.as_str()).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted);
    let plumbing = list
        .categories
        .iter()
        .find(|c| c.code == "plumbing_repair")
        .expect("plumbing listed");
    assert_eq!(
        (plumbing.name.as_str(), plumbing.plural.as_str()),
        ("Plumbing repair", "plumbers")
    );
    assert_eq!(plumbing.license_kind, "roc");
}

/// TD-LIC-01, WEB-023: a switched-off category reads off and a switched-on one on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_list_shows_each_switch(pool: PgPool) {
    let list = listed(&pool).await;
    let on = |code: &str| {
        list.categories
            .iter()
            .find(|c| c.code == code)
            .map(|c| c.enabled)
    };
    assert_eq!(on("garage_doors"), Some(false));
    assert_eq!(on("junk_removal"), Some(false));
    assert_eq!(on("plumbing_repair"), Some(true));
    assert_eq!(on("estate_cleanout"), Some(true));
}

/// TD-LIC-01, WEB-023, LIC-004: the not-offered names are listed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_list_names_the_kinds_not_offered(pool: PgPool) {
    let list = listed(&pool).await;
    let catalog = parse(FILE).expect("parse");
    let mut expected = catalog.not_offered_at_launch.clone();
    expected.sort_unstable();
    assert_eq!(list.not_offered, expected);
    assert!(list.not_offered.iter().any(|n| n == "towing"));
}

/// WEB-015, D-3879: a category with an empty plural makes the list an error, never a guessed name.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_empty_plural_is_an_error(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    load_embedded(&mut conn).await.expect("load");
    let text = FILE.replacen("plural: plumbers", "plural: \"\"", 1);
    assert_ne!(text, FILE);
    let catalog = parse(&text).expect("parse");
    assert_eq!(
        category_list_from(&mut conn, &catalog).await,
        Err(CatalogError::Invalid)
    );
}
