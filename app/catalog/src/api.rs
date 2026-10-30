//! The public face of `askhava-catalog`: everything other crates may call (D-481).
//!
//! Load the reviewed catalog file, read a category with its job types, license rule and fee rule,
//! list the categories that are on, test the not-offered kinds, and switch a category as the owner
//! (LIC-001 to LIC-005, AI-017). No function takes a license class: classes come from the file only.

use askhava_ops::api::Actor;
use sqlx::PgConnection;

use crate::{loader, service};

pub use crate::loader::{normalize, parse};
pub use crate::models::{Catalog, CatalogError, Category, JobType};
pub use crate::question_bank::{AskIf, Question, QuestionBank, QuestionSet};

/// Parses the catalog file compiled into the build and stores it (LIC-001). Run it at start-up and
/// after the file changes; it is safe to run again and never changes an existing category's switch,
/// so a category the owner turned on or off keeps that state (LIC-002).
///
/// # Errors
///
/// [`CatalogError::Invalid`] or [`CatalogError::Database`].
pub async fn load_embedded(db: &mut PgConnection) -> Result<(), CatalogError> {
    service::load(db, &loader::embedded()?).await
}

/// The question bank compiled into the build, checked against the compiled-in catalog: every enabled
/// category has a set of 3 to 8 questions in both languages (LIC-040, TD-LIC-24).
///
/// # Errors
///
/// [`CatalogError::Invalid`].
pub fn question_bank() -> Result<QuestionBank, CatalogError> {
    crate::question_bank::embedded_bank(&loader::embedded()?)
}

/// Parses question-bank text against a parsed catalog, refusing a malformed set (LIC-040).
///
/// # Errors
///
/// [`CatalogError::Invalid`].
pub fn parse_question_bank(text: &str, catalog: &Catalog) -> Result<QuestionBank, CatalogError> {
    crate::question_bank::parse_bank(text, catalog)
}

/// Stores an already parsed catalog (LIC-001); see [`load_embedded`].
///
/// # Errors
///
/// [`CatalogError::Database`].
pub async fn load(db: &mut PgConnection, catalog: &Catalog) -> Result<(), CatalogError> {
    service::load(db, catalog).await
}

/// The stored category with this code, or none (LIC-001). A category outside the K2b list is never
/// stored, so it reads as none.
///
/// # Errors
///
/// [`CatalogError::Database`].
pub async fn category(db: &mut PgConnection, code: &str) -> Result<Option<Category>, CatalogError> {
    service::category(db, code).await
}

/// True when the category exists and is switched on; anything else is off (LIC-003, AI-017).
///
/// # Errors
///
/// [`CatalogError::Database`].
pub async fn is_enabled(db: &mut PgConnection, code: &str) -> Result<bool, CatalogError> {
    Ok(service::category(db, code)
        .await?
        .is_some_and(|c| c.enabled))
}

/// The codes of the categories switched on, sorted (LIC-003).
///
/// # Errors
///
/// [`CatalogError::Database`].
pub async fn enabled_categories(db: &mut PgConnection) -> Result<Vec<String>, CatalogError> {
    service::enabled_codes(db).await
}

/// True when `name` is one of the kinds not offered at launch, so intake gives the not-offered
/// reply (LIC-004, TD-LIC-01).
///
/// # Errors
///
/// [`CatalogError::Database`].
pub async fn is_not_offered(db: &mut PgConnection, name: &str) -> Result<bool, CatalogError> {
    service::is_not_offered(db, name).await
}

/// Switches a category on or off and logs the change in the audit log in the same transaction
/// (LIC-002). Returns false, writing nothing, when nothing changed.
///
/// # Errors
///
/// [`CatalogError::OwnerOnly`] unless `actor` is the owner, [`CatalogError::UnknownCategory`],
/// [`CatalogError::NotOffered`] or [`CatalogError::LicenseUnresolved`] when switching on, or
/// [`CatalogError::Database`].
pub async fn set_category_enabled(
    db: &mut PgConnection,
    code: &str,
    on: bool,
    actor: Actor,
) -> Result<bool, CatalogError> {
    service::set_enabled(db, code, on, actor).await
}
