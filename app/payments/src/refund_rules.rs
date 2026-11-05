//! Automatic refund rules for an introduced pick, judged from the call log (FEE-022 to FEE-025,
//! FEE-035, CALL-007, D-047, D-048, D-049, D-141, D-143, D-243, D-286, D-336, D-440).
//!
//! Pure: facts in, a [`RefundDecision`] out; nothing here talks to Stripe or the database.
//! `picks` gathers the facts (the call log, replies, the cancel) and calls `payments::api`
//! (D-491). Talking means any call in either direction answered by a person, whatever its length;
//! a machine answer never counts and the homeowner's own calls are never business attempts
//! (CALL-007, D-141). Each pick is refunded at most once: a pick already refunded is never
//! refunded again, and every refund of a pick carries the one [`refund_label`] (FEE-026). A
//! no-show is never refunded (FEE-035, D-047); [`no_show`] only says whether one is recorded.

use askhava_core::api::is_business_hours;
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;

/// FEE-022: business attempts count only inside this long after the release.
pub const ATTEMPT_WINDOW: SignedDuration = SignedDuration::from_hours(72);
/// FEE-022: the unreachable refund is decided this long after the release.
pub const DECIDE_AFTER: SignedDuration = SignedDuration::from_hours(96);
/// FEE-022: the first and last counted attempts must be at least this far apart.
pub const ATTEMPT_GAP: SignedDuration = SignedDuration::from_hours(3);

/// Who placed a logged call (CALL-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// The business called the homeowner: a business call attempt.
    Business,
    /// The person who asked for the work (the homeowner) called the business: never a business
    /// attempt (D-141). Named for the role, as `payments` code never names a homeowner (FEE-009).
    Customer,
}

/// How a logged call was answered (CALL-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// A person answered: the business has talked to the homeowner, whatever the length.
    Person,
    /// Voicemail or another machine answered: never counts as talking (D-141).
    Machine,
    /// Nobody answered.
    NoAnswer,
}

/// One row of the pick's call log: when, who called, who answered (BIZ-035, CALL-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallRecord {
    /// When the call started.
    pub at: Timestamp,
    /// Who placed it.
    pub caller: Caller,
    /// How it was answered.
    pub answer: Answer,
}

/// CALL-007, D-141: whether any call in `calls`, in either direction, was answered by a person.
#[must_use]
pub fn talked(calls: &[CallRecord]) -> bool {
    calls.iter().any(|c| c.answer == Answer::Person)
}

/// The contact method the homeowner chose at the pick (D-049, D-143).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    /// The homeowner wants a call.
    Call,
    /// The homeowner prefers email; the masked line still opens (D-049).
    Email,
}

/// The homeowner's reply to the 72-hour email question or the day-3 check-in (JOB-027, D-143).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailCheck {
    /// No reply yet.
    NoReply,
    /// YES: the business emailed and agreed a time (stops the unreachable refund, D-243).
    Yes,
    /// NO: the business has not reached the homeowner.
    No,
}

/// Why an automatic refund is owed (FEE-022, FEE-023, FEE-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundReason {
    /// FEE-022: 2 qualifying attempts, nobody answered, the homeowner was told.
    Unreachable,
    /// FEE-023, FEE-024: the homeowner cancelled before the business talked to them.
    CancelledBeforeTalk,
    /// FEE-025: the charge was taken after the homeowner cancelled.
    ChargedAfterCancel,
}

/// The condition that was not met, for the decision email to the business (FEE-022, D-336).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmet {
    /// FEE-026: this pick was already refunded; a refund happens once.
    AlreadyRefunded,
    /// Nothing was charged for this pick, so there is nothing to refund.
    NothingCharged,
    /// CALL-007: a call in either direction was answered by a person.
    Talked,
    /// FEE-022: fewer than 2 business attempts were inside the window (0 or 1 counted).
    TooFewAttempts(u8),
    /// FEE-022: the counted attempts were less than 3 hours apart.
    AttemptsTooClose,
    /// FEE-022, D-286: Ask Hava neither sent nor attempted the missed-call text or its email.
    NoMissedCallNotice,
    /// FEE-022: the homeowner replied DONE or PROBLEM.
    DoneOrProblemReply,
    /// FEE-022, JOB-027, D-243: an email-contact homeowner replied YES.
    EmailedAndAgreed,
}

