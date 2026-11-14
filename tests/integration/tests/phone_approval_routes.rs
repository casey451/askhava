//! TD-CALL-25 (CALL-012, BIZ-033; D-1695, D-3081, D-3263, T435): a phone waiting for the owner's
//! approval never acts for the business on the call or text route, even with its digests planted
//! in `messaging`'s sender directory as a stale row would leave them. Its call hears the PIN
//! prompt (`calls::api::answer_call` through `party_of`'s `gate`); its STOP, `LINK <PIN>` or other
//! text takes an unknown number's path. After approval the same phone connects and its STOP
//! reaches `businesses::api::phone_stopped`. Signed fake Twilio requests through the served
//! routes (`app/server/src/inbound_voice.rs` and `inbound_text.rs`, compiled in with `#[path]`,
//! as `link_reply.rs` does). Fake numbers and IDs only.

use std::sync::Arc;

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{
    AUTO_REPLY_TEMPLATE, Acting, Roles, SenderLink, TeamError, add_member, add_owner,
    approve_phone, change_phone_at, create, link_by_reply, link_phone, reject_phone,
};
use askhava_calls::api::{Connected, PinWait, assign_number};
use askhava_core::api::{Environment, ManualClock, Settings};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::sender_add;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks};
use inbound_voice::{VOICE_PATH, VoiceHooks};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt as _;

/// The inbound text route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
#[allow(
    dead_code,
    reason = "only the inbound route and `Webhooks` are used; the retry items are the worker's"
)]
mod inbound_text;
/// The voice route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_voice.rs"]
mod inbound_voice;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const NUMBER: &str = "+15005550100";
const ASK_HAVA: &str = "+15005550006";
const HER: &str = "+19285550191";
const WAITING: &str = "+19285550195";
const CHANGED: &str = "+19285550196";
const OWNERS: &str = "+19285550197";
const REPLY: &str = "+19285550198";

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
    let config = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    config.pool().clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x75), &key(0x76), &url).expect("vault")
}

fn served(db: &PgPool) -> Arc<VoiceHooks> {
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
    let (pool, wait) = (pool(), PinWait::default());
    Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool,
        wait,
    })
}

struct Directory;

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &vault(), business_line, number.expose_secret())
            .await
            .map_err(|_| TeamError::Database)
    }
}

/// A business, its owner and a "Confirm and billing" member, and one line to her.
async fn shop(db: &mut PgConnection) -> (Uuid, Acting, Acting) {
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    let homeowner = store_contact(db, &vault(), &new).await.expect("contact");
    let business = create(db, false, at()).await.expect("business");
    let member = add_owner(db, business, Uuid::new_v4(), at())
        .await
        .expect("owner");
    let owner = Acting { business, member };
    let roles = Roles::new(false, true).expect("roles");
    let member = add_member(db, owner, Uuid::new_v4(), roles, at())
        .await
        .expect("member");
    let job = Uuid::new_v4();
    let connected = Connected {
        job,
        homeowner,
        business,
    };
    assign_number(db, &pool(), connected, at())
        .await
        .expect("line");
    (business, owner, Acting { business, member })
}

/// `number` linked by `by` (the directory filing or holding it as `link_phone` decides); the line.
async fn link(db: &mut PgConnection, by: Acting, number: &str) -> Uuid {
    let phone = SecretString::from(number.to_owned());
    link_phone(db, &Directory, (by, false), &phone, at())
        .await
        .expect("link")
}

