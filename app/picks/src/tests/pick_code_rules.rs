//! The pure pick-code and text-pick rules (TD-PICK-06, TD-PICK-07, TD-PICK-14, TD-PICK-40; PICK-013,
//! PICK-014, PICK-015, PICK-016, PICK-031, PICK-032, PICK-036, PICK-055, CON-029). Fake IDs only.

use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;

use crate::rules::{
    CodeRow, CodeState, CodeVia, JobView, Plan, SendAsk, SendCounts, SendRefused, Stage, TextPick,
    code_state, parse_code, plan_text_pick, read_text_pick, send_allowed,
};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + seconds).expect("time")
}

fn sent(tries: i32, last: Option<i64>) -> CodeRow {
    CodeRow {
        sent_at: at(0),
        tries,
        last_try_at: last.map(at),
        verified: false,
    }
}

/// TD-PICK-06 (PICK-013, D-101): valid at 9:59, refused at exactly 10:00 and at 10:01.
#[test]
fn td_pick_06_code_valid_at_959_refused_at_1000_and_1001() {
    assert_eq!(
        code_state(&sent(0, None), at(599)),
        CodeState::Open { left: 5 }
    );
    assert_eq!(code_state(&sent(0, None), at(600)), CodeState::Expired);
    assert_eq!(code_state(&sent(0, None), at(601)), CodeState::Expired);
    let used = CodeRow {
        verified: true,
        ..sent(1, Some(10))
    };
    assert_eq!(code_state(&used, at(20)), CodeState::Used);
}

/// TD-PICK-06 (PICK-013, D-015): the fifth wrong try starts a 15-minute cooldown from that try.
#[test]
fn td_pick_06_five_wrong_tries_then_a_15_minute_cooldown() {
    assert_eq!(
        code_state(&sent(4, Some(60)), at(61)),
        CodeState::Open { left: 1 }
    );
    let until = at(60) + SignedDuration::from_mins(15);
    let five = sent(5, Some(60));
    assert_eq!(code_state(&five, at(61)), CodeState::CoolingDown { until });
    assert_eq!(code_state(&five, at(959)), CodeState::CoolingDown { until });
    assert_eq!(code_state(&five, at(960)), CodeState::Expired);
}

/// TD-PICK-06 (PICK-014): a pasted or autofilled code with spaces or a dash reads; anything but
/// six digits does not, and the digits never show in `Debug`.
#[test]
fn td_pick_06_paste_and_autofill_forms_read() {
    for ok in ["123456", " 123 456 ", "123-456", "1 2 3 4 5 6"] {
        assert_eq!(parse_code(ok).expect(ok).digits(), "123456");
    }
    for bad in ["12345", "1234567", "12a456", "", "١٢٣٤٥٦"] {
        assert!(parse_code(bad).is_none(), "{bad}");
    }
    assert!(!format!("{:?}", parse_code("123456")).contains("123456"));
}

fn ask(via: CodeVia, own_session: bool, local_hour: i8) -> SendAsk {
    SendAsk {
        via,
        own_session,
        local_hour,
    }
}

/// PICK-055 (D-480): 5 per number, 3 calls per number, 10 per job, no voice code from 9 p.m. to
/// 8 a.m. unless from the phone's own session, and nothing at or above the global threshold.
#[test]
fn pick_055_verify_send_caps() {
    let text = ask(CodeVia::Text, false, 12);
    let call = ask(CodeVia::CallTapped, false, 12);
    let n = |to_number, calls_to_number, for_job, global| SendCounts {
        to_number,
        calls_to_number,
        for_job,
        global,
    };
    assert_eq!(send_allowed(text, n(4, 0, 9, 99), 100), Ok(()));
    assert_eq!(
        send_allowed(text, n(5, 0, 5, 0), 100),
        Err(SendRefused::NumberCap)
    );
    assert_eq!(
        send_allowed(text, n(0, 0, 10, 0), 100),
        Err(SendRefused::JobCap)
    );
    assert_eq!(
        send_allowed(call, n(3, 3, 3, 0), 100),
        Err(SendRefused::CallCap)
    );
    assert_eq!(send_allowed(text, n(3, 3, 3, 0), 100), Ok(()));
    assert_eq!(
        send_allowed(text, n(0, 0, 0, 100), 100),
        Err(SendRefused::GlobalCap)
    );
    for hour in [21, 23, 0, 7] {
        let night = ask(CodeVia::CallTapped, false, hour);
        assert_eq!(
            send_allowed(night, n(0, 0, 0, 0), 100),
            Err(SendRefused::CallQuietHours)
        );
        let own = ask(CodeVia::CallTapped, true, hour);
        assert_eq!(send_allowed(own, n(0, 0, 0, 0), 100), Ok(()));
        assert_eq!(
            send_allowed(ask(CodeVia::Text, false, hour), n(0, 0, 0, 0), 100),
            Ok(())
        );
    }
    for hour in [8, 20] {
        assert_eq!(
            send_allowed(ask(CodeVia::CallTapped, false, hour), n(0, 0, 0, 0), 100),
            Ok(())
        );
    }
}