/// The outcome of a refund rule (FEE-022 to FEE-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundDecision {
    /// FEE-022: not 96 hours since the release yet; ask again later.
    NotYet,
    /// Refund the pick's full charge once, keyed by [`refund_label`] (FEE-026).
    Refund(RefundReason),
    /// No refund, and the condition that failed (D-336).
    NoRefund(Unmet),
}

/// The facts FEE-022 judges at 96 hours, gathered by `picks` (D-491).
#[derive(Debug, Clone, Copy)]
pub struct UnreachableFacts<'a> {
    /// When the contact details were released to the business.
    pub released_at: Timestamp,
    /// PICK-018, D-440: an emergency job counts attempts at any hour.
    pub emergency: bool,
    /// The pick's call log, both directions.
    pub calls: &'a [CallRecord],
    /// D-286: the TXT-030 text or its email copy was sent or attempted (a failed delivery counts).
    pub notice_attempted: bool,
    /// The homeowner replied DONE or PROBLEM.
    pub done_or_problem: bool,
    /// The homeowner's contact choice (D-049).
    pub contact: Contact,
    /// The reply to the 72-hour email question (JOB-027); read only for email contact.
    pub email_check: EmailCheck,
    /// FEE-026: the pick has already been refunded.
    pub refunded_before: bool,
}

/// FEE-022, D-141, D-440: the business attempts that count, oldest first: business calls not
/// answered by a person, inside 72 hours of the release and 8 a.m. to 8 p.m. Arizona time
/// (any hour when `emergency`).
#[must_use]
pub fn counted_attempts(
    calls: &[CallRecord],
    released_at: Timestamp,
    emergency: bool,
) -> Vec<Timestamp> {
    let end = released_at
        .saturating_add(ATTEMPT_WINDOW)
        .unwrap_or(Timestamp::MAX);
    let mut times: Vec<Timestamp> = calls
        .iter()
        .filter(|c| c.caller == Caller::Business && c.answer != Answer::Person)
        .map(|c| c.at)
        .filter(|at| *at >= released_at && *at <= end)
        .filter(|at| emergency || is_business_hours(*at))
        .collect();
    times.sort();
    times
}

/// FEE-022: the attempt test alone, `None` when 2 counted attempts are at least 3 hours apart.
fn attempts_unmet(attempts: &[Timestamp]) -> Option<Unmet> {
    match (attempts.first(), attempts.last()) {
        (Some(first), Some(last)) if attempts.len() >= 2 => {
            (last.duration_since(*first) < ATTEMPT_GAP).then_some(Unmet::AttemptsTooClose)
        }
        _ => Some(Unmet::TooFewAttempts(
            u8::try_from(attempts.len()).unwrap_or(u8::MAX),
        )),
    }
}

/// FEE-022, FEE-026, D-141, D-243, D-286, D-336: the unreachable-homeowner refund, decided at 96
/// hours after the release. Checks run in a fixed order and the first that fails is named.
#[must_use]
pub fn decide_unreachable(facts: &UnreachableFacts<'_>, now: Timestamp) -> RefundDecision {
    let due = facts
        .released_at
        .saturating_add(DECIDE_AFTER)
        .unwrap_or(Timestamp::MAX);
    if now < due {
        return RefundDecision::NotYet;
    }
    let attempts = counted_attempts(facts.calls, facts.released_at, facts.emergency);
    let unmet = if facts.refunded_before {
        Some(Unmet::AlreadyRefunded)
    } else if talked(facts.calls) {
        Some(Unmet::Talked)
    } else if facts.done_or_problem {
        Some(Unmet::DoneOrProblemReply)
    } else if facts.contact == Contact::Email && facts.email_check == EmailCheck::Yes {
        Some(Unmet::EmailedAndAgreed)
    } else if let Some(failed) = attempts_unmet(&attempts) {
        Some(failed)
    } else if !facts.notice_attempted {
        Some(Unmet::NoMissedCallNotice)
    } else {
        None
    };
    unmet.map_or(
        RefundDecision::Refund(RefundReason::Unreachable),
        RefundDecision::NoRefund,
    )
}

