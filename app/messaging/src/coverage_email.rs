//! The owner-inbox email of one request no business covers (OPS-041; T215, D-435, D-3332,
//! D-3334, D-3336).
//!
//! `intake` queues one `intake.not_covered` task per request and trade when it logs recruiting
//! demand; `businesses`' `coverage_notice` handler reads it and calls [`queue_coverage_email`],
//! which stores one email per request and trade in T094's email outbox for the owner inbox: the
//! trade, the town, the request number and the count of requests not covered so far that
//! Arizona month, in the shared layout (`queue_account_email`). [`CoverageFacts`] has no field for a homeowner's name, phone, email,
//! address or words, so none can reach the email (SEC-034, PRIV-001). The owner inbox is one fixed
//! recipient ID, [`OWNER_INBOX`]; its address is given at send time by the composition, never
//! stored. The weekly digest's roll-up by trade is T204's.

use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::email::{MailboxKind, short_name};
use crate::models::EmailError;
use crate::templates::Language;

/// The email template ID of the not-covered email; with [`OWNER_INBOX`], the request and the
/// trade's [`trade_subject`] it makes the email's key, so one request and trade gets one email
/// (SEC-018, D-3336).
pub const COVERAGE_EMAIL_TEMPLATE: &str = "owner_not_covered";

/// The owner inbox's recipient ID in the email outbox: not a business mailbox, so no business
/// address directory resolves it; the composition sends it to the owner-inbox address.
pub const OWNER_INBOX: Uuid = Uuid::from_u128(0x0a5c_4a7a_0000_4000_8000_0000_0000_0041);

/// The key action that turns a trade label into the opaque subject ID [`trade_subject`] gives.
const TRADE_ACTION: &str = "email.owner_not_covered.trade";

/// OPS-041, SEC-018 (D-3336): the trade's subject ID in the email's key: the first 128 bits of
/// the keyed HMAC over the label, so the same trade always gives the same ID, two trades of one
/// request give two IDs, and the stored ID holds no words.
#[must_use]
pub fn trade_subject(secret: &SecretString, trade: &str) -> Uuid {
    let key = askhava_adapters::api::IdempotencyKey::derive(secret, TRADE_ACTION, &[trade]);
    let bits = key.as_str().chars().take(32).fold(0u128, |acc, c| {
        acc << 4 | u128::from(c.to_digit(16).unwrap_or(0))
    });
    Uuid::from_u128(bits)
}

/// What one not-covered email says (OPS-041): codes and counts only, no homeowner detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverageFacts<'a> {
    /// The request (`intake.conversation`), the email's subject ID.
    pub request: Uuid,
    /// The trade as the homeowner's reply named it, such as `boat lift`.
    pub trade: &'a str,
    /// The catalog town code, such as `lake_havasu_city`.
    pub town: &'a str,
    /// The request number (its lowest job number), when a job was opened.
    pub number: Option<i64>,
    /// Requests not covered so far this Arizona month, this one included.
    pub month_count: i64,
}

/// A catalog town code as words: `lake_havasu_city` is "Lake Havasu City".
#[must_use]
pub fn town_words(code: &str) -> String {
    code.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// OPS-041: the subject and lines of the not-covered email for `facts`, in English (the owner
/// reads English): trade, town, request number when there is one, and the month's count.
#[must_use]
pub fn coverage_lines(facts: &CoverageFacts<'_>) -> (String, Vec<String>) {
    let town = town_words(facts.town);
    let subject = format!(
        "{}: No business covers {} work in {town} yet",
        short_name(Language::En),
        facts.trade
    );
    let mut lines = vec![
        "A homeowner asked for work no business covers yet.".to_owned(),
        format!("Trade: {}", facts.trade),
        format!("Town: {town}"),
    ];
    lines.extend(facts.number.map(|n| format!("Request number: {n}")));
    lines.push(format!(
        "All requests not covered this month, counting this one: {}",
        facts.month_count
    ));
    (subject, lines)
}

/// OPS-041, SEC-018: stores the not-covered email for `facts` once for the owner inbox at `now`
/// (key over [`COVERAGE_EMAIL_TEMPLATE`], [`OWNER_INBOX`], the request and the trade's
/// [`trade_subject`], D-3336); returns the row ID and whether this call created it. `send_email` sends it to the owner-inbox address.
///
/// # Errors
///
/// As `queue_account_email`: [`EmailError::PostalAddress`], [`EmailError::Template`] or
/// [`EmailError::Database`].
pub async fn queue_coverage_email(
    db: &mut PgConnection,
    keys: (&SecretString, (&str, MailboxKind)),
    facts: &CoverageFacts<'_>,
    now: Timestamp,
) -> Result<(Uuid, bool), EmailError> {
    let (subject, lines) = coverage_lines(facts);
    let subjects = [facts.request, trade_subject(keys.0, facts.trade)];
    let ids = (COVERAGE_EMAIL_TEMPLATE, OWNER_INBOX, subjects);
    crate::api::queue_account_email(db, keys, (ids.0, ids.1, &ids.2), (&subject, &lines), now).await
}
