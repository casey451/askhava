//! The brand file (TD-WORD-34; WORD-001, WORD-008, WORD-021, TXT-006, FEE-049, D-1932).
//!
//! Example cases: the embedded file parses; each bad field is refused naming the field; a second
//! install is refused; `fill` replaces each brand token in English and Spanish, leaves `{job}`
//! alone and refuses `{brand_x}`. The fake brand "Fakeco" is invented for these tests.

use serde_json::{Value, json};

use crate::brand::{Brand, BrandError, brand, install};

/// A valid fake brand file, as JSON, to break one field at a time.
fn fake() -> Value {
    json!({
        "display_name": "Fakeco",
        "business_name": "Fakeco for Business",
        "short_name": "Fakeco",
        "legal_name": "Fakeco LLC",
        "spoken_legal_name": "Fakeco L L C",
        "domain": "fakeco.test",
        "email_from_name": "Fakeco",
        "email_from_address": "jobs@notify.fakeco.test",
        "statement_prefix": "FAKECO",
        "forbidden_alone": ["Fake"],
        "es": {
            "display_name": "Fakeco ES",
            "business_name": "Fakeco para Negocios",
            "short_name": "FakecoES",
            "legal_name": "Fakeco ES LLC"
        }
    })
}

fn parse(value: &Value) -> Result<Brand, BrandError> {
    Brand::parse(&value.to_string())
}

fn with(path: &[&str], new: Value) -> Result<Brand, BrandError> {
    let mut value = fake();
    let mut slot = &mut value;
    for key in path {
        slot = &mut slot[*key];
    }
    *slot = new;
    parse(&value)
}

#[test]
fn td_word_34_the_embedded_brand_parses_and_is_served() {
    let embedded = Brand::embedded().unwrap();
    assert!(embedded.legal_name().contains(embedded.display_name()));
    assert!(embedded.email_from_address().ends_with(embedded.domain()));
    assert!(!embedded.forbidden_alone().is_empty());
    assert!(!embedded.spoken_legal_name().is_empty());
    assert!(!embedded.email_from_name().is_empty());
    assert!(!embedded.business_name().is_empty());
    assert!(!embedded.es().business_name().is_empty());
    assert_eq!(brand().display_name(), embedded.display_name());
    assert!(parse(&fake()).is_ok());
}

#[test]
fn td_word_34_each_bad_field_is_refused_naming_the_field() {
    let cases: [(&[&str], Value, &str); 13] = [
        (&["display_name"], json!(""), "display_name"),
        (&["es", "legal_name"], json!(" "), "es.legal_name"),
        (&["forbidden_alone"], json!([]), "forbidden_alone"),
        (&["forbidden_alone"], json!([""]), "forbidden_alone"),
        (&["short_name"], json!("Fake:co"), "short_name"),
        (
            &["short_name"],
            json!("Fakeco Fakeco Fakeco1"),
            "short_name",
        ),
        (&["es", "short_name"], json!("Fake:"), "es.short_name"),
        (&["legal_name"], json!("Other LLC"), "legal_name"),
        (&["es", "legal_name"], json!("Other LLC"), "es.legal_name"),
        (&["domain"], json!("Fakeco.test"), "domain"),
        (&["domain"], json!("fakeco"), "domain"),
        (&["domain"], json!("-fakeco.test"), "domain"),
        (
            &["email_from_address"],
            json!("jobs@notfakeco.test"),
            "email_from_address",
        ),
    ];
    for (path, value, field) in cases {
        assert_eq!(with(path, value), Err(BrandError::Field(field)), "{field}");
    }
}

#[test]
fn td_word_34_a_sender_off_the_domain_is_refused() {
    for address in [
        "jobs@other.test",
        "@fakeco.test",
        "jobs fakeco.test",
        "a b@fakeco.test",
    ] {
        assert_eq!(
            with(&["email_from_address"], json!(address)),
            Err(BrandError::Field("email_from_address")),
            "{address}"
        );
    }
    assert!(with(&["email_from_address"], json!("help@fakeco.test")).is_ok());
}

#[test]
fn td_word_34_a_bad_statement_prefix_is_refused() {
    // 4 characters, 11 characters, all digits, lower case, and 10 characters, which leaves
    // `<prefix>* JOB 1234567` at 23 characters, over the statement's 22 (FEE-049).
    for prefix in ["FAKE", "FAKECOFAKEC", "1234567", "fakeco", "FAKECOFAKE"] {
        assert_eq!(
            with(&["statement_prefix"], json!(prefix)),
            Err(BrandError::Field("statement_prefix")),
            "{prefix}"
        );
    }
    assert!(with(&["statement_prefix"], json!("FAKECO123")).is_ok());
}