/// The facts of a homeowner's cancel, gathered by `picks` (FEE-023 to FEE-025).
#[derive(Debug, Clone, Copy)]
pub struct CancelFacts<'a> {
    /// When the homeowner cancelled.
    pub cancelled_at: Timestamp,
    /// When the pick's charge succeeded; `None` when nothing was charged ($0 or not yet).
    pub charged_at: Option<Timestamp>,
    /// The pick's call log, both directions.
    pub calls: &'a [CallRecord],
    /// FEE-026: the pick has already been refunded.
    pub refunded_before: bool,
}

/// FEE-023, FEE-024, FEE-025, FEE-026, D-048, D-141: the refund on a homeowner's cancel. A charge
/// taken after the cancel is always refunded; otherwise the business is refunded only if no call
/// in either direction was answered by a person before the cancel.
#[must_use]
pub fn decide_cancel(facts: &CancelFacts<'_>) -> RefundDecision {
    let Some(charged_at) = facts.charged_at else {
        return RefundDecision::NoRefund(Unmet::NothingCharged);
    };
    if facts.refunded_before {
        return RefundDecision::NoRefund(Unmet::AlreadyRefunded);
    }
    if charged_at > facts.cancelled_at {
        return RefundDecision::Refund(RefundReason::ChargedAfterCancel);
    }
    let before: Vec<CallRecord> = facts
        .calls
        .iter()
        .copied()
        .filter(|c| c.at <= facts.cancelled_at)
        .collect();
    if talked(&before) {
        RefundDecision::NoRefund(Unmet::Talked)
    } else {
        RefundDecision::Refund(RefundReason::CancelledBeforeTalk)
    }
}

/// Whether a no-show is recorded (FEE-035, D-047, D-143).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoShow {
    /// Too early to judge: under 72 hours since the release, call contact.
    NotYet,
    /// A no-show: close the call line, record it, alert Casey; never refunded (D-047).
    Recorded,
    /// Not a no-show.
    No,
}

/// FEE-035, D-047, D-143: call contact is a no-show when the business placed no call at all
/// within 72 hours of the release; email contact only when the homeowner replies NO at the day-3
/// check-in. A no-show is never refunded.
#[must_use]
pub fn no_show(
    contact: Contact,
    released_at: Timestamp,
    calls: &[CallRecord],
    day3: EmailCheck,
    now: Timestamp,
) -> NoShow {
    let end = released_at
        .saturating_add(ATTEMPT_WINDOW)
        .unwrap_or(Timestamp::MAX);
    match contact {
        Contact::Email if day3 == EmailCheck::No => NoShow::Recorded,
        Contact::Email => NoShow::No,
        Contact::Call if now < end => NoShow::NotYet,
        Contact::Call => {
            let called = calls
                .iter()
                .any(|c| c.caller == Caller::Business && c.at >= released_at && c.at <= end);
            if called { NoShow::No } else { NoShow::Recorded }
        }
    }
}

/// FEE-026: the one label every refund of a pick is keyed by, `pick-<id>-refund`, so a repeated
/// decision reuses it and Stripe refunds the pick once. No personal data.
#[must_use]
pub fn refund_label(pick: Uuid) -> String {
    format!("pick-{pick}-refund")
}
