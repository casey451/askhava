//! TD-INT-21, TD-INT-18, TD-INT-24 and the pure cases of TD-INT-22 (INT-039, INT-040, CARD-007,
//! AI-019, INT-002, INT-004): the question set one question at a time, the smell follow-up and
//! routing confidence. Fake request words only; the business name is invented.

use askhava_ai::api::WriteSources;
use askhava_catalog::api::{QuestionBank, QuestionSet, question_bank};
use askhava_privacy::api::{CommonWords, Known, NameMemory, ScrubbedText, scrub};

use crate::api::rules::{Check, check};
use crate::api::{
    AiTurn, BankRun, BankSettings, Decision, Given, Heard, MAX_ASKED, Next, Prompt, Reading,
    Scored, decide, keyword_reading, starts_now,
};

const WORDS: &str = "The house is warm and the system makes a noise";
const SMELL_TEXT: &str =
    "Ask Hava: What kind of smell? 1) Gas or rotten egg 2) Burning 3) Something else";
const AI_ON: BankSettings = BankSettings {
    ai_on: true,
    skip_threshold: None,
};

fn bank() -> QuestionBank {
    question_bank().expect("the shipped bank loads")
}

fn scrubbed(raw: &str) -> ScrubbedText {
    let common = CommonWords::default();
    scrub(
        raw,
        &Known {
            common: &common,
            vault_name: None,
        },
        &mut NameMemory::default(),
    )
}

