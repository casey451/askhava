//! The list of every stored category with its plural display name, and the kinds not offered, for
//! the site's services and coverage pages (WEB-015, WEB-023, D-3879).
//!
//! The plural is read from the catalog file compiled in (`loader::embedded`), never stored: the
//! tables keep the code, name, switch and license kind, the file keeps the plural. A stored code the
//! file lacks, or a category whose plural is empty, is an error, never a guessed name.

use std::collections::BTreeMap;

use sqlx::PgConnection;

use crate::loader;
use crate::models::{Catalog, CatalogError};

/// One stored category as the site lists it (WEB-015, WEB-023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryListing {
    /// The code name, such as `plumbing_repair`.
    pub code: String,
    /// The display name, such as `Plumbing repair`.
    pub name: String,
    /// The plural display name, lower case as it reads after "licensed", such as `plumbers`.
    pub plural: String,
    /// Whether the category is switched on (LIC-002, LIC-003).
    pub enabled: bool,
    /// The license kind: `none`, `roc`, `state_agency` or `roc_and_state_agency`.
    pub license_kind: String,
}

/// Every stored category in code order and the kinds not offered at launch (WEB-023, LIC-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryList {
    /// Every stored category, switched on or off, sorted by code.
    pub categories: Vec<CategoryListing>,
    /// The names of the kinds not offered at launch, sorted.
    pub not_offered: Vec<String>,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, CatalogError> {
    r.map_err(|_| CatalogError::Database)
}

/// Every stored category with its plural from the compiled-in catalog, and the not-offered kinds
/// (WEB-015, WEB-023, D-3879).
///
/// # Errors
///
/// [`CatalogError::Invalid`] when the compiled-in file is invalid, a stored code is missing from it
/// or its plural is empty; [`CatalogError::Database`].
pub async fn category_list(db: &mut PgConnection) -> Result<CategoryList, CatalogError> {
    category_list_from(db, &loader::embedded()?).await
}

/// [`category_list`] with the plurals read from `catalog` instead of the compiled-in file
/// (WEB-015, WEB-023).
///
/// # Errors
///
/// As [`category_list`].
pub async fn category_list_from(
    conn: &mut PgConnection,
    catalog: &Catalog,
) -> Result<CategoryList, CatalogError> {
    let plurals: BTreeMap<&str, &str> = catalog
        .categories
        .iter()
        .map(|d| (d.id.as_str(), d.plural.as_str()))
        .collect();
    let rows = db(sqlx::query!(
        "SELECT c.code, c.name, c.enabled, l.kind AS license_kind \
         FROM catalog.category c JOIN catalog.license_rule l ON l.category_id = c.id \
         ORDER BY c.code"
    )
    .fetch_all(&mut *conn)
    .await)?;
    let mut categories = Vec::with_capacity(rows.len());
    for r in rows {
        let plural = plurals.get(r.code.as_str()).copied().unwrap_or_default();
        if plural.trim().is_empty() {
            return Err(CatalogError::Invalid);
        }
        categories.push(CategoryListing {
            plural: plural.to_owned(),
            code: r.code,
            name: r.name,
            enabled: r.enabled,
            license_kind: r.license_kind,
        });
    }
    let not_offered = db(sqlx::query_scalar!(
        "SELECT name FROM catalog.not_offered ORDER BY name"
    )
    .fetch_all(&mut *conn)
    .await)?;
    Ok(CategoryList {
        categories,
        not_offered,
    })
}
