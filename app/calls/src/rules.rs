//! The pure call-log rules: who called, who answered, whether that is talking and whether it is a
//! business call attempt (CALL-007, CALL-008, D-141, D-1689, D-1690).
//!
//! A call answered by a person, in either direction and whatever its length, means the business
//! has talked to the homeowner; a machine answer never does; the 45-second mark is logged for
//! reports only and decides nothing; a call the homeowner makes is never a business call attempt.
//! `picks` reads the log through `calls::api::call_log` and hands it to `payments`' refund rules.

use jiff::SignedDuration;

/// CALL-007, D-141: the 45-second mark, logged for reports only; no rule reads it.
pub const REPORT_MARK: SignedDuration = SignedDuration::from_secs(45);

/// Who placed a finished call on a masked line (CALL-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// The business called the homeowner.
    Business,
    /// The homeowner called the business.
    Homeowner,
}

/// Who answered a finished call (CALL-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsweredBy {
    /// A person answered.
    Person,
    /// Voicemail, a fax or another machine answered.
    Machine,
    /// The call connected but nothing showed who answered: the provider's machine detection said
    /// `unknown` or sent no result (D-1690). Never counted as a person.
    Unknown,
    /// Nobody answered: busy, no answer, failed or cancelled.
    Nobody,
}

/// CALL-008, D-1690: who answered, from the voice provider's call status and answering-machine
/// result. Only a `completed` call was answered; only `human` is a person; any `machine_…` or
/// `fax` result is a machine; `unknown`, any other word or no result is [`AnsweredBy::Unknown`],
/// since only a detected person shows the business talked to the homeowner (CALL-007).
#[must_use]
pub fn answered_by(call_state: &str, detected: Option<&str>) -> AnsweredBy {
    if call_state != "completed" {
        return AnsweredBy::Nobody;
    }
    match detected {
        Some("human") => AnsweredBy::Person,
        Some(d) if d.starts_with("machine") || d == "fax" => AnsweredBy::Machine,
        _ => AnsweredBy::Unknown,
    }
}

/// CALL-007, D-141: a call answered by a person, either way and whatever its length, means the
/// business has talked to the homeowner; a machine, an unknown answerer or no answer never does.
#[must_use]
pub fn talked(answered: AnsweredBy) -> bool {
    answered == AnsweredBy::Person
}

/// CALL-007, D-141: only a call the business places is a business call attempt (FEE-022 counts
/// them); the homeowner's own calls never are.
#[must_use]
pub fn business_attempt(caller: Caller) -> bool {
    caller == Caller::Business
}

/// CALL-007, D-141: whether a call reached the 45-second mark; for reports only.
#[must_use]
pub fn reached_report_mark(length: SignedDuration) -> bool {
    length >= REPORT_MARK
}