#[test]
fn td_word_34_an_unknown_or_missing_field_does_not_parse() {
    let mut extra = fake();
    extra["nickname"] = json!("Fakey");
    assert!(matches!(parse(&extra), Err(BrandError::Parse(_))));
    let mut missing = fake();
    missing["es"] = json!({"display_name": "Fakeco"});
    assert!(matches!(parse(&missing), Err(BrandError::Parse(_))));
}

#[test]
fn td_word_34_a_second_install_is_refused() {
    let embedded = Brand::embedded().unwrap();
    let first = install(embedded.clone());
    assert!(first.is_ok() || first == Err(BrandError::AlreadyInstalled));
    assert_eq!(install(embedded.clone()), Err(BrandError::AlreadyInstalled));
    assert_eq!(brand(), &embedded);
}

#[test]
fn td_word_34_fill_replaces_brand_tokens_in_both_languages() {
    let fake = parse(&fake()).unwrap();
    let text = "{brand_short}: {brand}, {brand_business}, {brand_legal}, {domain}/help, job {job}";
    assert_eq!(
        fake.fill(text, false).unwrap(),
        "Fakeco: Fakeco, Fakeco for Business, Fakeco LLC, fakeco.test/help, job {job}"
    );
    assert_eq!(
        fake.fill(text, true).unwrap(),
        "FakecoES: Fakeco ES, Fakeco para Negocios, Fakeco ES LLC, fakeco.test/help, job {job}"
    );
    assert_eq!(fake.es().display_name(), "Fakeco ES");
    assert_eq!(fake.es().short_name(), "FakecoES");
    assert_eq!(fake.es().legal_name(), "Fakeco ES LLC");
}

#[test]
fn td_word_34_fill_refuses_an_unknown_brand_token_and_keeps_other_braces() {
    let fake = parse(&fake()).unwrap();
    assert_eq!(
        fake.fill("{brand_x} hi", false),
        Err(BrandError::UnknownToken("brand_x".to_owned()))
    );
    assert_eq!(fake.fill("a { b", false).unwrap(), "a { b");
    assert_eq!(fake.fill("{{job}} {}", false).unwrap(), "{{job}} {}");
    assert_eq!(fake.fill("plain", true).unwrap(), "plain");
    assert_eq!(fake.statement_prefix(), "FAKECO");
}

/// The fill cases `tests/docs/test_brand_literals.py` reads from this raw string and runs through
/// `scripts/brand.py`, so the
/// two read nested and stray braces the same way: each gives `filled`, or `unknown` names the
/// refused `{brand...}` token.
const FILL_CASES: &str = r#"
[
  {"text": "{{brand}}", "spanish": false, "filled": "{Fakeco}"},
  {"text": "{ {brand}", "spanish": false, "filled": "{ Fakeco"},
  {"text": "{brand{brand}}", "spanish": false, "filled": "{brandFakeco}"},
  {"text": "} {brand} {", "spanish": false, "filled": "} Fakeco {"},
  {"text": "{brand}{domain}", "spanish": false, "filled": "Fakecofakeco.test"},
  {"text": "{brand_short}: {job}", "spanish": true, "filled": "FakecoES: {job}"},
  {"text": "{brand", "spanish": false, "filled": "{brand"},
  {"text": "a { b", "spanish": false, "filled": "a { b"},
  {"text": "{{job}} {}", "spanish": false, "filled": "{{job}} {}"},
  {"text": "{ {brand_x}", "spanish": false, "unknown": "brand_x"},
  {"text": "{{brand_x}}", "spanish": false, "unknown": "brand_x"},
  {"text": "{brand }", "spanish": true, "unknown": "brand "}
]
"#;

#[test]
fn td_word_34_fill_reads_nested_and_stray_braces_like_brand_py() {
    let fake = parse(&fake()).unwrap();
    let cases: Vec<Value> = serde_json::from_str(FILL_CASES).unwrap();
    assert!(cases.len() >= 10);
    for case in cases {
        let text = case["text"].as_str().unwrap();
        let got = fake.fill(text, case["spanish"].as_bool().unwrap());
        let want = match case["unknown"].as_str() {
            Some(name) => Err(BrandError::UnknownToken(name.to_owned())),
            None => Ok(case["filled"].as_str().unwrap().to_owned()),
        };
        assert_eq!(got, want, "{text}");
    }
}

#[test]
fn td_word_34_a_trailing_newline_is_refused() {
    let cases: [(&[&str], &str, &str); 3] = [
        (&["domain"], "fakeco.test\n", "domain"),
        (&["statement_prefix"], "FAKECO\n", "statement_prefix"),
        (
            &["email_from_address"],
            "jobs@fakeco.test\n",
            "email_from_address",
        ),
    ];
    for (path, value, field) in cases {
        assert_eq!(
            with(path, json!(value)),
            Err(BrandError::Field(field)),
            "{field}"
        );
    }
}
