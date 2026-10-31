//! TD-PICK-01 (pure window), TD-PICK-02 (the fields a checked quote keeps) and the quote-form
//! cases of TD-FEE-27 (PICK-001, PICK-002, PICK-053, FEE-043, FEE-044, LIC-019). Fees are
//! `payments`' (D-491): here only the format, the range an hourly quote stands for and what the
//! form refuses. All amounts and words are made up.

use askhava_licensing::api::{LicenseKind, QuoteRefusal};
use jiff::Timestamp;
use proptest::prelude::*;

use crate::api::{
    Bounce, DayPart, Format, Period, Price, QuoteForm, QuoteRules, parse_quote, window_open,
};

fn rules(format: Format) -> QuoteRules {
    QuoteRules {
        format,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    }
}

fn form(price: Price) -> QuoteForm {
    QuoteForm {
        price,
        notes: "Parts and labor included.".to_owned(),
        original: "Parts and labor included.".to_owned(),
        ai_read: false,
        earliest_visit: Some(("2026-11-03".parse().expect("date"), DayPart::Morning)),
    }
}

fn at(text: &str) -> Timestamp {
    text.parse().expect("time")
}

/// TD-PICK-01: a card held overnight and sent at 08:00 takes a quote at 07:59:59 the next day,
/// not at 08:00:00 or 08:00:01; a weekend counts like any day (D-101).
#[test]
fn td_pick_01_the_window_is_24_hours_from_the_send() {
    let sent = at("2026-11-07T08:00:00-07:00");
    assert!(window_open(sent, at("2026-11-08T07:59:59-07:00")));
    assert!(!window_open(sent, at("2026-11-08T08:00:00-07:00")));
    assert!(!window_open(sent, at("2026-11-08T08:00:01-07:00")));
    assert!(window_open(sent, sent));
}

/// TD-PICK-02: each format keeps its amounts, period, setup charge, tiers, tick, notes, wording,
/// AI-read flag and earliest visit; a quote in another format than the job's is refused.
#[test]
fn td_pick_02_fields_are_kept_in_the_jobs_format() {
    let range = Price::Range {
        low: 120_000,
        high: 190_000,
        on_site: true,
    };
    let quote = parse_quote(form(range.clone()), &rules(Format::Range)).expect("range");
    assert_eq!(
        (quote.format(), quote.range()),
        (Format::Range, Some((120_000, 190_000)))
    );
    assert_eq!(quote.form.notes, "Parts and labor included.");
    assert_eq!(quote.price, range);
    let weekly = Price::Monthly {
        cents: 4_000,
        period: Period::Week,
        setup: Some(15_000),
    };
    let quote = parse_quote(form(weekly.clone()), &rules(Format::Monthly)).expect("weekly");
    assert_eq!((quote.format(), quote.price), (Format::Monthly, weekly));
    let tiers = Price::Tiers(vec![
        ("Basic".to_owned(), 15_000),
        ("Full".to_owned(), 25_000),
    ]);
    let quote = parse_quote(form(tiers), &rules(Format::Tiers)).expect("tiers");
    assert_eq!(quote.format(), Format::Tiers);
}

/// TD-PICK-02: more than 3 tiers, a zero amount, a range with no spread or a price in another
/// format than the job's is refused; the AI-read flag is kept.
#[test]
fn td_pick_02_refuses_what_the_format_cannot_hold() {
    let four = Price::Tiers((1..=4).map(|n| (format!("Package {n}"), 10_000)).collect());
    assert_eq!(
        parse_quote(form(four), &rules(Format::Tiers)),
        Err(Bounce::Amounts)
    );
    let mut read = form(Price::Firm(30_000));
    read.ai_read = true;
    assert!(
        parse_quote(read, &rules(Format::Firm))
            .expect("firm")
            .form
            .ai_read
    );
    assert_eq!(
        parse_quote(form(Price::Firm(30_000)), &rules(Format::Range)),
        Err(Bounce::WrongFormat)
    );
    let flat = Price::Range {
        low: 50_000,
        high: 50_000,
        on_site: false,
    };
    assert_eq!(
        parse_quote(form(flat), &rules(Format::Range)),
        Err(Bounce::Amounts)
    );
    assert_eq!(
        parse_quote(form(Price::Firm(0)), &rules(Format::Firm)),
        Err(Bounce::Amounts)
    );
}