/// Runs `f` with her scrubbed `WORDS` and one invented business name to keep out.
fn with<T>(f: impl FnOnce(&WriteSources<'_>) -> T) -> T {
    let words = scrubbed(WORDS);
    let names = ["Desert Fake Air".to_owned()];
    f(&WriteSources {
        words: &words,
        answers: &[],
        photo_notes: &[],
        business_names: &names,
    })
}

fn ask(run: &mut BankRun<'_>, words: &str, ai: &AiTurn<'_>) -> Option<Prompt> {
    match with(|s| run.next(words, ai, s)) {
        Next::Ask(p) => Some(p),
        Next::Done => None,
    }
}

/// Asks with no AI offer and answers each question in turn with `replies`; returns the IDs asked.
fn walk(run: &mut BankRun<'_>, words: &str, replies: &[&str]) -> Vec<String> {
    let mut asked = Vec::new();
    let mut replies = replies.iter();
    while let Some(p) = ask(run, words, &AiTurn::default()) {
        asked.push(p.id.clone());
        let reply = replies.next().copied().unwrap_or("not sure");
        assert!(
            matches!(run.hear(reply), Heard::Stored(_)),
            "{}: {reply}",
            p.id
        );
    }
    asked
}

/// TD-INT-21: the hvac set in its order, one at a time, "Not sure" last on every list, the
/// `ask_if` question left out when its condition fails, the answers stored for the card.
#[test]
fn td_int_21_questions_come_one_at_a_time_in_order_and_ask_if_applies() {
    let bank = bank();
    let mut run = BankRun::new(
        &bank.sets["hvac_repair"],
        false,
        false,
        BankSettings::default(),
    );
    let first = ask(&mut run, "", &AiTurn::default()).expect("first question");
    assert_eq!(first.text, "What kind of system is it?");
    assert_eq!(first.options.last().map(String::as_str), Some("Not sure"));
    assert_eq!(run.hear("2"), Heard::Stored(Given::Option(2)));
    let rest = walk(&mut run, "", &["1", "not sure", "No", "the unit is old"]);
    assert_eq!(rest, ["symptom", "age", "error_code", "details"]);
    let card = run.card_answers();
    assert_eq!(
        card[0],
        (
            "What kind of system is it?".into(),
            "Package unit on the roof or ground".into()
        )
    );
    assert_eq!(
        card[2],
        ("About how old is the system?".into(), "Not sure".into())
    );
    assert_eq!(card.len(), 4, "free text is never a card field");
}

/// TD-INT-21: "no sé" is accepted in Spanish, and the Spanish list ends "No sé".
#[test]
fn td_int_21_no_se_is_accepted() {
    let bank = bank();
    let mut run = BankRun::new(
        &bank.sets["hvac_repair"],
        true,
        false,
        BankSettings::default(),
    );
    let first = ask(&mut run, "", &AiTurn::default()).expect("first question");
    assert_eq!(first.options.last().map(String::as_str), Some("No sé"));
    assert_eq!(run.hear("No sé"), Heard::Stored(Given::NotSure));
    assert_eq!(run.hear("whatever"), Heard::Again, "nothing pending");
}

/// TD-INT-21: never more than 8 questions, an injected smell follow-up counted.
#[test]
fn td_int_21_never_more_than_eight() {
    let mut set: QuestionSet = bank().sets["hvac_repair"].clone();
    let base = set.questions[3].clone();
    set.questions = (0..8)
        .map(|i| {
            let mut q = base.clone();
            q.id = format!("q{i}");
            q
        })
        .collect();
    set.questions[0].options_en = Some(vec!["Strange noise or smell".into(), "Other".into()]);
    let mut run = BankRun::new(&set, false, false, BankSettings::default());
    let asked = walk(&mut run, "", &["1"]);
    assert_eq!(asked.len(), MAX_ASKED);
    assert_eq!(asked[1], "smell_kind");
}

/// TD-INT-21: with AI off the exact templates are asked (a rewording is ignored) and a question
/// her words answer is skipped only by the keyword rule.
#[test]
fn td_int_21_ai_off_asks_templates_and_skips_by_keyword_only() {
    let bank = bank();
    let mut run = BankRun::new(
        &bank.sets["hvac_repair"],
        false,
        false,
        BankSettings::default(),
    );
    let ai = AiTurn {
        reword: Some("Which system do you have?"),
        reading: Some(("system_type", 1, 1.0)),
        follow_up: None,
    };
    let p = ask(&mut run, "Swamp cooler pads are dry", &ai).expect("a question");
    assert_eq!(
        (p.id.as_str(), p.text.as_str(), p.reworded),
        ("symptom", "What's happening?", false)
    );
    assert_eq!(run.answers()[0], ("system_type".into(), Given::Option(3)));
}

/// TD-INT-21, D-492: with the stand-in AI on and no threshold, nothing is skipped at any confidence.
#[test]
fn td_int_21_ai_on_with_no_threshold_skips_nothing() {
    let bank = bank();
    let mut run = BankRun::new(&bank.sets["hvac_repair"], false, false, AI_ON);
    let ai = AiTurn {
        reading: Some(("system_type", 3, 1.0)),
        ..AiTurn::default()
    };
    let p = ask(&mut run, "Swamp cooler pads are dry", &ai).expect("a question");
    assert_eq!(p.id, "system_type");
}

/// TD-INT-21: with the threshold 0.9, a reading at 0.90 skips and one at 0.89 asks.
#[test]
fn td_int_21_threshold_skips_at_090_and_asks_at_089() {
    let bank = bank();
    let set = &bank.sets["hvac_repair"];
    let settings = BankSettings {
        ai_on: true,
        skip_threshold: Some(0.9),
    };
    for (confidence, first) in [(0.90, "symptom"), (0.89, "system_type")] {
        let mut run = BankRun::new(set, false, false, settings);
        let ai = AiTurn {
            reading: Some(("system_type", 1, confidence)),
            ..AiTurn::default()
        };
        assert_eq!(
            ask(&mut run, WORDS, &ai).expect("question").id,
            first,
            "{confidence}"
        );
    }
}

/// TD-INT-21, AI-019: a reworded question keeps the bank's options; a price, a cause word or a
/// business name sends the fixed template instead.
#[test]
fn td_int_21_rewording_keeps_options_and_falls_back_on_a_check() {
    let bank = bank();
    let set = &bank.sets["hvac_repair"];
    let template_options = {
        let mut run = BankRun::new(set, false, false, BankSettings::default());
        ask(&mut run, WORDS, &AiTurn::default())
            .expect("question")
            .options
    };
    let mut run = BankRun::new(set, false, false, AI_ON);
    let ai = AiTurn {
        reword: Some("What kind of cooling system does the house have?"),
        ..AiTurn::default()
    };
    let p = ask(&mut run, WORDS, &ai).expect("question");
    assert!(p.reworded);
    assert_eq!(p.options, template_options);
    for bad in [
        "What system is it? A visit is about $89.",
        "What system is it? It probably needs a new part.",
        "What system is it? Desert Fake Air can help.",
    ] {
        let mut run = BankRun::new(set, false, false, AI_ON);
        let ai = AiTurn {
            reword: Some(bad),
            ..AiTurn::default()
        };
        let p = ask(&mut run, WORDS, &ai).expect("question");
        assert_eq!(
            (p.text.as_str(), p.reworded),
            ("What kind of system is it?", false),
            "{bad}"
        );
    }
}

/// TD-INT-21, D-431: one follow-up, only from the set's `kb_trade` brief.
#[test]
fn td_int_21_follow_up_only_from_the_sets_brief() {
    let bank = bank();
    let mut run = BankRun::new(&bank.sets["hvac_repair"], false, false, AI_ON);
    ask(&mut run, WORDS, &AiTurn::default()).expect("first");
    run.hear("1");
    let wrong = AiTurn {
        follow_up: Some(("Is the noise a hum or a rattle?", "plumbing")),
        ..AiTurn::default()
    };
    assert_eq!(ask(&mut run, WORDS, &wrong).expect("next").id, "symptom");
    run.hear("1");
    let right = AiTurn {
        follow_up: Some(("Is the noise a hum or a rattle?", "hvac")),
        ..AiTurn::default()
    };
    assert_eq!(
        ask(&mut run, WORDS, &right).expect("follow-up").id,
        "follow_up"
    );
    assert_eq!(run.hear("a rattle"), Heard::Stored(Given::Text));
    assert_ne!(
        ask(&mut run, WORDS, &right).expect("next").id,
        "follow_up",
        "only one"
    );
}

/// TD-INT-21, INT-029: on an emergency the questions wait for the cards, then only the
/// `emergency` ones are asked.
#[test]
fn td_int_21_emergency_questions_come_after_the_cards() {
    assert!(!starts_now(true, false));
    assert!(starts_now(true, true) && starts_now(false, false));
    let bank = bank();
    let mut run = BankRun::new(
        &bank.sets["hvac_repair"],
        false,
        true,
        BankSettings::default(),
    );
    assert_eq!(walk(&mut run, "", &["2"]), ["symptom"]);
}

/// TD-INT-18: "Strange noise or smell" asks the exact smell question; 1 and 2 give the INT-004
/// text and hold, 3 continues; the same follow-up comes after a smell answer in any set.
#[test]
fn td_int_18_smell_answer_asks_what_kind_and_1_or_2_sends_the_911_text() {
    let bank = bank();
    for (reply, heard) in [
        ("1", Heard::Safety911),
        ("2", Heard::Safety911),
        ("3", Heard::Stored(Given::Option(3))),
    ] {
        let mut run = BankRun::new(
            &bank.sets["hvac_repair"],
            false,
            false,
            BankSettings::default(),
        );
        ask(&mut run, "", &AiTurn::default());
        run.hear("1");
        ask(&mut run, "", &AiTurn::default());
        run.hear("5");
        let smell = ask(&mut run, "", &AiTurn::default()).expect("smell question");
        assert_eq!(smell.render(), SMELL_TEXT);
        assert_eq!(run.hear(reply), heard, "{reply}");
    }
    let mut set = bank.sets["electrical"].clone();
    set.questions[0].options_en = Some(vec!["Strange noise or smell".into(), "Other".into()]);
    let mut run = BankRun::new(&set, false, false, BankSettings::default());
    ask(&mut run, "", &AiTurn::default());
    run.hear("1");
    assert_eq!(
        ask(&mut run, "", &AiTurn::default())
            .expect("smell")
            .render(),
        SMELL_TEXT
    );
}

/// TD-INT-18, INT-004: a smell answer and the smell list are never skipped, by keyword or by a
/// reading over the threshold, so answer 1 still gives the 911 text.
#[test]
fn td_int_18_smell_answers_are_never_skipped() {
    let bank = bank();
    let words = "Strange noise or smell like gas or rotten egg";
    let mut run = BankRun::new(
        &bank.sets["hvac_repair"],
        false,
        false,
        BankSettings::default(),
    );
    for (id, reply) in [("system_type", "1"), ("symptom", "5"), ("smell_kind", "1")] {
        let p = ask(&mut run, words, &AiTurn::default()).expect("asked");
        assert_eq!(p.id, id);
        let heard = run.hear(reply);
        assert_eq!(heard == Heard::Safety911, id == "smell_kind", "{id}");
    }
    let settings = BankSettings {
        ai_on: true,
        skip_threshold: Some(0.9),
    };
    let mut run = BankRun::new(&bank.sets["hvac_repair"], false, false, settings);
    for (id, answer, reply) in [
        ("system_type", 1, "1"),
        ("symptom", 5, "5"),
        ("smell_kind", 1, "1"),
    ] {
        let ai = AiTurn {
            reading: Some((id, answer, 1.0)),
            ..AiTurn::default()
        };
        if id == "system_type" {
            let p = ask(&mut run, WORDS, &AiTurn::default()).expect("asked");
            assert_eq!(p.id, id);
        } else {
            let p = ask(&mut run, WORDS, &ai).expect("asked");
            assert_eq!(p.id, id, "a sure reading never skips a smell answer");
        }
        let heard = run.hear(reply);
        assert_eq!(heard == Heard::Safety911, id == "smell_kind", "{id}");
    }
    let mut set = bank.sets["electrical"].clone();
    set.questions[0].options_en = Some(vec!["Strange noise or smell".into(), "Other".into()]);
    let first = set.questions[0].id.clone();
    let mut run = BankRun::new(&set, false, false, BankSettings::default());
    let p = ask(&mut run, "strange noise or smell", &AiTurn::default()).expect("asked");
    assert_eq!(p.id, first, "a smell answer of no stated kind is asked");
}

/// TD-INT-18: two smell answers in one set ask the smell question once (INT-039, D-1269).
#[test]
fn td_int_18_smell_question_is_asked_once() {
    let mut set = bank().sets["electrical"].clone();
    for q in &mut set.questions[..2] {
        q.options_en = Some(vec!["Strange noise or smell".into(), "Other".into()]);
        q.ask_if = None;
    }
    let mut run = BankRun::new(&set, false, false, BankSettings::default());
    let asked = walk(&mut run, "", &["1", "3", "1"]);
    let smells = asked.iter().filter(|id| *id == "smell_kind").count();
    assert_eq!(asked[1], "smell_kind");
    assert_eq!(smells, 1, "{asked:?}");
}

/// TD-INT-24: a boat "Fuel smell" answer sends nothing extra; "I smell gas" still matches INT-002.
#[test]
fn td_int_24_fuel_smell_sends_nothing_extra() {
    let bank = bank();
    let mut run = BankRun::new(
        &bank.sets["boat_repair"],
        false,
        false,
        BankSettings::default(),
    );
    ask(&mut run, "", &AiTurn::default());
    run.hear("2");
    assert_eq!(
        ask(&mut run, "", &AiTurn::default()).expect("symptom").id,
        "symptom"
    );
    assert_eq!(run.hear("5"), Heard::Stored(Given::Option(5)));
    let next = ask(&mut run, "", &AiTurn::default()).expect("the request continues");
    assert_ne!(next.id, "smell_kind");
    assert!(matches!(
        check("I smell gas near the boat", false),
        Check::Match(_)
    ));
}

fn scored(trade: &'static str, job_type: &'static str, confidence: f64) -> Scored<'static> {
    Scored {
        reading: Reading {
            trade,
            job_type: Some(job_type),
        },
        confidence,
    }
}

/// TD-INT-22 (INT-040): agreeing readings at high confidence ask nothing; a disagreement or a
/// reading below the setting asks the trade question offering both trades.
#[test]
fn td_int_22_agreement_at_high_confidence_asks_no_trade_question() {
    let kw = keyword_reading("my toilet keeps running");
    let agree = decide(
        &kw,
        Some(scored("plumbing_repair", "toilet repair", 0.95)),
        0.8,
    );
    assert!(matches!(agree, Decision::GoOn { ref job_type, .. } if job_type == "toilet repair"));
    let low = decide(
        &kw,
        Some(scored("plumbing_repair", "toilet repair", 0.79)),
        0.8,
    );
    assert!(matches!(low, Decision::Ask(_)));
    let Decision::Ask(q) = decide(
        &kw,
        Some(scored("hvac_repair", "AC not cooling", 0.99)),
        0.8,
    ) else {
        panic!("a disagreement asks");
    };
    assert_eq!(q.options, ["hvac_repair", "plumbing_repair"]);
    assert!(matches!(
        decide(&keyword_reading("hello there"), None, 0.8),
        Decision::NoTrade
    ));
}
