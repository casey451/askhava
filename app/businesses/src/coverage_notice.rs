//! The owner-inbox email for each request no business covers (OPS-041; T215, D-435, D-3332,
//! D-3334).
//!
//! `intake::routing` queues one `intake.not_covered` task (one per request and trade) in the
//! transaction that logs the request as recruiting demand. [`run_coverage_notice`] handles it:
//! it reads the payload's trade, town, request number and month count (no homeowner detail is in
//! it), has `messaging::api::queue_coverage_email` store the email once for the owner inbox and
//! sends it through `messaging::api::send_email` to the address in [`CoverageMailer`]. The
//! handler lives here, beside the other recruiting rules (BIZ-048), as architecture section 12
//! says; `businesses` may not name `intake`, so the composition registers it under
//! `intake::api::NOT_COVERED_KIND`. A rerun stores and sends nothing more (SEC-018). Recruiting
//! from the count follows BIZ-048 and is not done here.

use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{Claimed, Clock, HandlerError};
use askhava_messaging::api::{
    CoverageFacts, DailyLimit, EmailSent, EmailTo, MailboxKind, queue_coverage_email, send_email,
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The handler error while the composition has no coverage email sender: the task fails and is
/// tried again, so no not-covered email is lost.
pub const NO_COVERAGE_SENDER: &str = "no owner-inbox email sender is configured";
/// The handler error for a payload that is not what `intake` writes.
pub const BAD_COVERAGE_PAYLOAD: &str = "not-covered payload is malformed";

const FAILED: HandlerError = HandlerError("not-covered email failed");

/// The owner-inbox sender: the email key, the footer's postal line, the provider, the daily
/// limit, the owner-inbox address and the clock. No `Debug` (the address, SEC-034).
pub struct CoverageMailer<E> {
    /// `ASKHAVA_ID_SECRET`, the email outbox key.
    pub secret: SecretString,
    /// The footer's postal line and its mailbox kind (SEC-028).
    pub postal: (String, MailboxKind),
    /// The email provider.
    pub provider: E,
    /// The OPS-038 daily limit.
    pub limit: DailyLimit,
    /// The owner inbox's address; never stored or logged.
    pub owner_inbox: SecretString,
    /// The worker's clock.
    pub clock: Arc<dyn Clock>,
}

/// The not-covered payload as `intake` writes it, checked once (rust brain R1).
struct Payload {
    request: Uuid,
    trade: String,
    town: String,
    number: Option<i64>,
    month_count: i64,
}

/// A lower-case code (`[a-z][a-z0-9_]{0,63}`), as `intake.recruiting_demand` stores it.
fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A trade label: letters, spaces, `&`, `/`, `-` and `,`, at most 64 characters, so no number,
/// link or address can ride in it.
fn is_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.chars().count() <= 64
        && label
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '&' | '/' | '-' | ','))
}

/// OPS-041: reads the task payload; anything else is [`BAD_COVERAGE_PAYLOAD`].
fn payload(task: &Claimed) -> Result<Payload, HandlerError> {
    let p = &task.payload;
    let bad = HandlerError(BAD_COVERAGE_PAYLOAD);
    let request = p["conversation"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(bad)?;
    let text = |field: &str| p[field].as_str().map(str::to_owned).ok_or(bad);
    let (trade, town) = (text("label")?, text("town")?);
    let number = match &p["number"] {
        serde_json::Value::Null => None,
        n => Some(n.as_i64().filter(|n| *n > 0).ok_or(bad)?),
    };
    let month_count = p["month_count"].as_i64().filter(|n| *n > 0).ok_or(bad)?;
    if !is_label(&trade) || !is_code(&town) {
        return Err(bad);
    }
    Ok(Payload {
        request,
        trade,
        town,
        number,
        month_count,
    })
}

/// OPS-041, D-435: emails the owner inbox once about one not-covered request: trade, town,
/// request number and the month's count, no homeowner detail; `Ok` once the provider took it,
/// now or on an earlier run.
///
/// # Errors
///
/// [`BAD_COVERAGE_PAYLOAD`] for a malformed payload; [`NO_COVERAGE_SENDER`] without a sender; a
/// fixed sentence for a held email or a failed store or send (the queue tries again).
pub async fn run_coverage_notice<E: EmailAdapter + Send + Sync>(
    pool: PgPool,
    mailer: Option<Arc<CoverageMailer<E>>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let found = payload(&task)?;
    let mailer = mailer.ok_or(HandlerError(NO_COVERAGE_SENDER))?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let facts = CoverageFacts {
        request: found.request,
        trade: &found.trade,
        town: &found.town,
        number: found.number,
        month_count: found.month_count,
    };
    let now = mailer.clock.now();
    let postal = (mailer.postal.0.as_str(), mailer.postal.1);
    let (row, _) = queue_coverage_email(&mut db, (&mailer.secret, postal), &facts, now)
        .await
        .map_err(|_| FAILED)?;
    let keys = (&mailer.secret, &mailer.provider, mailer.limit);
    let to = EmailTo::Business(mailer.owner_inbox.expose_secret());
    match send_email(&mut db, keys, row, to, now).await {
        Ok(EmailSent::Sent(_) | EmailSent::AlreadySent) => Ok(()),
        Ok(EmailSent::Waiting(_)) => Err(HandlerError("not-covered email is held")),
        Err(_) => Err(FAILED),
    }
}
