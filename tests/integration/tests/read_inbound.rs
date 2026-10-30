//! The inbound reader from outside `privacy` (TD-PRIV-11, TD-AI-01; PRIV-026, INT-012, AI-002, AI-004, D-093).
//!
//! Proves through `privacy::api` alone that an inbound text yields flags and `ScrubbedText` only;
//! that inbound email is accepted only with a valid, fresh Svix signature; that a sentinel body's
//! contact details appear nowhere in what the reader returns; that a sender failing SPF, DKIM or
//! DMARC may only opt out; and that the email blind index matches a sender to a business and to a
//! homeowner under separate derivations. Storage in `privacy.message_body` and the day-31 cut are
//! TD-PRIV-11's later cases (D-801). All data, keys and secrets are fake.

use askhava_privacy::api::{
    CommonWords, EmailContext, InboundError, InboundWords, Known, NameMemory, SignedDelivery,
    Standing, Vault, email_index, read_inbound, read_inbound_email,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::Sha256;

const WORDS: InboundWords<'static> = InboundWords {
    emergency: &["gas", "smok", "carbon monoxide", "sparks"],
    opt_out: &["stop", "cancel", "unsubscribe", "parar"],
    help: &["help", "ayuda"],
    start: &["start"],
};
const SENTINEL: &str = "zq-sentinel-7f3a";
const NOW: i64 = 1_793_000_000;

fn secret() -> SecretString {
    SecretString::from(format!(
        "whsec_{}",
        STANDARD.encode(b"fake-resend-signing-key")
    ))
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava");
    Vault::new(&key(0x31), &key(0x41), &url).expect("vault keys")
}

fn sign(id: &str, stamp: &str, body: &[u8]) -> String {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(b"fake-resend-signing-key").expect("key");
    mac.update(format!("{id}.{stamp}.").as_bytes());
    mac.update(body);
    format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
}

fn body(from: &str, text: &str, dmarc: &str) -> Vec<u8> {
    let data = serde_json::json!({
        "type": "email.received",
        "data": {"from": from, "text": text, "auth": {"spf": "pass", "dkim": "pass", "dmarc": dmarc}}
    });
    serde_json::to_vec(&data).expect("json")
}

fn read(
    body: &[u8],
    stamp: i64,
    signature: Option<&str>,
) -> Result<askhava_privacy::api::InboundEmail, InboundError> {
    let (secret, vault, common) = (secret(), vault(), CommonWords::default());
    let stamp = stamp.to_string();
    let good = sign("msg_1", &stamp, body);
    let delivery = SignedDelivery {
        id: "msg_1",
        timestamp: &stamp,
        signature: signature.unwrap_or(&good),
        body,
    };
    let ctx = EmailContext {
        secret: &secret,
        vault: &vault,
        words: WORDS,
        known: Known {
            common: &common,
            vault_name: None,
        },
        now: Timestamp::from_second(NOW).expect("now"),
    };
    read_inbound_email(&delivery, &ctx, &mut NameMemory::default())
}

/// D-093, AI-004: an inbound text yields flags from the raw text and only scrubbed words.
#[test]
fn an_inbound_text_yields_flags_and_scrubbed_text() {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let mut memory = NameMemory::default();
    let gas = read_inbound(
        "I smell GAS, this is Pat, 928-555-0161",
        &WORDS,
        &known,
        &mut memory,
    );
    assert!(gas.flags.emergency && !gas.flags.opt_out);
    assert_eq!(gas.text.as_str(), "I smell GAS, this is [removed]");
    let calm = read_inbound("leak at the house", &WORDS, &known, &mut memory);
    assert!(!calm.flags.emergency);
    let cancel = read_inbound(" Cancel! ", &WORDS, &known, &mut memory);
    assert!(cancel.flags.opt_out && !cancel.flags.help);
    let not_sole = read_inbound("cancel my appointment", &WORDS, &known, &mut memory);
    assert!(!not_sole.flags.opt_out);
    assert!(
        read_inbound("AYUDA", &WORDS, &known, &mut memory)
            .flags
            .help
    );
    assert!(
        read_inbound("start", &WORDS, &known, &mut memory)
            .flags
            .start
    );
    let later = read_inbound("pat here again", &WORDS, &known, &mut memory);
    assert_eq!(
        later.text.as_str(),
        "[removed] here again",
        "INT-012: the stated name is kept for later turns"
    );
}

/// TD-PRIV-11: a signed email reads; the sentinel's contact details appear nowhere in the result.
#[test]
fn a_signed_email_reads_and_leaks_no_sentinel() {
    let text =
        format!("Thanks, I'll take it. Reach me {SENTINEL}@example.com or 928 555 0162. - Zqbert");
    let from = format!("Zqbert Sender <{SENTINEL}@Example.com>");
    let email = read(&body(&from, &text, "pass"), NOW - 10, None).expect("signed");
    assert_eq!(email.standing, Standing::Authenticated);
    let shown = format!("{email:?}");
    for leak in [SENTINEL, "0162", "zqbert", "Zqbert"] {
        assert!(!shown.contains(leak), "{leak} leaked: {shown}");
    }
}

/// TD-PRIV-11 (PRIV-026): a sender failing DMARC may opt out and may not act or change data.
#[test]
fn a_dmarc_failure_may_only_opt_out() {
    let email = read(&body("a@example.com", "UNSUBSCRIBE", "fail"), NOW, None).expect("signed");
    assert_eq!(email.standing, Standing::OptOutOnly);
    assert!(email.flags.opt_out);
    assert!(!email.auth.dmarc && email.auth.spf && email.auth.dkim);
    let change = read(
        &body("a@example.com", "new address: 12 Test Way", "none"),
        NOW,
        None,
    );
    assert_eq!(change.expect("signed").standing, Standing::OptOutOnly);
}

/// PRIV-026: a bad, missing or stale signature and a wrong event type are refused.
#[test]
fn unsigned_or_stale_deliveries_are_refused() {
    let good = body("a@example.com", "hello", "pass");
    let forged = sign("msg_2", &NOW.to_string(), &good);
    assert_eq!(
        read(&good, NOW, Some(&forged)).err(),
        Some(InboundError::Signature)
    );
    assert_eq!(
        read(&good, NOW, Some("")).err(),
        Some(InboundError::Signature)
    );
    assert_eq!(
        read(&good, NOW - 301, None).err(),
        Some(InboundError::Stale)
    );
    let other = br#"{"type":"email.sent","data":{"from":"a@example.com","text":"x","auth":{"spf":"pass","dkim":"pass","dmarc":"pass"}}}"#;
    assert_eq!(read(other, NOW, None).err(), Some(InboundError::Payload));
}

/// TD-PRIV-11 (PRIV-024, PRIV-026): the email blind index matches a sender to its business and to a
/// homeowner under separate derivations, whatever the case or display name.
#[test]
fn the_email_index_matches_by_purpose() {
    let vault = vault();
    let email = read(
        &body("Biz Test <Office@Example.com>", "hi", "pass"),
        NOW,
        None,
    )
    .expect("signed");
    assert_eq!(
        email.sender.business,
        email_index(&vault, "office@example.com", true)
    );
    assert_eq!(
        email.sender.homeowner,
        email_index(&vault, "OFFICE@example.com", false)
    );
    assert_ne!(email.sender.business, email.sender.homeowner);
    assert_ne!(
        email.sender.business,
        email_index(&vault, "other@example.com", true)
    );
}
