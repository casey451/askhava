//! One AI call from request to record (AI-001, AI-006, AI-021, AI-023, AI-024, AI-025).
//!
//! [`read_intake`] makes no call for a web-chat session before its phone code is verified, and
//! none past the daily cap (which alerts once a day) (AI-021). Otherwise it builds the request
//! (the versioned instruction, the scrubbed text inside `<homeowner_message>`, the schema in
//! `output_config`, the model setting, no tools: AI-006, AI-015, AI-022, AI-024, AI-025), sends
//! it with the AI-023 timeout and retries, uses the reply only on an `end_turn` stop with one
//! schema-valid text block (AI-005), and records the call with the model ID, instruction version
//! and request ID (AI-024). Every other ending returns [`AiOff::CallEnded`], and a key or spend
//! error raises an owner alert (AI-023). A reading is stored without text in `ai.reading` and
//! streamed off-database with the call record (AI-013). No transaction is open during the network
//! call (R11). The AI-008 gate in front of it is `switch::read_intake`.

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use askhava_ops::api::{NewAlert, Severity, StreamEvent, is_code_name, raise_alert, stream_event};
use askhava_privacy::api::ScrubbedText;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{
    self, AiConfig, AiError, CallRow, Endpoint, Fault, INTAKE_READING, Reply, Transport,
};
use crate::schemas::{
    Discard, IntakeReading, Language, OpenTrades, Tag, intake_schema, parse_reading, wrap,
};

/// The most retries after the first attempt (AI-023).
pub(crate) const MAX_RETRIES: u32 = 2;
/// The first backoff wait; each retry doubles it (AI-023).
pub(crate) const BACKOFF_BASE: Duration = Duration::from_secs(1);
/// A reading needs a few hundred tokens (`docs/brains/ai-intake.md` section 2).
const MAX_TOKENS: u32 = 512;

/// Where the text came from: web chat calls the AI only after the phone code is verified (AI-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// An inbound text message.
    Text,
    /// A web-chat session.
    WebChat {
        /// Whether its phone code was verified.
        code_verified: bool,
    },
}

/// What one intake reading needs (AI-010).
#[derive(Debug)]
pub struct IntakeCall<'a> {
    /// The record the reading is about (the message), for the idempotency key and the call record.
    pub subject: Uuid,
    /// Where the text came from (AI-021).
    pub origin: Origin,
    /// The scrubbed text, the only text `ai` accepts (AI-004).
    pub text: &'a ScrubbedText,
    /// The trades open in the catalog (AI-005).
    pub open: &'a OpenTrades,
}

/// Why the step runs on the AI-off path (INT-021, AI-021, AI-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiOff {
    /// A web-chat session whose phone code is not verified: no call was made.
    NotVerified,
    /// The daily AI-call cap is reached: no call was made.
    DailyCap,
    /// The call ended without a usable reading (error, timeout, stop, schema or catalog).
    CallEnded,
    /// In live, the AI-008 switch is off (or a record it needs is missing) and zero retention is
    /// not signed: no call was made, and intake runs keyword-only.
    SwitchOff,
}

/// The intake step's result (AI-010).
#[derive(Debug, Clone, PartialEq)]
pub enum Intake {
    /// A checked reading.
    Reading(IntakeReading),
    /// Run this step on the AI-off path.
    AiOff(AiOff),
}

/// How one call ended other than with a reading (AI-005, AI-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallEnd {
    TimedOut,
    Connection,
    Http {
        status: u16,
        alert: Option<&'static str>,
    },
    Malformed,
    Refusal,
    MaxTokens,
    OtherStop,
    Discarded(Discard),
}

impl CallEnd {
    /// The `ai.call_stat.outcome` code.
    fn code(self) -> &'static str {
        match self {
            Self::TimedOut => "timed_out",
            Self::Connection => "connection",
            Self::Http { .. } => "http_error",
            Self::Malformed => "malformed",
            Self::Refusal => "refusal",
            Self::MaxTokens => "max_tokens",
            Self::OtherStop => "other_stop",
            Self::Discarded(Discard::Schema) => "schema",
            Self::Discarded(Discard::Catalog) => "catalog",
        }
    }
}

/// The owner alert a fault raises, if any (AI-023): a 401 or 403, a spend-limit 400, or a 429
/// without `retry-after`.
pub(crate) fn alert_for(fault: Fault) -> Option<&'static str> {
    match fault {
        Fault::Status {
            status: 401 | 403, ..
        } => Some("ai.key_rejected"),
        Fault::Status {
            status: 400,
            spend_limit: true,
            ..
        }
        | Fault::Status {
            status: 429,
            retry_after: None,
            ..
        } => Some("ai.spend_limit"),
        _ => None,
    }
}

