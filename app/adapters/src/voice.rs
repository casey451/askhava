//! The voice adapter: standard programmable voice behind one trait (CALL-017, CALL-021, SEC-018).
//!
//! It opens a call with a masked pool number as caller ID and ends it. It has
//! no method that buys, provisions or releases a number: the pool is the fixed
//! list from settings (CALL-017, D-160), and the adapter takes a
//! [`PoolNumber`], which only that list can produce. Never Twilio Proxy
//! (CALL-021).
//!
//! Every call it opens carries a [`CallPlan`] (CALL-022, OPS-028, SEC-016; D-2762, D-2771,
//! D-3112): the call's script sent inline as Twilio's `Twiml` parameter (at most
//! [`MAX_TWIML`] characters; never a `Url`, so a script holding a number never sits in a URL),
//! a `StatusCallback` URL with its `StatusCallbackEvent` list, and, when who answered matters,
//! answering-machine detection (`MachineDetection=DetectMessageEnd`, `AsyncAmd=true`,
//! `AsyncAmdStatusCallback` on the same URL). Twilio's Calls API refuses a create without one of
//! `Url`, `Twiml` or `ApplicationSid`
//! (<https://www.twilio.com/docs/voice/api/call-resource#create-a-call-resource>,
//! <https://www.twilio.com/docs/voice/answering-machine-detection>), so no create leaves without
//! a script.

use std::fmt;
use std::future::Future;

use secrecy::{ExposeSecret, SecretString};

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

/// The longest inline script Twilio's `Twiml` parameter takes, in characters.
pub const MAX_TWIML: usize = 4_000;

/// The call progress events every call asks Twilio to post (`StatusCallbackEvent`).
pub const CALL_EVENTS: [&str; 4] = ["initiated", "ringing", "answered", "completed"];

/// What Twilio runs on a call and where it posts the call's progress (CALL-022, OPS-028, D-2762).
/// The fields are private: [`CallPlan::new`] checks the script's length and the callback URL. Its
/// `Debug` shows neither the script (it can hold a number) nor the URL.
#[derive(Clone)]
pub struct CallPlan {
    twiml: SecretString,
    status_callback: String,
    detect_machine: bool,
}

impl CallPlan {
    /// A plan running `twiml` (sent inline) and posting its progress to `status_callback`;
    /// `None` when the script is empty or over [`MAX_TWIML`] characters, or the URL is not
    /// `http(s)://`.
    #[must_use]
    pub fn new(twiml: SecretString, status_callback: String) -> Option<Self> {
        let length = twiml.expose_secret().chars().count();
        let web = status_callback.starts_with("https://") || status_callback.starts_with("http://");
        ((1..=MAX_TWIML).contains(&length) && web).then_some(Self {
            twiml,
            status_callback,
            detect_machine: false,
        })
    }

    /// The same plan asking for Twilio's asynchronous answering-machine detection, its result
    /// posted to the status callback URL (OPS-028, D-2771).
    #[must_use]
    pub fn detecting_machine(self) -> Self {
        Self {
            detect_machine: true,
            ..self
        }
    }

    /// The call's script (TwiML).
    #[must_use]
    pub fn twiml(&self) -> &SecretString {
        &self.twiml
    }

    /// Where Twilio posts the call's progress and, when asked, its detection result.
    #[must_use]
    pub fn status_callback(&self) -> &str {
        &self.status_callback
    }

    /// Whether the plan asks for answering-machine detection.
    #[must_use]
    pub fn detects_machine(&self) -> bool {
        self.detect_machine
    }

    /// The create request's form after `To` and `From` (D-2762, D-2771).
    fn form(&self) -> Vec<(&'static str, &str)> {
        let mut form = vec![
            ("Twiml", self.twiml.expose_secret()),
            ("StatusCallback", self.status_callback.as_str()),
            ("StatusCallbackMethod", "POST"),
        ];
        form.extend(CALL_EVENTS.iter().map(|e| ("StatusCallbackEvent", *e)));
        if self.detect_machine {
            form.extend([
                ("MachineDetection", "DetectMessageEnd"),
                ("AsyncAmd", "true"),
                ("AsyncAmdStatusCallback", self.status_callback.as_str()),
                ("AsyncAmdStatusCallbackMethod", "POST"),
            ]);
        }
        form
    }
}

impl fmt::Debug for CallPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallPlan")
            .field("detect_machine", &self.detect_machine)
            .finish_non_exhaustive()
    }
}

/// Placing and ending calls (SEC-018: each takes its derived key).
pub trait VoiceAdapter {
    /// Dials `to` with the pool number `caller_id` shown (CALL-017), running `plan`'s script and
    /// posting its progress to `plan`'s callback (D-2762, D-3112).
    fn open_call(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        caller_id: &PoolNumber,
        plan: &CallPlan,
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
        plan: &CallPlan,
    ) -> Result<CallState, AdapterError> {
        let url = format!("{}/Calls.json", self.http.account_url());
        let mut form = vec![("To", to), ("From", caller_id.as_str())];
        form.extend(plan.form());
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
