//! The quote-card license lines (TD-LIC-11, TD-LIC-14, TD-LIC-25; LIC-026, LIC-032, LIC-034,
//! LIC-037, LIC-038, WORD-016). Every business, number and date is fake.

use jiff::civil::date;

use crate::api::{
    Agency, CardLanguage, HANDYMAN_CARD_LINE, Issuer, LicenseFacts, NoLicenseFacts, licensed_lines,
    no_license_lines,
};
use crate::lines::parse_templates;

fn roc(since_year: Option<i16>) -> LicenseFacts {
    LicenseFacts {
        issuer: Issuer::Roc,
        number: "ROC123456".to_string(),
        checked_on: date(2026, 10, 28),
        since_year,
    }
}

fn agency(a: Agency, since_year: Option<i16>) -> LicenseFacts {
    LicenseFacts {
        issuer: Issuer::Agency(a),
        number: "A-0042".to_string(),
        checked_on: date(2026, 11, 3),
        since_year,
    }
}

fn texts(lines: &[crate::api::CardLine]) -> Vec<&str> {
    lines.iter().map(|l| l.text.as_str()).collect()
}

/// TD-LIC-11: the ROC line has the check date and a link (LIC-026).
#[test]
fn td_lic_11_roc_line_has_date_and_link() {
    let lines = licensed_lines(&roc(None), CardLanguage::English).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0].text,
        "ROC #ROC123456 showed Active on the ROC website when we checked on October 28, 2026. Check it yourself."
    );
    assert_eq!(lines[0].link.as_deref(), Some("https://roc.az.gov/"));
}

/// TD-LIC-11: another agency's line names the agency and links to it (LIC-034, D-057).
#[test]
fn td_lic_11_agency_line_has_date_and_link() {
    let lines =
        licensed_lines(&agency(Agency::SeptageHauler, None), CardLanguage::English).unwrap();
    assert_eq!(
        lines[0].text,
        "ADEQ license #A-0042 showed Active on the ADEQ website when we checked on November 3, 2026. Check it yourself."
    );
    assert_eq!(lines[0].link.as_deref(), Some("https://azdeq.gov/"));
    let alarm =
        licensed_lines(&agency(Agency::AlarmBusiness, None), CardLanguage::English).unwrap();
    assert!(
        alarm[0]
            .text
            .starts_with("Board of Technical Registration license #A-0042")
    );
    assert!(alarm[0].link.is_some());
}

/// TD-LIC-11 / LIC-047: a self-attested pest license gets no check line here.
#[test]
fn td_lic_11_pest_gets_no_check_line() {
    let lines = licensed_lines(
        &agency(Agency::PestManagement, Some(2010)),
        CardLanguage::English,
    )
    .unwrap();
    assert!(lines.is_empty());
}

/// TD-LIC-11: the Spanish check line has the same facts.
#[test]
fn td_lic_11_spanish_lines() {
    let lines = licensed_lines(&roc(Some(2009)), CardLanguage::Spanish).unwrap();
    assert_eq!(
        texts(&lines),
        [
            "ROC #ROC123456 figuraba como Activa en el sitio web del ROC cuando lo revisamos el 28 de octubre de 2026. Compruébelo usted mismo.",
            "Licencia del ROC desde 2009",
        ]
    );
    let lines = licensed_lines(
        &agency(Agency::HomeInspector, Some(2015)),
        CardLanguage::Spanish,
    )
    .unwrap();
    assert!(lines[0].text.contains("Board of Technical Registration"));
    assert_eq!(
        lines[1].text,
        "Licencia de Board of Technical Registration desde 2015"
    );
}

/// TD-LIC-11: insurance shows on a no-license card as reported and is never required (LIC-032).
#[test]
fn td_lic_11_insurance_shown_never_required() {
    let facts = |insurance| NoLicenseFacts {
        handyman: false,
        city_since_year: None,
        insurance,
    };
    let yes = no_license_lines(&facts(Some(true)), CardLanguage::English).unwrap();
    let no = no_license_lines(&facts(Some(false)), CardLanguage::English).unwrap();
    let none = no_license_lines(&facts(None), CardLanguage::English).unwrap();
    assert_eq!(yes[1].text, "Insurance (business-reported): Yes");
    assert_eq!(no[1].text, "Insurance (business-reported): No");
    assert_eq!(texts(&none), ["City business license on file"]);
    let es = no_license_lines(&facts(Some(true)), CardLanguage::Spanish).unwrap();
    assert_eq!(es[1].text, "Seguro (informado por el negocio): Sí");
}

