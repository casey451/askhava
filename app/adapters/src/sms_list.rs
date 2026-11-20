//! Listing Twilio's sent log for the replay after a restore (OPS-015, OPS-017; D-2764, D-3155,
//! D-3889).
//!
//! Twilio stores no custom key on a message, and its Messages list filters only by `To`, `From`
//! and `DateSent` (whole UTC days), so the replay never asks for the app's derived key: it lists
//! every message sent from one of the app's numbers over a window ([`SmsSentLog::list_sent`]:
//! `From`, `DateSent>` and `DateSent<`, following `next_page_uri`) and keeps those whose send time
//! falls inside it. The recipient and body come back as secrets; only `messaging` compares them
//! with its outbox rows (`messaging::api::replay_match`). A separate trait over [`SmsAdapter`],
//! so no existing texting fake changes (D-1753). A read, so it takes no idempotency key (D-596).

use std::future::Future;

use jiff::Timestamp;
use jiff::fmt::rfc2822::DateTimeParser;
use secrecy::SecretString;

use crate::base::{AdapterError, is_sid};
use crate::sms::{SmsAdapter, TwilioSms};

/// The most pages one listing follows (each up to [`SENT_LOG_PAGE_SIZE`] messages); a longer
/// chain is refused as [`AdapterError::Decode`] rather than followed for ever.
pub const SENT_LOG_MAX_PAGES: usize = 200;
/// Twilio's largest Messages page.
pub const SENT_LOG_PAGE_SIZE: &str = "1000";

/// One message in Twilio's sent log: its SID, `To`, `From`, body (`None` once redacted) and send
/// time. `To` and the body are secrets, so `Debug` prints neither (PRIV-016).
#[derive(Debug)]
pub struct SentMessage {
    /// The message SID (`SM…` or `MM…`).
    pub sid: String,
    /// The recipient's number; never logged or stored outside `messaging`.
    pub to: SecretString,
    /// The sending number.
    pub from: String,
    /// The text, `None` when Twilio holds none (redacted).
    pub body: Option<SecretString>,
    /// Twilio's `date_sent`.
    pub sent_at: Timestamp,
}

/// Listing the texts already sent (OPS-017, D-2764, D-3155).
pub trait SmsSentLog: SmsAdapter {
    /// Every message sent from `from` with a send time from `window.0` through `window.1`, all
    /// pages; a message with no send time yet is left out.
    fn list_sent(
        &self,
        from: &str,
        window: (Timestamp, Timestamp),
    ) -> impl Future<Output = Result<Vec<SentMessage>, AdapterError>> + Send;
}

/// One page of Twilio's Messages list; never logged.
#[derive(serde::Deserialize)]
struct MessagePage {
    messages: Vec<RawMessage>,
    next_page_uri: Option<String>,
}

/// The Message fields the replay needs; never logged.
#[derive(serde::Deserialize)]
struct RawMessage {
    sid: String,
    to: String,
    from: String,
    body: Option<String>,
    date_sent: Option<String>,
}

impl RawMessage {
    /// The message as listed; `None` before Twilio sent it. A malformed SID or date is `Decode`.
    fn sent(self) -> Result<Option<SentMessage>, AdapterError> {
        let Some(date) = self.date_sent else {
            return Ok(None);
        };
        if !(is_sid(&self.sid, "SM") || is_sid(&self.sid, "MM")) {
            return Err(AdapterError::Decode);
        }
        let sent_at = DateTimeParser::new()
            .parse_timestamp(&date)
            .map_err(|_| AdapterError::Decode)?;
        Ok(Some(SentMessage {
            sid: self.sid,
            to: SecretString::from(self.to),
            from: self.from,
            body: self.body.filter(|b| !b.is_empty()).map(SecretString::from),
            sent_at,
        }))
    }
}

impl SmsSentLog for TwilioSms {
    /// GET `Messages.json` with `From`, `DateSent>` and `DateSent<` (the window's UTC days) and
    /// `PageSize`, then each `next_page_uri` on the same host (SEC-032).
    async fn list_sent(
        &self,
        from: &str,
        (start, end): (Timestamp, Timestamp),
    ) -> Result<Vec<SentMessage>, AdapterError> {
        let (after, before) = (day(start), day(end));
        let first = format!("{}/Messages.json", self.http.account_url());
        let query = [
            ("From", from),
            ("DateSent>", after.as_str()),
            ("DateSent<", before.as_str()),
            ("PageSize", SENT_LOG_PAGE_SIZE),
        ];
        let mut page: MessagePage = self.http.get_json(&first, &query).await?;
        let mut out = Vec::new();
        for _ in 0..SENT_LOG_MAX_PAGES {
            for raw in page.messages {
                out.extend(
                    raw.sent()?
                        .filter(|m| start <= m.sent_at && m.sent_at <= end),
                );
            }
            let Some(next) = page.next_page_uri else {
                return Ok(out);
            };
            page = self.http.get_json(&self.http.page_url(&next)?, &[]).await?;
        }
        Err(AdapterError::Decode)
    }
}

/// A timestamp's UTC day, `YYYY-MM-DD`, as Twilio's `DateSent` filters take it.
fn day(at: Timestamp) -> String {
    at.strftime("%Y-%m-%d").to_string()
}
