//! The license class map against its golden rows, the matching oracle and small test maps
//! (TD-LIC-19, TD-LIC-21; LIC-035, LIC-043, LIC-046, CARD-001, D-392, D-459).

use serde::Deserialize;

use crate::api::{
    ClassMap, ClassMapError, Eligibility, Holder, Job, LicenseKind, NoCardReason, Opening,
    RequestIntake, eligibility, request_intake,
};

const EXAMPLES: &str = include_str!("../../../../docs/catalog/matching_examples.yaml");
const CATEGORIES: &str = include_str!("../../../../docs/catalog/categories.yaml");

/// A two-category test catalog: plumbing with a refused gas row, and a closed pool category.
const TEST_CATS: &str = "
categories:
  - {id: plumbing_repair, enabled: true, launch_open: true, job_types: [leak repair, drain clearing], refused: [{job: gas line work}]}
  - {id: pool_cleaning, enabled: true, launch_open: false, job_types: [weekly pool service]}
  - {id: junk_removal, enabled: false, launch_open: false, job_types: [junk haul-away]}
";

fn shipped() -> &'static ClassMap {
    ClassMap::shipped().expect("the shipped license map loads")
}

fn map(classes: &str) -> Result<ClassMap, ClassMapError> {
    let text = format!("commercial_only: [A, C-]\nclasses:\n{classes}");
    ClassMap::load(&text, TEST_CATS)
}

/// TD-LIC-19 (LIC-043, LIC-035): the golden rows of `docs/catalog/license_map.yaml`.
#[test]
fn td_lic_19_golden_rows() {
    let m = shipped();
    let yes = |class: &str, cat: &str, jt: &str| m.class_covers(class, cat, jt);
    assert!(yes("CR-37", "plumbing_repair", "leak repair"));
    assert!(yes("CR-37", "plumbing_repair", "water heater replacement"));
    assert!(!yes("CR-37", "plumbing_repair", "gas line work"));
    assert!(yes(
        "R-37R Sewers, Drains and Pipe Laying",
        "plumbing_repair",
        "drain clearing"
    ));
    for jt in [
        "leak repair",
        "toilet repair",
        "faucet replacement",
        "gas line work",
    ] {
        assert!(!yes(
            "R-37R Sewers, Drains and Pipe Laying",
            "plumbing_repair",
            jt
        ));
    }
    assert!(yes("CR-79", "hvac_repair", "furnace repair"));
    let ac = "R-39R Air Conditioning and Refrigeration";
    assert!(yes(ac, "hvac_repair", "AC not cooling"));
    assert!(!yes(ac, "hvac_repair", "furnace repair"));
    assert!(!yes("CR-39", "hvac_repair", "furnace repair"));
    assert!(yes(
        "CR-58",
        "hvac_repair",
        "evaporative (swamp) cooler repair"
    ));
    assert!(yes("R-37R Gas Piping", "plumbing_repair", "gas line work"));
    assert!(!yes("R-37R Plumbing", "plumbing_repair", "gas line work"));
    assert!(yes("r-37r  plumbing", "plumbing_repair", "leak repair"));
}

/// TD-LIC-19 (LIC-043): B and KB-2 get the general repair job types and nothing that R4-9-103
/// leaves to other licensed contractors.
#[test]
fn td_lic_19_general_classes_never_get_trade_work() {
    let m = shipped();
    for class in ["B", "KB-2", "KB-1", "B-3"] {
        assert!(
            m.class_covers(class, "handyman", "drywall patch"),
            "{class}"
        );
        assert!(
            m.class_covers(class, "painting", "interior painting"),
            "{class}"
        );
        assert!(
            m.class_covers(class, "roof_repair", "leak repair"),
            "{class}"
        );
        assert!(!m.class_covers(class, "pavers_hardscape", "irrigation install"));
        let trades = [
            "electrical",
            "plumbing_repair",
            "hvac_repair",
            "hvac_replacement",
        ];
        let all = m.job_types(&[class]);
        for (cat, _) in &all {
            assert!(
                !trades.contains(&cat.as_str()) && cat != "pool_repair",
                "{class} {cat}"
            );
        }
    }
    assert!(shipped().class_covers("KA-5", "pool_repair", "pump repair"));
    assert!(!shipped().class_covers("KA-5", "pool_repair", "gas pool heater"));
    assert!(shipped().class_covers("KA-6", "pool_repair", "gas pool heater"));
}

