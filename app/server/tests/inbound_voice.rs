//! TD-CALL-14 (CALL-002, CALL-005, SEC-016, SEC-017, FEE-022; D-1755, D-1836, D-2091 to D-2095):
//! signed fake Twilio voice requests through the real route. Fake numbers (555-01xx and the
//! 500-555 test range), fake names, CallSids and IDs only.

use std::sync::Arc;

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{
    Connected, VoiceKeys, assign_number, call_start, pin_tries_today, stored_pin,
};
use askhava_core::api::{Environment, MIGRATOR, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::sender_add;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_server::inbound_text::Webhooks;
use askhava_server::inbound_voice::{VOICE_PATH, VoiceHooks, routes};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt as _;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const NUMBER: &str = "+15005550100";
const HER: &str = "+19285550171";
const SHOP: &str = "+19285550172";
const STRANGER: &str = "+19285550173";
const PASSED_A: &str = "TN-Validation-Passed-A";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn pool() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", NUMBER),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

fn hooks(db: &PgPool) -> Arc<VoiceHooks> {
    let text = Webhooks {
        pool: db.clone(),
        clock: Arc::new(ManualClock::new(at())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    };
    Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool: pool(),
        wait: askhava_calls::api::PinWait::default(),
    })
}

/// Her fake contact and a business with `lines` open lines to her; the line IDs.
async fn setup(db: &mut PgConnection, lines: usize) -> Vec<Uuid> {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let business = Uuid::new_v4();
    sender_add(db, &vault(), (business, Uuid::new_v4()), SHOP)
        .await
        .expect("link");
    let mut ids = Vec::new();
    for _ in 0..lines {
        let job = Connected {
            job: Uuid::new_v4(),
            homeowner: her,
            business,
        };
        ids.push(
            assign_number(db, &pool(), job, at())
                .await
                .expect("line")
                .id(),
        );
    }
    ids
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// POSTs `fields` to the voice path with `query`, signed with `token`; status and body.
async fn post(
    v: &Arc<VoiceHooks>,
    query: &str,
    fields: &[(&str, &str)],
    token: &str,
) -> (StatusCode, String) {
    let path = format!("{VOICE_PATH}{query}");
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(token.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(Arc::clone(v))
        .oneshot(request)
        .await
        .expect("answer");
    let status = answer.status();
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

/// One turn of call `sid` from `from`: `query` names the turn, `digits` what was keyed.
async fn turn(
    v: &Arc<VoiceHooks>,
    (sid, from, verstat): (&str, &str, &str),
    query: &str,
    digits: Option<&str>,
) -> String {
    let mut fields = vec![
        ("CallSid", sid),
        ("From", from),
        ("To", NUMBER),
        ("StirVerstat", verstat),
    ];
    fields.extend(digits.map(|d| ("Digits", d)));
    let (status, body) = post(v, query, &fields, TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    body
}

fn sid(n: u32) -> String {
    format!("CA{n:032}")
}

/// A PIN that is none of `pins`.
fn wrong_for(pins: &[String]) -> String {
    (0..10_000_u16)
        .map(|n| format!("{n:04}"))
        .find(|p| !pins.contains(p))
        .expect("a free PIN")
}

/// TD-CALL-14 (SEC-016): a forged signature gets 403 and changes nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_14_a_forged_signature_gets_403(db: PgPool) {
    let v = hooks(&db);
    let fields = [
        ("CallSid", "CA00000000000000000000000000000001"),
        ("From", SHOP),
        ("To", NUMBER),
    ];
    let (status, body) = post(&v, "", &fields, "not_the_token").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!body.contains("Dial"));
}

/// TD-CALL-14 (CALL-002, FEE-022, SEC-017, D-1836): a linked phone with one line connects with
/// the pool number as caller ID; the start is stored once per parent CallSid, a repeat changing
/// nothing; no line ID is in the reply.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_14_a_linked_phone_with_one_line_connects(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let line = setup(&mut conn, 1).await[0];
    let v = hooks(&db);
    let call = (sid(1).leak() as &str, SHOP, PASSED_A);
    let first = turn(&v, call, "", None).await;
    assert!(first.contains(&format!("callerId=\"{NUMBER}\"")), "{first}");
    assert!(first.contains(HER) && !first.contains(&line.to_string()));
    assert_eq!(turn(&v, call, "", None).await, first);
    let start = call_start(&mut conn, &sid(1)).await.expect("read");
    assert_eq!(start, Some((line, at())));
}

/// TD-CALL-14 (CALL-003, CALL-004, SEC-017, D-1755): an unlinked caller hears the PIN prompt and
/// connects on the stored PIN only; a repeated CallSid answers the same step and records no
/// second try.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_14_an_unlinked_caller_connects_on_the_stored_pin_only(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let lines = setup(&mut conn, 2).await;
    let mut pins = Vec::new();
    for line in &lines {
        let pin = stored_pin(&mut conn, *line)
            .await
            .expect("read")
            .expect("pin");
        pins.push(pin.digits());
    }
    let v = hooks(&db);
    let call = (sid(2).leak() as &str, STRANGER, PASSED_A);
    let prompt = turn(&v, call, "", None).await;
    assert!(
        prompt.contains("<Gather") && prompt.contains("?try=1"),
        "{prompt}"
    );
    let wrong = wrong_for(&pins);
    let again = turn(&v, call, "?try=1", Some(&wrong)).await;
    assert!(again.contains("?try=2") && !again.contains("Dial"));
    assert_eq!(turn(&v, call, "?try=1", Some(&wrong)).await, again);
    let keys = VoiceKeys {
        id_secret: &v.hooks.id_secret,
        version: 1,
        pool: v.pool.numbers(),
    };
    let stranger = SecretString::from(STRANGER);
    let today = pin_tries_today(&mut conn, &keys, (&stranger, NUMBER), at()).await;
    assert_eq!(today, Ok(1));
    let dial = turn(&v, call, "?try=2", Some(&pins[1])).await;
    assert!(dial.contains("<Dial") && dial.contains(HER), "{dial}");
    assert!(!dial.contains("Testa") && !dial.contains("Fake St"));
}

/// TD-CALL-14 (CALL-005, CALL-023, D-026): after two wrong tries an A-attested call from the paid
/// business hears first name and street, and connects on its confirm; another attestation or an
/// unlinked caller hears nothing more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_14_only_an_a_attested_paid_business_hears_the_read_back(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let lines = setup(&mut conn, 2).await;
    let mut pins = Vec::new();
    for line in &lines {
        let pin = stored_pin(&mut conn, *line)
            .await
            .expect("read")
            .expect("pin");
        pins.push(pin.digits());
    }
    let wrong = wrong_for(&pins);
    let v = hooks(&db);
    let cases = [
        (3, SHOP, PASSED_A, true),
        (4, SHOP, "TN-Validation-Passed-B", false),
        (5, STRANGER, PASSED_A, false),
    ];
    for (n, from, verstat, hears) in cases {
        let call = (sid(n).leak() as &str, from, verstat);
        assert!(turn(&v, call, "", None).await.contains("?try=1"));
        turn(&v, call, "?try=1", Some(&wrong)).await;
        let second = turn(&v, call, "?try=2", Some(&wrong)).await;
        let heard = second.contains("Testa") && second.contains("123 Fake St");
        assert_eq!(heard, hears, "{second}");
        assert!(!second.contains("Lake Havasu") && !second.contains("Example"));
        if hears {
            let dial = turn(&v, call, "?confirm=2", Some("1")).await;
            assert!(dial.contains("<Dial") && dial.contains(HER), "{dial}");
        }
    }
}
