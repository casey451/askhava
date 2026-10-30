//! Forged webhooks are refused in the adapter before any module sees them (TD-SEC-03, TD-SEC-18,
//! TD-SEC-27; SEC-016, SEC-039, SEC-049, SEC-051; D-820).
//!
//! Each vendor's genuine delivery passes and a forged signature, a tampered body or a missing
//! header is refused; `refuse` answers 403 and writes one log line naming only the vendor and the
//! reason. The Stripe source check takes only the committed `ips_webhooks.json` addresses in
//! sandbox and live and only the stand-in's host in stand-in mode, from a list compiled in, never
//! fetched (the route's address cases are TD-SEC-31's). All secrets, numbers and IDs are fake.

use std::io::Write;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterMode, Delivery, SignatureError, VerifiedWebhook, WebhookVendor, check_webhook_source,
    refuse, verify_webhook, webhook_source_list,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::SecretString;
use sha1::Sha1;
use sha2::Sha256;

const NOW: i64 = 1_793_000_000;
const URL: &str = "https://hooks.example.test/twilio/status";
const TWILIO_BODY: &[u8] =
    b"MessageSid=SM00000000000000000000000000000001&MessageStatus=sent&To=%2B15005550006";
const STRIPE_SECRET: &str = "whsec_fake_stripe_secret";
const EVENT: &[u8] = br#"{"id":"evt_fake_001","type":"payment_intent.succeeded"}"#;
/// The fake Svix key; the `whsec_` text is built at run time so no secret-shaped literal is
/// committed.
const SVIX_KEY: &[u8] = b"fake-svix-key-for-tests";

/// A delivery's headers as (name, value) pairs.
type Headers<'a> = &'a [(&'a str, &'a str)];

/// The adapter's check of one delivery posted to [`URL`] at [`NOW`].
fn check(
    vendor: WebhookVendor,
    secret: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) -> Result<VerifiedWebhook, SignatureError> {
    check_at(vendor, secret, URL, body, headers)
}

fn check_at(
    vendor: WebhookVendor,
    secret: &str,
    url: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) -> Result<VerifiedWebhook, SignatureError> {
    let delivery = Delivery { url, body, headers };
    verify_webhook(vendor, &SecretString::from(secret), delivery, NOW)
}

