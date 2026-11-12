//! The markets file and the ZCTA centroid table (TD-INT-37; INT-048, INT-022, D-1518). The
//! Phoenix metro market is a fixture here only, never in the reviewed file. The Arizona bounds
//! below are the test's check of TD-INT-37's "coordinates inside Arizona". Fake data only.

use crate::markets::{
    Centroid, MarketRefused, centroids, embedded, miles_between, parse_centroids, parse_markets,
};

/// The Phoenix metro fixture's three towns, ZCTAs from the Census ZCTA-to-place file.
const PHOENIX_TOWNS: &str = "
      - {code: phoenix, name: Phoenix, zips: ['85003', '85004', '85006']}
      - {code: scottsdale, name: Scottsdale, zips: ['85250', '85257']}
      - {code: tempe, name: Tempe, zips: ['85282', '85284']}";

fn market(id: &str, state: &str, zone: &str, towns: &str) -> String {
    format!(
        "  - id: {id}\n    name: Fixture {id}\n    state: {state}\n    time_zone: {zone}\n    \
         identity_sources: [city]\n    towns:{towns}\n"
    )
}

fn file(markets: &[String]) -> String {
    format!("markets:\n{}", markets.concat())
}

fn phoenix() -> String {
    market("phoenix-metro", "AZ", "America/Phoenix", PHOENIX_TOWNS)
}

/// TD-INT-37: the reviewed file holds one market, Lake Havasu City, in AZ on `America/Phoenix`.
#[test]
fn td_int_37_the_reviewed_file_holds_one_market() {
    let markets = embedded().expect("the reviewed file parses");
    let ids: Vec<&str> = markets.markets().iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["lake-havasu-city"]);
    let m = &markets.markets()[0];
    assert_eq!(
        (m.state.as_str(), m.time_zone.as_str()),
        ("AZ", "America/Phoenix")
    );
    assert_eq!(m.identity_sources, ["city"]);
    assert!(m.decision.is_none());
}

/// TD-INT-37: each listed ZIP gives its town; a reviewed extra ZIP gives the town and no
/// location; an unknown ZIP gives neither.
#[test]
fn td_int_37_town_for_zip_and_extra_zips() {
    let markets = embedded().expect("parses");
    for t in markets.markets().iter().flat_map(|m| &m.towns) {
        for zip in &t.zips {
            let town = markets.town_for_zip(zip).expect("listed ZIP has a town");
            assert_eq!(town.code, t.code);
            assert!(
                centroids().expect("table").contains_key(zip),
                "{zip} has a centroid"
            );
        }
        for zip in &t.extra_zips {
            assert_eq!(
                markets.town_for_zip(zip).map(|x| x.code),
                Some(t.code.clone())
            );
            assert!(
                !centroids().expect("table").contains_key(zip),
                "{zip} has no centroid"
            );
        }
    }
    assert_eq!(markets.town_for_zip("99999"), None);
    assert!(!centroids().expect("table").contains_key("99999"));
}

/// TD-INT-37: the Phoenix metro fixture maps each of its towns to its market.
#[test]
fn td_int_37_phoenix_fixture_maps_its_towns() {
    let markets = parse_markets(&file(&[phoenix()])).expect("fixture parses");
    for (zip, code) in [
        ("85004", "phoenix"),
        ("85257", "scottsdale"),
        ("85284", "tempe"),
    ] {
        let town = markets.town_for_zip(zip).expect("town");
        assert_eq!(
            (town.code.as_str(), town.market.as_str()),
            (code, "phoenix-metro")
        );
        assert!(centroids().expect("table").contains_key(zip));
    }
    assert_eq!(markets.towns().len(), 3);
}

