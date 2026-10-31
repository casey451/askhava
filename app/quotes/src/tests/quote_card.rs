//! The quote card's pure cases (TD-PICK-18, TD-PICK-29, TD-PICK-44; PICK-003, PICK-004,
//! PICK-005, PICK-040, PICK-041, PICK-044). Made-up businesses and quotes only.

use askhava_licensing::api::{CardLanguage, CardLine};
use jiff::civil::date;

use crate::api::{
    DayPart, Format, POLICY_SUMMARY_SENTENCE, Parts, Permit, QuoteCard, QuoteFacts, Spanish,
    SummarySetting, SwitchRefused, TripFee, check_summary, switch_on,
};

const ORIGINAL: &str = "Replace the kitchen faucet, 285 parts and labor";

fn facts(trip_fee: TripFee) -> QuoteFacts {
    QuoteFacts {
        trip_fee,
        parts: Parts::Included,
        permit: Permit::NotNeeded,
        may_change: true,
        earliest_visit: (date(2026, 11, 3), DayPart::Morning),
        spanish: Spanish::Both,
    }
}

fn card(format: Format, trip_fee: TripFee, after_hours: bool) -> QuoteCard {
    QuoteCard {
        business_name: "Desert Sample Plumbing".to_owned(),
        license_lines: vec![CardLine {
            text: "ROC #000000 showed Active on the ROC website when we checked on November 1, 2026. Check it yourself.".to_owned(),
            link: Some("https://example.test/roc".to_owned()),
        }],
        after_hours,
        format,
        facts: facts(trip_fee),
        original: ORIGINAL.to_owned(),
        summary: None,
    }
}

fn texts(card: &QuoteCard, language: CardLanguage) -> Vec<String> {
    card.lines(language).into_iter().map(|l| l.text).collect()
}

/// TD-PICK-18: name, license line with its link, the check line, the six facts in their fixed
/// order, the format definition and the business's own words, in English and in Spanish.
#[test]
fn td_pick_18_facts_in_fixed_order_in_both_languages() {
    let c = card(Format::Firm, TripFee::Counts, false);
    let lines = c.lines(CardLanguage::English);
    assert_eq!(lines[1].link.as_deref(), Some("https://example.test/roc"));
    let en = texts(&c, CardLanguage::English);
    assert_eq!(en[0], "Desert Sample Plumbing");
    assert_eq!(
        en[2],
        "Ask Hava doesn't rank businesses; check it yourself."
    );
    assert_eq!(en[3], "Independent business, not affiliated with Ask Hava.");
    let want = [
        "Trip fee counts toward the repair: Yes",
        "Parts and materials: Included",
        "Permit: Not needed",
        "Price may change after seeing the job: Yes",
        "Earliest visit: November 3, morning",
        "Speaks Spanish: On the phone and in person",
        "Firm price: the business's stated price for the work described. Ask for any change in writing before work starts.",
    ];
    assert_eq!(en[4..11], want);
    assert_eq!(en[11], format!("The business's own words: {ORIGINAL}"));
    assert_eq!(en.len(), 12);
    let es = texts(&c, CardLanguage::Spanish);
    assert_eq!(
        es[4],
        "El cargo por venir se descuenta de la reparación: Sí"
    );
    assert_eq!(
        es[8],
        "Lo más pronto que puede venir: 3 de noviembre, en la mañana"
    );
    assert_eq!(es[11], format!("Palabras del negocio: {ORIGINAL}"));
}

/// TD-PICK-18: each format has its own definition; no trip fee adds "Free estimate" (never on a
/// visit-fee quote); a "tonight" visit names its night; no card
/// shows a phone, an address, stars, counts or the "minutes this quote took" field.
#[test]
fn td_pick_18_definitions_free_estimate_and_nothing_extra() {
    let formats = [
        Format::Firm,
        Format::Range,
        Format::Monthly,
        Format::Tiers,
        Format::DiagnosticVisit,
    ];
    let defs: Vec<String> = formats
        .iter()
        .map(|f| texts(&card(*f, TripFee::Counts, false), CardLanguage::English)[10].clone())
        .collect();
    for (i, d) in defs.iter().enumerate() {
        assert!(d.contains(": "), "{d}");
        assert!(defs.iter().skip(i + 1).all(|o| o != d));
    }
    let free = texts(
        &card(Format::Firm, TripFee::None, false),
        CardLanguage::English,
    );
    assert_eq!(free[4], "Trip fee counts toward the repair: No trip fee");
    assert_eq!(
        free[11],
        "Free estimate: the business comes out and prices the job on site."
    );
    let visit = texts(
        &card(Format::DiagnosticVisit, TripFee::None, false),
        CardLanguage::English,
    );
    assert!(!visit.iter().any(|l| l.starts_with("Free estimate")));
    let mut tonight = card(Format::Firm, TripFee::Counts, false);
    tonight.facts.earliest_visit.1 = DayPart::Tonight;
    let night = texts(&tonight, CardLanguage::English);
    assert_eq!(night[8], "Earliest visit: the night of November 3");
    for language in [CardLanguage::English, CardLanguage::Spanish] {
        let all = texts(&card(Format::Range, TripFee::None, true), language).join("\n");
        for banned in [
            "minutes", "minutos", "★", "stars", "rating", "best", "vetted", "Phone:",
        ] {
            assert!(!all.contains(banned), "{banned}");
        }
    }
}

