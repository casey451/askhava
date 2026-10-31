//! The agency rules against the written oracle and the catalog files (TD-LIC-06, TD-LIC-17;
//! LIC-012 to LIC-016, LIC-047, D-110): matching examples M09 to M12 from
//! `docs/catalog/matching_examples.yaml`, the license map's `agencies` section, the catalog's
//! pest row and the migration's agency codes all agree with `models::Agency`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::api::{
    Agency, CityListStanding, Eligibility, Holder, Job, LicenseKind, PestRecordCategory,
    eligibility,
};

const EXAMPLES: &str = include_str!("../../../../docs/catalog/matching_examples.yaml");
const LICENSE_MAP: &str = include_str!("../../../../docs/catalog/license_map.yaml");
const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");
const MIGRATION: &str = include_str!("../../../../migrations/0048_licensing_agency_rules.sql");

/// The rules this task decides; an example citing one of them is checked here.
const OWN_RULES: [&str; 4] = ["LIC-012", "LIC-013", "LIC-014", "LIC-016"];

#[derive(Deserialize)]
struct Examples {
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    id: String,
    job: ExampleJob,
    business: ExampleBusiness,
    expected: Expected,
}

#[derive(Deserialize)]
struct ExampleJob {
    category: String,
    candidates: Vec<String>,
}

#[derive(Deserialize)]
struct ExampleBusiness {
    roc: Vec<String>,
    agencies: Vec<String>,
    handyman: bool,
    city_list: bool,
}

#[derive(Deserialize)]
struct Expected {
    outcome: String,
    rules: Vec<String>,
    map_covers: Option<bool>,
    condition: Option<String>,
}

#[derive(Deserialize)]
struct LicenseMap {
    agencies: Vec<MapAgency>,
}

#[derive(Deserialize)]
struct MapAgency {
    license: String,
    #[serde(default)]
    job_types: BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    record_category_needed: BTreeMap<String, String>,
    #[serde(default)]
    no_license_categories: bool,
}

#[derive(Deserialize)]
struct Categories {
    categories: Vec<CategoryRow>,
}

#[derive(Deserialize)]
struct CategoryRow {
    id: String,
    enabled: bool,
    #[serde(default)]
    launch_open: bool,
    #[serde(default)]
    advertised: Option<bool>,
    #[serde(default)]
    licensed_only_decision: Option<String>,
    license: CategoryLicense,
    handyman_allowed: bool,
    job_types: Vec<String>,
}

#[derive(Deserialize)]
struct CategoryLicense {
    kind: String,
}

fn categories() -> Vec<CategoryRow> {
    serde_saphyr::from_str::<Categories>(CATEGORIES)
        .expect("categories parse")
        .categories
}

/// The example's business, as `quotes` would describe it: its ROC classes cover the job when the
/// oracle's `map_covers` says so and it holds any class at all.
fn holder(business: &ExampleBusiness, expected: &Expected) -> Holder {
    let agencies = business
        .agencies
        .iter()
        .map(|name| Agency::from_map_name(name).expect("agency"));
    Holder {
        roc_covers: !business.roc.is_empty() && expected.map_covers == Some(true),
        agencies: agencies.collect(),
        pest: None,
        handyman: business.handyman,
        city_list: if business.city_list {
            CityListStanding::Matched
        } else {
            CityListStanding::Pending
        },
    }
}

/// TD-LIC-06 (LIC-012, LIC-013, LIC-014, LIC-016, D-110): every oracle example citing this task's
/// rules (M09 to M12) comes out as written: a card, a handyman card carrying a condition, or none.
#[test]
fn matching_examples_for_agency_rules_hold() {
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let rows = categories();
    let mut checked = BTreeSet::new();
    for example in examples.examples.iter().filter(|e| {
        e.expected
            .rules
            .iter()
            .any(|r| OWN_RULES.contains(&r.as_str()))
    }) {
        let row = rows
            .iter()
            .find(|c| c.id == example.job.category)
            .expect("category");
        let holder = holder(&example.business, &example.expected);
        for candidate in &example.job.candidates {
            let job = Job {
                category: row.id.clone(),
                job_type: candidate.clone(),
                license: LicenseKind::parse(&row.license.kind).expect("kind"),
                handyman_allowed: row.handyman_allowed,
                listed: row.job_types.contains(candidate),
            };
            let got = eligibility(&job, &holder);
            let card = !matches!(got, Eligibility::NoCard(_));
            assert_eq!(card, example.expected.outcome == "card", "{}", example.id);
            let handyman_card = matches!(got, Eligibility::HandymanCard);
            assert_eq!(
                handyman_card,
                example.expected.condition.is_some(),
                "{}",
                example.id
            );
        }
        checked.insert(example.id.as_str());
    }
    assert_eq!(checked, BTreeSet::from(["M09", "M10", "M11", "M12"]));
}

