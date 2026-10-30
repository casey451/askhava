//! The records `catalog` keeps and reads: the file's shape, the stored category and the errors
//! (LIC-001, LIC-003, AI-017).
//!
//! The file types mirror `docs/catalog/categories.yaml`; fields the catalog does not store (the
//! review note, refused jobs, launch-open flag) are ignored on read. Every value that ends up in a
//! table comes from that reviewed file or from the owner's switch, never from AI output (AI-017).

use std::collections::BTreeMap;

use serde::Deserialize;
use sqlx::types::Uuid;
use thiserror::Error;

/// Why a `catalog` call failed. No variant carries a value or the database's message (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    /// The catalog file did not parse or broke one of its own rules.
    #[error("the catalog file is not valid")]
    Invalid,
    /// No category with that code is stored (a category not in the K2b list is never stored).
    #[error("no such category")]
    UnknownCategory,
    /// The category is one of the kinds not offered at launch (LIC-004); it cannot be switched on.
    #[error("this kind is not offered at launch")]
    NotOffered,
    /// A licensed category with no resolved ROC class stays off (LIC-003, AI-017).
    #[error("the license class is not resolved")]
    LicenseUnresolved,
    /// Only the owner switches a category (LIC-002, SEC-006).
    #[error("only the owner may switch a category")]
    OwnerOnly,
    /// The database refused or dropped the statement.
    #[error("the catalog database statement failed")]
    Database,
}

/// The license rule of a category (LIC-001): none, a ROC class, another state agency or both.
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct LicenseRule {
    /// `none`, `roc`, `state_agency` or `roc_and_state_agency`.
    pub kind: String,
    /// The ROC classes that may receive the card; empty means unresolved for a ROC kind.
    #[serde(default)]
    pub classes: Vec<String>,
    /// The other state agency whose record is checked.
    pub agency: Option<String>,
}

/// One category as `docs/catalog/categories.yaml` lists it (LIC-001).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CategoryDef {
    /// The code name, such as `plumbing_repair`.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The trade group.
    pub group: String,
    /// Whether the K2b review confirmed it (LIC-003); false stays off.
    pub enabled: bool,
    /// The license rule.
    pub license: LicenseRule,
    /// Whether handymen may also receive cards (LIC-019 to LIC-025).
    pub handyman_allowed: bool,
    /// Whether the $1,000 handyman limit applies (LIC-020).
    pub construction_type: bool,
    /// Whether the fee basis is 4 months of the monthly price (REC-002).
    pub recurring: bool,
    /// The set introduction fee by job size for a trip-fee quote, in dollars (FEE-004).
    pub trip_fee_tier: i32,
    /// Whether the category takes after-hours emergency requests.
    pub after_hours_emergency: bool,
    /// The job types.
    pub job_types: Vec<String>,
    /// In a recurring category, the job types that are one-time jobs.
    #[serde(default)]
    pub one_time_job_types: Vec<String>,
    /// Job types whose pick charges no introduction fee (FEE-045).
    #[serde(default)]
    pub fee_free_job_types: Vec<String>,
    /// Each job type's kind, where the category has them (LIC-041).
    #[serde(default)]
    pub job_type_kinds: BTreeMap<String, String>,
    /// The category's quote format.
    pub quote_format: String,
    /// Job types whose quotes use another format.
    #[serde(default)]
    pub job_type_formats: BTreeMap<String, String>,
    /// The words that match a request to this category.
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// The whole file: the categories and the kinds not offered at launch (LIC-004).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Catalog {
    /// The kinds not offered at launch; none of them can be switched on (LIC-004).
    pub not_offered_at_launch: Vec<String>,
    /// The categories of the K2b list.
    pub categories: Vec<CategoryDef>,
}

/// A stored job type with its flags (LIC-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobType {
    /// The job type name.
    pub name: String,
    /// True for a one-time job in a recurring category.
    pub one_time: bool,
    /// True when the pick charges no introduction fee (FEE-045).
    pub fee_free: bool,
    /// `repair`, `replace` or `install`, where the category sets one.
    pub kind: Option<String>,
    /// The quote format when it differs from the category's.
    pub quote_format: Option<String>,
}

/// A stored category with its license rule and fee rule (LIC-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    /// The record ID, the subject of the switch's audit row.
    pub id: Uuid,
    /// The code name.
    pub code: String,
    /// The display name.
    pub name: String,
    /// Whether the category is switched on (LIC-002, LIC-003).
    pub enabled: bool,
    /// Whether the fee basis is 4 months of the monthly price.
    pub recurring: bool,
    /// Whether the category takes after-hours emergency requests.
    pub after_hours_emergency: bool,
    /// The category's quote format.
    pub quote_format: String,
    /// The words that match a request to this category.
    pub keywords: Vec<String>,
    /// The license kind: `none`, `roc`, `state_agency` or `roc_and_state_agency`.
    pub license_kind: String,
    /// The ROC classes; empty means unresolved for a ROC kind.
    pub license_classes: Vec<String>,
    /// The other state agency, for the agency kinds.
    pub agency: Option<String>,
    /// Whether handymen may also receive cards.
    pub handyman_allowed: bool,
    /// Whether the $1,000 handyman limit applies.
    pub construction_type: bool,
    /// The set introduction fee by job size for a trip-fee quote, in dollars.
    pub trip_fee_tier: i32,
    /// The job types, sorted by name.
    pub job_types: Vec<JobType>,
}
