//! Routing outcomes right after the ZIP and area check (TD-INT-19, TD-INT-14, TD-LIC-07,
//! TD-LIC-01: INT-016, INT-017, INT-018, INT-022, LIC-017, LIC-018). Pure cases only;
//! the database cases are `tests/integration/tests/not_covered.rs`. Fake text only.

use crate::api::{
    Outcome, Request, Town, TownRefused, Why, asks_property_management, check_towns, move_refused,
    route,
};

const AREA: [&str; 1] = ["lake_havasu_city"];

fn request<'a>(town: &'a str, trade: &'a str, open: bool, words: &'a str) -> Request<'a> {
    Request {
        town,
        area: &AREA,
        trade,
        trade_open: open,
        words,
        move_states: None,
        job_types: &[],
    }
}

/// TD-INT-19 (INT-022, D-382): the towns-list check accepts Arizona towns and refuses Needles, CA
/// and Laughlin, NV with the legal-review line.
#[test]
fn td_int_19_the_towns_list_holds_arizona_towns_only() {
    let good = [
        Town {
            name: "Lake Havasu City",
            state: "AZ",
        },
        Town {
            name: "Kingman",
            state: "az",
        },
    ];
    assert_eq!(check_towns(&good), Ok(()));
    for (name, state) in [("Needles", "CA"), ("Laughlin", "NV")] {
        let err = check_towns(&[Town { name, state }]).expect_err("refused");
        assert!(
            err.to_string()
                .ends_with("needs a new legal review (D-382)")
        );
        assert!(matches!(err, TownRefused::NotArizona { .. }));
    }
}

/// TD-INT-19 (INT-022): a request from a town outside the list closes early, before any other
/// check, even for a trade that is not open.
#[test]
fn td_int_19_a_town_outside_the_list_closes_early() {
    assert_eq!(
        route(&request("needles", "boat_lift", false, "")),
        Outcome::OutOfArea
    );
    assert_eq!(
        route(&request(
            "lake_havasu_city",
            "plumbing_repair",
            true,
            "leak"
        )),
        Outcome::Continue
    );
}

/// TD-INT-14, TD-LIC-01 (INT-016, INT-017): a trade that is not open is not covered.
#[test]
fn td_int_14_a_boat_lift_request_is_not_covered() {
    assert_eq!(
        route(&request(
            "lake_havasu_city",
            "boat_lift",
            false,
            "boat lift stuck"
        )),
        Outcome::NotCovered(Why::TradeNotOpen)
    );
}

/// TD-LIC-07 (LIC-017, D-082): a move that starts or ends outside Arizona is refused.
#[test]
fn td_lic_07_moves_outside_arizona_are_refused() {
    assert!(!move_refused("AZ", "AZ"));
    assert!(move_refused("AZ", "NV"));
    assert!(move_refused("CA", "AZ"));
    let mut m14 = request("lake_havasu_city", "local_moving", true, "move");
    m14.move_states = Some(("AZ", "NV"));
    assert_eq!(route(&m14), Outcome::NotCovered(Why::MoveOutsideArizona));
    m14.move_states = Some(("AZ", "AZ"));
    assert_eq!(route(&m14), Outcome::Continue);
}

/// TD-LIC-07 (LIC-017, M14): the catalog's refused "move leaving Arizona" job type is refused
/// even when no states are known.
#[test]
fn td_lic_07_the_refused_move_job_type_is_refused_without_states() {
    let mut m14 = request(
        "lake_havasu_city",
        "local_moving",
        true,
        "move to Las Vegas",
    );
    m14.job_types = &["move leaving Arizona"];
    assert_eq!(route(&m14), Outcome::NotCovered(Why::MoveOutsideArizona));
    m14.job_types = &["in-town move"];
    assert_eq!(route(&m14), Outcome::Continue);
}

/// TD-LIC-07 (LIC-018): home watch asking for rent collection or leasing is refused,
/// in English and Spanish; an ordinary house check goes on.
#[test]
fn td_lic_07_home_watch_for_rent_or_leasing_is_refused() {
    for words in [
        "check the house and collect rent",
        "need someone to show the home to renters",
        "lease it out",
        "cobrar la renta cada mes",
        "buscar inquilinos",
    ] {
        assert!(asks_property_management(words), "{words}");
    }
    for words in [
        "weekly house check while we are away",
        "clean after the renters check out",
        "turnover cleaning when my tenant moves out",
        "limpieza cuando salga el inquilino",
        "the lease ends friday, clean the unit",
    ] {
        assert!(!asks_property_management(words), "{words}");
    }
    let asked = request("lake_havasu_city", "home_watch", true, "tenant screening");
    assert_eq!(route(&asked), Outcome::NotCovered(Why::PropertyManagement));
    let other = request(
        "lake_havasu_city",
        "plumbing_repair",
        true,
        "my tenant says it leaks",
    );
    assert_eq!(route(&other), Outcome::Continue);
}