fn sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).unwrap();
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn twilio_sig(token: &str, data: &str) -> String {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(data.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

fn stripe_header(secret: &str, t: i64, body: &[u8]) -> String {
    let message = [format!("{t}.").as_bytes(), body].concat();
    format!("t={t},v1={}", hex(&sha256(secret.as_bytes(), &message)))
}

fn svix_sig(id: &str, t: i64, body: &[u8]) -> String {
    let message = [format!("{id}.{t}.").as_bytes(), body].concat();
    format!("v1,{}", STANDARD.encode(sha256(SVIX_KEY, &message)))
}

/// TD-SEC-03; SEC-016: Twilio's signature covers the URL and every sorted field.
#[test]
fn forged_twilio_signature_is_refused() {
    let (twilio, token) = (WebhookVendor::Twilio, "fake_twilio_auth_token");
    let data =
        format!("{URL}MessageSidSM00000000000000000000000000000001MessageStatussentTo+15005550006");
    let good = twilio_sig(token, &data);
    let ok = check(twilio, token, TWILIO_BODY, &[("X-Twilio-Signature", &good)]).unwrap();
    assert_eq!(ok.vendor(), WebhookVendor::Twilio);
    assert!(ok.event_key().is_none());
    assert!(ok.params().contains(&("To".into(), "+15005550006".into())));
    let forged = twilio_sig("another_token", &data);
    let tampered =
        b"MessageSid=SM00000000000000000000000000000001&MessageStatus=delivered&To=%2B15005550006";
    let cases: [(&[u8], Headers<'_>, SignatureError); 4] = [
        (
            TWILIO_BODY,
            &[("x-twilio-signature", &forged)],
            SignatureError::Forged,
        ),
        (
            tampered,
            &[("x-twilio-signature", &good)],
            SignatureError::Forged,
        ),
        (TWILIO_BODY, &[], SignatureError::Missing),
        (
            TWILIO_BODY,
            &[("x-twilio-signature", "not base64!")],
            SignatureError::Missing,
        ),
    ];
    for (body, headers, want) in cases {
        assert_eq!(check(twilio, token, body, headers).unwrap_err(), want);
    }
    let other_url = "https://hooks.example.test/twilio/inbound";
    let moved = check_at(
        twilio,
        token,
        other_url,
        TWILIO_BODY,
        &[("x-twilio-signature", &good)],
    );
    assert_eq!(moved.unwrap_err(), SignatureError::Forged);
}

/// TD-SEC-03; SEC-016: a Stripe signature under another secret, or over another body, is forged;
/// during a secret roll any matching `v1` passes.
#[test]
fn forged_stripe_signature_is_refused() {
    let stripe = WebhookVendor::Stripe;
    let run = |body: &[u8], header: &str| {
        check(stripe, STRIPE_SECRET, body, &[("Stripe-Signature", header)])
    };
    let good = stripe_header(STRIPE_SECRET, NOW, EVENT);
    let event = run(EVENT, &good).unwrap();
    assert_eq!(event.event_key().unwrap().as_str(), "evt_fake_001");
    assert_eq!(event.body(), EVENT);
    let forged = stripe_header("whsec_not_ours", NOW, EVENT);
    let tampered = br#"{"id":"evt_fake_001","type":"charge.refunded"}"#;
    assert_eq!(run(EVENT, &forged).unwrap_err(), SignatureError::Forged);
    assert_eq!(run(tampered, &good).unwrap_err(), SignatureError::Forged);
    for header in ["", "v1=00", "t=1,v1=zz"] {
        assert_eq!(run(EVENT, header).unwrap_err(), SignatureError::Missing);
    }
    let none = check(stripe, STRIPE_SECRET, EVENT, &[]).unwrap_err();
    assert_eq!(none, SignatureError::Missing);
    let v1 = good.split_once(",v1=").unwrap().1;
    let rolled = format!("t={NOW},v1={},v1={v1}", "ab".repeat(32));
    assert!(run(EVENT, &rolled).is_ok());
    let no_id = br#"{"type":"payment_intent.succeeded"}"#;
    let header = stripe_header(STRIPE_SECRET, NOW, no_id);
    assert_eq!(run(no_id, &header).unwrap_err(), SignatureError::NoEventId);
}

/// TD-SEC-18, TD-SEC-27; SEC-039, SEC-051: a forged, tampered or re-labelled Svix delivery is
/// refused.
#[test]
fn forged_svix_webhook_is_refused() {
    let resend = WebhookVendor::Resend;
    let secret = format!("whsec_{}", STANDARD.encode(SVIX_KEY));
    let body = br#"{"type":"email.bounced","data":{"email_id":"em_fake_1"}}"#;
    let sig = svix_sig("msg_fake_1", NOW, body);
    let stamp = NOW.to_string();
    let headers = |id| {
        [
            ("svix-id", id),
            ("svix-timestamp", stamp.as_str()),
            ("svix-signature", sig.as_str()),
        ]
    };
    let ok = check(resend, &secret, body, &headers("msg_fake_1")).unwrap();
    assert_eq!(ok.event_key().unwrap().as_str(), "msg_fake_1");
    let wrong_key = format!("whsec_{}", STANDARD.encode(b"other-key"));
    let tampered = br#"{"type":"email.delivered","data":{"email_id":"em_fake_1"}}"#;
    let refused = [
        check(resend, &wrong_key, body, &headers("msg_fake_1")),
        check(resend, &secret, tampered, &headers("msg_fake_1")),
        check(resend, &secret, body, &headers("msg_fake_2")),
    ];
    for result in refused {
        assert_eq!(result.unwrap_err(), SignatureError::Forged);
    }
    let missing = check(resend, &secret, body, &headers("msg_fake_1")[..2]);
    assert_eq!(missing.unwrap_err(), SignatureError::Missing);
}

/// TD-SEC-18; SEC-039: a forged or tampered Lob delivery is refused.
#[test]
fn forged_lob_webhook_is_refused() {
    let (lob, secret) = (WebhookVendor::Lob, "fake_lob_webhook_secret");
    let body = br#"{"id":"evt_lob_fake_1","event_type":{"id":"letter.delivered"}}"#;
    let message = [format!("{NOW}.").as_bytes(), body].concat();
    let sig = hex(&sha256(secret.as_bytes(), &message));
    let stamp = NOW.to_string();
    let headers = |sig| {
        [
            ("Lob-Signature", sig),
            ("Lob-Signature-Timestamp", stamp.as_str()),
        ]
    };
    let ok = check(lob, secret, body, &headers(&sig)).unwrap();
    assert_eq!(ok.event_key().unwrap().as_str(), "evt_lob_fake_1");
    let forged = hex(&sha256(b"not_the_secret", &message));
    let tampered = br#"{"id":"evt_lob_fake_1","event_type":{"id":"letter.returned"}}"#;
    let forged = check(lob, secret, body, &headers(&forged));
    assert_eq!(forged.unwrap_err(), SignatureError::Forged);
    let tampered = check(lob, secret, tampered, &headers(&sig));
    assert_eq!(tampered.unwrap_err(), SignatureError::Forged);
    let missing = check(lob, secret, body, &headers(&sig)[..1]);
    assert_eq!(missing.unwrap_err(), SignatureError::Missing);
}

/// A writer that keeps every log line for the assertion.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// TD-SEC-03; SEC-016: a refused delivery answers 403 and leaves one log line with the vendor and
/// reason only, never the signature or body.
#[test]
fn refused_delivery_gets_403_and_one_log_line() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer.clone())
        .finish();
    let status = tracing::subscriber::with_default(subscriber, || {
        refuse(WebhookVendor::Stripe, SignatureError::Forged)
    });
    assert_eq!(status, 403);
    let held = lines.0.lock().unwrap_or_else(PoisonError::into_inner);
    let text = String::from_utf8(held.clone()).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    let line: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(line["level"], "WARN");
    assert_eq!(line["fields"]["vendor"], "stripe");
    assert_eq!(line["fields"]["reason"], "forged");
    assert_eq!(line["fields"]["message"], "webhook refused");
}

/// TD-SEC-27, TD-SEC-31's adapter half; SEC-049: sandbox and live take only Stripe's listed
/// addresses, stand-in mode only the stand-in's host; the list is the committed file, compiled in.
#[test]
fn stripe_source_is_checked_against_the_committed_list() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../app/adapters/data/stripe_webhook_ips.json"
    );
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let listed: Vec<IpAddr> = file["WEBHOOKS"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ip| ip.as_str().unwrap().parse().unwrap())
        .collect();
    assert!(!listed.is_empty());
    assert_eq!(webhook_source_list(), listed.as_slice());
    let stripe: IpAddr = "3.18.12.63".parse().unwrap();
    let mapped: IpAddr = "::ffff:3.18.12.63".parse().unwrap();
    let outsider: IpAddr = "198.51.100.7".parse().unwrap();
    let stand_in: IpAddr = "127.0.0.1".parse().unwrap();
    for mode in [AdapterMode::Sandbox, AdapterMode::Live] {
        assert!(check_webhook_source(mode, stripe, stand_in).is_ok());
        assert!(check_webhook_source(mode, mapped, stand_in).is_ok());
        let refused = check_webhook_source(mode, outsider, stand_in).unwrap_err();
        assert_eq!(refused, SignatureError::Source);
        assert!(check_webhook_source(mode, stand_in, stand_in).is_err());
    }
    let mode = AdapterMode::StandIn;
    assert!(check_webhook_source(mode, stand_in, stand_in).is_ok());
    assert!(check_webhook_source(mode, stripe, stand_in).is_err());
    assert!(check_webhook_source(mode, outsider, stand_in).is_err());
}

/// TD-SEC-27; SEC-049: the adapter source never fetches Stripe's list; it includes the file.
#[test]
fn stripe_list_is_never_fetched_at_startup() {
    let source = include_str!("../../../app/adapters/src/signatures.rs");
    assert!(source.contains("include_str!(\"../data/stripe_webhook_ips.json\")"));
    assert!(!source.contains("stripe.com/files"));
    assert!(!source.contains("reqwest"));
}
