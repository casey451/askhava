//! TD-CALL-29 (CALL-020, SEC-016; D-227, D-4361, D-5534): signed fake Twilio voice requests
//! through the assembled router (`build_router`) with the main number passed by
//! `RouterParts::with_main_number`. Fake numbers (555-01xx and the 500-555 test range) and
//! CallSids only; the expected greeting is read from `calls::api::greeting_twiml`, never typed.

use std::sync::Arc;

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{greeting_twiml, hangup_twiml};
use askhava_core::api::{Environment, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_privacy::api::Vault;
use askhava_server::inbound_text::Webhooks;
use askhava_server::inbound_voice::{VOICE_PATH, VoiceHooks, routes};
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use tower::ServiceExt as _;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const POOL_NUMBER: &str = "+15005550100";
const MAIN: &str = "+15005550109";
const UNKNOWN: &str = "+15005550108";
const CALLER: &str = "+19285550174";

fn masked_pool() -> MaskedPool {
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
        ("ASKHAVA_MASKED_NUMBERS", POOL_NUMBER),
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

fn voice_hooks(db: &PgPool) -> Arc<VoiceHooks> {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let text = Webhooks {
        pool: db.clone(),
        clock: Arc::new(ManualClock::new(
            "2026-11-02T17:00:00Z".parse().expect("instant"),
        )),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: Vault::new(&key(0x73), &key(0x74), &url).expect("vault"),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    };
    Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool: masked_pool(),
        wait: askhava_calls::api::PinWait::default(),
    })
}

/// The assembled router with the voice hooks and the fake main number.
fn assembled(db: &PgPool) -> Router {
    let hooks: Hooks = (None, Some(voice_hooks(db)), None, None);
    let parts = RouterParts::from(hooks).with_main_number(Some(MAIN.to_owned()));
    build_router(db.clone(), Router::new(), parts)
}

/// A first-turn call from the fake caller to `to`, signed with `token`; status and body.
async fn call(router: Router, to: &str, token: &str) -> (StatusCode, String) {
    let fields = [
        ("CallSid", "CA00000000000000000000000000000029"),
        ("From", CALLER),
        ("To", to),
    ];
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(token.as_bytes()).expect("key");
    mac.update(format!("{BASE}{VOICE_PATH}").as_bytes());
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", v.replace('+', "%2B")))
        .collect();
    let request = Request::post(VOICE_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = router.oneshot(request).await.expect("answer");
    let status = answer.status();
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

/// TD-CALL-29 (CALL-020, D-227): a call to the main number hears the English then Spanish
/// greeting with the 911 line and a hang-up, and no `Record`, `Dial` or voicemail verb.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_29_the_main_number_hears_the_greeting(db: PgPool) {
    let (status, body) = call(assembled(&db), MAIN, TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, greeting_twiml());
    let english = body.find("language=\"en-US\"").expect("English");
    let spanish = body.find("language=\"es-MX\"").expect("Spanish");
    assert!(english < spanish, "English first");
    assert!(body.contains("hang up and call 911"), "{body}");
    assert!(body.ends_with("<Hangup/></Response>"), "{body}");
    for verb in ["<Record", "<Dial", "<Voicemail", "<Gather", "<Number"] {
        assert!(!body.contains(verb), "{verb} in {body}");
    }
}

/// TD-CALL-29 (CALL-020): a call to a number neither the main number nor in the pool hangs up.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_29_an_unknown_number_still_hangs_up(db: PgPool) {
    let (status, body) = call(assembled(&db), UNKNOWN, TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, hangup_twiml());
}

/// TD-CALL-29 (CALL-020): `inbound_voice::routes` served without `build_router`'s layer has no
/// main number, so a call to it hangs up as before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_29_the_bare_route_hangs_up_on_the_main_number(db: PgPool) {
    for to in [MAIN, UNKNOWN] {
        let (status, body) = call(routes(voice_hooks(&db)), to, TOKEN).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, hangup_twiml(), "{to}");
    }
}

/// TD-CALL-29 (SEC-016): an unsigned (wrongly signed) call to the main number is refused and
/// hears no greeting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_29_an_unsigned_call_is_refused(db: PgPool) {
    let (status, body) = call(assembled(&db), MAIN, "not_the_auth_token").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!body.contains("911"), "{body}");
}