/// TD-LIC-19 (LIC-043): an unclear row is never inherited, even where the included class lists
/// the job type itself.
#[test]
fn td_lic_19_unclear_row_is_never_inherited() {
    let rows = "  - {class: R-1, job_types: {plumbing_repair: {all: true, also: [gas line work]}}}
  - {class: CR-1, includes: [R-1]}
unclear:
  - {class: R-1, category: plumbing_repair, job_type: gas line work}
";
    let m = map(rows).expect("test map loads");
    assert!(m.class_covers("CR-1", "plumbing_repair", "leak repair"));
    assert!(!m.class_covers("CR-1", "plumbing_repair", "gas line work"));
    assert!(!m.class_covers("R-1", "plumbing_repair", "gas line work"));
    assert!(!shipped().class_covers("CR-6", "pool_repair", "gas pool heater"));
}

/// TD-LIC-19 (LIC-035): a subclassification gets only job types whose scope tags it holds; an
/// unmapped or bare subclassification gets nothing.
#[test]
fn td_lic_19_subclassification_scope_and_unmapped() {
    let rows = "  - {class: R-1R Drains, scope: [drain], job_types: {plumbing_repair: {all: true}}}
scope_tags:
  plumbing_repair: {leak repair: [plumbing], drain clearing: [plumbing, drain]}
";
    let m = map(rows).expect("test map loads");
    assert!(m.class_covers("R-1R Drains", "plumbing_repair", "drain clearing"));
    assert!(!m.class_covers("R-1R Drains", "plumbing_repair", "leak repair"));
    for class in ["R-37R", "R-39R", "R-37R Something Else", ""] {
        assert!(shipped().job_types(&[class]).is_empty(), "{class}");
    }
}

/// TD-LIC-19 (CARD-001, LIC-043): a business that ticks a job type its class doesn't cover gets
/// no card, because `roc_covers` comes from the map lookup; the same business gets the card for a
/// job type the map gives it. A map lookup skipped (cover assumed) would fail both asserts.
#[test]
fn td_lic_19_ticked_but_not_covered_gets_no_card() {
    let job = |cat: &str, jt: &str| Job {
        category: cat.to_owned(),
        job_type: jt.to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: true,
        listed: true,
    };
    let classes = ["R-37R Sewers, Drains and Pipe Laying"];
    let decide = |j: &Job| {
        let holder = Holder {
            roc_covers: shipped().covers(&classes, &j.category, &j.job_type),
            ..Holder::default()
        };
        eligibility(j, &holder)
    };
    let leak = job("plumbing_repair", "leak repair");
    assert_eq!(
        decide(&leak),
        Eligibility::NoCard(NoCardReason::RocNotCovered)
    );
    assert_eq!(
        decide(&job("plumbing_repair", "drain clearing")),
        Eligibility::Card
    );
    assert!(!shipped().covers(&["R-11"], "hvac_repair", "AC not cooling"));
    assert!(shipped().covers(&["R-11", "R-39"], "hvac_repair", "AC not cooling"));
}

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
}

#[derive(Deserialize)]
struct Expected {
    map_covers: Option<bool>,
}

/// TD-LIC-19 (LIC-043, LIC-035): every ROC-only matching example's `map_covers` agrees with the
/// loaded map (the oracle written before this code, D-110).
#[test]
fn td_lic_19_matching_examples_agree() {
    let examples: Examples = serde_saphyr::from_str(EXAMPLES).expect("examples parse");
    let mut checked = 0;
    for ex in examples.examples {
        let (Some(want), true) = (ex.expected.map_covers, ex.business.agencies.is_empty()) else {
            continue;
        };
        let roc: Vec<&str> = ex.business.roc.iter().map(String::as_str).collect();
        let got = ex
            .job
            .candidates
            .iter()
            .all(|jt| shipped().covers(&roc, &ex.job.category, jt));
        assert_eq!(got, want, "{}", ex.id);
        checked += 1;
    }
    assert!(checked >= 4, "only {checked} examples checked");
}

