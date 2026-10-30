//! The letters adapter: Lob behind one trait (SEC-018, SEC-032).
//!
//! Sends the identity letter to a business address and reads its status back.
//! The letter body (addresses, file, colour) is built by the caller as Lob's
//! JSON; Lob's Basic auth carries the key as user (D-453).

use std::future::Future;
use std::sync::Arc;

use reqwest::Method;
use secrecy::SecretString;

use crate::base::{AdapterError, Auth, IdempotencyKey, VendorHttp, is_vendor_id};

/// What Lob says about a letter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct SentLetter {
    /// `ltr_…`.
    pub id: String,
    /// The expected delivery day, `YYYY-MM-DD`, when Lob gives one.
    #[serde(default)]
    pub expected_delivery_date: Option<String>,
}

/// Sending identity letters (SEC-018 on every send).
pub trait LettersAdapter {
    /// Sends `letter` (Lob's letter JSON).
    fn send_letter(
        &self,
        idempotency: &IdempotencyKey,
        letter: &serde_json::Value,
    ) -> impl Future<Output = Result<SentLetter, AdapterError>> + Send;

    /// Reads one letter back; `id` must be `ltr_…` or the call fails with
    /// [`AdapterError::InvalidId`] (SEC-032).
    fn fetch_letter(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<SentLetter, AdapterError>> + Send;
}

/// The one HTTP Lob client (D-453).
#[derive(Debug, Clone)]
pub struct LobLetters {
    http: VendorHttp,
}

impl LobLetters {
    /// A client for `base_url` (already checked by `check_base_url`) and `key`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] for an unusable base URL.
    pub fn new(base_url: &str, key: SecretString) -> Result<Self, AdapterError> {
        let http = VendorHttp::new(base_url, Auth::BasicUser(Arc::new(key)))?;
        Ok(Self { http })
    }
}

impl LettersAdapter for LobLetters {
    async fn send_letter(
        &self,
        idempotency: &IdempotencyKey,
        letter: &serde_json::Value,
    ) -> Result<SentLetter, AdapterError> {
        let request = self
            .http
            .request(Method::POST, &["v1", "letters"], Some(idempotency));
        self.http.json(request.json(letter)).await
    }

    async fn fetch_letter(&self, id: &str) -> Result<SentLetter, AdapterError> {
        if !is_vendor_id(id, "ltr_") {
            return Err(AdapterError::InvalidId);
        }
        let request = self.http.request(Method::GET, &["v1", "letters", id], None);
        self.http.json(request).await
    }
}
