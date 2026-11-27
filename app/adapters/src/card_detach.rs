//! The Stripe card detach a leaving business's saved cards go through (BIZ-051; T734, D-4727,
//! D-5670).
//!
//! [`StripeCardDetach`] sends `POST /v1/payment_methods/{pm}/detach` with the same client the
//! payments adapter builds (the configured base URL, the secret key as `Authorization: Bearer`,
//! the 15-second timeout, no redirects, no proxy; SEC-032). The `Idempotency-Key` is derived from
//! the payment method's ID under the secret key itself, so a repeated detach reuses it and the key
//! never carries personal data (SEC-018). The key never reaches `Debug`, an error or a log line
//! (R6). `adapters` names no domain crate (`kit/layers.json`), so this is a plain call: the
//! composition wraps it in `portals`' `CardDetach` (`server::business_leave_routes`).

use std::sync::Arc;

use reqwest::Method;
use secrecy::SecretString;

use crate::base::{AdapterError, Auth, IdempotencyKey, VendorHttp, is_vendor_id};

/// The idempotency action a detach's key is derived under (SEC-018).
const DETACH_ACTION: &str = "stripe.payment_method.detach";

/// BIZ-051: the Stripe client that detaches a saved card from its customer.
#[derive(Clone)]
pub struct StripeCardDetach {
    http: VendorHttp,
    key: Arc<SecretString>,
}

impl std::fmt::Debug for StripeCardDetach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeCardDetach").finish_non_exhaustive()
    }
}

impl StripeCardDetach {
    /// A client for `base_url` (already checked by `check_base_url`) and the secret `key`.
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] for an unusable base URL.
    pub fn new(base_url: &str, key: SecretString) -> Result<Self, AdapterError> {
        let key = Arc::new(key);
        let http = VendorHttp::new(base_url, Auth::Bearer(Arc::clone(&key)))?;
        Ok(Self { http, key })
    }

    /// BIZ-051, SEC-018: detaches `payment_method` (`pm_…`) from its customer at Stripe; the
    /// answer's body is not kept (no card data, FEE-014).
    ///
    /// # Errors
    ///
    /// [`AdapterError::InvalidId`] for an ID that is not `pm_…` (nothing sent); the HTTP errors,
    /// a Stripe refusal as [`AdapterError::Vendor`] with its status only.
    pub async fn detach_card(&self, payment_method: &str) -> Result<(), AdapterError> {
        if !is_vendor_id(payment_method, "pm_") {
            return Err(AdapterError::InvalidId);
        }
        let idempotency = IdempotencyKey::derive(&self.key, DETACH_ACTION, &[payment_method]);
        let path = ["v1", "payment_methods", payment_method, "detach"];
        let request = self.http.request(Method::POST, &path, Some(&idempotency));
        self.http.send(request.form(&[] as &[(&str, &str)])).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! BIZ-051: the ID check sends nothing; `Debug` holds no key. Fake key only.

    use super::*;

    fn client() -> StripeCardDetach {
        let key = SecretString::from("sk_test_fake_t734_key".to_owned());
        StripeCardDetach::new("http://127.0.0.1:9/stripe", key).expect("client")
    }

    #[tokio::test]
    async fn a_non_payment_method_id_is_refused_before_sending() {
        for bad in ["cus_Fake1", "pm_", "pm_a/b", ""] {
            assert_eq!(
                client().detach_card(bad).await,
                Err(AdapterError::InvalidId),
                "{bad}"
            );
        }
    }

    #[test]
    fn debug_holds_no_key() {
        assert!(!format!("{:?}", client()).contains("sk_test"));
    }

    #[test]
    fn a_bad_base_url_is_bad_config() {
        let key = SecretString::from("sk_test_fake".to_owned());
        assert!(matches!(
            StripeCardDetach::new("not a url", key),
            Err(AdapterError::BadConfig)
        ));
    }
}