/// The wait before retry number `attempts` (1-based), or none when AI-023 allows no retry:
/// only a connection error, a 500, 504 or 529 (exponential backoff) or a 429 with `retry-after`.
pub(crate) fn retry_wait(fault: Fault, attempts: u32) -> Option<Duration> {
    if attempts > MAX_RETRIES {
        return None;
    }
    match fault {
        Fault::Connection
        | Fault::Status {
            status: 500 | 504 | 529,
            ..
        } => Some(BACKOFF_BASE * 2u32.pow(attempts - 1)),
        Fault::Status {
            status: 429,
            retry_after: Some(seconds),
            ..
        } => Some(Duration::from_secs(seconds)),
        _ => None,
    }
}

/// Sends `body` for AI job `job` to `/v1/messages` with the AI-023 retries, all within the
/// timeout setting; returns the result and the number of attempts made.
pub(crate) async fn send<T: Transport>(
    transport: &T,
    config: &AiConfig,
    job: &str,
    subject: Uuid,
    body: &Value,
) -> (Result<Reply, CallEnd>, u32) {
    let mut attempts = 0;
    let run = async {
        loop {
            attempts += 1;
            match transport.post(Endpoint::Messages, job, subject, body).await {
                Ok(reply) => return Ok(reply),
                Err(fault) => match retry_wait(fault, attempts) {
                    Some(wait) => tokio::time::sleep(wait).await,
                    None => return Err(end_of(fault)),
                },
            }
        }
    };
    let result = tokio::time::timeout(config.timeout(), run).await;
    (result.unwrap_or(Err(CallEnd::TimedOut)), attempts)
}

fn end_of(fault: Fault) -> CallEnd {
    match fault {
        Fault::Connection => CallEnd::Connection,
        Fault::Status { status, .. } => CallEnd::Http {
            status,
            alert: alert_for(fault),
        },
        Fault::Malformed => CallEnd::Malformed,
    }
}

/// The intake request: no `tools` key at all (AI-006), the versioned instruction (AI-015), the
/// tagged scrubbed text (AI-025), the model setting (AI-024) and the schema file (AI-022).
pub(crate) fn intake_request(config: &AiConfig, text: &ScrubbedText) -> Value {
    json!({
        "model": config.model(),
        "max_tokens": MAX_TOKENS,
        "system": INTAKE_READING.text,
        "messages": [{"role": "user", "content": wrap(Tag::HomeownerMessage, text.as_str())}],
        "output_config": {"format": {"type": "json_schema", "schema": intake_schema()}},
    })
}

/// The reading in a reply: only an `end_turn` stop with exactly one text block is read (AI-005).
pub(crate) fn reading_from_reply(
    body: &Value,
    open: &OpenTrades,
) -> Result<IntakeReading, CallEnd> {
    match body["stop_reason"].as_str() {
        Some("end_turn") => {}
        Some("refusal") => return Err(CallEnd::Refusal),
        Some("max_tokens") => return Err(CallEnd::MaxTokens),
        _ => return Err(CallEnd::OtherStop),
    }
    let text = match body["content"].as_array().map(Vec::as_slice) {
        Some([block]) if block["type"] == "text" => block["text"].as_str(),
        _ => None,
    };
    let text = text.ok_or(CallEnd::Discarded(Discard::Schema))?;
    parse_reading(text, open).map_err(CallEnd::Discarded)
}

