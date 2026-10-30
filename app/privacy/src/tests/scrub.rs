//! The scrubber's fixed cases and the adversarial corpus (TD-AI-01, TD-AI-05, TD-AI-09; AI-002, AI-003, AI-004, AI-009, AI-020).
//!
//! The corpus is 63 English and Spanish contact snippets (fullwidth and Arabic-Indic digits,
//! zero-width characters inside numbers, spelled-out, compound ("twenty-eight", "double five")
//! and mixed numbers, "O"/"l" for 0/1, "at"/"dot" emails, @handles, bare domains, cross streets,
//! house numbers without a suffix, gate codes, sign-offs, "mi nombre es"), each placed in four
//! message frames: 252 cases. Recall on the listed contact tokens must be at least 99%, or
//! this test, and so `Ci`, fails (AI-020). All names, numbers and addresses are fake.

use secrecy::SecretString;

use crate::api::{CommonWords, Known, NameMemory, REMOVED, ScrubbedText, scrub};

/// One snippet and the contact tokens (lower-cased, as they would appear after normalizing)
/// that must not survive.
const CONTACTS: &[(&str, &[&str])] = &[
    ("call 928 555 0142", &["928", "555", "0142"]),
    ("928-555-0143", &["555", "0143"]),
    ("(928) 555.0144", &["555", "0144"]),
    ("928.555.0145", &["0145"]),
    ("+1 928 555 0146", &["555", "0146"]),
    ("９２８５５５０１４７", &["5550147"]),
    ("٩٢٨٥٥٥٠١٤٨", &["5550148"]),
    ("۹۲۸۵۵۵۰۱۴۹", &["5550149"]),
    ("928\u{200B}555\u{200D}0150", &["0150", "555"]),
    (
        "nine two eight five five five zero one five one",
        &["nine two eight", "zero one five one"],
    ),
    ("9two8 555 01five2", &["9two8", "01five2"]),
    (
        "nueve dos ocho cinco cinco cinco cero uno cinco tres",
        &["nueve dos ocho", "cero uno"],
    ),
    ("mi cel es 928 555 01 54", &["555"]),
    ("text me at 928/555/0155", &["0155"]),
    ("my number is 928 5 5 5 0 1 5 6", &["0 1 5 6"]),
    ("nine2eight5five5oh157", &["nine2eight"]),
    ("llámame al 928-555-0158", &["0158"]),
    ("928 555 zero one five nine", &["555", "zero one five nine"]),
    ("email me dave@example.com", &["dave@example.com"]),
    ("steve at gmail dot com", &["steve", "gmail"]),
    ("steve at gmail.com", &["steve", "gmail"]),
    ("ana.test at example dot com", &["ana.test", "example"]),
    (
        "correo: maria arroba example punto com",
        &["maria", "example"],
    ),
    (
        "pat dot test at example dot org",
        &["pat", "test", "example"],
    ),
    ("STEVE@EXAMPLE.COM", &["steve@example.com"]),
    ("write to kim_test@example.org.", &["kim_test"]),
    ("find me on insta @steve_test", &["steve_test"]),
    ("dm @casa.test.99", &["casa.test.99"]),
    ("see example.com/x", &["example.com"]),
    ("https://example.org/job?id=7", &["example.org"]),
    ("www.example.net", &["example.net"]),
    ("my site is testplumbing.co", &["testplumbing"]),
    ("this is Dave", &["dave"]),
    ("im steve", &["steve"]),
    ("I'm Ana Testa", &["ana", "testa"]),
    ("It's Kimberly", &["kimberly"]),
    ("MY NAME IS ROSA TESTER", &["rosa", "tester"]),
    ("soy Lupe", &["lupe"]),
    ("me llamo Rocío Prueba", &["rocío", "prueba"]),
    ("mi nombre es Pedro", &["pedro"]),
    ("I’m Joan", &["joan"]),
    ("Thanks, Steve", &["steve"]),
    ("thank you - Ana Testa", &["ana", "testa"]),
    ("Gracias, Marta", &["marta"]),
    ("— Bob Tester", &["bob", "tester"]),
    ("regards, Leeann Example", &["leeann", "example"]),
    ("12 Test Way", &["12", "test way"]),
    ("1234 N Sample Ave", &["1234", "sample"]),
    ("4500 Example Blvd", &["4500", "example"]),
    ("100 W 5th St", &["100", "5th"]),
    ("calle Prueba 123", &["prueba", "123"]),
    ("at Test Ave and Sample Dr", &["test ave", "sample"]),
    (
        "corner of Mesa Test Rd & Ocotillo Test Ln",
        &["mesa", "ocotillo"],
    ),
    (
        "call nine twenty-eight five five five zero one six one",
        &["twenty-eight", "zero one six one"],
    ),
    (
        "nine two eight double five five zero one six two",
        &["double five", "zero one six two"],
    ),
    ("text 928 555 O1l3", &["555", "o1l3"]),
    ("cel 928 555 0l64", &["555", "0l64"]),
    ("I'm at 2150 Mesquite", &["2150", "mesquite"]),
    ("vivo en 3301 Palo Verde", &["3301", "palo", "verde"]),
    ("gate code 4321", &["4321"]),
    ("código del portón 9876", &["9876"]),
    ("pin #2468", &["2468"]),
    (
        "This is Dave, call 928 555 0142 or dave@example.com, see example.com/x, 12 Test Way",
        &["dave", "555", "example.com", "12 test", "test way"],
    ),
];

