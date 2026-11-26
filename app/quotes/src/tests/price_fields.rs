//! TD-A11Y-05 (A11Y-030; D-4326): a package fault names the first faulty package's name or price
//! field, no package at all names `tier1_name`, and a range fault names the bad end, each with
//! today's words. Package names and amounts are made up.

use crate::api::{Format, PackagePart, PriceFields, PriceProblem, RangeEnd, parse_price};

const PACKAGE_WORDS: &str = "Give each package a name and a price in dollars.";
const RANGE_WORDS: &str = "Enter a low and a high price in dollars.";

/// The packages form with `tiers` as typed.
fn packages(tiers: [(&str, &str); 3]) -> PriceFields {
    PriceFields {
        tiers: tiers.map(|(n, p)| (n.to_owned(), p.to_owned())),
        ..PriceFields::default()
    }
}

/// The field a refused `fields` names in `format`, with the problem's words.
fn named(format: Format, fields: &PriceFields) -> (&'static str, &'static str) {
    let problem = parse_price(format, fields).expect_err("the form is refused");
    (problem.field(), problem.message())
}

#[test]
fn td_a11y_05_a_package_fault_names_the_first_faulty_package_part() {
    let cases = [
        ([("", ""), ("", ""), ("", "")], "tier1_name"),
        ([("Basic", "100"), ("", ""), ("Full", "")], "tier3_price"),
        ([("Basic", "100"), ("", ""), ("Full", "ten")], "tier3_price"),
        ([("Basic", "100"), ("", "250"), ("Full", "")], "tier2_name"),
        ([("Basic", ""), ("", "250"), ("", "")], "tier1_price"),
        ([("", "90"), ("Better", ""), ("", "")], "tier1_name"),
        ([("Basic", "100"), ("Better", "x"), ("", "")], "tier2_price"),
    ];
    for (tiers, field) in cases {
        let got = named(Format::Tiers, &packages(tiers));
        assert_eq!(got, (field, PACKAGE_WORDS), "{field}");
    }
}

#[test]
fn td_a11y_05_a_range_fault_names_the_bad_end() {
    let range = |low: &str, high: &str| PriceFields {
        low: low.to_owned(),
        high: high.to_owned(),
        ..PriceFields::default()
    };
    let cases = [
        (("", "400"), "low"),
        (("abc", "abc"), "low"),
        (("200", ""), "high"),
        (("200", "4OO"), "high"),
    ];
    for ((low, high), field) in cases {
        let got = named(Format::Range, &range(low, high));
        assert_eq!(got, (field, RANGE_WORDS), "{low} to {high}");
    }
}

#[test]
fn td_a11y_05_every_package_and_range_variant_has_its_element() {
    let ends = [(RangeEnd::Low, "low"), (RangeEnd::High, "high")];
    for (end, field) in ends {
        assert_eq!(PriceProblem::Range(end).field(), field);
    }
    for package in 1..=3u8 {
        let name = PriceProblem::Tiers {
            package,
            part: PackagePart::Name,
        };
        let price = PriceProblem::Tiers {
            package,
            part: PackagePart::Price,
        };
        assert_eq!(name.field(), format!("tier{package}_name"));
        assert_eq!(price.field(), format!("tier{package}_price"));
    }
    let stray = PriceProblem::Tiers {
        package: 0,
        part: PackagePart::Name,
    };
    assert_eq!(stray.field(), "tier1_name");
}