/// LIC-043: a malformed or inconsistent map is refused, so nothing is covered by mistake.
#[test]
fn bad_maps_are_refused() {
    let unknown_jt = "  - {class: R-1, job_types: {plumbing_repair: {only: [roof leak]}}}\n";
    let cases = [
        (
            "  - {class: R-1}\n  - {class: r-1}\n",
            ClassMapError::DuplicateClass("r-1".into()),
        ),
        (
            "  - {class: C-37, includes: [R-1]}\n  - {class: R-1}\n",
            ClassMapError::CommercialOnlyMapped("C-37".into()),
        ),
        (
            "  - {class: R-1, job_types: {roofing: {all: true}}}\n",
            ClassMapError::UnknownCategory("R-1".into()),
        ),
        (unknown_jt, ClassMapError::UnknownJobType("R-1".into())),
        (
            "  - {class: CR-1, includes: [R-9]}\n",
            ClassMapError::UnknownInclude("CR-1".into()),
        ),
        (
            "  - {class: X, includes: [Y]}\n  - {class: Y, includes: [X]}\n",
            ClassMapError::IncludeCycle("X".into()),
        ),
        (
            "  - {class: R-1, project_limit: \"5000\"}\n",
            ClassMapError::BadLimit("R-1".into()),
        ),
    ];
    for (rows, want) in cases {
        assert_eq!(map(rows).err(), Some(want), "{rows}");
    }
    for bad in ["5000.0", "5x00.00", ".00", "1.a0", "184467440737095517.00"] {
        let rows = format!("  - {{class: R-1, project_limit: \"{bad}\"}}\n");
        assert_eq!(
            map(&rows).err(),
            Some(ClassMapError::BadLimit("R-1".into())),
            "{bad}"
        );
    }
    assert_eq!(
        ClassMap::load("classes: 3", TEST_CATS).err(),
        Some(ClassMapError::Malformed)
    );
    assert_eq!(
        ClassMap::load("", "x: 1").err(),
        Some(ClassMapError::Malformed)
    );
}

/// TD-LIC-21 (LIC-046, D-459): every enabled shipped category is launch-open and the disabled ones
/// are off.
#[test]
fn td_lic_21_every_enabled_category_is_open_now() {
    #[derive(Deserialize)]
    struct Cats {
        categories: Vec<Cat>,
    }
    #[derive(Deserialize)]
    struct Cat {
        id: String,
        enabled: bool,
    }
    let cats: Cats = serde_saphyr::from_str(CATEGORIES).expect("catalog parses");
    for cat in cats.categories {
        let want = if cat.enabled {
            Opening::OpenNow
        } else {
            Opening::Off
        };
        assert_eq!(shipped().opening(&cat.id), want, "{}", cat.id);
    }
    assert_eq!(shipped().opening("towing"), Opening::Off);
}

/// TD-LIC-21 (LIC-046, OPS-006, INT-017): "Coming later" takes waitlist sign-ups only; open needs
/// 2 active businesses; a disabled launch-open category fails the load.
#[test]
fn td_lic_21_launch_open_flag_and_active_businesses() {
    let m = map("  - {class: R-1}\n").expect("test map loads");
    assert_eq!(m.opening("pool_cleaning"), Opening::ComingLater);
    assert_eq!(m.opening("junk_removal"), Opening::Off);
    let open = m.opening("plumbing_repair");
    assert_eq!(request_intake(open, 1, 2), RequestIntake::TooFewBusinesses);
    assert_eq!(request_intake(open, 2, 2), RequestIntake::Open);
    for active in [0, 5] {
        assert_eq!(
            request_intake(Opening::ComingLater, active, 2),
            RequestIntake::WaitlistOnly
        );
        assert_eq!(
            request_intake(Opening::Off, active, 2),
            RequestIntake::NotRouted
        );
    }
    let bad = "categories:\n  - {id: junk_removal, enabled: false, launch_open: true}\n";
    assert_eq!(
        ClassMap::load("commercial_only: []\nclasses: []\n", bad).err(),
        Some(ClassMapError::LaunchOpenNotEnabled("junk_removal".into()))
    );
}
