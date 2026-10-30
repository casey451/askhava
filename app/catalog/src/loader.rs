//! Reads and checks `docs/catalog/categories.yaml` before anything is stored (LIC-001, LIC-003,
//! LIC-004, AI-017).
//!
//! The file is compiled in, so the running service never depends on a folder at run time. Parsing is
//! pure: a category whose code or name is on the not-offered list is forced off here, and a duplicate
//! code, an unknown license kind, quote format or job-type kind, or a fee tier outside $25 to $75 is
//! refused, so the tables never hold a value the review did not set.

use std::collections::BTreeSet;

use crate::models::{Catalog, CatalogError, CategoryDef, LicenseRule};

/// The reviewed catalog file, compiled in.
const EMBEDDED: &str = include_str!("../../../docs/catalog/categories.yaml");

const LICENSE_KINDS: [&str; 4] = ["none", "roc", "state_agency", "roc_and_state_agency"];
const QUOTE_FORMATS: [&str; 5] = ["range", "monthly", "tiers", "firm", "diagnostic_visit"];
const JOB_KINDS: [&str; 3] = ["repair", "replace", "install"];

/// Lower case with `_` read as a space, so `mobile_vets` and `Mobile vets` compare equal (LIC-004).
#[must_use]
pub fn normalize(name: &str) -> String {
    name.trim().to_lowercase().replace('_', " ")
}

/// The catalog compiled into the build (LIC-001).
///
/// # Errors
///
/// [`CatalogError::Invalid`] if the compiled-in file breaks a rule of [`parse`].
pub fn embedded() -> Result<Catalog, CatalogError> {
    parse(EMBEDDED)
}

/// Parses and checks a catalog file. A category on the not-offered list comes back with `enabled`
/// false whatever the file says (LIC-003, LIC-004); a license class is only ever read from the file
/// (AI-017).
///
/// # Errors
///
/// [`CatalogError::Invalid`] for a file that does not parse or breaks a rule above.
pub fn parse(text: &str) -> Result<Catalog, CatalogError> {
    let mut catalog: Catalog = serde_saphyr::from_str(text).map_err(|_| CatalogError::Invalid)?;
    let blocked: BTreeSet<String> = catalog
        .not_offered_at_launch
        .iter()
        .map(|n| normalize(n))
        .collect();
    let mut seen = BTreeSet::new();
    for def in &mut catalog.categories {
        check(def)?;
        if !seen.insert(def.id.clone()) {
            return Err(CatalogError::Invalid);
        }
        if blocked.contains(&normalize(&def.id)) || blocked.contains(&normalize(&def.name)) {
            def.enabled = false;
        }
    }
    Ok(catalog)
}

/// One category's own rules.
fn check(def: &CategoryDef) -> Result<(), CatalogError> {
    let formats = std::iter::once(&def.quote_format).chain(def.job_type_formats.values());
    let ok = LICENSE_KINDS.contains(&def.license.kind.as_str())
        && (25..=75).contains(&def.trip_fee_tier)
        && formats
            .into_iter()
            .all(|f| QUOTE_FORMATS.contains(&f.as_str()))
        && def
            .job_type_kinds
            .values()
            .all(|k| JOB_KINDS.contains(&k.as_str()))
        && !def.job_types.is_empty()
        && def
            .one_time_job_types
            .iter()
            .chain(&def.fee_free_job_types)
            .chain(def.job_type_kinds.keys())
            .chain(def.job_type_formats.keys())
            .all(|j| def.job_types.contains(j));
    if ok && license_fits(&def.license) {
        Ok(())
    } else {
        Err(CatalogError::Invalid)
    }
}

/// The license rule's own shape, as the table's CHECK constraints hold it: an agency exactly for
/// the agency kinds, classes only for the ROC kinds.
fn license_fits(rule: &LicenseRule) -> bool {
    let agency_kind = matches!(rule.kind.as_str(), "state_agency" | "roc_and_state_agency");
    let roc_kind = matches!(rule.kind.as_str(), "roc" | "roc_and_state_agency");
    agency_kind == rule.agency.is_some() && (roc_kind || rule.classes.is_empty())
}
