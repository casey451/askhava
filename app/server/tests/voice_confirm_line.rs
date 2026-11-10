//! TD-CALL-17 (CALL-005, CALL-002; D-026, D-2201): signed fake Twilio voice requests through the
//! served voice route. After two bad PINs from a business's linked phone the read-back names one
//! line; the confirm press connects that line only, while it is still open on the pool number and
//! still the caller's business's, never a line opened, closed or moved since. Fake numbers
//! (555-01xx and the 500-555 test range), fake names, streets, CallSids and IDs only.

use std::sync::Arc;

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{
    CloseReason, Connected, PinWait, VoiceKeys, assign_number, call_start, close_line,
    pin_tries_today, stored_pin,
};
use askhava_core::api::{MIGRATOR, ManualClock, Settings};
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
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt as _;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const NUMBER: &str = "+15005550100";
const SHOP: &str = "+19285550172";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key";
const PASSED_A: &str = "TN-Validation-Passed-A";
/// Three fake homeowners: name, phone, address; the read-back says the first name and street.
const PEOPLE: [(&str, &str, &str); 3] = [
    (
        "Ada Example",
        "+19285550181",
        "11 Alder St, Lake Havasu City, AZ 86403",
    ),
    (
        "Bea Example",
        "+19285550182",
        "22 Birch St, Lake Havasu City, AZ 86403",
    ),
    (
        "Cora Example",
        "+19285550183",
        "33 Cedar St, Lake Havasu City, AZ 86403",
    ),
];

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

/// The served voice route's state with its clock at `now`.
fn hooks(db: &PgPool, now: Timestamp) -> Arc<VoiceHooks> {
    let text = Webhooks {
        pool: db.clone(),
        clock: Arc::new(ManualClock::new(now)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from(ID_SECRET.to_owned()),
        vault: vault(),
        names: Vec::new(),
        intake: IntakeSettings::default(),
    };
    Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool: pool(),
        wait: PinWait::default(),
    })
}