/// TD-LIC-06 (LIC-012, LIC-013, LIC-015, LIC-016): every agency in the license map is an
/// [`Agency`] opening exactly its one category, the city list stands for the no-license
/// categories, and every agency-licensed catalog category has its agency.
#[test]
fn agency_table_matches_the_license_map_and_catalog() {
    let map: LicenseMap = serde_saphyr::from_str(LICENSE_MAP).expect("license map parses");
    let mut seen = BTreeSet::new();
    for entry in map.agencies.iter().filter(|a| !a.no_license_categories) {
        let agency = Agency::from_map_name(&entry.license).expect("mapped agency");
        let opened: Vec<&str> = entry.job_types.keys().map(String::as_str).collect();
        assert_eq!(opened, [agency.category()], "{}", entry.license);
        seen.insert(agency);
    }
    assert_eq!(seen, BTreeSet::from(Agency::ALL));
    assert_eq!(
        map.agencies
            .iter()
            .filter(|a| a.no_license_categories)
            .count(),
        1
    );
    for row in categories().iter().filter(|c| c.enabled) {
        let kind = LicenseKind::parse(&row.license.kind).expect("every kind parses");
        let agency_kind = matches!(
            kind,
            LicenseKind::StateAgency | LicenseKind::RocAndStateAgency
        );
        assert_eq!(
            agency_kind,
            Agency::for_category(&row.id).is_some(),
            "{}",
            row.id
        );
    }
}

/// TD-LIC-17 (LIC-012, LIC-047): termite and pre-emergent job types need exactly the record
/// categories the map names, and pest control is open at sign-up like any other trade, switched on
/// for licensees only by its decision, and not advertised.
#[test]
fn pest_row_and_record_categories_match_the_files() {
    let map: LicenseMap = serde_saphyr::from_str(LICENSE_MAP).expect("license map parses");
    let pest = map
        .agencies
        .iter()
        .find(|a| a.license == Agency::PestManagement.map_name())
        .expect("pest");
    let needed: BTreeSet<&str> = pest
        .record_category_needed
        .keys()
        .map(String::as_str)
        .collect();
    let rows = categories();
    let row = rows
        .iter()
        .find(|c| c.id == Agency::PestManagement.category())
        .expect("pest row");
    for job_type in &row.job_types {
        let category = PestRecordCategory::needed_for(job_type);
        assert_eq!(
            category.is_some(),
            needed.contains(job_type.as_str()),
            "{job_type}"
        );
    }
    assert_eq!(
        PestRecordCategory::needed_for("termites"),
        Some(PestRecordCategory::WoodDestroying)
    );
    assert_eq!(
        PestRecordCategory::needed_for("pre-emergent weed control"),
        Some(PestRecordCategory::WeedControl)
    );
    assert!(row.enabled && row.launch_open);
    assert_eq!(row.advertised, Some(false));
    assert_eq!(row.licensed_only_decision.as_deref(), Some("D-402"));
    let advertised = rows.iter().filter(|c| c.advertised == Some(true)).count();
    assert_eq!(advertised, 0);
}

/// TD-LIC-06 (LIC-047): the migration's `agency` check lists exactly the [`Agency`] codes.
#[test]
fn migration_agency_codes_match() {
    let check = MIGRATION.split("agency IN (").nth(1).expect("agency check");
    let list = check.split(')').next().expect("list end");
    let codes: BTreeSet<&str> = list
        .split(',')
        .map(|c| c.trim().trim_matches('\''))
        .collect();
    let expected: BTreeSet<&str> = Agency::ALL.into_iter().map(Agency::code).collect();
    assert_eq!(codes, expected);
}