/// TD-LIC-11: a licensed card never carries an insurance line.
#[test]
fn td_lic_11_licensed_card_has_no_insurance_line() {
    for facts in [roc(Some(2009)), agency(Agency::HomeInspector, Some(2012))] {
        let lines = licensed_lines(&facts, CardLanguage::English).unwrap();
        assert!(lines.iter().all(|l| !l.text.contains("Insurance")));
    }
}

/// TD-LIC-14: "ROC license since 2009" from the record's issue year, none without one (LIC-037).
#[test]
fn td_lic_14_roc_since_line_only_with_a_year() {
    let with = licensed_lines(&roc(Some(2009)), CardLanguage::English).unwrap();
    assert_eq!(with[1].text, "ROC license since 2009");
    assert_eq!(with[1].link, None);
    let without = licensed_lines(&roc(None), CardLanguage::English).unwrap();
    assert!(without.iter().all(|l| !l.text.contains("since")));
}

/// TD-LIC-14: the city line has a year only when the list gave a date, after the handyman line
/// (LIC-038).
#[test]
fn td_lic_14_city_line_and_handyman_order() {
    let facts = |handyman, city_since_year| NoLicenseFacts {
        handyman,
        city_since_year,
        insurance: None,
    };
    let dated = no_license_lines(&facts(false, Some(2018)), CardLanguage::English).unwrap();
    assert_eq!(texts(&dated), ["City business license on file since 2018"]);
    let undated = no_license_lines(&facts(false, None), CardLanguage::English).unwrap();
    assert_eq!(texts(&undated), ["City business license on file"]);
    let handyman = no_license_lines(&facts(true, Some(2018)), CardLanguage::English).unwrap();
    assert_eq!(
        texts(&handyman),
        [
            HANDYMAN_CARD_LINE,
            "City business license on file since 2018"
        ]
    );
    let es = no_license_lines(&facts(false, Some(2018)), CardLanguage::Spanish).unwrap();
    assert_eq!(
        es[0].text,
        "Licencia comercial de la ciudad registrada desde 2018"
    );
}

/// TD-LIC-14: no card line is a rating, review count, hire count or ranking (WORD-016).
#[test]
fn td_lic_14_no_rating_review_or_ranking_words() {
    let mut all = Vec::new();
    for language in [CardLanguage::English, CardLanguage::Spanish] {
        all.extend(licensed_lines(&roc(Some(2009)), language).unwrap());
        all.extend(licensed_lines(&agency(Agency::HomeInspector, Some(2015)), language).unwrap());
        let facts = NoLicenseFacts {
            handyman: true,
            city_since_year: Some(2018),
            insurance: Some(true),
        };
        all.extend(no_license_lines(&facts, language).unwrap());
    }
    let banned = [
        "rating",
        "review",
        "stars",
        "rank",
        "best",
        "vetted",
        "hired",
        "reseña",
        "calificación",
    ];
    for line in &all {
        let text = line.text.to_lowercase();
        assert!(banned.iter().all(|w| !text.contains(w)), "{text}");
    }
}

/// TD-LIC-25: an agency licensee's "<Agency> license since <year>" only when the record has a date.
#[test]
fn td_lic_25_agency_since_line_only_with_a_date() {
    for a in [
        Agency::HomeInspector,
        Agency::AlarmBusiness,
        Agency::SeptageHauler,
    ] {
        let name = if a == Agency::SeptageHauler {
            "ADEQ"
        } else {
            "Board of Technical Registration"
        };
        let with = licensed_lines(&agency(a, Some(2014)), CardLanguage::English).unwrap();
        assert_eq!(with[1].text, format!("{name} license since 2014"));
        let without = licensed_lines(&agency(a, None), CardLanguage::English).unwrap();
        assert_eq!(without.len(), 1);
    }
}

/// The template file parses, and a short month list or bad file is refused.
#[test]
fn template_file_parses_and_bad_files_are_refused() {
    let text = include_str!("../../templates/license_lines.json");
    assert!(parse_templates(text).is_some());
    assert!(parse_templates("{").is_none());
    let short = text.replacen("\"January\", ", "", 1);
    assert!(parse_templates(&short).is_none());
}