/// Fake homeowner `n` of [`PEOPLE`] in the vault.
async fn homeowner(db: &mut PgConnection, n: usize) -> Uuid {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let (name, phone, address) = PEOPLE[n];
    let new = NewContact {
        name: s(name),
        phone: s(phone),
        email: None,
        address: s(address),
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// A new open line on the pool number for `business` to `homeowner`: its job and line IDs.
async fn line(db: &mut PgConnection, homeowner: Uuid, business: Uuid) -> (Uuid, Uuid) {
    let job = Connected {
        job: Uuid::new_v4(),
        homeowner,
        business,
    };
    let id = assign_number(db, &pool(), job, at())
        .await
        .expect("line")
        .id();
    (job.job, id)
}

/// The scene: SHOP linked (as `phone`) to business `x`, which holds an older line to Cora and the
/// newer line A to Ada on the pool number, so two wrong PINs read back A.
struct Scene {
    x: Uuid,
    phone: Uuid,
    a_job: Uuid,
    a: Uuid,
    people: [Uuid; 3],
    wrong: String,
}

async fn scene(db: &mut PgConnection) -> Scene {
    let people = [
        homeowner(db, 0).await,
        homeowner(db, 1).await,
        homeowner(db, 2).await,
    ];
    let (x, phone) = (Uuid::new_v4(), Uuid::new_v4());
    sender_add(db, &vault(), (x, phone), SHOP)
        .await
        .expect("link");
    let (_, older) = line(db, people[2], x).await;
    let (a_job, a) = line(db, people[0], x).await;
    let mut pins = Vec::new();
    for id in [older, a] {
        pins.push(
            stored_pin(db, id)
                .await
                .expect("read")
                .expect("pin")
                .digits(),
        );
    }
    let wrong = (0..10_000_u16)
        .map(|n| format!("{n:04}"))
        .find(|p| !pins.contains(p))
        .expect("a free PIN");
    Scene {
        x,
        phone,
        a_job,
        a,
        people,
        wrong,
    }
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// One signed turn of call `sid` from SHOP with `verstat`: `query` names the turn, `digits` what
/// was keyed; the 200 body.
async fn turn(
    v: &Arc<VoiceHooks>,
    (sid, verstat): (&str, &str),
    query: &str,
    digits: &str,
) -> String {
    let mut fields = vec![
        ("CallSid", sid),
        ("From", SHOP),
        ("To", NUMBER),
        ("StirVerstat", verstat),
    ];
    if !digits.is_empty() {
        fields.push(("Digits", digits));
    }
    let path = format!("{VOICE_PATH}{query}");
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in fields {
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
    assert_eq!(answer.status(), StatusCode::OK);
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

fn sid(n: u32) -> String {
    format!("CA{n:032}")
}

/// Two wrong PINs on call `sid` from SHOP (A attestation): the read-back TwiML of try 2.
async fn read_back(v: &Arc<VoiceHooks>, sid: &str, wrong: &str) -> String {
    let call = (sid, PASSED_A);
    assert!(turn(v, call, "", "").await.contains("?try=1"));
    assert!(turn(v, call, "?try=1", wrong).await.contains("?try=2"));
    turn(v, call, "?try=2", wrong).await
}

/// Whether `xml` dials homeowner `n` of [`PEOPLE`].
fn dials(xml: &str, n: usize) -> bool {
    xml.contains("<Dial") && xml.contains(PEOPLE[n].1)
}

/// TD-CALL-17 (CALL-005, CALL-002): the unchanged case reads back line A, keeps the confirm URL to
/// the try number, and connects exactly A; a repeated identical confirm connects A again and writes
/// no second `call_start`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_17_the_confirm_connects_exactly_the_line_read_back(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let s = scene(&mut conn).await;
    let v = hooks(&db, at());
    let call = sid(1);
    let words = read_back(&v, &call, &s.wrong).await;
    assert!(
        words.contains("Ada") && words.contains("11 Alder St"),
        "{words}"
    );
    assert!(!words.contains("Cora") && !words.contains("Example"));
    assert!(words.contains("?confirm=2\""), "{words}");
    assert!(!words.contains(&s.a.to_string()) && !words.contains(&s.a.simple().to_string()));
    let dial = turn(&v, (&call, PASSED_A), "?confirm=2", "1").await;
    assert!(dials(&dial, 0) && !dials(&dial, 2), "{dial}");
    assert_eq!(
        call_start(&mut conn, &call).await.expect("start"),
        Some((s.a, at()))
    );
    let later = hooks(&db, at() + SignedDuration::from_secs(20));
    let again = turn(&later, (&call, PASSED_A), "?confirm=2", "1").await;
    assert!(dials(&again, 0) && !dials(&again, 2), "{again}");
    assert_eq!(
        call_start(&mut conn, &call).await.expect("start"),
        Some((s.a, at()))
    );
}

/// TD-CALL-17 (CALL-005, CALL-002): a second line B of the same business opened between the
/// read-back and the press is never connected; the press connects A.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_17_a_line_opened_after_the_read_back_is_never_connected(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let s = scene(&mut conn).await;
    let v = hooks(&db, at());
    let call = sid(2);
    assert!(read_back(&v, &call, &s.wrong).await.contains("Ada"));
    let (_, b) = line(&mut conn, s.people[1], s.x).await;
    let dial = turn(&v, (&call, PASSED_A), "?confirm=2", "1").await;
    assert!(dials(&dial, 0) && !dials(&dial, 1), "{dial}");
    assert_eq!(
        call_start(&mut conn, &call).await.expect("start"),
        Some((s.a, at()))
    );
    assert_ne!(s.a, b);
}

/// TD-CALL-17 (CALL-005, CALL-002): line A closed while a newer line B of the same business is
/// open connects nobody and answers as an unconfirmed press (the next PIN prompt).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_17_a_closed_read_back_line_connects_nobody(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let s = scene(&mut conn).await;
    let v = hooks(&db, at());
    let call = sid(3);
    assert!(read_back(&v, &call, &s.wrong).await.contains("Ada"));
    let closed = close_line(&mut conn, (s.a_job, 1), CloseReason::NoShow, at()).await;
    assert_eq!(closed.expect("close"), Some(s.a));
    line(&mut conn, s.people[1], s.x).await;
    let answer = turn(&v, (&call, PASSED_A), "?confirm=2", "1").await;
    assert!(!answer.contains("<Dial"), "{answer}");
    assert!(answer.contains("?try=3"), "{answer}");
    assert_eq!(call_start(&mut conn, &call).await.expect("start"), None);
}

/// TD-CALL-17 (CALL-005, CALL-002): once the caller's phone belongs to another business holding
/// line B on the number, the press connects nobody (never B) and answers as an unconfirmed press.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_17_a_line_no_longer_the_callers_business_connects_nobody(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let s = scene(&mut conn).await;
    let v = hooks(&db, at());
    let call = sid(4);
    assert!(read_back(&v, &call, &s.wrong).await.contains("Ada"));
    let y = Uuid::new_v4();
    line(&mut conn, s.people[1], y).await;
    sender_add(&mut conn, &vault(), (y, s.phone), SHOP)
        .await
        .expect("move the phone");
    let answer = turn(&v, (&call, PASSED_A), "?confirm=2", "1").await;
    assert!(!answer.contains("<Dial"), "{answer}");
    assert!(answer.contains("?try=3"), "{answer}");
    assert_eq!(call_start(&mut conn, &call).await.expect("start"), None);
}

/// TD-CALL-17 (CALL-005, D-026): a confirm on a try whose row has no `readback_line` connects
/// nobody: the first try, and a second try heard without A attestation then confirmed with it.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_17_a_try_that_read_nothing_back_connects_nobody(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let s = scene(&mut conn).await;
    let v = hooks(&db, at());
    let first = sid(5);
    turn(&v, (&first, PASSED_A), "", "").await;
    turn(&v, (&first, PASSED_A), "?try=1", &s.wrong).await;
    let early = turn(&v, (&first, PASSED_A), "?confirm=1", "1").await;
    assert!(!early.contains("<Dial"), "{early}");
    let other = sid(6);
    let b_call = (other.as_str(), "TN-Validation-Passed-B");
    turn(&v, b_call, "", "").await;
    turn(&v, b_call, "?try=1", &s.wrong).await;
    let second = turn(&v, b_call, "?try=2", &s.wrong).await;
    assert!(
        !second.contains("Ada") && second.contains("?try=3"),
        "{second}"
    );
    let forged = turn(&v, (&other, PASSED_A), "?confirm=2", "1").await;
    assert!(!forged.contains("<Dial"), "{forged}");
    for call in [&first, &other] {
        assert_eq!(call_start(&mut conn, call).await.expect("start"), None);
    }
}