/// TD-FEE-27 (W35, W36): $95 an hour for 3 to 5 hours is the range $285 to $475; with no hours it
/// is a site-visit quote in a `diagnostic_visit` job and goes back for hours in a `range` job.
#[test]
fn td_fee_27_hourly_quotes_need_hours() {
    let hourly = Price::Hourly {
        rate: 9_500,
        hours: Some((3, 5)),
    };
    let quote = parse_quote(form(hourly.clone()), &rules(Format::Range)).expect("hourly");
    assert_eq!(
        (quote.format(), quote.range()),
        (Format::Range, Some((28_500, 47_500)))
    );
    let visit = parse_quote(form(hourly), &rules(Format::DiagnosticVisit)).expect("visit job");
    assert_eq!(visit.format(), Format::Range);
    let bare = Price::Hourly {
        rate: 9_500,
        hours: None,
    };
    let quote = parse_quote(form(bare.clone()), &rules(Format::DiagnosticVisit)).expect("visit");
    assert_eq!(
        (quote.format(), quote.range()),
        (Format::DiagnosticVisit, None)
    );
    assert_eq!(
        parse_quote(form(bare), &rules(Format::Range)),
        Err(Bounce::NeedsHours)
    );
    let one = Price::Hourly {
        rate: 9_500,
        hours: Some((4, 4)),
    };
    let quote = parse_quote(form(one), &rules(Format::Range)).expect("one estimate");
    assert_eq!(quote.range(), Some((38_000, 38_000)));
    let backwards = Price::Hourly {
        rate: 9_500,
        hours: Some((5, 3)),
    };
    assert_eq!(
        parse_quote(form(backwards), &rules(Format::Range)),
        Err(Bounce::Amounts)
    );
}

/// TD-FEE-27: a visit-fee quote in `hvac_replacement` (a `range` category) is refused, and a
/// handyman hourly quote whose top reaches $1,000 is refused; $999.99 at the top passes.
#[test]
fn td_fee_27_visit_fee_and_handyman_limit_refused() {
    let visit = Price::VisitFee(8_900);
    assert_eq!(
        parse_quote(form(visit.clone()), &rules(Format::Range)),
        Err(Bounce::WrongFormat)
    );
    assert!(parse_quote(form(visit), &rules(Format::DiagnosticVisit)).is_ok());
    let handyman = QuoteRules {
        license: LicenseKind::None,
        handyman: true,
        certified: true,
        ..rules(Format::Range)
    };
    let top = Price::Hourly {
        rate: 10_000,
        hours: Some((6, 10)),
    };
    let refused = Err(Bounce::Handyman(QuoteRefusal::OverHandymanLimit));
    assert_eq!(parse_quote(form(top), &handyman), refused);
    let under = Price::Range {
        low: 50_000,
        high: 99_999,
        on_site: false,
    };
    assert!(parse_quote(form(under), &handyman).is_ok());
}

proptest! {
    /// TD-FEE-27 property: a setup charge never changes whether a recurring quote passes, nor
    /// the amount the handyman limit judges (FEE-044).
    #[test]
    fn td_fee_27_setup_never_counts(cents in 1u64..200_000, setup in 1u64..10_000_000) {
        let handyman = QuoteRules {
            format: Format::Monthly,
            license: LicenseKind::None,
            handyman: true,
            certified: true,
            ..rules(Format::Monthly)
        };
        let with = Price::Monthly { cents, period: Period::Month, setup: Some(setup) };
        let without = Price::Monthly { cents, period: Period::Month, setup: None };
        let a = parse_quote(form(with), &handyman).map(|q| q.format());
        let b = parse_quote(form(without), &handyman).map(|q| q.format());
        prop_assert_eq!(a, b);
    }
}
