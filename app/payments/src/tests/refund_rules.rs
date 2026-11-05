//! The automatic refund rules case by case (TD-FEE-13, TD-FEE-24; FEE-022 to FEE-025, FEE-035,
//! CALL-007): attempt spacing, answered calls, replies, cancels and no-shows. Fake times only.

use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;

use crate::refund_rules::{
    Answer, CallRecord, Caller, CancelFacts, Contact, EmailCheck, NoShow, RefundDecision,
    RefundReason, Unmet, UnreachableFacts, decide_cancel, decide_unreachable, no_show,
    refund_label,
};

/// 9:00 a.m. Arizona time on a fake release day.
fn release() -> Timestamp {
    "2026-03-02T16:00:00Z".parse().expect("time")
}

fn at(hours: i64, secs: i64) -> Timestamp {
    release() + SignedDuration::from_hours(hours) + SignedDuration::from_secs(secs)
}

fn call(at: Timestamp, caller: Caller, answer: Answer) -> CallRecord {
    CallRecord { at, caller, answer }
}

fn facts(calls: &[CallRecord]) -> UnreachableFacts<'_> {
    UnreachableFacts {
        released_at: release(),
        emergency: false,
        calls,
        notice_attempted: true,
        done_or_problem: false,
        contact: Contact::Call,
        email_check: EmailCheck::NoReply,
        refunded_before: false,
    }
}

fn decide(f: &UnreachableFacts<'_>) -> RefundDecision {
    decide_unreachable(f, at(96, 0))
}

/// Two unanswered business attempts at 10:00 and `gap` later (FEE-022).
fn two(gap: i64, second: Answer) -> [CallRecord; 2] {
    [
        call(at(1, 0), Caller::Business, Answer::NoAnswer),
        call(at(1, gap), Caller::Business, second),
    ]
}

const UNREACHABLE: RefundDecision = RefundDecision::Refund(RefundReason::Unreachable);

/// TD-FEE-13 (FEE-022, D-141): 3:00:00 apart refunds at 96 hours, not before; 2:59:59 is one.
#[test]
fn td_fee_13_attempt_spacing() {
    let ok = two(3 * 3600, Answer::Machine);
    assert_eq!(decide(&facts(&ok)), UNREACHABLE);
    assert_eq!(
        decide_unreachable(&facts(&ok), at(95, 3599)),
        RefundDecision::NotYet
    );
    let close = two(3 * 3600 - 1, Answer::NoAnswer);
    assert_eq!(
        decide(&facts(&close)),
        RefundDecision::NoRefund(Unmet::AttemptsTooClose)
    );
    let late = [ok[0], call(at(73, 0), Caller::Business, Answer::NoAnswer)];
    assert_eq!(
        decide(&facts(&late)),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(1))
    );
}

/// TD-FEE-13 (FEE-022, PICK-018, D-440): a 9 p.m. attempt counts only on an emergency job.
#[test]
fn td_fee_13_window_hours() {
    let night = [
        ok_first(),
        call(at(12, 0), Caller::Business, Answer::NoAnswer),
    ];
    assert_eq!(
        decide(&facts(&night)),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(1))
    );
    let urgent = UnreachableFacts {
        emergency: true,
        ..facts(&night)
    };
    assert_eq!(decide(&urgent), UNREACHABLE);
}

fn ok_first() -> CallRecord {
    call(at(1, 0), Caller::Business, Answer::NoAnswer)
}

/// TD-FEE-13 (FEE-022, CALL-007, D-141): answered calls and the homeowner's own calls.
#[test]
fn td_fee_13_answered_calls() {
    let short = [
        call(at(1, 0), Caller::Business, Answer::Person),
        call(at(5, 0), Caller::Business, Answer::Person),
    ];
    assert_eq!(
        decide(&facts(&short)),
        RefundDecision::NoRefund(Unmet::Talked)
    );
    let mut theirs = two(3 * 3600, Answer::NoAnswer).to_vec();
    theirs.push(call(at(6, 0), Caller::Customer, Answer::Person));
    assert_eq!(
        decide(&facts(&theirs)),
        RefundDecision::NoRefund(Unmet::Talked)
    );
    let missed = [
        ok_first(),
        call(at(5, 0), Caller::Customer, Answer::NoAnswer),
    ];
    assert_eq!(
        decide(&facts(&missed)),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(1))
    );
}

