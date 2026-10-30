//! The question bank loads and refuses malformed sets; no warranty is ever covered (TD-LIC-24,
//! TD-LIC-16; LIC-040, LIC-041, LIC-042). Fake data only.

use crate::loader::{embedded, parse};
use crate::question_bank::{embedded_bank, parse_bank};

const CAT: &str = "not_offered_at_launch: []\ncategories:\n  - id: c1\n    name: C one\n    group: g\n    \
    enabled: true\n    license: {kind: none}\n    handyman_allowed: false\n    construction_type: false\n    \
    recurring: false\n    trip_fee_tier: 25\n    after_hours_emergency: false\n    job_types: [a]\n    \
    quote_format: range\n";

fn q(id: &str, extra: &str) -> String {
    format!(
        "{{id: {id}, card_field: true, en: \"E\", es: \"S\", options_en: [a, b], options_es: [x, y]{extra}}}"
    )
}

fn bank(questions: &[String]) -> String {
    format!(
        "version: 1\nsets:\n  c1:\n    version: 1\n    questions:\n      - {}\n",
        questions.join("\n      - ")
    )
}

fn three() -> Vec<String> {
    vec![q("a", ""), q("b", ""), q("c", "")]
}

fn refused(text: &str) -> bool {
    parse_bank(text, &parse(CAT).expect("catalog")).is_err()
}

/// TD-LIC-24, LIC-040: the real bank loads with a set for every enabled category, each intake
/// question's card-field flag set only on multiple choice, both languages with equal option lists.
#[test]
fn the_real_bank_loads_with_every_enabled_category() {
    let catalog = embedded().expect("catalog");
    let bank = embedded_bank(&catalog).expect("the real bank loads");
    for c in catalog.categories.iter().filter(|c| c.enabled) {
        let set = &bank.sets[&c.id];
        assert!((3..=8).contains(&set.questions.len()), "{}", c.id);
        assert!(set.photos.len() <= 3);
        for question in &set.questions {
            assert!(!question.en.is_empty() && !question.es.is_empty());
            assert!(!question.card_field || question.options_en.is_some());
            assert_eq!(
                question.options_en.as_ref().map(Vec::len),
                question.options_es.as_ref().map(Vec::len)
            );
        }
    }
    assert!(
        bank.sets["plumbing_repair"]
            .questions
            .iter()
            .any(|x| x.card_field)
    );
}

/// TD-LIC-24, LIC-040: a well-formed three-question set loads.
#[test]
fn a_well_formed_set_loads() {
    assert!(!refused(&bank(&three())));
}

/// TD-LIC-24, LIC-040: fewer than 3 or more than 8 questions are refused.
#[test]
fn a_set_outside_three_to_eight_is_refused() {
    assert!(refused(&bank(&three()[..2])));
    let nine: Vec<String> = (0..9).map(|i| q(&format!("q{i}"), "")).collect();
    assert!(refused(&bank(&nine)));
    assert!(!refused(&bank(&nine[..8])));
}

/// TD-LIC-24, LIC-040: a free-text card field is refused; free text without the flag loads.
#[test]
fn a_free_text_card_field_is_refused() {
    let free = |flag: bool| {
        let mut v = three();
        v[2] = format!("{{id: d, card_field: {flag}, en: \"E\", es: \"S\"}}");
        bank(&v)
    };
    assert!(refused(&free(true)));
    assert!(!refused(&free(false)));
}

/// TD-LIC-24, LIC-040: a missing language and unequal option lists are refused.
#[test]
fn a_missing_language_and_unequal_options_are_refused() {
    let mut v = three();
    v[0] = "{id: a, card_field: true, en: \"E\", options_en: [a, b], options_es: [x, y]}".into();
    assert!(refused(&bank(&v)));
    let mut v = three();
    v[0] = "{id: a, card_field: true, en: \"E\", es: \"S\", options_en: [a, b], options_es: [x]}"
        .into();
    assert!(refused(&bank(&v)));
    let mut v = three();
    v[0] = "{id: a, card_field: true, en: \"E\", es: \"S\", options_en: [a, b]}".into();
    assert!(refused(&bank(&v)));
}

