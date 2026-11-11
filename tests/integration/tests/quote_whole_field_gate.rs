//! A quote's contact check reads the whole field as well as each line, and a per-visit price
//! carries its visit schedule (T342; TD-PICK-58: PICK-007, REC-010; D-2334, D-2333, D-3019,
//! D-3020, D-3021, D-3022). Fake numbers, made-up domains and made-up prices only.
//!
//! The card gate reads line by line, so "555\n123\n4567" passed a quote's check; `text_bounce`
//! now also gates the field with every whitespace run folded to one space. Ordinary measures and
//! short numbers on separate lines still pass, and a card keeps its lines. A per-visit price with
//! no visit schedule bounces at the form's submission; one with a schedule, or "Other" (a monthly
//! price), is accepted.

use askhava_licensing::api::LicenseKind;
use askhava_quotes::api::{
    Bounce, Format, OTHER_SCHEDULE, Period, Price, PriceFields, PriceProblem, QuoteForm,
    QuoteRules, VisitSchedule, final_gate, parse_price, parse_quote,
};

/// Where a business's words go on a quote: the notes, the original wording or a tier name.
#[derive(Debug, Clone, Copy)]
enum Place {
    Notes,
    Original,
    Tier,
}

const PLACES: [Place; 3] = [Place::Notes, Place::Original, Place::Tier];

fn rules(format: Format) -> QuoteRules {
    QuoteRules {
        format,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    }
}

/// Checks a quote holding `text` at `place`: a firm price for notes and wording, a tiers price
/// whose only package is named `text` for a tier name.
fn check(text: &str, place: Place) -> Result<(), Bounce> {
    let (price, format, notes, original) = match place {
        Place::Notes => (Price::Firm(45_000), Format::Firm, text, ""),
        Place::Original => (Price::Firm(45_000), Format::Firm, "", text),
        Place::Tier => (
            Price::Tiers(vec![(text.to_owned(), 45_000)]),
            Format::Tiers,
            "",
            "",
        ),
    };
    let form = QuoteForm {
        price,
        notes: notes.to_owned(),
        original: original.to_owned(),
        ai_read: false,
        earliest_visit: None,
    };
    parse_quote(form, &rules(format)).map(|_| ())
}

/// TD-PICK-58 (PICK-007, D-2334): a phone number split by a line feed, a carriage return and
/// line feed, a tab or a no-break space bounces in notes, original wording and a tier name.
#[test]
fn td_pick_58_split_phone_number_bounces_everywhere() {
    for sep in ["\n", "\r\n", "\t", "\u{a0}"] {
        let text = format!("555{sep}123{sep}4567");
        for place in PLACES {
            assert_eq!(
                check(&text, place),
                Err(Bounce::Contact),
                "{place:?} {sep:?}"
            );
        }
    }
    let mixed = "Thanks\n555 \r\n 123\t\u{a0}4567";
    for place in PLACES {
        assert_eq!(check(mixed, place), Err(Bounce::Contact), "{place:?}");
    }
}

/// TD-PICK-58 (PICK-007, D-2334): an email or a domain split across lines bounces in every
/// place.
#[test]
fn td_pick_58_split_email_or_domain_bounces() {
    for text in [
        "pat@\nexample.com",
        "pat\n@example.com",
        "Visit\nexample-pools\n.com",
        "Visit example-pools\r\n.com",
        "pools at example\ndot com",
        "pools\nat example\tdot com",
        "examplepools.\ncom",
        "examplepools\n.\ncom",
        "examplepools.c\nom",
        "Visit\nexamplepools\n.c\nom",
        "Visit examplepools.\r\nnet now",
    ] {
        for place in PLACES {
            assert_eq!(
                check(text, place),
                Err(Bounce::Contact),
                "{place:?} {text:?}"
            );
        }
    }
}

/// TD-PICK-58 (PICK-007): ordinary measures, counts and short numbers on separate lines pass
/// in every place.
#[test]
fn td_pick_58_measures_and_short_numbers_still_pass() {
    for text in [
        "3 x 4-foot boards",
        "2 coats\n3 days",
        "2\n3\n10",
        "Basic\tplan",
        "Two visits\r\nParts included",
        "Pump replaced.\nParts included.\n...\nDone",
        "Pump replaced.\nParts included",
        "Prices:\n$450\n$300\n$120",
        "Sizes\n24\n36\n48\n60",
        "Sizes\r\n24\r\n36\r\n48\r\n60",
    ] {
        for place in PLACES {
            assert_eq!(check(text, place), Ok(()), "{place:?} {text:?}");
        }
    }
}

/// TD-PICK-58 (PICK-007, D-3022): a list of numbers keeps its line breaks only while no
/// stretch of it is phone-shaped (7, 10 or 11 digits, number words counting one, or 3 number
/// words), so a phone number padded with other numbers or words still bounces, and a list whose
/// neighbours add up to 7 digits bounces too: privacy wins over the list.
#[test]
fn td_pick_58_phone_shaped_number_lists_bounce() {
    for text in [
        "Call 55\n51\n23\n4567",
        "Sizes\n24\n555\n123\n4567\n99",
        "555\n123\n45\nsix\nseven",
        "five\nfive\nfive",
        "Prices:\n$1200\n$450",
    ] {
        for place in PLACES {
            assert_eq!(
                check(text, place),
                Err(Bounce::Contact),
                "{place:?} {text:?}"
            );
        }
    }
}

/// TD-PICK-58 (CARD-008, D-2334): the card gate is unchanged, so a card keeps its lines.
#[test]
fn td_pick_58_card_keeps_its_lines() {
    let card = "Pool pump hums\nNeeds a new seal\n2 coats\n3 days";
    assert_eq!(final_gate(card), card);
}

fn per_visit(schedule: &str) -> PriceFields {
    PriceFields {
        amount: "160.00".to_owned(),
        per: "visit".to_owned(),
        schedule: schedule.to_owned(),
        ..PriceFields::default()
    }
}

fn submit(fields: &PriceFields) -> Result<(Price, Option<VisitSchedule>), PriceProblem> {
    let (price, schedule) = parse_price(Format::Monthly, fields)?;
    let form = QuoteForm {
        price: price.clone(),
        notes: "Chemicals included.".to_owned(),
        original: String::new(),
        ai_read: false,
        earliest_visit: None,
    };
    assert!(parse_quote(form, &rules(Format::Monthly)).is_ok());
    Ok((price, schedule))
}

/// TD-PICK-58 (REC-010, D-2333, D-3020): a per-visit quote with no visit schedule bounces at
/// submission with its reason; one with a schedule keeps it; "Other" is a monthly price.
#[test]
fn td_pick_58_per_visit_price_needs_its_schedule() {
    for missing in ["", "twice_a_week"] {
        assert_eq!(submit(&per_visit(missing)), Err(PriceProblem::Schedule));
    }
    assert!(!PriceProblem::Schedule.message().is_empty());
    let visit = |setup| Price::Monthly {
        cents: 16_000,
        period: Period::Visit,
        setup,
    };
    for schedule in VisitSchedule::ALL {
        let taken = submit(&per_visit(schedule.code()));
        assert_eq!(taken, Ok((visit(None), Some(schedule))));
    }
    let other = submit(&per_visit(OTHER_SCHEDULE)).expect("other");
    let monthly = Price::Monthly {
        cents: 16_000,
        period: Period::Month,
        setup: None,
    };
    assert_eq!(other, (monthly, None));
}
