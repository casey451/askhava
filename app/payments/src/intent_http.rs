//! [`IntentStripe`] over the one HTTP Stripe client, `StripePayments` (FEE-014, FEE-016,
//! FEE-036, FEE-047, D-1424, D-1754).
//!
//! `create_intent` posts the pick's PaymentIntent with `confirm=false` and returns its `pi_…`;
//! `confirm_intent` confirms it on-session under one attempt's key and returns the intent as
//! Stripe left it. Stripe's 402 decline stays an `AdapterError::Vendor { status: 402 }`, which
//! `confirm_and_pay` reads as a decline; nothing here retries (FEE-016). The client secret the
//! adapter returns is dropped here: `IntentStripe` answers only the IDs and the status, and the
//! Confirm route (T261) reads the secret from the adapter for a bank approval (D-1482).

use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentIntent, StripePayments};

use crate::charge::IntentStripe;

impl IntentStripe for StripePayments {
    async fn create_intent(
        &self,
        idempotency: &IdempotencyKey,
        customer: &str,
        amount_cents: u32,
        pick_id: &str,
    ) -> Result<String, AdapterError> {
        let made = self
            .create_unconfirmed_intent(idempotency, customer, amount_cents, pick_id)
            .await?;
        Ok(made.id)
    }

    async fn confirm_intent(
        &self,
        idempotency: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        let answer = self
            .confirm_payment_intent(idempotency, intent, payment_method)
            .await?;
        Ok(answer.intent)
    }
}

#[cfg(test)]
mod tests {
    //! TD-FEE-44 from Rust: `impl IntentStripe for StripePayments` against
    //! `python3 -m stand_ins.server --port 0` (fake data only, D-1754).

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::process::{Child, Command, Stdio};

