//! The AI adapter: the Claude Messages API shape behind one trait, reachable
//! during the build only at the deterministic stand-in (AI-009, SEC-018).
//!
//! [`StandInAi`] is the only client: it accepts only an `http` base URL on a
//! loopback IP literal and sends the fixed non-key `stand-in`, so no real AI
//! key can be used before the Phase 2 Railway test project (AI-009); that
//! phase's task adds the real client. The request body is built by `ai` from `ScrubbedText` (AI-004) and
//! passed through unchanged; this crate never sees homeowner text otherwise.

use std::future::Future;
use std::sync::Arc;

use reqwest::Method;
use secrecy::SecretString;

use crate::base::{AdapterError, Auth, IdempotencyKey, VendorHttp};

/// The Messages API version header value.
const API_VERSION: &str = "2023-06-01";
/// What the stand-in client sends as `x-api-key`: not a key (AI-009).
const STAND_IN_KEY: &str = "stand-in";

/// Asking the model (AI-009; SEC-018 on every call).
pub trait AiAdapter {
    /// POSTs a Messages API `request` (built by `ai`) and returns the reply JSON.
    fn create_message(
        &self,
        idempotency: &IdempotencyKey,
        request: &serde_json::Value,
    ) -> impl Future<Output = Result<serde_json::Value, AdapterError>> + Send;
}

/// The AI client of the build: the deterministic stand-in only (AI-009).
#[derive(Debug, Clone)]
pub struct StandInAi {
    http: VendorHttp,
}

impl StandInAi {
    /// A client for the stand-in at `base_url` (AI-009).
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] unless `base_url` is `http` on a loopback
    /// IP literal (`127.0.0.0/8` or `[::1]`, never a name such as `localhost`
    /// that DNS or a hosts file could point elsewhere), so the vendor's real
    /// API can never be reached with it.
    pub fn new(base_url: &str) -> Result<Self, AdapterError> {
        let url = reqwest::Url::parse(base_url).map_err(|_| AdapterError::BadConfig)?;
        let host = url.host_str().unwrap_or_default();
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let loopback = host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
        if url.scheme() != "http" || !loopback {
            return Err(AdapterError::BadConfig);
        }
        let key = Arc::new(SecretString::from(STAND_IN_KEY));
        let http = VendorHttp::new(base_url, Auth::Header("x-api-key", key))?;
        Ok(Self { http })
    }
}

impl AiAdapter for StandInAi {
    async fn create_message(
        &self,
        idempotency: &IdempotencyKey,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, AdapterError> {
        let call = self
            .http
            .request(Method::POST, &["v1", "messages"], Some(idempotency));
        let call = call.header("anthropic-version", API_VERSION).json(request);
        self.http.json(call).await
    }
}
