//! [`ReconcileStripe`] over the one HTTP Stripe client, `StripePayments` (FEE-029, FEE-051,
//! D-2304, D-2305).
//!
//! `list_charges_page` is one `GET /v1/charges` page through the adapter's
//! `list_charge_page`, in Stripe's own order (newest first) and with Stripe's cursor (the last
//! charge listed, of any status); `list_charges` follows those pages to the end. Each listed
//! charge becomes a [`StripeCharge`]: a pick ID that is not a UUID is no pick, and a missing
//! PaymentIntent or balance transaction becomes an empty ID, which
//! [`StripeCharge::is_well_formed`] refuses, so the reconcile raises its mismatch alert rather
//! than guessing. `fetch_charge` keeps the trait's provided body.

use askhava_adapters::api::{AdapterError, ListedCharge, StripePayments};
use jiff::Timestamp;
use sqlx::types::Uuid;

use crate::models::StripeCharge;
use crate::reconcile::{ChargePage, PAGE_SIZE, ReconcileStripe};

/// FEE-029, FEE-051: one listed charge as the reconcile reads it.
fn charge_of(listed: ListedCharge) -> StripeCharge {
    let (balance_transaction, fee_cents) = listed.balance_transaction.unwrap_or_default();
    StripeCharge {
        id: listed.id,
        payment_intent: listed.payment_intent.unwrap_or_default(),
        pick: listed.pick_id.and_then(|p| Uuid::parse_str(&p).ok()),
        amount_cents: listed.amount,
        refunded_cents: listed.amount_refunded,
        dispute: listed.dispute,
        balance_transaction,
        fee_cents,
    }
}

impl ReconcileStripe for StripePayments {
    async fn list_charges(&self, since: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        let mut all = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self
                .list_charges_page(since, after.as_deref(), PAGE_SIZE)
                .await?;
            all.extend(page.charges);
            match page.next {
                Some(next) => after = Some(next),
                None => return Ok(all),
            }
        }
    }

    async fn list_charges_page(
        &self,
        since: Timestamp,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ChargePage, AdapterError> {
        let page = self
            .list_charge_page(since.as_second(), after, limit.clamp(1, PAGE_SIZE))
            .await?;
        Ok(ChargePage {
            charges: page.charges.into_iter().map(charge_of).collect(),
            next: page.next,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! TD-FEE-61 from Rust: `impl ReconcileStripe for StripePayments` against
    //! `python3 -m stand_ins.server --port 0` (fake data only, D-2305). [`StandIn`] is shared
    //! with `dispute_http`'s tests.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::process::{Child, Command, Stdio};

    use askhava_adapters::api::{IdempotencyKey, ListedCharge, PaymentsAdapter, StripePayments};
    use jiff::Timestamp;
    use secrecy::SecretString;
    use serde_json::Value;

    use super::charge_of;
    use crate::reconcile::ReconcileStripe;

    /// The stand-in on the free port it bound itself, killed on drop.
    pub(crate) struct StandIn(Child, pub(crate) String);

    impl Drop for StandIn {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Starts the stand-in with `--port 0` and reads its base URL from the "listening" line.
    pub(crate) fn stand_in() -> StandIn {
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
        StandIn(child, base)
    }

    impl StandIn {
        /// The Stripe client for this stand-in.
        pub(crate) fn stripe(&self) -> StripePayments {
            let base = format!("{}/stripe", self.1);
            StripePayments::new(&base, SecretString::from("sk_test_fake")).expect("client")
        }

        /// A stand-in control call (`/_stand_in/...`, JSON body) as plain HTTP/1.1; its JSON.
        pub(crate) fn control(&self, path: &str, body: &str) -> Value {
            let address = self.1.trim_start_matches("http://");
            let request = format!(
                "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = TcpStream::connect(address).expect("connect");
            stream.write_all(request.as_bytes()).expect("send");
            let mut reply = String::new();
            stream.read_to_string(&mut reply).expect("reply");
            assert!(reply.contains(" 200 "), "{reply}");
            let (_, json) = reply.split_once("\r\n\r\n").expect("body");
            serde_json::from_str(json).expect("json")
        }
    }

    pub(crate) fn key(label: &str) -> IdempotencyKey {
        IdempotencyKey::derive(&SecretString::from("fake-id-secret"), "t335", &[label])
    }

    /// A paid fake pick: its customer and a PaymentIntent charged at once; the intent's ID.
    pub(crate) async fn paid(stripe: &StripePayments, pick: &str, cents: u32) -> String {
        let customer = stripe
            .create_customer(&key(&format!("{pick}-customer")), "biz-fake-1")
            .await
            .expect("customer");
        let charge = askhava_adapters::api::Charge {
            customer: &customer,
            payment_method: "pm_card_visa",
            amount_cents: cents,
            pick_id: pick,
        };
        let intent = stripe
            .create_payment_intent(&key(&format!("{pick}-intent")), charge)
            .await
            .expect("intent");
        assert_eq!(intent.status, "succeeded");
        intent.id
    }

    /// TD-FEE-61 (FEE-029, FEE-051): pages of one follow the cursor to the end, newest first,
    /// with each charge's fee, refund and pick; the full list holds the same charges.
    #[tokio::test]
    async fn td_fee_61_the_pages_follow_the_cursor_with_fee_refund_and_pick() {
        let server = stand_in();
        let stripe = server.stripe();
        let picks = ["9f1c2d3e-0000-4000-8000-000000000001", "not-a-uuid"];
        let first = paid(&stripe, picks[0], 7500).await;
        let second = paid(&stripe, picks[1], 2500).await;
        stripe
            .send_refund(&key("refund-1"), &first)
            .await
            .expect("refund");
        let since = Timestamp::UNIX_EPOCH;
        let one = stripe.list_charges_page(since, None, 1).await.expect("one");
        assert_eq!(one.charges[0].payment_intent, second);
        assert_eq!(one.charges[0].pick, None);
        let after = one.next.clone().expect("a next page");
        let two = stripe
            .list_charges_page(since, Some(&after), 1)
            .await
            .expect("two");
        let charge = &two.charges[0];
        assert_eq!(two.next, None);
        assert_eq!(
            (
                charge.payment_intent.as_str(),
                charge.pick.map(|p| p.to_string())
            ),
            (first.as_str(), Some(picks[0].to_owned()))
        );
        assert_eq!(
            (charge.amount_cents, charge.refunded_cents, charge.fee_cents),
            (7500, 7500, 248)
        );
        assert!(charge.is_well_formed() && charge.balance_transaction.starts_with("txn_"));
        let all = stripe.list_charges(since).await.expect("all");
        assert_eq!(all, [one.charges, two.charges].concat());
        let later = Timestamp::now() + jiff::SignedDuration::from_hours(1);
        assert!(stripe.list_charges(later).await.expect("none").is_empty());
    }

    /// FEE-030: a listed charge missing its balance transaction is malformed, never guessed.
    #[test]
    fn a_charge_without_a_balance_transaction_is_malformed() {
        let listed = ListedCharge {
            id: "ch_fake1".into(),
            payment_intent: Some("pi_fake1".into()),
            pick_id: None,
            amount: 2500,
            amount_refunded: 0,
            balance_transaction: None,
            dispute: None,
        };
        assert!(!charge_of(listed).is_well_formed());
    }
}
