//! The texting adapter: Twilio Messaging behind one trait (SEC-018, SEC-032).
//!
//! Sends one rendered text; consent, quiet hours, the suppression list and the
//! template choice are `messaging`'s work before it calls here. The derived key
//! rides in `X-Askhava-Message-Key` so the sent log can be matched after a
//! restore (OPS-017).

use std::future::Future;

use crate::base::{AdapterError, IdempotencyKey, TwilioHttp};

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
