//! The voice adapter: standard programmable voice behind one trait (CALL-017, CALL-021, SEC-018).
//!
//! It opens a call with a masked pool number as caller ID and ends it. It has
//! no method that buys, provisions or releases a number: the pool is the fixed
//! list from settings (CALL-017, D-160), and the adapter takes a
//! [`PoolNumber`], which only that list can produce. Never Twilio Proxy
//! (CALL-021).

use std::future::Future;

use crate::base::{AdapterError, IdempotencyKey, TwilioHttp, is_sid};
use crate::config::PoolNumber;

/// What Twilio said about a call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct CallState {
    /// The call SID (`CA…`).
    pub sid: String,
    /// Twilio's call status (`queued`, `completed`, …).
    pub status: String,
}

/// Placing and ending calls (SEC-018: each takes its derived key).
pub trait VoiceAdapter {
    /// Dials `to` with the pool number `caller_id` shown (CALL-017).
    fn open_call(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        caller_id: &PoolNumber,
    ) -> impl Future<Output = Result<CallState, AdapterError>> + Send;

    /// Ends the call `call_sid` (`CA…`; anything else is [`AdapterError::InvalidId`]).
    fn end_call(
        &self,
        idempotency: &IdempotencyKey,
        call_sid: &str,
    ) -> impl Future<Output = Result<CallState, AdapterError>> + Send;
}

/// The one HTTP voice client; stand-in, sandbox or live by its base URL (D-453).
#[derive(Debug, Clone)]
pub struct TwilioVoice {
    pub(crate) http: TwilioHttp,
}

impl VoiceAdapter for TwilioVoice {
    async fn open_call(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        caller_id: &PoolNumber,
    ) -> Result<CallState, AdapterError> {
        let url = format!("{}/Calls.json", self.http.account_url());
        let form = [("To", to), ("From", caller_id.as_str())];
        self.http.post_form(&url, &form, Some(idempotency)).await
    }

    async fn end_call(
        &self,
        idempotency: &IdempotencyKey,
        call_sid: &str,
    ) -> Result<CallState, AdapterError> {
        if !is_sid(call_sid, "CA") {
            return Err(AdapterError::InvalidId);
        }
        let url = format!("{}/Calls/{call_sid}.json", self.http.account_url());
        let form = [("Status", "completed")];
        self.http.post_form(&url, &form, Some(idempotency)).await
    }
}
