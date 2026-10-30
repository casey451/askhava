//! The catalog file parses and obeys its rules (TD-LIC-01; LIC-001, LIC-003, LIC-004, LIC-005,
//! AI-017). Fake data only.

use crate::loader::{embedded, normalize, parse};

fn def(id: &str, name: &str, enabled: bool) -> String {
    format!(
        "not_offered_at_launch: [mobile vets]\ncategories:\n  - id: {id}\n    name: {name}\n    \
         group: g\n    enabled: {enabled}\n    license: {{kind: none}}\n    handyman_allowed: false\n    \
         construction_type: false\n    recurring: false\n    trip_fee_tier: 25\n    \
         after_hours_emergency: false\n    job_types: [a]\n    quote_format: range\n"
    )
}

/// TD-LIC-01, LIC-001: every category in the real file loads with its job types and fee tier.
#[test]
fn the_embedded_file_loads_every_category() {
    let catalog = embedded().expect("the real file parses");
    assert_eq!(catalog.categories.len(), 39);
    assert!(catalog.categories.iter().all(|c| !c.job_types.is_empty()));
    assert_eq!(catalog.not_offered_at_launch.len(), 8);
}

/// TD-LIC-01, LIC-005: garage doors and junk removal are off, estate cleanouts and donation pickup on.
#[test]
fn garage_doors_and_junk_removal_are_off_and_cleanouts_are_on() {
    let catalog = embedded().expect("parses");
    let on = |id: &str| {
        catalog
            .categories
            .iter()
            .find(|c| c.id == id)
            .expect("listed")
            .enabled
    };
    assert!(!on("garage_doors") && !on("junk_removal"));
    assert!(on("estate_cleanout") && on("donation_pickup"));
}

/// TD-LIC-01, LIC-003: an unconfirmed category in the file stays off.
#[test]
fn an_unconfirmed_category_stays_off() {
    let catalog = parse(&def("window_tint", "Window tint", false)).expect("parses");
    assert!(!catalog.categories[0].enabled);
}

/// TD-LIC-01, LIC-004: a category whose code or name is a not-offered kind is forced off.
#[test]
fn a_not_offered_kind_is_forced_off_even_when_the_file_says_on() {
    for (id, name) in [("mobile_vets", "Pets"), ("pets", "Mobile Vets")] {
        let catalog = parse(&def(id, name, true)).expect("parses");
        assert!(!catalog.categories[0].enabled, "{id} {name}");
    }
    assert_eq!(normalize(" Mobile_Vets "), "mobile vets");
}

/// AI-017, LIC-001: a license kind, quote format or fee tier the review never set is refused.
#[test]
fn an_unknown_license_kind_or_fee_tier_is_refused() {
    let good = def("pets", "Pets", true);
    assert!(parse(&good).is_ok());
    for bad in [
        good.replace("kind: none", "kind: guess"),
        good.replace("trip_fee_tier: 25", "trip_fee_tier: 99"),
        good.replace("quote_format: range", "quote_format: free"),
        good.replace("job_types: [a]", "job_types: []"),
        good.replace(
            "    job_types: [a]\n",
            "    job_types: [a]\n    one_time_job_types: [zz]\n",
        ),
        format!(
            "{good}{}",
            good.split_once("categories:\n").expect("has list").1
        ),
        "not a catalog".to_owned(),
    ] {
        assert!(parse(&bad).is_err(), "{bad}");
    }
}
