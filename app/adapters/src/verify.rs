//! The Verify adapter: Twilio Verify behind one trait (SEC-018, SEC-032).
//!
//! Sends a one-time code by text, email or, only for a homeowner's tap, a call
//! (D-366; that rule is the caller's), and checks it. Sending acts outside and
//! takes a derived key; `check_` is a read-only prefix for the architecture
//! test (D-596).

use std::future::Future;

use crate::base::{AdapterError, IdempotencyKey, TwilioHttp};

/// How the code reaches the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeChannel {
    /// A text.
    Sms,
    /// An email.
    Email,
    /// A voice call, only when a homeowner taps "Call me with the code instead" (D-366).
    Call,
}

impl CodeChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sms => "sms",
            Self::Email => "email",
            Self::Call => "call",
        }
    }
}

#[derive(serde::Deserialize)]
struct Verification {
    sid: String,
    valid: bool,
}

/// Sending and checking codes.
pub trait VerifyAdapter {
    /// Sends a code to `to` by `channel`; returns the verification SID (SEC-018).
    fn send_code(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    /// Whether `code` is the one sent to `to`.
    fn check_code(
        &self,
        to: &str,
        code: &str,
    ) -> impl Future<Output = Result<bool, AdapterError>> + Send;
}

/// The one HTTP Verify client; stand-in, sandbox or live by its base URL (D-453).
#[derive(Debug, Clone)]
pub struct TwilioVerify {
    pub(crate) http: TwilioHttp,
}

impl TwilioVerify {
    fn url(&self, step: &str) -> String {
        let http = &self.http;
        format!(
            "{}/v2/Services/{}/{step}",
            http.verify_base, http.verify_service
        )
    }
}

impl VerifyAdapter for TwilioVerify {
    async fn send_code(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let form = [("To", to), ("Channel", channel.as_str())];
        let url = self.url("Verifications");
        let started: Verification = self.http.post_form(&url, &form, Some(idempotency)).await?;
        Ok(started.sid)
    }

    async fn check_code(&self, to: &str, code: &str) -> Result<bool, AdapterError> {
        let form = [("To", to), ("Code", code)];
        let url = self.url("VerificationCheck");
        let checked: Verification = self.http.post_form(&url, &form, None).await?;
        Ok(checked.valid)
    }
}
