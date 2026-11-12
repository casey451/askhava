//! The texting adapter: Twilio Messaging behind one trait (SEC-018, SEC-032).
//!
//! Sends one rendered text; consent, quiet hours, the suppression list and the
//! template choice are `messaging`'s work before it calls here. The derived key
//! rides in `X-Askhava-Message-Key` so the sent log can be matched after a
//! restore (OPS-017). [`SmsBodyDelete`] redacts a text's body at Twilio for
//! the day-31 purge (PRIV-027, TXT-020, D-1841). [`SmsBodyRead`] reads an
//! inbound text's body back from Twilio's copy for the bounded retry of a text
//! a module error stopped, which never stores it (TXT-012, TXT-020, D-801,
//! D-2307).

use std::future::Future;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};

use crate::base::{AdapterError, IdempotencyKey, TwilioHttp, is_sid};
use crate::config::TwilioConfig;

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

/// One message as Twilio still holds it (TXT-012, D-2307): its `From`, its `To` and its body. The
/// body is a [`SecretString`] and `Debug` prints neither number nor body (PRIV-016, D-801).
pub struct ReadMessage {
    /// The sender's number, as Twilio stored it.
    pub from: String,
    /// The number the text was sent to.
    pub to: String,
    /// The text; never stored or logged.
    pub body: SecretString,
}

impl std::fmt::Debug for ReadMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadMessage").finish_non_exhaustive()
    }
}

/// What [`SmsBodyRead::fetch_message_body`] answers (TXT-020, D-2307).
#[derive(Debug)]
pub enum MessageBody {
    /// Twilio still holds the body.
    Body(ReadMessage),
    /// Redacted (day 31, or `Delete my data`) or unknown (404, code 20404): no body to read.
    Gone,
}

/// Reading an inbound text's body back from Twilio's copy (TXT-012, TXT-020, D-801, D-2307). A
/// read only (`fetch_`, D-596), so it takes no idempotency key.
pub trait SmsBodyRead {
    /// GET `Messages/{sid}.json` (`SM…` or `MM…`): the body with its numbers, or
    /// [`MessageBody::Gone`] when the body is empty (redacted) or Twilio answers 404 with code
    /// 20404. A malformed SID is `InvalidId` before any request.
    fn fetch_message_body(
        &self,
        sid: &str,
    ) -> impl Future<Output = Result<MessageBody, AdapterError>> + Send;
}

/// The Message resource's fields the read needs; never logged.
#[derive(serde::Deserialize)]
struct MessageResource {
    from: Option<String>,
    to: Option<String>,
    body: Option<String>,
}

/// Twilio's error body, for its code.
#[derive(serde::Deserialize)]
struct ErrorCode {
    code: Option<i64>,
}

/// The Twilio Messages reader (D-2307): its own client with the shared client's limits (15-second
/// timeout, no redirects, no environment proxy, SEC-032) and Basic auth from [`TwilioConfig`];
/// stand-in, sandbox or live by the config's base URL (D-453).
#[derive(Clone)]
pub struct TwilioMessageReader {
    client: reqwest::Client,
    account_url: String,
    account_sid: String,
    auth_token: SecretString,
}

impl std::fmt::Debug for TwilioMessageReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioMessageReader")
            .finish_non_exhaustive()
    }
}

impl TwilioMessageReader {
    /// The reader over `config`'s base URL, account and auth token (SEC-032).
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotSent`] when the HTTP client cannot be built.
    pub fn new(config: &TwilioConfig) -> Result<Self, AdapterError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| AdapterError::NotSent)?;
        Ok(Self {
            client,
            account_url: format!(
                "{}/2010-04-01/Accounts/{}",
                config.api_base(),
                config.account_sid()
            ),
            account_sid: config.account_sid().to_owned(),
            auth_token: config.auth_token().clone(),
        })
    }
}

impl SmsBodyRead for TwilioMessageReader {
    /// TXT-020, D-2307, SEC-032: the SID is checked before it reaches the path; the answer's body
    /// goes only into [`ReadMessage::body`].
    async fn fetch_message_body(&self, sid: &str) -> Result<MessageBody, AdapterError> {
        if !(is_sid(sid, "SM") || is_sid(sid, "MM")) {
            return Err(AdapterError::InvalidId);
        }
        let url = format!("{}/Messages/{sid}.json", self.account_url);
        let request = self
            .client
            .get(url)
            .basic_auth(&self.account_sid, Some(self.auth_token.expose_secret()));
        let response = request.send().await.map_err(|e| {
            if e.is_builder() || e.is_connect() {
                AdapterError::NotSent
            } else {
                AdapterError::OutcomeUnknown
            }
        })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AdapterError::OutcomeUnknown)?;
        read_answer(status.as_u16(), &bytes)
    }
}

/// Sorts Twilio's answer to the read: 404 with 20404 is `Gone`, any other error status is
/// `Vendor`, an empty body is `Gone`, and a resource without its numbers is `Decode`.
fn read_answer(status: u16, bytes: &[u8]) -> Result<MessageBody, AdapterError> {
    if !(200..300).contains(&status) {
        let code = serde_json::from_slice::<ErrorCode>(bytes)
            .ok()
            .and_then(|e| e.code);
        return match (status, code) {
            (404, Some(20404)) => Ok(MessageBody::Gone),
            _ => Err(AdapterError::Vendor { status, code }),
        };
    }
    let found: MessageResource = serde_json::from_slice(bytes).map_err(|_| AdapterError::Decode)?;
    let body = found.body.unwrap_or_default();
    if body.is_empty() {
        return Ok(MessageBody::Gone);
    }
    match (found.from, found.to) {
        (Some(from), Some(to)) => Ok(MessageBody::Body(ReadMessage {
            from,
            to,
            body: SecretString::from(body),
        })),
        _ => Err(AdapterError::Decode),
    }
}
