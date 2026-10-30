//! Proves the fixture generator yields fake data only (PRIV-019, D-109, TD-PRIV-02).
//!
//! Every generated name is on the fake list and on the PII scanner's list,
//! every phone is in the 555-0100 to 555-0199 block under a listed area code,
//! every email is at `example.com`, every street is a "Test Way", and the
//! sentinel values are fixed and never generated.

use std::collections::HashSet;

use askhava_fixtures::{
    FAKE_AREA_CODES, FAKE_NAMES, SENTINEL_EMAIL, SENTINEL_NAME, SENTINEL_PHONE, SENTINEL_STREET,
    fake_email, fake_name, fake_person, fake_phone, fake_street, sentinel_person,
};

/// Indexes checked: a long run from zero plus the far end of the range.
fn indexes() -> impl Iterator<Item = u32> {
    (0..2_000).chain([u32::MAX - 1, u32::MAX])
}

/// The line number (`01xx`) of a `+1 <listed area> 555 <line>` phone, if it has that shape.
fn line_number(phone: &str) -> Option<u16> {
    let rest = phone.strip_prefix("+1")?;
    if rest.len() != 10 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let area: u16 = rest[..3].parse().ok()?;
    if !FAKE_AREA_CODES.contains(&area) || &rest[3..6] != "555" {
        return None;
    }
    rest[6..].parse().ok()
}

/// The quoted names in `scripts/pii_scan.py`'s `FAKE_NAMES` set.
fn scanner_fake_names() -> HashSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/pii_scan.py");
    let source = std::fs::read_to_string(path).expect("scanner readable");
    let block = &source[source.find("FAKE_NAMES = {").expect("fake-name list")..];
    let block = &block[..block.find('}').expect("list closed")];
    block
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_generated_name_is_on_the_fake_list_and_the_scanner_list() {
    let scanner = scanner_fake_names();
    for index in indexes() {
        let name = fake_name(index);
        assert!(FAKE_NAMES.contains(&name), "{name}: not on the fake list");
        assert!(scanner.contains(name), "{name}: not on the scanner list");
    }
    assert!(
        scanner.contains(SENTINEL_NAME),
        "sentinel not on the scanner list"
    );
}

#[test]
fn every_phone_is_in_the_fictional_block() {
    for index in indexes() {
        let phone = fake_phone(index);
        let line = line_number(&phone).unwrap_or_else(|| panic!("{phone}: malformed"));
        assert!((100..=199).contains(&line), "{phone}: outside 555-01xx");
        assert_ne!(phone, SENTINEL_PHONE, "sentinel phone generated");
    }
    let one_cycle: HashSet<String> = (0..495).map(fake_phone).collect();
    assert_eq!(one_cycle.len(), 495, "phones repeat within one cycle");
}

#[test]
fn every_email_is_at_example_com() {
    for index in indexes() {
        let email = fake_email(index);
        let (local, domain) = email.split_once('@').expect("an email has an @");
        assert_eq!(domain, "example.com", "{email}: not at example.com");
        assert!(
            !local.is_empty() && !local.contains('@'),
            "{email}: malformed"
        );
        assert_ne!(email, SENTINEL_EMAIL, "sentinel email generated");
    }
}

#[test]
fn every_street_is_a_test_way() {
    for index in indexes() {
        let street = fake_street(index);
        let number = street.strip_suffix(" Test Way").expect("a Test Way");
        assert!(
            number.parse::<u64>().is_ok_and(|n| n >= 1),
            "{street}: no number"
        );
        assert_ne!(street, SENTINEL_STREET, "sentinel street generated");
    }
}

#[test]
fn a_fake_person_is_built_from_the_generators_and_is_repeatable() {
    for index in indexes() {
        let person = fake_person(index);
        assert_eq!(person.name, fake_name(index));
        assert_eq!(person.phone, fake_phone(index));
        assert_eq!(person.email, fake_email(index));
        assert_eq!(person.street, fake_street(index));
        assert_eq!(person, fake_person(index));
    }
}

#[test]
fn the_sentinel_values_are_fixed() {
    let sentinel = sentinel_person();
    assert_eq!(sentinel.name, "Zelda Canary");
    assert_eq!(sentinel.phone, "+19285550199");
    assert_eq!(sentinel.email, "canary@example.com");
    assert_eq!(sentinel.street, "1 Canary Way");
    assert_eq!(line_number(SENTINEL_PHONE), Some(199));
    assert!(
        !FAKE_NAMES.contains(&SENTINEL_NAME),
        "sentinel name generated"
    );
}
