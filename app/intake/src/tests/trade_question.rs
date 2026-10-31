//! The trade question, word answers, the INT-010 after-window word share and INT-032's two trades
//! (TD-INT-04, TD-INT-35: INT-010, INT-012, INT-013, INT-015, INT-032). Pure cases, fake text only.

use jiff::{SignedDuration, Timestamp};

use crate::api::{
    Opinion, Reading, TradeAnswer, TradeQuestion, TradeStep, keyword_reading, second_opinion,
    starts_second, two_trades,
};
use crate::rules::{JOIN_SHARE_PERCENT, JOIN_WINDOW, follow_up_joins, shares_words};

fn question(options: &[&str], fallback: &str) -> TradeQuestion {
    TradeQuestion {
        options: options.iter().map(|o| (*o).to_owned()).collect(),
        fallback: fallback.to_owned(),
    }
}

fn hvac_or_handyman() -> TradeQuestion {
    question(&["hvac_repair", "handyman"], "hvac_repair")
}

fn trade(t: &str) -> TradeAnswer {
    TradeAnswer::Trade(t.to_owned())
}

/// TD-INT-04 (INT-012, D-103): "just a small repair" chooses handyman; "repair" alone, and a
/// word that only contains "air", never match heating/AC.
#[test]
fn td_int_04_word_answers_match_whole_words_only() {
    let q = hvac_or_handyman();
    assert_eq!(q.read("just a small repair"), trade("handyman"));
    assert_eq!(q.read("repair"), TradeAnswer::Unusable);
    assert_eq!(q.read("the repairman said so"), TradeAnswer::Unusable);
    assert_eq!(q.read("the AC one"), trade("hvac_repair"));
    assert_eq!(q.read("Heating!"), trade("hvac_repair"));
    assert_eq!(q.read("2"), trade("handyman"));
    assert_eq!(q.read("option #1 please"), trade("hvac_repair"));
    assert_eq!(q.read("3"), TradeAnswer::Unusable);
    assert_eq!(q.read("1 or 2"), TradeAnswer::Unusable);
    assert_eq!(q.read("AC or handyman"), TradeAnswer::Unusable);
    assert_eq!(q.read("Not sure"), TradeAnswer::NotSure);
    assert_eq!(q.read("no sé"), TradeAnswer::NotSure);
}

/// TD-INT-04 (INT-015, D-103): the re-ask offers the same numbered options; a second unusable
/// reply applies the fallback and intake moves on, so the question is never asked a third time.
#[test]
fn td_int_04_reask_repeats_options_then_the_fallback_applies() {
    let readings = [
        Reading {
            trade: "electrical",
            job_type: None,
        },
        Reading {
            trade: "plumbing_repair",
            job_type: None,
        },
    ];
    let kw = keyword_reading("my thermostat is blank");
    let Opinion::Ask(q) = second_opinion(readings, &kw) else {
        panic!("the readings differ, so the trade is in doubt");
    };
    assert_eq!(q.options, ["electrical", "plumbing_repair", "hvac_repair"]);
    assert_eq!(
        q.step(q.read("hmm"), 1),
        TradeStep::AskAgain(q.options.clone()),
        "the re-ask repeats the numbered options"
    );
    assert_eq!(
        q.step(q.read("whatever"), 2),
        TradeStep::Fallback("hvac_repair".to_owned()),
        "exactly one keyword trade matched, so it is the fallback"
    );
    assert_eq!(
        q.step(q.read("not sure"), 1),
        TradeStep::Fallback("hvac_repair".to_owned())
    );
    assert_eq!(
        q.step(q.read("3"), 1),
        TradeStep::Chosen("hvac_repair".to_owned())
    );
}

/// TD-INT-04 (INT-015, D-103): with no keyword trade, or more than one, the fallback is the first
/// reading.
#[test]
fn td_int_04_fallback_is_the_first_reading_without_exactly_one_keyword_trade() {
    let readings = [
        Reading {
            trade: "electrical",
            job_type: None,
        },
        Reading {
            trade: "handyman",
            job_type: None,
        },
    ];
    for words in ["something is wrong", "the faucet and the furnace"] {
        let Opinion::Ask(q) = second_opinion(readings, &keyword_reading(words)) else {
            panic!("the readings differ");
        };
        assert_eq!(
            q.step(TradeAnswer::Unusable, 2),
            TradeStep::Fallback("electrical".to_owned())
        );
    }
}

/// TD-INT-04 (INT-013, D-104): agreeing readings with no other keyword trade settle the trade;
/// otherwise the question offers every distinct trade, up to three.
#[test]
fn td_int_04_question_only_when_the_trade_is_in_doubt() {
    let both = |t| {
        [Reading {
            trade: t,
            job_type: None,
        }; 2]
    };
    let settled = second_opinion(both("hvac_repair"), &keyword_reading("AC is warm"));
    assert_eq!(settled, Opinion::Settled("hvac_repair".to_owned()));
    let quiet = second_opinion(both("handyman"), &keyword_reading("odd noise"));
    assert_eq!(quiet, Opinion::Settled("handyman".to_owned()));
    let kw = keyword_reading("faucet, outlet and door");
    let Opinion::Ask(q) = second_opinion(both("hvac_repair"), &kw) else {
        panic!("the keywords name other trades");
    };
    assert_eq!(q.options, ["hvac_repair", "plumbing_repair", "electrical"]);
}

fn at(seconds: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + SignedDuration::from_secs(seconds)
}

fn numbered(range: std::ops::Range<usize>) -> String {
    range.map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ")
}

/// TD-INT-35 (INT-010, D-485): after the window a same-trade message joins only when at least
/// half of its words, common words left out, are in the description: 5 of 10 joins, 49 of 100
/// opens a new job; one at exactly 2:00:00 follows that rule, and inside the window anything joins.
#[test]
fn td_int_35_after_two_hours_the_word_share_decides() {
    let description = numbered(0..49);
    let ten = format!("the {} my {}", numbered(0..5), numbered(200..205));
    assert!(shares_words(&ten, &description, JOIN_SHARE_PERCENT));
    let hundred = format!("{} {}", numbered(0..49), numbered(300..351));
    assert!(!shares_words(&hundred, &description, JOIN_SHARE_PERCENT));
    assert!(!shares_words("the my is", "the my is", JOIN_SHARE_PERCENT));
    let two_hours = at(7200);
    let joins = |now, same, msg: &str| {
        follow_up_joins(
            at(0),
            now,
            JOIN_WINDOW,
            same,
            msg,
            &description,
            JOIN_SHARE_PERCENT,
        )
    };
    assert!(joins(two_hours, true, &ten));
    assert!(!joins(two_hours, true, &hundred));
    assert!(
        !joins(two_hours, false, &ten),
        "another trade after the window opens a new job"
    );
    assert!(joins(at(7199), false, "different trade words"));
}

/// TD-INT-35 (INT-032, D-493): "Leaky faucet and broken outlet" starts plumbing and offers
/// electrical through the "That's 2 jobs" reply; ALSO or TAMBIÉN alone starts the second job.
#[test]
fn td_int_35_two_trades_and_the_also_reply() {
    assert_eq!(
        two_trades("Leaky faucet and broken outlet"),
        Some(("plumbing_repair", "electrical"))
    );
    assert_eq!(two_trades("Leaky faucet under the sink"), None);
    assert!(starts_second("ALSO"));
    assert!(starts_second("También!"));
    assert!(starts_second(" also "));
    assert!(!starts_second("also the door"));
    assert!(!starts_second("ok"));
}