/// TD-PICK-07 (PICK-015, D-290): "2 YES", "2 yes please", "2 SÍ", "2 si" and "1042 2 YES" read;
/// other replies do not.
#[test]
fn td_pick_07_text_pick_forms() {
    let two = Some(TextPick {
        job: None,
        quote: 2,
    });
    for words in [
        "2 YES",
        "2 yes please",
        "2 SÍ",
        "2 sí",
        "2 si",
        "2 Yes!",
        " 2   yes ",
    ] {
        assert_eq!(read_text_pick(words), two, "{words}");
    }
    let named = TextPick {
        job: Some(1042),
        quote: 2,
    };
    assert_eq!(read_text_pick("1042 2 YES"), Some(named));
    for words in [
        "YES",
        "2",
        "yes 2",
        "0 YES",
        "2 no",
        "2 yesterday",
        "two yes",
        "1042 2",
    ] {
        assert_eq!(read_text_pick(words), None, "{words}");
    }
}

fn job(n: u128, stage: Stage) -> JobView {
    JobView {
        job: Uuid::from_u128(n),
        number: 1000 + i64::try_from(n).expect("small"),
        stage,
        paused: false,
        terms_sent: true,
        quotes: vec![true, true, false],
        waiting: None,
    }
}

fn pick(quote: usize) -> TextPick {
    TextPick { job: None, quote }
}

/// TD-PICK-07 (PICK-031, D-068): one job in Quotes ready picks; two ask which, and a named request
/// number then picks on that one.
#[test]
fn td_pick_07_one_job_picks_two_ask_which() {
    let one = [job(1, Stage::Ready), job(2, Stage::Collecting)];
    assert_eq!(plan_text_pick(pick(2), &one), Plan::Pick(one[0].job));
    let two = [job(1, Stage::Ready), job(2, Stage::Waiting)];
    assert_eq!(plan_text_pick(pick(2), &two), Plan::Which(vec![1001, 1002]));
    let named = TextPick {
        job: Some(1002),
        quote: 1,
    };
    assert_eq!(plan_text_pick(named, &two), Plan::Pick(two[1].job));
    let unknown = TextPick {
        job: Some(1009),
        quote: 1,
    };
    assert_eq!(plan_text_pick(unknown, &two), Plan::Which(vec![1001, 1002]));
    assert_eq!(plan_text_pick(pick(3), &one), Plan::NoQuote(one[0].job));
    assert_eq!(plan_text_pick(pick(4), &one), Plan::NoQuote(one[0].job));
}

/// TD-PICK-14 (PICK-032, PICK-036, D-147): another quote while a pick waits replaces it, the same
/// one repeats; with nothing waiting for a pick the status reply goes.
#[test]
fn td_pick_14_replace_same_and_status() {
    let mut waiting = job(1, Stage::Waiting);
    waiting.waiting = Some(1);
    let jobs = [waiting.clone()];
    assert_eq!(plan_text_pick(pick(2), &jobs), Plan::Replace(waiting.job));
    assert_eq!(plan_text_pick(pick(1), &jobs), Plan::Same(waiting.job));
    let none = [
        job(1, Stage::Accepted),
        job(2, Stage::Collecting),
        job(3, Stage::Final),
    ];
    assert_eq!(plan_text_pick(pick(2), &none), Plan::Status);
    assert_eq!(plan_text_pick(pick(2), &[]), Plan::Status);
}

/// TD-PICK-40 (PICK-016) and TD-CON-12 (CON-029): with no Terms line sent the line goes first; a
/// paused job takes no pick.
#[test]
fn td_pick_40_terms_line_first_and_td_con_12_pause_blocks() {
    let mut fresh = job(1, Stage::Ready);
    fresh.terms_sent = false;
    assert_eq!(
        plan_text_pick(pick(1), &[fresh.clone()]),
        Plan::Terms(fresh.job)
    );
    let mut paused = job(1, Stage::Waiting);
    paused.paused = true;
    assert_eq!(
        plan_text_pick(pick(1), &[paused.clone()]),
        Plan::Paused(paused.job)
    );
}

/// TD-CON-12 (PICK-031, CON-029): a paused job does not count toward "exactly one job", so the one
/// job not paused picks without a which-request question; with two not paused the question lists
/// only those; a named paused job is still refused.
#[test]
fn td_con_12_a_paused_job_is_not_counted_as_open() {
    let mut paused = job(1, Stage::Waiting);
    paused.paused = true;
    let other = job(2, Stage::Ready);
    let both = [paused.clone(), other.clone()];
    assert_eq!(plan_text_pick(pick(1), &both), Plan::Pick(other.job));
    let named = TextPick {
        job: Some(paused.number),
        quote: 1,
    };
    assert_eq!(plan_text_pick(named, &both), Plan::Paused(paused.job));
    let three = [paused, other, job(3, Stage::Ready)];
    assert_eq!(
        plan_text_pick(pick(1), &three),
        Plan::Which(vec![1002, 1003])
    );
}