/// TD-PICK-29: with the setting off no summary is made; once on, a summary that adds a price,
/// scope or warranty is rejected and one using only the business's words shows under them,
/// labeled in the card's language.
#[test]
fn td_pick_29_summary_off_by_default_and_never_adds_a_claim() {
    assert_eq!(SummarySetting::default(), SummarySetting::Off);
    let faithful = "Replace the faucet, 285 with parts and labor";
    assert_eq!(check_summary(SummarySetting::Off, ORIGINAL, faithful), None);
    for added in [
        "Replace the faucet for 250",
        "Replace the faucet and the sink",
        "Replace the faucet, 1-year warranty",
    ] {
        assert_eq!(
            check_summary(SummarySetting::On, ORIGINAL, added),
            None,
            "{added}"
        );
    }
    let mut c = card(Format::Firm, TripFee::Counts, false);
    let off = texts(&c, CardLanguage::English).join("\n");
    assert!(!off.contains("Summary by"));
    c.summary = check_summary(SummarySetting::On, ORIGINAL, faithful);
    assert!(c.summary.is_some());
    let en = texts(&c, CardLanguage::English);
    assert_eq!(en[11], format!("The business's own words: {ORIGINAL}"));
    assert_eq!(
        en[12],
        format!(
            "Summary by Ask Hava's software. The business's own words above control. {faithful}"
        )
    );
    let es = texts(&c, CardLanguage::Spanish);
    assert!(es[12].starts_with(
        "Resumen hecho por el software de Ask Hava. Las palabras del negocio arriba son las que valen."
    ));
}

/// TD-PICK-29: the setting switches on only with Casey's recorded decision and a published policy
/// version holding the AI-018 summary sentence (a fake version is seeded, D-493).
#[test]
fn td_pick_29_switch_needs_decision_and_policy_sentence() {
    let today = date(2026, 12, 1);
    let policy = format!("Fake policy v9. {POLICY_SUMMARY_SENTENCE}");
    let live = (policy.as_str(), today);
    assert_eq!(switch_on(None, live, today), Err(SwitchRefused::NoDecision));
    assert_eq!(
        switch_on(Some("D-"), live, today),
        Err(SwitchRefused::NoDecision)
    );
    assert_eq!(
        switch_on(Some("yes"), live, today),
        Err(SwitchRefused::NoDecision)
    );
    let without = ("Fake policy v8.", today);
    let refused = Err(SwitchRefused::PolicyLacksSentence);
    assert_eq!(switch_on(Some("D-9001"), without, today), refused);
    let later = (policy.as_str(), date(2026, 12, 2));
    assert_eq!(switch_on(Some("D-9001"), later, today), refused);
    assert_eq!(
        switch_on(Some("D-9001"), live, today),
        Ok(SummarySetting::On)
    );
}

/// TD-PICK-44: only an after-hours business's card says "Gets emergency requests at night", it
/// adds one line and moves nothing else (the cards' order on the page is T052's).
#[test]
fn td_pick_44_after_hours_line_only_when_ticked() {
    let night = card(Format::Firm, TripFee::Counts, true);
    let day = card(Format::Firm, TripFee::Counts, false);
    let (n, d) = (
        texts(&night, CardLanguage::English),
        texts(&day, CardLanguage::English),
    );
    assert_eq!(n[4], "Gets emergency requests at night");
    assert!(!d.iter().any(|l| l.contains("emergency")));
    let mut without = n.clone();
    without.remove(4);
    assert_eq!(without, d);
    let es = texts(&night, CardLanguage::Spanish);
    assert_eq!(es[4], "Recibe solicitudes de emergencia de noche");
}