/// `number`'s digests planted in the sender directory as phone `line`, as a stale row would be.
async fn plant(db: &mut PgConnection, business_line: (Uuid, Uuid), number: &str) {
    sender_add(db, &vault(), business_line, number)
        .await
        .expect("plant");
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `fields` POSTed to `path`, signed as Twilio signs a webhook; the status and body.
async fn post(v: &Arc<VoiceHooks>, path: &str, fields: &[(&str, &str)]) -> String {
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let routes =
        inbound_voice::routes(Arc::clone(v)).merge(inbound_text::routes(Arc::clone(&v.hooks)));
    let answer = routes.oneshot(request).await.expect("answer");
    assert_eq!(answer.status(), StatusCode::OK, "{path}");
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

/// Whether call `n` from `from` to the pool number connects at once (else it hears the prompt).
async fn connects(v: &Arc<VoiceHooks>, n: u32, from: &str) -> bool {
    let sid = format!("CA{n:032}");
    let fields = [("CallSid", sid.as_str()), ("From", from), ("To", NUMBER)];
    let twiml = post(v, VOICE_PATH, &fields).await;
    assert!(
        twiml.contains("<Dial") ^ twiml.contains("<Gather"),
        "{twiml}"
    );
    twiml.contains("<Dial")
}

async fn text(v: &Arc<VoiceHooks>, n: u32, from: &str, words: &str) {
    let sid = format!("SM{n:032}");
    let fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    post(v, INBOUND_PATH, &fields).await;
}

/// What a business text changes: `phone_stopped` audit rows, business auto-replies, handled LINK
/// replies (refusals) and staff phones.
async fn acted(db: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.phone_stopped'), \
         (SELECT count(*) FROM messaging.outbox WHERE template_id = $1), \
         (SELECT count(*) FROM calls.link_reply_refusal), \
         (SELECT count(*) FROM businesses.staff_phone)",
    )
    .bind(AUTO_REPLY_TEMPLATE)
    .fetch_one(db)
    .await
    .expect("counts")
}

/// A waiting phone's other text, `LINK <PIN>` and STOP each act for nobody: no field moves.
async fn texts_act_for_nobody(db: &PgPool, v: &Arc<VoiceHooks>, sid: u32, from: &str) {
    let before = acted(db).await;
    text(v, sid, from, "hello, can you come Tuesday").await;
    text(v, sid + 1, from, "LINK 1234").await;
    text(v, sid + 2, from, "STOP").await;
    assert_eq!(acted(db).await, before, "a waiting phone's text acted");
}

/// TD-CALL-25: a member's link waits; planted in the directory, its call hears the PIN prompt and
/// its texts act for nobody. Once the owner approves, the same call connects and its STOP reaches
/// `phone_stopped`. A member's change of that approved phone then behaves as the waiting phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_25_a_waiting_phone_never_acts_until_approved(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let (business, owner, member) = shop(&mut conn).await;
    let line = link(&mut conn, member, WAITING).await;
    plant(&mut conn, (business, line), WAITING).await;
    let v = served(&db);
    assert!(!connects(&v, 1, WAITING).await, "a waiting phone");
    texts_act_for_nobody(&db, &v, 1, WAITING).await;
    let unplant = "DELETE FROM messaging.sender_directory WHERE line_id = $1";
    sqlx::query(unplant)
        .bind(line)
        .execute(&mut *conn)
        .await
        .expect("unplant");
    approve_phone(&mut conn, owner, (line, at()), at())
        .await
        .expect("approve");
    assert!(connects(&v, 2, WAITING).await, "the approved phone");
    let before = acted(&db).await;
    text(&v, 10, WAITING, "STOP").await;
    assert_eq!(acted(&db).await.0, before.0 + 1, "its STOP");
    let new = SecretString::from(CHANGED.to_owned());
    change_phone_at(&mut conn, &Directory, (member, line), &new, at())
        .await
        .expect("change");
    plant(&mut conn, (business, line), CHANGED).await;
    assert!(!connects(&v, 3, CHANGED).await, "a changed number");
    texts_act_for_nobody(&db, &v, 20, CHANGED).await;
}

/// TD-CALL-25 (D-3263): a declined phone's row is gone, so its standing is `None`: its digests are
/// gone from the directory and the held list, and none are planted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_25_a_declined_phones_digests_are_gone(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let (_, owner, member) = shop(&mut conn).await;
    let line = link(&mut conn, member, WAITING).await;
    reject_phone(&mut conn, owner, (line, at()), at())
        .await
        .expect("decline");
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM messaging.sender_directory WHERE line_id = $1) \
         + (SELECT count(*) FROM messaging.sender_held WHERE line_id = $1)",
    )
    .bind(line)
    .fetch_one(&mut *conn)
    .await
    .expect("count");
    assert_eq!(left, 0, "a declined phone's digests are gone");
}

/// TD-CALL-25: the owner's staff phone and the LINK-reply phone (T391) work at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_25_the_owners_and_the_link_reply_phones_work_at_once(db: PgPool) {
    let mut conn = db.acquire().await.expect("connection");
    let (business, owner, _) = shop(&mut conn).await;
    link(&mut conn, owner, OWNERS).await;
    let reply = link_by_reply(&mut conn, business, at())
        .await
        .expect("reply");
    plant(&mut conn, (business, reply), REPLY).await;
    let v = served(&db);
    assert!(connects(&v, 1, OWNERS).await, "the owner's phone");
    assert!(connects(&v, 2, REPLY).await, "the LINK-reply phone");
}
