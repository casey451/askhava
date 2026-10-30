//! The email adapter: Resend behind one trait (SEC-018, SEC-032, BIZ-056).
//!
//! Sends one rendered email (the From, footer, wording and language are
//! `messaging`'s work before it calls here) and removes one address from the
//! provider's team-wide suppression list before a re-verification email
//! (BIZ-056; the caller writes the audit row). The suppression path is
//! `DELETE /suppressions/{address}`, the shape T133's fake Resend serves (D-492,
//! D-725). An address is percent-encoded as one path segment.

use std::future::Future;
use std::sync::Arc;

use reqwest::Method;
use secrecy::SecretString;

use crate::base::{AdapterError, Auth, IdempotencyKey, VendorHttp};

/// One outgoing email, rendered. Not `Debug`: it holds an address (R6).
#[derive(serde::Serialize)]
pub struct Email<'a> {
    /// `"Ask Hava" <jobs@notify.askhava.com>`.
    pub from: &'a str,
    /// The one recipient.
    pub to: &'a str,
    /// The monitored help address; never no-reply.
    pub reply_to: &'a str,
    /// The job or account fact.
    pub subject: &'a str,
    /// The HTML part.
    pub html: &'a str,
    /// The plain-text part.
    pub text: &'a str,
}

#[derive(serde::Deserialize)]
struct Sent {
    id: String,
}

/// Sending email and clearing a suppression (SEC-018 on both).
pub trait EmailAdapter {
    /// Sends `email`; returns the provider's email ID.
    fn send_email(
        &self,
        idempotency: &IdempotencyKey,
        email: &Email<'_>,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    /// Removes `address` from the provider's suppression list (BIZ-056).
    fn remove_suppression(
        &self,
        idempotency: &IdempotencyKey,
        address: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;
}

/// The one HTTP Resend client (D-453).
#[derive(Debug, Clone)]
pub struct ResendEmail {
    http: VendorHttp,
}

impl ResendEmail {
    /// A client for `base_url` (already checked by `check_base_url`) and `key`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] for an unusable base URL.
    pub fn new(base_url: &str, key: SecretString) -> Result<Self, AdapterError> {
        let http = VendorHttp::new(base_url, Auth::Bearer(Arc::new(key)))?;
        Ok(Self { http })
    }
}

impl EmailAdapter for ResendEmail {
    async fn send_email(
        &self,
        idempotency: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let request = self
            .http
            .request(Method::POST, &["emails"], Some(idempotency));
        let sent: Sent = self.http.json(request.json(email)).await?;
        Ok(sent.id)
    }

    async fn remove_suppression(
        &self,
        idempotency: &IdempotencyKey,
        address: &str,
    ) -> Result<(), AdapterError> {
        let path = ["suppressions", address];
        let request = self.http.request(Method::DELETE, &path, Some(idempotency));
        self.http.send(request).await.map(drop)
    }
}