/// TD-INT-37: the loader's refusals, each for its own reason.
#[test]
fn td_int_37_the_loader_refuses_bad_markets() {
    let one_town = "\n      - {code: kingman_fixture, name: Kingman, zips: ['86401']}";
    let refused = |markets: &[String]| parse_markets(&file(markets)).expect_err("refused");
    let twice = market("second", "AZ", "America/Phoenix", PHOENIX_TOWNS);
    assert_eq!(
        refused(&[phoenix(), twice]),
        MarketRefused::TownInTwoMarkets
    );
    let dup = market("phoenix-metro", "AZ", "America/Phoenix", one_town);
    assert_eq!(refused(&[phoenix(), dup]), MarketRefused::DuplicateId);
    let zone = market("m", "AZ", "America/Nowhere_Fixture", one_town);
    assert_eq!(refused(&[zone]), MarketRefused::UnknownTimeZone);
    assert_eq!(
        refused(&[market("m", "AZ", "America/Phoenix", " []")]),
        MarketRefused::NoTowns
    );
    let nevada = market("m", "NV", "America/Los_Angeles", one_town);
    assert_eq!(
        refused(std::slice::from_ref(&nevada)),
        MarketRefused::StateNeedsDecision
    );
    let reviewed = nevada.replace("    towns:", "    decision: D-9999\n    towns:");
    assert!(parse_markets(&file(&[reviewed])).is_ok());
    let shared = "\n      - {code: a, name: A, zips: ['85281']}\n      - {code: b, name: B, zips: ['85281']}";
    let shared = market("m", "AZ", "America/Phoenix", shared);
    assert_eq!(refused(&[shared]), MarketRefused::ZipInTwoTowns);
    let short = market(
        "m",
        "AZ",
        "America/Phoenix",
        "\n      - {code: a, name: A, zips: ['8528']}",
    );
    assert_eq!(refused(&[short]), MarketRefused::Malformed);
}

/// TD-INT-37, INT-048: a state other than AZ needs a decision ID of `D-` and digits only.
#[test]
fn td_int_37_a_decision_id_is_d_and_digits() {
    let nevada = market(
        "m",
        "NV",
        "America/Los_Angeles",
        "\n      - {code: kingman_fixture, name: Kingman, zips: ['86401']}",
    );
    for bad in ["'D-'", "'D-x'", "'D-15x'", "'d-1515'", "'1515'"] {
        let with = nevada.replace("    towns:", &format!("    decision: {bad}\n    towns:"));
        assert_eq!(
            parse_markets(&file(&[with])).expect_err(bad),
            MarketRefused::StateNeedsDecision
        );
    }
    let good = nevada.replace("    towns:", "    decision: 'D-1515'\n    towns:");
    assert!(parse_markets(&file(&[good])).is_ok());
}

/// TD-INT-37: every centroid row has a 5-digit code and a point inside Arizona; the header names
/// the Census source and vintage; a malformed row is refused.
#[test]
fn td_int_37_the_centroid_table_is_census_arizona() {
    let text = include_str!("../../data/zcta_centroids_az.csv");
    let header: String = text.lines().take_while(|l| l.starts_with('#')).collect();
    assert!(header.contains("US Census Bureau") && header.contains("Gazetteer"));
    assert!(header.contains("https://www2.census.gov/") && header.contains("Vintage: 2025"));
    let table = centroids().expect("table parses");
    assert!(table.len() > 400);
    for (zip, c) in table {
        assert!(zip.len() == 5 && zip.bytes().all(|b| b.is_ascii_digit()));
        assert!((31.33..=37.0).contains(&c.lat), "{zip} latitude");
        assert!((-114.82..=-109.045).contains(&c.lon), "{zip} longitude");
    }
    for bad in [
        "8640,34.1,-114.1",
        "86403,north,-114.1",
        "86403,34.1",
        "1,2,3,4",
    ] {
        assert_eq!(parse_centroids(bad), Err(MarketRefused::Malformed));
    }
    assert!(parse_centroids("86403,34.1,-114.1\n86403,34.1,-114.1").is_err());
}

/// TD-INT-37: `miles_between` is symmetric and zero for one point.
#[test]
fn td_int_37_miles_between_is_symmetric() {
    let table = centroids().expect("table");
    let (a, b) = (table["85004"], table["86403"]);
    assert!(miles_between(a, a).abs() < 1e-9);
    assert!((miles_between(a, b) - miles_between(b, a)).abs() < 1e-9);
    let north = Centroid { lat: 1.0, lon: 0.0 };
    let one_degree = miles_between(Centroid { lat: 0.0, lon: 0.0 }, north);
    assert!(
        (one_degree - 69.09).abs() < 0.01,
        "one degree of latitude is about 69 miles"
    );
}
