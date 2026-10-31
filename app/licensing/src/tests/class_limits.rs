//! The commercial-only stop at sign-up and the R-61 and R-62 project limits (TD-LIC-20; LIC-044,
//! LIC-045, D-392).

use crate::api::{ClassMap, ProjectLimitRefusal, QuotePrice, STRUCTURAL_JOB_TYPES, SignUpOutcome};

fn shipped() -> &'static ClassMap {
    ClassMap::shipped().expect("the shipped license map loads")
}

fn limit(classes: &[&str], price: QuotePrice) -> Result<(), ProjectLimitRefusal> {
    shipped().check_project_limit(classes, "handyman", "drywall patch", price)
}

fn over(limit_cents: u64) -> Result<(), ProjectLimitRefusal> {
    Err(ProjectLimitRefusal::OverProjectLimit { limit_cents })
}

/// TD-LIC-20 (LIC-044): a C-37-only record is commercial-only and not activated; C-37 plus R-11
/// activates with the electrical job types.
#[test]
fn td_lic_20_commercial_only_is_stopped_at_sign_up() {
    for classes in [&["C-37"][..], &["A", "B-1", "B-2"], &["c-11", "A-12"]] {
        assert_eq!(
            shipped().sign_up(classes),
            SignUpOutcome::CommercialOnly,
            "{classes:?}"
        );
    }
    let SignUpOutcome::Activate(job_types) = shipped().sign_up(&["C-37", "R-11"]) else {
        panic!("C-37 plus R-11 activates");
    };
    assert!(job_types.contains(&("electrical".into(), "outlet repair".into())));
    assert!(job_types.iter().all(|(cat, _)| cat == "electrical"));
    let SignUpOutcome::Activate(dual) = shipped().sign_up(&["CR-11"]) else {
        panic!("a dual class activates");
    };
    assert_eq!(dual, job_types);
}

/// LIC-044, LIC-035 (D-984): a record whose classes reach no home job type is not activated.
#[test]
fn unmapped_records_are_not_activated() {
    for classes in [&[][..], &["R-37R"], &["R-99", "C-37"]] {
        assert_eq!(
            shipped().sign_up(classes),
            SignUpOutcome::NothingMapped,
            "{classes:?}"
        );
    }
}

/// TD-LIC-20 (LIC-045): R-62 at a top of $5,000.00 is accepted and $5,000.01 refused; R-61 at
/// $50,000.00 and $50,000.01 likewise; a range is judged by its top.
#[test]
fn td_lic_20_project_limits_on_the_top_of_the_price() {
    assert_eq!(limit(&["R-62"], QuotePrice::Fixed(500_000)), Ok(()));
    assert_eq!(limit(&["R-62"], QuotePrice::Fixed(500_001)), over(500_000));
    let range = QuotePrice::Range {
        low: 100_000,
        high: 500_001,
    };
    assert_eq!(limit(&["R-62"], range), over(500_000));
    assert_eq!(limit(&["R-61"], QuotePrice::Fixed(5_000_000)), Ok(()));
    assert_eq!(
        limit(&["R-61"], QuotePrice::Fixed(5_000_001)),
        over(5_000_000)
    );
    assert_eq!(
        limit(&["R-62", "R-61"], QuotePrice::Fixed(5_000_000)),
        Ok(())
    );
    assert_eq!(limit(&["R-62"], QuotePrice::TripFee), Ok(()));
}

/// LIC-045: the limit binds only when every qualifying class has one; a class that does not
/// cover the job type neither lifts nor sets it.
#[test]
fn limit_applies_only_when_every_qualifying_class_has_one() {
    let big = QuotePrice::Fixed(9_000_000);
    assert_eq!(limit(&["R-62", "B"], big), Ok(()));
    assert_eq!(limit(&["R-62", "R-11"], big), over(500_000));
    assert_eq!(limit(&["R-11"], big), Err(ProjectLimitRefusal::NotCovered));
    assert_eq!(limit(&[], big), Err(ProjectLimitRefusal::NotCovered));
}

/// LIC-045 (D-983): R-62 never receives a structural job type, even where its row grants one.
#[test]
fn r_62_never_receives_structural_work() {
    for jt in STRUCTURAL_JOB_TYPES {
        assert!(!shipped().class_covers("R-62", "handyman", jt), "{jt}");
    }
    let cats = "categories:\n  - {id: handyman, enabled: true, launch_open: true, job_types: [shelves], refused: [{job: wall removal or any permit-type job}]}\n";
    let rows = "commercial_only: []
classes:
  - {class: R-62, project_limit: \"5000.00\", no_structural: true, job_types: {handyman: {all: true, also: [wall removal or any permit-type job]}}}
  - {class: R-61, job_types: {handyman: {all: true, also: [wall removal or any permit-type job]}}}
";
    let m = ClassMap::load(rows, cats).expect("test map loads");
    assert!(m.class_covers("R-62", "handyman", "shelves"));
    assert!(!m.class_covers("R-62", "handyman", "wall removal or any permit-type job"));
    assert!(m.class_covers("R-61", "handyman", "wall removal or any permit-type job"));
}
