//! TD-PICK-03 (PICK-007): a quote whose notes, wording or tier names hold phone digits, a link, an
//! email, spelled-out numbers or "text me" / "call me" wording goes back to the business, and the
//! line it reads names no value. Every number and address here is made up (555 numbers,
//! `example.com`).

use askhava_catalog::api::RESTRICTED_WORDS;
use askhava_licensing::api::LicenseKind;

use crate::api::{Bounce, Format, Price, QuoteForm, QuoteRules, parse_quote};

const RULES: QuoteRules = QuoteRules {
    format: Format::Firm,
    construction_type: false,
    license: LicenseKind::None,
    handyman: false,
    certified: false,
};

fn with_notes(notes: &str) -> QuoteForm {
    QuoteForm {
        price: Price::Firm(25_000),
        notes: notes.to_owned(),
        original: String::new(),
        ai_read: false,
        earliest_visit: None,
    }
}

const CONTACT: [&str; 9] = [
    "Call 928-555-0142 for a faster answer.",
    "Reach us at 928 555 0142.",
    "Photos at www.example.com of past work.",
    "See https://example.com/work first.",
    "Email pat@example.com with questions.",
    "My cell is nine two eight five five five.",
    "Just text me and I can come sooner.",
    "Call me before you pick.",
    "Fastest is to message me directly.",
];

/// TD-PICK-03: each contact shape in the notes bounces.
#[test]
fn td_pick_03_contact_in_the_notes_bounces() {
    for notes in CONTACT {
        let got = parse_quote(with_notes(notes), &RULES);
        assert_eq!(got, Err(Bounce::Contact), "{notes}");
    }
}

/// TD-PICK-03: the same check covers the original wording and each tier name.
#[test]
fn td_pick_03_wording_and_tier_names_are_checked() {
    let mut text_quote = with_notes("");
    text_quote.original = "250 for the job, text me at 928-555-0142".to_owned();
    assert_eq!(parse_quote(text_quote, &RULES), Err(Bounce::Contact));
    let mut tiers = with_notes("");
    tiers.price = Price::Tiers(vec![("Basic 928-555-0142".to_owned(), 9_000)]);
    let tier_rules = QuoteRules {
        format: Format::Tiers,
        ..RULES
    };
    assert_eq!(parse_quote(tiers, &tier_rules), Err(Bounce::Contact));
}

/// TD-PICK-03: ordinary job notes pass unchanged, a carrier-restricted phrase is sent back on its
/// own line, and no bounce line repeats what the business wrote.
#[test]
fn td_pick_03_clean_notes_pass_and_lines_hold_no_value() {
    for notes in [
        "Price includes parts and labor. Two year warranty on the motor.",
        "Can start Tuesday morning; about 3 hours of work.",
        "",
    ] {
        assert!(parse_quote(with_notes(notes), &RULES).is_ok(), "{notes}");
    }
    let restricted = format!("We do not sell {}.", RESTRICTED_WORDS[0]);
    assert_eq!(
        parse_quote(with_notes(&restricted), &RULES),
        Err(Bounce::Restricted)
    );
    for bounce in [
        Bounce::Contact,
        Bounce::Restricted,
        Bounce::NeedsHours,
        Bounce::Amounts,
    ] {
        assert!(!bounce.message().contains("555"));
        assert!(!bounce.message().to_lowercase().contains("example"));
    }
}