/// TD-CALL-17 (CALL-005, SEC-017): Twilio's resend of the read-back turn reads back A from the
/// stored row, not the newer line B opened since, and counts nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_17_a_resent_read_back_turn_reads_back_the_bound_line(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let s = scene(&mut conn).await;
    let v = hooks(&db, at());
    let call = sid(7);
    assert!(read_back(&v, &call, &s.wrong).await.contains("Ada"));
    line(&mut conn, s.people[1], s.x).await;
    let resent = turn(&v, (&call, PASSED_A), "?try=2", &s.wrong).await;
    assert!(
        resent.contains("Ada") && resent.contains("11 Alder St"),
        "{resent}"
    );
    assert!(
        !resent.contains("Bea") && !resent.contains("22 Birch St"),
        "{resent}"
    );
    assert!(resent.contains("?confirm=2\""), "{resent}");
    let (secret, masked) = (SecretString::from(ID_SECRET.to_owned()), pool());
    let keys = VoiceKeys {
        id_secret: &secret,
        version: 1,
        pool: masked.numbers(),
    };
    let caller = SecretString::from(SHOP.to_owned());
    let tries = pin_tries_today(&mut conn, &keys, (&caller, NUMBER), at()).await;
    assert_eq!(tries.expect("tries"), 2);
}
