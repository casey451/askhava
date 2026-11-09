//! The texting adapter: Twilio Messaging behind one trait (SEC-018, SEC-032).
//!
//! Sends one rendered text; consent, quiet hours, the suppression list and the
//! template choice are `messaging`'s work before it calls here. The derived key
//! rides in `X-Askhava-Message-Key` so the sent log can be matched after a
//! restore (OPS-017). [`SmsBodyDelete`] redacts a text's body at Twilio for
//! the day-31 purge (PRIV-027, TXT-020, D-1841).

use std::future::Future;

use crate::base::{AdapterError, IdempotencyKey, TwilioHttp, is_sid};

/// What Twilio said about an accepted text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct SentText {
    /// The message SID (`SM…`).
    pub sid: String,
    /// Twilio's status, `queued` on acceptance.
    pub status: String,
}

/// Sending texts (SEC-018: every send takes its derived key).
pub trait SmsAdapter {
    /// Sends `body` from `from` to `to` under `idempotency` (SEC-018).
    fn send_text(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        from: &str,
        body: &str,
    ) -> impl Future<Output = Result<SentText, AdapterError>> + Send;
}

/// Removing a text's body at Twilio, sent or received (PRIV-027, TXT-020,
/// D-1803, D-1841). A separate trait so no existing [`SmsAdapter`] fake
/// changes (D-1753). Twilio documents body redaction as updating the Message
/// with an empty `Body`; it keeps the message record (numbers, status, dates)
/// and removes the text, and a repeat answers the same way, so a retried
/// purge is safe (SEC-018).
pub trait SmsBodyDelete: SmsAdapter {
    /// Redacts the body of message `sid` (`SM…` or `MM…`); `Ok` only when
    /// Twilio answered with an empty body. A message Twilio no longer has is
    /// `Vendor { status: 404, code: Some(20404) }`, which leaves no body there.
    fn delete_message_body(
        &self,
        idempotency: &IdempotencyKey,
        sid: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;
}

/// The body field of Twilio's answer to a redaction; never logged.
#[derive(serde::Deserialize)]
struct RedactedText {
    body: String,
}

/// The one HTTP texting client; stand-in, sandbox or live by its base URL (D-453).
#[derive(Debug, Clone)]
pub struct TwilioSms {
    pub(crate) http: TwilioHttp,
}

impl SmsAdapter for TwilioSms {
    async fn send_text(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        let url = format!("{}/Messages.json", self.http.account_url());
        let form = [("To", to), ("From", from), ("Body", body)];
        self.http.post_form(&url, &form, Some(idempotency)).await
    }
}

impl SmsBodyDelete for TwilioSms {
    /// POST `Messages/{sid}.json` with `Body=` empty (PRIV-027, TXT-020, SEC-032:
    /// the SID is checked before it reaches the path).
    async fn delete_message_body(
        &self,
        idempotency: &IdempotencyKey,
        sid: &str,
    ) -> Result<(), AdapterError> {
        if !(is_sid(sid, "SM") || is_sid(sid, "MM")) {
            return Err(AdapterError::InvalidId);
        }
        let url = format!("{}/Messages/{sid}.json", self.http.account_url());
        let answer: RedactedText = self
            .http
            .post_form(&url, &[("Body", "")], Some(idempotency))
            .await?;
        if answer.body.is_empty() {
            Ok(())
        } else {
            Err(AdapterError::Decode)
        }
    }
}
