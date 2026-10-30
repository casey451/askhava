//! Shared pieces of every adapter: the derived idempotency key, the adapter
//! error, and the Twilio-shaped HTTP client that the texting, voice and Verify
//! adapters share (SEC-018, SEC-032).
//!
//! There is one client and no fake code path: stand-in, sandbox and live differ
//! only by the base URLs and credentials in [`TwilioConfig`] (D-453).

use std::sync::Arc;

use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use sha2::Sha256;
use thiserror::Error;

use crate::config::TwilioConfig;
use crate::sms::TwilioSms;
use crate::verify::TwilioVerify;
use crate::voice::TwilioVoice;

/// The key every outside action carries (SEC-018): HMAC-SHA256 under the
/// caller's `ID_SECRET` over the action name and the IDs the action is about,
/// so a retry, or a replay after a restore, reproduces it, and it holds no
/// personal data (`docs/brains/rust-production.md` R13). Built only by
/// [`IdempotencyKey::derive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Derives the key for `action` (for example `sms.send`) on the record IDs
    /// in `ids` (SEC-018). Pass record IDs, never phone numbers or text.
    ///
    /// # Panics
    ///
    /// Never: HMAC accepts a key of any length.
    #[must_use]
    pub fn derive(secret: &SecretString, action: &str, ids: &[&str]) -> Self {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.expose_secret().as_bytes())
            .expect("HMAC accepts a key of any length");
        for part in std::iter::once(action).chain(ids.iter().copied()) {
            mac.update(&(part.len() as u64).to_be_bytes());
            mac.update(part.as_bytes());
        }
        let digest = mac.finalize().into_bytes();
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// The key as sent in the `Idempotency-Key` header: 64 lower-case hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why an adapter call failed. No variant carries a URL, a phone number or a
/// vendor message, only codes (R5, PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    /// The HTTP client could not be built or the request did not complete.
    #[error("vendor request did not complete")]
    Transport,
    /// The vendor answered with an error status and, when it sent one, its error code.
    #[error("vendor refused the request with status {status}")]
    Vendor {
        /// The HTTP status.
        status: u16,
        /// The vendor's numeric error code (Twilio's `code`), if any.
        code: Option<i64>,
    },
    /// The vendor's success body did not have the expected shape.
    #[error("vendor response did not parse")]
    Decode,
    /// A vendor ID passed in is not of the vendor's form, so it never reaches a URL path.
    #[error("vendor ID has the wrong form")]
    InvalidId,
}

/// Builds the texting, voice and Verify clients from one checked config; the
/// mode in it decided the base URLs, so this is the only place a client is
/// chosen and there is no separate fake (D-453, SEC-032).
///
/// # Errors
///
/// [`AdapterError::Transport`] when the HTTP client cannot be built.
pub fn twilio_clients(
    config: &TwilioConfig,
) -> Result<(TwilioSms, TwilioVoice, TwilioVerify), AdapterError> {
    let http = TwilioHttp::new(config)?;
    Ok((
        TwilioSms { http: http.clone() },
        TwilioVoice { http: http.clone() },
        TwilioVerify { http },
    ))
}

/// Whether `id` is a Twilio SID with the given two-letter prefix and 32 hex or
/// alphanumeric characters, so it is safe in a URL path.
pub(crate) fn is_sid(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|tail| tail.len() == 32 && tail.bytes().all(|b| b.is_ascii_alphanumeric()))
}

#[derive(serde::Deserialize)]
struct TwilioErrorBody {
    code: Option<i64>,
}

/// One HTTP client for the Twilio APIs, shared by the texting, voice and
/// Verify adapters; cheap to clone.
#[derive(Debug, Clone)]
pub(crate) struct TwilioHttp {
    client: reqwest::Client,
    pub(crate) api_base: Arc<str>,
    pub(crate) verify_base: Arc<str>,
    pub(crate) account_sid: Arc<str>,
    pub(crate) verify_service: Arc<str>,
    auth_token: Arc<SecretString>,
}

impl TwilioHttp {
    pub(crate) fn new(config: &TwilioConfig) -> Result<Self, AdapterError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|_| AdapterError::Transport)?;
        Ok(Self {
            client,
            api_base: config.api_base().into(),
            verify_base: config.verify_base().into(),
            account_sid: config.account_sid().into(),
            verify_service: config.verify_service().into(),
            auth_token: Arc::new(config.auth_token().clone()),
        })
    }

    /// The Messages and Calls root: `<api_base>/2010-04-01/Accounts/<sid>`.
    pub(crate) fn account_url(&self) -> String {
        format!("{}/2010-04-01/Accounts/{}", self.api_base, self.account_sid)
    }

    /// POSTs a form with Basic auth; `key` goes in `Idempotency-Key` and, for
    /// texts, in `X-Askhava-Message-Key` too, the derived key the sent log is
    /// queried by after a restore (OPS-017; SEC-018).
    pub(crate) async fn post_form<T: DeserializeOwned>(
        &self,
        url: &str,
        form: &[(&str, &str)],
        key: Option<&IdempotencyKey>,
    ) -> Result<T, AdapterError> {
        let mut request = self
            .client
            .post(url)
            .basic_auth(&*self.account_sid, Some(self.auth_token.expose_secret()))
            .form(form);
        if let Some(key) = key {
            request = request
                .header("Idempotency-Key", key.as_str())
                .header("X-Askhava-Message-Key", key.as_str());
        }
        let response = request.send().await.map_err(|_| AdapterError::Transport)?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AdapterError::Transport)?;
        if !status.is_success() {
            let code = serde_json::from_slice::<TwilioErrorBody>(&bytes)
                .ok()
                .and_then(|body| body.code);
            return Err(AdapterError::Vendor {
                status: status.as_u16(),
                code,
            });
        }
        serde_json::from_slice(&bytes).map_err(|_| AdapterError::Decode)
    }
}
