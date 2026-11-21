//! The scrubber's fixed cases and the adversarial corpus (TD-AI-01, TD-AI-05, TD-AI-09, TD-AI-31; AI-002, PRIV-005, AI-003, AI-004, AI-009, AI-020).
//!
//! The corpus is 96 English and Spanish contact snippets (fullwidth and Arabic-Indic digits,
//! zero-width characters inside numbers, spelled-out, compound ("twenty-eight", "double five")
//! and mixed numbers, "O"/"l" for 0/1, "at"/"dot" emails, @handles, bare domains, cross streets,
//! house numbers without a suffix, gate codes, sign-offs, "mi nombre es"; T351's spaced gate
//! codes, lower-case streets, spoken dashes, "it is"/"its"/"<name> here" names, "Best," sign-offs
//! and `(at)`/`(dot)` and spaced `@` emails, D-2622; T658's lower-case Spanish and all-caps
//! addresses, D-4327; T756's connector-joined phones, lone-"." and percent-encoded addresses and
//! keycap digits, D-4765), each placed in four message frames. Recall on the listed contact tokens must be at least 99%, or this test, and so `Ci`,
//! fails (AI-020). All names, numbers and addresses are fake.

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
    ("Gate code is 4 4 1 2", &["4 4 1 2"]),
    ("Gate code 44 12", &["44 12"]),
    ("i live at 2150 mesquite in havasu", &["2150", "mesquite"]),
    ("928 dash 555 dash 0134", &["555", "0134"]),
    ("Steve Smith here", &["steve", "smith"]),
    ("Hi, it is Steve", &["steve"]),
    ("Hey its steve", &["steve"]),
    ("Sink is leaking. Best, Steve", &["steve"]),
    (
        "Thanks, Steve Smith 928-555-0134",
        &["steve", "smith", "0134"],
    ),
    ("steve(at)gmail(dot)com", &["steve", "gmail"]),
    ("reach me at steve @ gmail.com", &["steve", "gmail"]),
    ("vivo en 3301 palo verde", &["3301", "palo", "verde"]),
    ("estoy en el 3301 palo verde", &["3301", "palo", "verde"]),
    ("mi casa es 2150 mesquite", &["2150", "mesquite"]),
    (
        "mi dirección es 3301 palo verde",
        &["3301", "palo", "verde"],
    ),
    (
        "mi direccion es 3301 palo verde",
        &["3301", "palo", "verde"],
    ),
    (
        "se me rompió el calentador, vivo en 3301 palo verde",
        &["3301", "palo", "verde"],
    ),
    (
        "WATER HEATER LEAKING 2150 MESQUITE LAKE HAVASU",
        &["2150", "mesquite"],
    ),
    ("2150 MESQUITE", &["2150", "mesquite"]),
    (
        "AC OUT AT THE HOUSE 4410 SAGUARO PLEASE CALL",
        &["4410", "saguaro"],
    ),
    // T756 (D-4765): phones split by connector words, in digits and in words.
    (
        "nine two eight and five five five and zero one two three",
        &["nine two eight", "five five five", "zero one two three"],
    ),
    (
        "nueve dos ocho y cinco cinco cinco y cero uno siete dos",
        &["nueve dos ocho", "cinco cinco cinco", "cero uno siete dos"],
    ),
    ("928 and 555 and 0171", &["928", "555", "0171"]),
    ("mi cel es 928 y 555 y 0172", &["928", "555", "0172"]),
    ("call 928 then 555 then 0173", &["928", "555", "0173"]),
    ("928 luego 555 luego 0174", &["928", "555", "0174"]),
    ("555 and 0175", &["555", "0175"]),
    ("1 and 928 and 555 and 0176", &["928", "555", "0176"]),
    ("928 - and then - 555 0177", &["928", "555", "0177"]),
    (
        "nine two eight 555 y luego cero uno siete ocho",
        &["nine", "555", "cero"],
    ),
    // A lone "." before the top-level domain, after an at or `@`.
    ("steve at gmail . com", &["steve", "gmail"]),
    ("correo: maria arroba example . com", &["maria", "example"]),
    ("steve @ gmail . com", &["steve", "gmail"]),
    ("pat . test at example . org", &["pat", "test", "example"]),
    // Percent-encoded addresses and links.
    ("steve%40example.com", &["steve", "example"]),
    ("escríbeme a maria%40example.com", &["maria", "example"]),
    ("STEVE%40EXAMPLE%2ECOM", &["steve", "example"]),
    ("https%3A%2F%2Fexample.org%2Fjob", &["example"]),
    ("steve%40example.com%ff", &["steve", "example"]),
    // Keycap digits, spaced and grouped.
    (
        "call 9\u{fe0f}\u{20e3} 2\u{fe0f}\u{20e3} 8\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 0\u{fe0f}\u{20e3} 1\u{fe0f}\u{20e3} 7\u{fe0f}\u{20e3} 9\u{fe0f}\u{20e3}",
        &["5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3}"],
    ),
    (
        "mi cel es 9\u{fe0f}\u{20e3} 2\u{fe0f}\u{20e3} 8\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 0\u{fe0f}\u{20e3} 1\u{fe0f}\u{20e3} 8\u{fe0f}\u{20e3} 0\u{fe0f}\u{20e3}",
        &["5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3}"],
    ),
    (
        "9\u{fe0f}\u{20e3}2\u{fe0f}\u{20e3}8\u{fe0f}\u{20e3} 5\u{fe0f}\u{20e3}5\u{fe0f}\u{20e3}5\u{fe0f}\u{20e3} 0\u{fe0f}\u{20e3}1\u{fe0f}\u{20e3}8\u{fe0f}\u{20e3}1\u{fe0f}\u{20e3}",
        &["5\u{fe0f}\u{20e3}5\u{fe0f}\u{20e3}5\u{fe0f}\u{20e3}"],
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

/// TD-AI-31 (AI-002, AI-020, PRIV-005; D-4327): lower-case Spanish addresses after a cue and
/// all-caps house numbers with all-caps street words lose the number and the street words.
#[test]
fn spanish_and_all_caps_addresses_are_removed() {
    let cases: &[(&str, &[&str])] = &[
        ("vivo en 3301 palo verde", &["3301", "palo", "verde"]),
        ("estoy en el 3301 palo verde", &["3301", "palo", "verde"]),
        ("mi casa es 2150 mesquite", &["2150", "mesquite"]),
        (
            "mi dirección es 3301 palo verde",
            &["3301", "palo", "verde"],
        ),
        (
            "mi direccion es 3301 palo verde",
            &["3301", "palo", "verde"],
        ),
        (
            "se me rompió el calentador, vivo en 3301 palo verde",
            &["3301", "palo", "verde"],
        ),
        (
            "WATER HEATER LEAKING 2150 MESQUITE LAKE HAVASU",
            &["2150", "MESQUITE"],
        ),
        ("2150 MESQUITE", &["2150", "MESQUITE"]),
        (
            "AC OUT AT THE HOUSE 4410 SAGUARO PLEASE CALL",
            &["4410", "SAGUARO"],
        ),
        ("WATER HEATER LEAKING 2150 O'LEARY", &["2150", "O'LEARY"]),
        (
            "WATER HEATER LEAKING 2150 PALO-VERDE",
            &["2150", "PALO-VERDE"],
        ),
        (
            "AC quit last night. WE ARE HOME, 4410 SAGUARO",
            &["4410", "SAGUARO"],
        ),
    ];
    for (text, gone) in cases {
        let out = run(text);
        for word in *gone {
            assert!(!out.as_str().contains(word), "{word} kept in {out:?}");
        }
    }
}

/// TD-AI-31 (AI-002, D-4327): sizes, short numbers, Spanish non-addresses, mixed-case acronyms
/// and texting reply or command words keep every word.
#[test]
fn sizes_replies_and_acronyms_keep_every_word() {
    for text in [
        "WATER HEATER 2 YEARS OLD",
        "50 GALLON WATER HEATER LEAKING",
        "TANK IS 120 GALLONS",
        "ROOF IS 1500 SQ FT",
        "ERROR CODE 41 ON THE AC",
        "instalado en 2015 junto al garaje",
        "la casa es 1500 pies cuadrados",
        "el calentador tiene 10 años",
        "YES PLEASE SEND SOMEONE",
        "My GFCI trips",
        "CLOSE 1042 YES",
        "HOLD 1042 BUSY",
        "CONFIRM 2150 SI",
        "el voltaje es 220 voltios",
        "la cuenta es 1500 dólares",
        "Unit 1042 YES works fine",
    ] {
        assert_eq!(run(text).as_str(), text);
    }
}

/// AI-020, AI-002 (D-4765): a connector word joins digit runs only at a phone length, a lone "."
/// is a dot only after an at or `@`, and percent-decoding is one pass, so ordinary text stays.
#[test]
fn connector_words_lone_dots_and_percent_signs_keep_ordinary_text() {
    for text in [
        "2 and 3 bedrooms",
        "between 9 and 5",
        "de 2 y 3 recámaras",
        "the 1200 and 1300 block",
        "built between 2015 and 2018",
        "1500 and 2000 square feet",
        "$25 and $75",
        "entre 2015 y 2018, luego 3 goteras",
        "the AC died . me and my husband",
        "se rompió . me urge",
        "steve%2540example.com",
        "50% off and 100%done",
    ] {
        assert_eq!(run(text).as_str(), text);
    }
}

/// D-4765's recorded residual: a pair that happens to join to a phone length is cut, the safer
/// error, and a bad percent sequence beside an address does not hide it (PRIV-005, AI-003).
#[test]
fn a_pair_joining_to_a_phone_length_is_cut() {
    assert_eq!(
        run("150 and 2000 square feet").as_str(),
        "[removed] square feet"
    );
    assert_eq!(
        run("write steve%40example.com%zz now").as_str(),
        "write [removed] now"
    );
}