/// Reads one scrubbed request (AI-005, AI-010, AI-021, AI-023, AI-024).
///
/// # Errors
///
/// [`AiError::Database`] when the cap count or the call record cannot be written.
pub async fn read_intake<T: Transport>(
    db: &mut PgConnection,
    transport: &T,
    config: &AiConfig,
    call: &IntakeCall<'_>,
) -> Result<Intake, AiError> {
    if call.origin
        == (Origin::WebChat {
            code_verified: false,
        })
    {
        return Ok(Intake::AiOff(AiOff::NotVerified));
    }
    let mut row = CallRow {
        subject: call.subject,
        instruction: INTAKE_READING,
        model: config.model(),
        request_id: None,
        outcome: "daily_cap",
        http_status: None,
        attempts: 0,
        alert: Some("ai.daily_cap"),
    };
    if models::calls_today(db).await? >= config.daily_cap() {
        record(db, &row, None).await?;
        return Ok(Intake::AiOff(AiOff::DailyCap));
    }
    let body = intake_request(config, call.text);
    let (sent, attempts) = send(transport, config, INTAKE_READING.job, call.subject, &body).await;
    let result = sent.and_then(|reply| {
        row.request_id = reply.request_id.filter(|id| is_request_id(id));
        reading_from_reply(&reply.body, call.open)
    });
    row.attempts = i32::try_from(attempts).unwrap_or(i32::MAX);
    (row.outcome, row.http_status, row.alert) = match result {
        Ok(_) => ("ok", None, None),
        Err(CallEnd::Http { status, alert }) => ("http_error", Some(i32::from(status)), alert),
        Err(end) => (end.code(), None, None),
    };
    record(db, &row, result.as_ref().ok()).await?;
    Ok(result.map_or(Intake::AiOff(AiOff::CallEnded), Intake::Reading))
}

/// A request ID safe to store: 1 to 128 ASCII letters, digits, `_` or `-`.
fn is_request_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// Writes the call record and, for the first alert of its kind today, raises the owner alert
/// and stores its ID on the record, and the reading, if any, with its off-database copy, in one
/// short transaction (AI-013, AI-021, AI-023).
async fn record(
    db: &mut PgConnection,
    row: &CallRow<'_>,
    reading: Option<&IntakeReading>,
) -> Result<(), AiError> {
    let mut tx = db.begin().await.map_err(|_| AiError::Database)?;
    let mut alert_id = None;
    if let Some(kind) = row.alert
        && !models::alerted_today(&mut tx, kind).await?
    {
        let alert = NewAlert {
            kind,
            severity: Severity::High,
            subject: None,
        };
        alert_id = Some(
            raise_alert(&mut tx, &alert)
                .await
                .map_err(|_| AiError::Database)?,
        );
    }
    models::insert_call(&mut tx, row, alert_id).await?;
    if let Some(reading) = reading {
        write_reading(&mut tx, row.subject, reading).await?;
    }
    tx.commit().await.map_err(|_| AiError::Database)
}

/// Stores the reading's confidences and flags in `ai.reading`, which has no text column, and
/// queues its trade, job type, flags and language for the off-database stream in the same
/// transaction (AI-013, PRIV-029, D-734, D-842).
async fn write_reading(
    db: &mut PgConnection,
    subject: Uuid,
    reading: &IntakeReading,
) -> Result<(), AiError> {
    let spanish = reading.language == Language::Es;
    let id = sqlx::query_scalar!(
        "INSERT INTO ai.reading (subject_id, trade_confidence, job_type_confidence, emergency, \
         urgent, major, spanish) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        subject,
        reading.trade_confidence,
        reading.job_type_confidence,
        reading.emergency,
        reading.urgent,
        reading.major,
        spanish,
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| AiError::Database)?;
    let flag = |on: bool| if on { "true" } else { "false" };
    let fields: Vec<(&'static str, &'static str)> = [
        ("trade", interned(code_of(&reading.trade))),
        ("job_type", interned(code_of(&reading.job_type))),
        ("emergency", flag(reading.emergency)),
        ("urgent", flag(reading.urgent)),
        ("major", flag(reading.major)),
        ("language", if spanish { "es" } else { "en" }),
    ]
    .into_iter()
    .filter(|(_, value)| is_code_name(value))
    .collect();
    let event = StreamEvent::AiReading {
        reading_id: id,
        fields: &fields,
    };
    stream_event(db, &event)
        .await
        .map_err(|_| AiError::Database)
}

/// A catalog trade code or job-type name as a code name: lower-case letters and digits, every
/// other run of characters one `_` ("evaporative (swamp) cooler repair" becomes
/// `evaporative_swamp_cooler_repair`) (AI-013, D-842).
pub(crate) fn code_of(value: &str) -> String {
    let lower = value.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .collect();
    words.join("_")
}

/// `value` as a `&'static str`, which the stream's field list takes (D-734). Only codes of
/// catalog-checked trades and job types reach it, so the set it keeps is bounded by the catalog
/// (D-842).
fn interned(value: String) -> &'static str {
    static SEEN: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(known) = seen.get(value.as_str()) {
        return known;
    }
    let leaked: &'static str = Box::leak(value.into_boxed_str());
    seen.insert(leaked);
    leaked
}
