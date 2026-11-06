//! The masked call's TwiML and its finished-call log (CALL-006, CALL-007, CALL-008, D-1689 to
//! D-1691).
//!
//! [`dial_twiml`] is the one TwiML that bridges a masked call: it always says `do-not-record` and
//! holds no recording verb (CALL-006). [`finished_call`] reads the provider's verified status
//! callback; [`log_finished_call`] appends it to `calls.call_log` once per call SID, so a repeated
//! callback logs nothing twice (JOB-006); [`call_log`] reads a line's log back for `picks`.

use askhava_adapters::api::PoolNumber;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::CallsError;
use crate::rules::{AnsweredBy, Caller, answered_by, reached_report_mark};

/// CALL-006, CALL-008: the TwiML that dials `to` from the pool number `caller_id`, never recorded,
/// with the provider's machine detection so the log can say who answered. T080 adds the dialed
/// leg's final status callback, which carries that result (D-1690). `None` when `to` is not
/// E.164, so nothing but digits reaches the XML. The result holds a phone number: send it only as
/// the webhook's reply, never log it (PRIV-016).
#[must_use]
pub fn dial_twiml(caller_id: &PoolNumber, to: &SecretString) -> Option<SecretString> {
    let to = to.expose_secret();
    let digits = to.strip_prefix('+')?;
    if !(8..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let from = caller_id.as_str();
    Some(SecretString::from(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Dial callerId=\"{from}\" \
         record=\"do-not-record\"><Number machineDetection=\"Enable\">{to}</Number></Dial>\
         </Response>"
    )))
}

/// One finished masked call: its line, who called, who answered and its length (CALL-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedCall {
    /// The `calls.number_pool` line it was on.
    pub line: Uuid,
    /// The provider's call SID (`CA…`), one log row per call.
    pub call_sid: String,
    /// Who placed it.
    pub caller: Caller,
    /// Who answered it.
    pub answered: AnsweredBy,
    /// How long it lasted, in whole seconds; unsigned, so it always fits the log's `bigint`.
    pub length_seconds: u32,
    /// When it ended.
    pub ended_at: Timestamp,
}

impl FinishedCall {
    /// How long it lasted (CALL-008).
    #[must_use]
    pub fn length(&self) -> SignedDuration {
        SignedDuration::from_secs(i64::from(self.length_seconds))
    }
}

/// CALL-008, D-1690: the finished call in a verified status callback's `params`, on `line`, placed
/// by `caller` (the voice handler knows the party). `params` must be the dialed leg's own final
/// callback (its SID, status, length and `AnsweredBy`), never the first leg's. `None` unless it
/// carries a `CA…` SID, a final status and a whole-second `CallDuration` (missing on an unanswered
/// call: 0).
#[must_use]
pub fn finished_call(
    line: Uuid,
    caller: Caller,
    params: &[(String, String)],
    ended_at: Timestamp,
) -> Option<FinishedCall> {
    let get = |key: &str| {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let sid = get("CallSid")?;
    let tail = sid.strip_prefix("CA")?;
    if tail.len() != 32 || !tail.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let status = get("CallStatus")?;
    let finals = ["completed", "busy", "no-answer", "failed", "canceled"];
    if !finals.contains(&status) {
        return None;
    }
    let seconds: u32 = get("CallDuration").map_or(Some(0), |d| d.parse().ok())?;
    Some(FinishedCall {
        line,
        call_sid: sid.to_owned(),
        caller,
        answered: answered_by(status, get("AnsweredBy")),
        length_seconds: seconds,
        ended_at,
    })
}

/// CALL-007, CALL-008: appends `call` to `calls.call_log` with the 45-second mark (reports only);
/// `true` when logged, `false` when its SID was already logged (safe to run twice, JOB-006).
///
/// # Errors
///
/// [`CallsError::Database`] when the insert fails, an unknown line included.
pub async fn log_finished_call(
    db: &mut PgConnection,
    call: &FinishedCall,
) -> Result<bool, CallsError> {
    let done = sqlx::query!(
        "INSERT INTO calls.call_log (line_id, call_sid, caller, answered_by, length_seconds, \
         reached_45s, ended_at) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (call_sid) DO NOTHING",
        call.line,
        call.call_sid,
        caller_word(call.caller),
        answered_word(call.answered),
        i64::from(call.length_seconds),
        reached_report_mark(call.length()),
        call.ended_at.to_sqlx()
    )
    .execute(db)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// One logged call as read back (CALL-008): who called, who answered, length, the 45-second mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoggedCall {
    /// Who placed it.
    pub caller: Caller,
    /// Who answered it.
    pub answered: AnsweredBy,
    /// How long it lasted.
    pub length: SignedDuration,
    /// Whether it reached the 45-second mark; reports only (CALL-007).
    pub reached_45s: bool,
    /// When it ended.
    pub ended_at: Timestamp,
}

/// CALL-008: the line's logged calls, oldest first.
///
/// # Errors
///
/// [`CallsError::Database`] when the read fails or a row holds a word this code does not know.
pub async fn call_log(db: &mut PgConnection, line: Uuid) -> Result<Vec<LoggedCall>, CallsError> {
    let rows = sqlx::query!(
        "SELECT caller, answered_by, length_seconds, reached_45s, ended_at FROM calls.call_log \
         WHERE line_id = $1 ORDER BY ended_at, id",
        line
    )
    .fetch_all(db)
    .await
    .map_err(|_| CallsError::Database)?;
    rows.into_iter()
        .map(|r| {
            Ok(LoggedCall {
                caller: parse_caller(&r.caller).ok_or(CallsError::Database)?,
                answered: parse_answered(&r.answered_by).ok_or(CallsError::Database)?,
                length: SignedDuration::from_secs(r.length_seconds),
                reached_45s: r.reached_45s,
                ended_at: r.ended_at.to_jiff(),
            })
        })
        .collect()
}

/// The stored word for a caller.
fn caller_word(caller: Caller) -> &'static str {
    match caller {
        Caller::Business => "business",
        Caller::Homeowner => "homeowner",
    }
}

/// The stored word for who answered.
fn answered_word(answered: AnsweredBy) -> &'static str {
    match answered {
        AnsweredBy::Person => "person",
        AnsweredBy::Machine => "machine",
        AnsweredBy::Nobody => "nobody",
        AnsweredBy::Unknown => "unknown",
    }
}

/// The caller a stored word names.
fn parse_caller(word: &str) -> Option<Caller> {
    [Caller::Business, Caller::Homeowner]
        .into_iter()
        .find(|c| caller_word(*c) == word)
}

/// Who answered, from its stored word.
fn parse_answered(word: &str) -> Option<AnsweredBy> {
    [
        AnsweredBy::Person,
        AnsweredBy::Machine,
        AnsweredBy::Nobody,
        AnsweredBy::Unknown,
    ]
    .into_iter()
    .find(|a| answered_word(*a) == word)
}