/// TD-FEE-13, TD-FEE-24 (FEE-022, JOB-027, D-243, D-286, D-336): replies, the notice and once.
#[test]
fn td_fee_13_replies_notice_and_once() {
    let calls = two(4 * 3600, Answer::NoAnswer);
    let base = facts(&calls);
    let cases = [
        (
            UnreachableFacts {
                done_or_problem: true,
                ..base
            },
            Unmet::DoneOrProblemReply,
        ),
        (
            UnreachableFacts {
                refunded_before: true,
                ..base
            },
            Unmet::AlreadyRefunded,
        ),
        (
            UnreachableFacts {
                notice_attempted: false,
                ..base
            },
            Unmet::NoMissedCallNotice,
        ),
        (
            UnreachableFacts {
                contact: Contact::Email,
                email_check: EmailCheck::Yes,
                ..base
            },
            Unmet::EmailedAndAgreed,
        ),
    ];
    for (f, unmet) in cases {
        assert_eq!(decide(&f), RefundDecision::NoRefund(unmet));
    }
    let yes_on_call = UnreachableFacts {
        email_check: EmailCheck::Yes,
        ..base
    };
    assert_eq!(decide(&yes_on_call), UNREACHABLE);
    let pick = Uuid::nil();
    assert_eq!(refund_label(pick), refund_label(pick));
    assert_eq!(refund_label(pick), format!("pick-{pick}-refund"));
}

/// TD-FEE-24 (FEE-022, D-286): a failed missed-call text still counts as attempted (the facts
/// say attempted), and a business that never called is not refunded.
#[test]
fn td_fee_24_failed_text_and_no_calls() {
    let calls = two(3 * 3600, Answer::NoAnswer);
    let email = UnreachableFacts {
        contact: Contact::Email,
        ..facts(&calls)
    };
    assert_eq!(decide(&email), UNREACHABLE);
    assert_eq!(
        decide(&facts(&[])),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(0))
    );
}

fn cancel(charged: Option<Timestamp>, calls: &[CallRecord], refunded: bool) -> RefundDecision {
    decide_cancel(&CancelFacts {
        cancelled_at: at(10, 0),
        charged_at: charged,
        calls,
        refunded_before: refunded,
    })
}

/// TD-FEE-13 (FEE-023, FEE-024, FEE-025, FEE-026, D-048): cancel before and after talking.
#[test]
fn td_fee_13_cancel() {
    let paid = Some(release());
    let machine = [call(at(2, 0), Caller::Business, Answer::Machine)];
    let before_cancel = RefundDecision::Refund(RefundReason::CancelledBeforeTalk);
    assert_eq!(cancel(paid, &machine, false), before_cancel);
    let later = [call(at(11, 0), Caller::Business, Answer::Person)];
    assert_eq!(cancel(paid, &later, false), before_cancel);
    let spoke = [call(at(9, 0), Caller::Customer, Answer::Person)];
    assert_eq!(
        cancel(paid, &spoke, false),
        RefundDecision::NoRefund(Unmet::Talked)
    );
    let after = RefundDecision::Refund(RefundReason::ChargedAfterCancel);
    assert_eq!(cancel(Some(at(10, 1)), &spoke, false), after);
    assert_eq!(
        cancel(paid, &[], true),
        RefundDecision::NoRefund(Unmet::AlreadyRefunded)
    );
    assert_eq!(
        cancel(None, &[], false),
        RefundDecision::NoRefund(Unmet::NothingCharged)
    );
}

/// TD-FEE-13 (FEE-035, D-047, D-143): no-shows by contact method; never a refund.
#[test]
fn td_fee_13_no_show() {
    let none: [CallRecord; 0] = [];
    let ns =
        |contact, calls: &[CallRecord], day3, now| no_show(contact, release(), calls, day3, now);
    assert_eq!(
        ns(Contact::Call, &none, EmailCheck::NoReply, at(71, 3599)),
        NoShow::NotYet
    );
    assert_eq!(
        ns(Contact::Call, &none, EmailCheck::NoReply, at(72, 0)),
        NoShow::Recorded
    );
    let rang = [call(at(71, 0), Caller::Business, Answer::NoAnswer)];
    assert_eq!(
        ns(Contact::Call, &rang, EmailCheck::NoReply, at(80, 0)),
        NoShow::No
    );
    let theirs = [call(at(5, 0), Caller::Customer, Answer::Person)];
    assert_eq!(
        ns(Contact::Call, &theirs, EmailCheck::NoReply, at(80, 0)),
        NoShow::Recorded
    );
    assert_eq!(
        ns(Contact::Email, &none, EmailCheck::NoReply, at(80, 0)),
        NoShow::No
    );
    assert_eq!(
        ns(Contact::Email, &none, EmailCheck::No, at(80, 0)),
        NoShow::Recorded
    );
    assert_eq!(
        decide(&facts(&none)),
        RefundDecision::NoRefund(Unmet::TooFewAttempts(0))
    );
}