    use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentsAdapter, StripePayments};
    use secrecy::{ExposeSecret, SecretString};

    use crate::charge::IntentStripe;

    /// The stand-in on the free port it bound itself, killed on drop.
    struct StandIn(Child, String);

    impl Drop for StandIn {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Starts the stand-in with `--port 0` and reads the port from its "listening" line, so no
    /// other process can take the port between a look-up and the bind.
    fn stand_in() -> StandIn {
        let mut child = Command::new("python3")
            .args(["-m", "stand_ins.server", "--port", "0"])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .stdout(Stdio::piped())
            .spawn()
            .expect("python3 starts the stand-in");
        let mut line = String::new();
        let stdout = child.stdout.take().expect("piped stdout");
        BufReader::new(stdout)
            .read_line(&mut line)
            .expect("listening line");
        let base = line
            .trim_end()
            .strip_prefix("stand-ins listening on ")
            .expect("the stand-in's listening line")
            .to_owned();
        StandIn(child, format!("{base}/stripe"))
    }

    fn key(label: &str) -> IdempotencyKey {
        IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "t260", &[label])
    }

    async fn opened(server: &StandIn, pick: &str) -> (StripePayments, String) {
        let stripe =
            StripePayments::new(&server.1, SecretString::from("sk_test_fake")).expect("client");
        let customer = stripe
            .create_customer(&key(&format!("{pick}-customer")), "biz-fake-1")
            .await
            .expect("customer");
        let intent = stripe
            .create_intent(&key(&format!("{pick}-intent")), &customer, 7500, pick)
            .await
            .expect("intent");
        (stripe, intent)
    }

    /// TD-FEE-44 (FEE-036, FEE-047): the intent is made unconfirmed with a client secret and
    /// charges nothing; the same key reaches the same intent.
    #[tokio::test]
    async fn td_fee_44_an_unconfirmed_intent_has_a_secret_and_no_charge() {
        let server = stand_in();
        let (stripe, intent) = opened(&server, "pick-fake-1").await;
        let customer = stripe
            .create_customer(&key("pick-fake-1-customer"), "biz-fake-1")
            .await
            .expect("customer");
        let made = stripe
            .create_unconfirmed_intent(&key("pick-fake-1-intent"), &customer, 7500, "pick-fake-1")
            .await
            .expect("replayed");
        assert_eq!(
            (made.id.as_str(), made.status.as_str()),
            (intent.as_str(), "requires_payment_method")
        );
        assert!(made.client_secret.expose_secret().starts_with(&intent));
        assert!(!format!("{made:?}").contains("_secret_"));
        let fetched = stripe.fetch_payment_intent(&intent).await.expect("fetch");
        assert_eq!(
            (fetched.status.as_str(), fetched.metadata.pick_id.as_str()),
            ("requires_payment_method", "pick-fake-1")
        );
        let refused = stripe
            .create_intent(&key("x"), "biz-fake-1", 7500, "pick-fake-1")
            .await;
        assert_eq!(refused, Err(AdapterError::InvalidId));
    }

    /// TD-FEE-44 (FEE-016): the success card succeeds once per key; a repeat answers the same.
    #[tokio::test]
    async fn td_fee_44_the_success_card_succeeds_once_per_key() {
        let server = stand_in();
        let (stripe, intent) = opened(&server, "pick-fake-2").await;
        let paid = stripe
            .confirm_intent(&key("pick-fake-2-try1"), &intent, "pm_card_visa")
            .await
            .expect("paid");
        assert_eq!(
            (paid.status.as_str(), paid.amount, paid.id.as_str()),
            ("succeeded", 7500, intent.as_str())
        );
        let again = stripe
            .confirm_intent(&key("pick-fake-2-try1"), &intent, "pm_card_visa")
            .await
            .expect("replayed");
        assert_eq!(again, paid);
        let bad = stripe
            .confirm_intent(&key("y"), "ch_fake", "pm_card_visa")
            .await;
        assert_eq!(bad, Err(AdapterError::InvalidId));
    }

    /// TD-FEE-44 (FEE-016): the decline card is Stripe's 402; a new key then pays.
    #[tokio::test]
    async fn td_fee_44_the_decline_card_declines() {
        let server = stand_in();
        let (stripe, intent) = opened(&server, "pick-fake-3").await;
        let declined = stripe
            .confirm_intent(&key("pick-fake-3-try1"), &intent, "pm_card_chargeDeclined")
            .await;
        assert!(matches!(
            declined,
            Err(AdapterError::Vendor { status: 402, .. })
        ));
        let paid = stripe
            .confirm_intent(&key("pick-fake-3-try2"), &intent, "pm_card_visa")
            .await
            .expect("second try");
        assert_eq!(paid.status, "succeeded");
    }

    /// TD-FEE-44 (FEE-014): the 3-D Secure card answers `requires_action` with the client
    /// secret; the stand-in's bank approval then settles it as passed or failed.
    #[tokio::test]
    async fn td_fee_44_the_3ds_card_needs_the_bank_approval() {
        let server = stand_in();
        for (pick, result, settled) in [
            ("pick-fake-4", "pass", "succeeded"),
            ("pick-fake-5", "fail", "requires_payment_method"),
        ] {
            let (stripe, intent) = opened(&server, pick).await;
            let wants = stripe
                .confirm_payment_intent(
                    &key(&format!("{pick}-try1")),
                    &intent,
                    "pm_card_authenticationRequired",
                )
                .await
                .expect("requires action");
            assert_eq!(wants.intent.status, "requires_action");
            let secret = wants.client_secret.expect("secret for the approval");
            assert!(secret.expose_secret().starts_with(&intent));
            approve(&server, &intent, result);
            let after = stripe.fetch_payment_intent(&intent).await.expect("fetch");
            assert_eq!(after.status, settled);
        }
    }

    /// The bank's answer through the stand-in-only route (the browser's step in real Stripe),
    /// as one plain HTTP/1.1 request; the reply must be 200.
    fn approve(server: &StandIn, intent: &str, result: &str) {
        let address = server
            .1
            .trim_start_matches("http://")
            .trim_end_matches("/stripe");
        let body = format!("result={result}");
        let request = format!(
            "POST /stripe/v1/payment_intents/{intent}/stand_in_authenticate HTTP/1.1\r\n\
             Host: {address}\r\nAuthorization: Bearer sk_test_fake\r\n\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut stream = TcpStream::connect(address).expect("connect");
        stream.write_all(request.as_bytes()).expect("send");
        let mut status = String::new();
        BufReader::new(stream)
            .read_line(&mut status)
            .expect("status line");
        assert!(
            status.starts_with("HTTP/1.0 200") || status.starts_with("HTTP/1.1 200"),
            "{status}"
        );
    }
}