/// Message frames the snippets are placed in; each ends with the snippet, as sign-offs do.
const FRAMES: &[&str] = &[
    "{}",
    "AC quit last night. {}",
    "Hola, el calentador no funciona. {}",
    "Need a plumber, leak under the sink!! {}",
];

fn run(text: &str) -> ScrubbedText {
    run_with(text, None, &mut NameMemory::default())
}

fn run_with(text: &str, vault: Option<&SecretString>, memory: &mut NameMemory) -> ScrubbedText {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: vault,
    };
    scrub(text, &known, memory)
}

/// TD-AI-09 (AI-020, AI-002): the corpus holds at least 200 cases and scores at least 99% recall.
#[test]
fn the_corpus_scores_at_least_99_percent_recall() {
    let (mut cases, mut total, mut missed) = (0, 0, Vec::new());
    for frame in FRAMES {
        for (snippet, tokens) in CONTACTS {
            cases += 1;
            let out = run(&frame.replace("{}", snippet)).as_str().to_lowercase();
            total += tokens.len();
            missed.extend(
                tokens
                    .iter()
                    .filter(|t| out.contains(*t))
                    .map(|t| (t, out.clone())),
            );
        }
    }
    assert!(cases >= 200, "the corpus has {cases} cases");
    let recall = 1.0 - missed.len() as f64 / total as f64;
    assert!(recall >= 0.99, "recall {recall:.4}; missed {missed:?}");
}

/// TD-AI-01 (D-146): "seeing scorpions im steve" loses "steve"; "I'm having a leak" keeps every word.
#[test]
fn a_stated_name_goes_and_common_words_stay() {
    assert_eq!(
        run("seeing scorpions im steve").as_str(),
        "seeing scorpions im [removed]"
    );
    assert_eq!(run("I'm having a leak").as_str(), "I'm having a leak");
    assert_eq!(
        run("it's leaking under the sink").as_str(),
        "it's leaking under the sink"
    );
}

/// TD-AI-01 (AI-003): the vault name is removed wherever it appears, in any case.
#[test]
fn the_vault_name_is_removed() {
    let vault = SecretString::from("Zelda Example");
    let out = run_with(
        "ZELDA here, example of a leak? zelda's sink",
        Some(&vault),
        &mut NameMemory::default(),
    );
    assert_eq!(
        out.as_str(),
        "[removed] here, [removed] of a leak? [removed] sink"
    );
}

/// TD-AI-01 (AI-003): every vault-name word goes, even one on the common-words list.
#[test]
fn a_vault_name_on_the_common_list_is_still_removed() {
    let vault = SecretString::from("Joy Early");
    let out = run_with(
        "Joy says call early",
        Some(&vault),
        &mut NameMemory::default(),
    );
    assert_eq!(out.as_str(), "[removed] says call [removed]");
}

/// TD-AI-09 (AI-020): both words of a two-word name are remembered for later turns.
#[test]
fn both_words_of_a_stated_name_are_remembered() {
    let mut memory = NameMemory::default();
    run_with("my name is Rosa Tester", None, &mut memory);
    let out = run_with("Tester here, and Rosa says hi", None, &mut memory);
    assert_eq!(out.as_str(), "[removed] here, and [removed] says hi");
}

/// D-093, AI-002: everyday numbers and words stay; only phone-length runs and named houses go.
#[test]
fn everyday_numbers_stay() {
    for text in [
        "the 1200 sq ft house is twenty years old",
        "once a week, 2 o 3 times",
        "CLOSE 1042 YES",
    ] {
        assert_eq!(run(text).as_str(), text);
    }
}

/// TD-AI-09 (AI-020): a name given at the second turn is absent from the fourth turn's text.
#[test]
fn a_name_stated_earlier_is_removed_later() {
    let mut memory = NameMemory::default();
    let turns = [
        "my AC is out",
        "this is Marisol",
        "1",
        "Marisol again, any news?",
    ];
    let outs: Vec<String> = turns
        .iter()
        .map(|t| run_with(t, None, &mut memory).as_str().to_owned())
        .collect();
    assert_eq!(outs[3], "[removed] again, any news?");
    assert!(
        !format!("{memory:?}").contains("arisol"),
        "memory never prints a name"
    );
}

/// TD-AI-05 (AI-002, D-373): business quote text is scrubbed the same way.
#[test]
fn quote_text_loses_every_contact() {
    let quote =
        "This is Dave, call 928 555 0142 or dave@example.com, see example.com/x, 12 Test Way";
    let out = run(quote);
    assert_eq!(
        out.as_str(),
        format!("This is {REMOVED} call {REMOVED} or {REMOVED} see {REMOVED}")
    );
}

/// D-093: short commands survive the scrub.
#[test]
fn short_commands_survive() {
    for command in ["2 YES", "CLOSE 1042", "1", "ok", "STOP", "HELP"] {
        assert_eq!(run(command).as_str(), command);
    }
}

/// AI-020: format characters, including the tag block, are gone from the text the AI sees.
#[test]
fn format_characters_are_stripped() {
    let hidden = "leak\u{E0049}\u{E0047}\u{200B} in\u{FEFF} the hall";
    assert_eq!(run(hidden).as_str(), "leak in the hall");
}

/// TD-AI-01 (AI-009, AI-001): CI configuration holds no Claude API key; the build uses the stand-in.
#[test]
fn ci_configuration_holds_no_ai_key() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows");
    let marker = ["sk", "ant", ""].join("-");
    for entry in std::fs::read_dir(root).expect("workflows") {
        let text = std::fs::read_to_string(entry.expect("entry").path()).unwrap_or_default();
        assert!(
            !text.contains(&marker),
            "an AI key marker in CI configuration"
        );
    }
}
