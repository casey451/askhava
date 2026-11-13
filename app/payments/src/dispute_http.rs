//! [`DisputeStripe`] over the one HTTP Stripe client, `StripePayments` (FEE-027, FEE-050,
//! D-2305).
//!
//! `submit_evidence` sends the evidence pack's `evidence[…]` fields with `submit=true` through
//! the adapter's `submit_dispute_evidence` (`POST /v1/disputes/{id}`) under the key
//! `run_dispute_submit` derives from the dispute, so a rerun after a lost answer replays
//! Stripe's first answer and submits once. Nothing here retries.

use askhava_adapters::api::{AdapterError, IdempotencyKey, StripePayments};

use crate::disputes::DisputeStripe;

impl DisputeStripe for StripePayments {
    async fn submit_evidence(
        &self,
        idempotency: &IdempotencyKey,
        dispute: &str,
        form: &[(&'static str, String)],
    ) -> Result<(), AdapterError> {
        self.submit_dispute_evidence(idempotency, dispute, form)
            .await
    }
}

#[cfg(test)]
mod tests {
    //! TD-FEE-61 from Rust: `impl DisputeStripe for StripePayments` against the stand-in, and
    //! the dispute it opened read back through `ReconcileStripe` (fake data only, D-2305).

    use askhava_adapters::api::AdapterError;
    use jiff::Timestamp;

    use crate::disputes::DisputeStripe;
    use crate::reconcile::ReconcileStripe;
    use crate::reconcile_http::tests::{key, paid, stand_in};

    /// TD-FEE-61 (FEE-027, FEE-050): the listed charge names its dispute and amount; the
    /// evidence is submitted once per key, a repeat of the key replays, a new key is refused.
    #[tokio::test]
    async fn td_fee_61_the_evidence_is_submitted_once_per_key() {
        let server = stand_in();
        let stripe = server.stripe();
        let intent = paid(&stripe, "9f1c2d3e-0000-4000-8000-000000000002", 7500).await;
        let body = format!(r#"{{"payment_intent":"{intent}"}}"#);
        let opened = server.control("/_stand_in/stripe/disputes", &body);
        let dispute = opened["id"].as_str().expect("dp_ ID").to_owned();
        let listed = stripe
            .list_charges(Timestamp::UNIX_EPOCH)
            .await
            .expect("list");
        assert_eq!(listed[0].dispute, Some((dispute.clone(), 7500)));
        let form = [
            ("evidence[uncategorized_text]", "fake record".to_owned()),
            ("submit", "true".to_owned()),
        ];
        let first = key("dp-submit");
        stripe
            .submit_evidence(&first, &dispute, &form)
            .await
            .expect("submitted");
        stripe
            .submit_evidence(&first, &dispute, &form)
            .await
            .expect("replayed");
        let again = stripe
            .submit_evidence(&key("dp-submit-2"), &dispute, &form)
            .await;
        assert!(matches!(
            again,
            Err(AdapterError::Vendor { status: 400, .. })
        ));
        let missing = stripe.submit_evidence(&first, "ch_fake1", &form).await;
        assert_eq!(missing, Err(AdapterError::InvalidId));
    }
}