/// TD-LIC-24, LIC-040: a repeated id, a condition on a later question and over 3 photos are refused.
#[test]
fn ids_conditions_and_photos_are_checked() {
    assert!(refused(&bank(&[q("a", ""), q("a", ""), q("c", "")])));
    let later = q("a", ", ask_if: {question: c, answers: [1]}");
    assert!(refused(&bank(&[later, q("b", ""), q("c", "")])));
    let earlier = q("c", ", ask_if: {question: a, answers: [2]}");
    assert!(!refused(&bank(&[q("a", ""), q("b", ""), earlier])));
    let not_sure = q("c", ", ask_if: {question: a, answers: [3]}");
    assert!(!refused(&bank(&[q("a", ""), q("b", ""), not_sure])));
    let bad_answer = q("c", ", ask_if: {question: a, answers: [4]}");
    assert!(refused(&bank(&[q("a", ""), q("b", ""), bad_answer])));
    let photos = bank(&three()).replace(
        "    questions:",
        "    photos: [p1, p2, p3, p4]\n    questions:",
    );
    assert!(refused(&photos));
}

/// TD-LIC-24, LIC-040: a size band needs one band floor per option; photos need a trade file name.
#[test]
fn band_floors_and_photo_trades_are_checked() {
    let band = |floor: &str| {
        let mut v = three();
        v[0] = q("a", &format!(", size_band: true{floor}"));
        bank(&v)
    };
    assert!(!refused(&band(", band_floor: [\"0\", \"500.00\"]")));
    assert!(refused(&band("")));
    assert!(refused(&band(", band_floor: [\"0\"]")));
    assert!(refused(&band(", band_floor: [\"0\", \"5\", \"9\"]")));
    let photos = bank(&three()).replace("    questions:", "    photos: [p1]\n    questions:");
    assert!(refused(&photos));
    assert!(!refused(
        &photos.replace("    photos:", "    kb_trade: t\n    photos:")
    ));
}

/// TD-LIC-24, LIC-040: an enabled category with no set, and a set for an unknown category, are refused.
#[test]
fn a_missing_or_unknown_set_is_refused() {
    assert!(refused("version: 1\nsets: {}\n"));
    assert!(refused(&bank(&three()).replace("  c1:", "  zz:")));
}

/// TD-LIC-16, LIC-041: the five split categories carry repair and replace kinds, and electrical also
/// install, so a business can untick some; every job type has a kind and every kind is one of the three.
/// Plumbing and heating and cooling have no install job type yet: each needs a `scope_tags` row in
/// `docs/catalog/license_map.yaml`, the follow-up named in D-605 and D-756.
#[test]
fn the_split_categories_have_job_type_kinds() {
    let catalog = embedded().expect("catalog");
    let kinds = |ids: &[&str]| -> Vec<String> {
        catalog
            .categories
            .iter()
            .filter(|c| ids.contains(&c.id.as_str()))
            .flat_map(|c| c.job_type_kinds.values().cloned())
            .collect()
    };
    for ids in [
        &["plumbing_repair"][..],
        &["hvac_repair", "hvac_replacement"],
        &["electrical"],
        &["roof_repair"],
        &["painting"],
    ] {
        for c in catalog
            .categories
            .iter()
            .filter(|c| ids.contains(&c.id.as_str()))
        {
            for j in &c.job_types {
                assert!(c.job_type_kinds.contains_key(j), "{} {j} has no kind", c.id);
            }
        }
        let k = kinds(ids);
        assert!(
            k.iter().any(|x| x == "repair") && k.iter().any(|x| x == "replace"),
            "{ids:?}"
        );
        assert!(
            k.iter()
                .all(|x| ["repair", "replace", "install"].contains(&x.as_str()))
        );
    }
    assert!(kinds(&["electrical"]).iter().any(|x| x == "install"));
}

/// LIC-042, D-376: a category, job type or keyword that covers a warranty, protection plan or
/// guarantee is refused; the real file has none.
#[test]
fn a_warranty_category_job_type_or_keyword_is_refused() {
    assert!(parse(CAT).is_ok());
    for (from, to) in [
        ("name: C one", "name: Home warranty plans"),
        ("job_types: [a]", "job_types: [extended warranty]"),
        (
            "job_types: [a]",
            "job_types: [a]\n    keywords: [protection plan]",
        ),
        ("id: c1", "id: service_contract"),
        ("job_types: [a]", "job_types: [work guarantee]"),
    ] {
        assert!(parse(&CAT.replace(from, to)).is_err(), "{to}");
    }
}
