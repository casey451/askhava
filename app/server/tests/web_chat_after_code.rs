//! TD-INT-43 and TD-AI-30 (INT-027, PRIV-008, PRIV-005, PRIV-001, AI-003, AI-020, AI-002; T649,
//! D-4294): the web chat after the code, through the assembled router's `/request` with the
//! chat's code keys over a fake Verify that approves every code. While her name-and-address
//! question is open, her chat answer lands on her own vault contact (the one the release finds by
//! her number), no kept description or intake answer holds its words and it is not asked again;
//! an age under 13 then also deletes the chat session row; LIST keeps its path. With no question
//! open a message is kept as before, scrubbed of her vault name and of a name she stated in an
//! earlier turn; before the code it is scrubbed with no vault name. The question is asked through
//! `intake::api` after her ZIP (setup only). Fake data only: 555-01xx numbers, made-up names and streets.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_core::api::{MIGRATOR, Settings};
use askhava_intake::api::rules::{Inbound, NAME_ADDRESS_TEMPLATE, UNDER_18_TEMPLATE};
use askhava_intake::api::{
    JobStart, Opened, TextIn, ZipStep, answer_zip, ask_name_address, begin_job,
};
use askhava_messaging::api::{Language, render};
use askhava_privacy::api::{CONTACT_FIELDS_SET, Vault, find_by_phone};
use askhava_server::chat_codes::chat_keys;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const PATH: &str = "/request";
const PRE: &str = "__Host-askhava-pre";
/// The fake Verify's answer to both a send and a check: every code is right.
const APPROVED: &str = r#"{"sid":"VE00000000000000000000000000000649","valid":true}"#;
/// Her first message: made-up job words with no name, number or address.
const FIRST: &str = "the kitchen sink drips under the cabinet";
/// Her made-up answer to the name-and-address question.
const ANSWER: &str = "Maria Sample, 42 Sample St";
/// The chat's fake `ASKHAVA_ID_SECRET`.
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0649";
/// The page's line after a turn the text path took.
const TEXTED: &str = "We&#39;ll answer by text";

/// A fake Verify on a loopback port that approves every send and check; its base URL.
async fn fake_verify() -> String {
    let verify = Router::new()
        .fallback(|| async { ([(header::CONTENT_TYPE, "application/json")], APPROVED) });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let at = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, verify).await });
    format!("http://{at}")
}

/// A fake vault key, `1:` and 32 bytes of `b` in base64.
fn vault_key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

/// The vault the chat's keys hold (the same fake keys).
fn vault() -> Vault {
    let url = SecretString::from("postgres://fake@127.0.0.1:1/none".to_owned());
    let key = |b| SecretString::from(vault_key(b));
    Vault::new(&key(0x49), &key(0x64), &url).expect("vault")
}

/// The assembled router with the chat's code keys over a fake Verify.
async fn router(pool: &PgPool) -> Router {
    let verify = fake_verify().await;
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_ID_SECRET", ID_SECRET),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &verify),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("VAULT_KEY", &vault_key(0x49)),
        ("VAULT_INDEX_KEY", &vault_key(0x64)),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let keys = chat_keys(&settings, pairs).expect("chat keys");
    assert!(keys.is_some(), "the chat's code keys are configured");
    let parts = RouterParts::from((None, None, None, None)).with_chat_codes(keys);
    build_router(pool.clone(), Router::new(), parts)
}

/// Sends `method` to `/request` with `cookie` and `form`: the page and the cookie it set.
async fn send(app: &Router, method: &str, cookie: &str, form: String) -> (String, String) {
    let mut request = Request::builder()
        .method(method)
        .uri(PATH)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from(form))
        .expect("request");
    let at = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 49)), 40_000);
    request.extensions_mut().insert(ConnectInfo(at));
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status().as_u16(), 200, "status");
    let pre = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|c| c.to_str().ok()?.split(';').next().map(str::to_owned))
        .find(|pair| pair.starts_with(&format!("{PRE}=")))
        .unwrap_or_default();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (String::from_utf8_lossy(&body).into_owned(), pre)
}

/// One browser's verified chat: its page parts, number, vault contact and request.
struct Chat {
    app: Router,
    cookie: String,
    token: String,
    key: String,
    number: String,
    contact: Uuid,
    request: Uuid,
}

impl Chat {
    /// Opens the page for chat `n`, sends [`FIRST`], its fake number and a code.
    async fn verified(pool: &PgPool, n: u16) -> Self {
        let app = router(pool).await;
        let (page, cookie) = send(&app, "GET", "", String::new()).await;
        let token = page.split("name=\"csrf\" value=\"").nth(1);
        let token = token.and_then(|r| r.split('"').next()).expect("token");
        let mut chat = Self {
            app,
            cookie,
            token: token.to_owned(),
            key: format!("{:032x}", 0x0649_0000_u64 + u64::from(n)),
            number: format!("+1928555{:04}", 100 + n),
            contact: Uuid::nil(),
            request: Uuid::nil(),
        };
        for step in ["message", "number", "code"] {
            chat.post(step, FIRST).await;
        }
        let row = |column: &str| {
            format!(
                "SELECT {column}::text AS v FROM intake.web_chat_session \
                 WHERE key_digest = sha256(convert_to('{}', 'UTF8'))",
                chat.key
            )
        };
        let id = |v: String| Uuid::parse_str(&v).expect("a verified session");
        chat.contact = id(text(pool, row("contact_id")).await);
        chat.request = id(text(pool, row("conversation_id")).await);
        let start = JobStart {
            conversation: chat.request,
            words: FIRST,
            emergency: false,
            trade: "plumbing",
            at: Timestamp::now(),
        };
        let mut db = pool.acquire().await.expect("db");
        let opened = begin_job(&mut db, &start).await.expect("job");
        assert!(
            matches!(opened, Opened::Created(_)),
            "her job opened (setup)"
        );
        chat
    }

    async fn post(&self, step: &str, words: &str) -> String {
        let form = format!(
            "csrf={}&chat={}&message={}&g=&said=&step={step}&words={}&number={}&code=123456",
            self.token,
            self.key,
            Uuid::new_v4(),
            encode(words),
            encode(&self.number),
        );
        send(&self.app, "POST", &self.cookie, form).await.0
    }

    /// Posts `words` as one chat message: the page.
    async fn say(&self, words: &str) -> String {
        self.post("message", words).await
    }
}

fn encode(words: &str) -> String {
    words
        .bytes()
        .map(|b| match b {
            b' ' => "+".to_owned(),
            b if b.is_ascii_alphanumeric() => char::from(b).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// Her ZIP stored, then her name-and-address question asked on `chat`'s request through
/// `intake::api` (setup only).
async fn ask(pool: &PgPool, chat: &Chat) {
    let reply = |words| TextIn {
        inbox_id: Uuid::new_v4(),
        contact: chat.contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: Timestamp::now(),
    };
    let (mut db, secret) = (pool.acquire().await.expect("db"), secret());
    let zip = reply("86403");
    let zip = answer_zip(&mut db, &secret, (&zip, chat.request), Default::default()).await;
    assert!(matches!(zip.expect("zip"), ZipStep::Stored(_)), "her ZIP");
    let asked = ask_name_address(&mut db, &secret, &reply("ok"), chat.request, false).await;
    assert!(asked.expect("asked").is_some(), "the question asked");
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

/// The one text value `v` of `sql` (one row), which reads other modules' tables, run through
/// `query_to_xml` as `web_chat_stops.rs` does: `server` names no other schema in a query macro.
async fn text(pool: &PgPool, sql: String) -> String {
    let v = sqlx::query_scalar!(
        r#"SELECT coalesce((xpath('/row/v/text()', query_to_xml($1::text, false, true, '')))[1]::text, '') AS "v!""#,
        sql
    )
    .fetch_one(pool)
    .await
    .expect("value");
    v.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Every kept description of `request`, and every intake answer's choice, joined.
async fn kept(pool: &PgPool, request: Uuid) -> String {
    let words = format!(
        "SELECT string_agg(words, ' | ') AS v FROM quotes.job_description \
         WHERE request_id = '{request}'"
    );
    let choices = format!(
        "SELECT string_agg(question_id || '=' || coalesce(choice, ''), ' | ') AS v \
         FROM intake.intake_answer WHERE conversation_id = '{request}'"
    );
    format!(
        "{} || {}",
        text(pool, words).await,
        text(pool, choices).await
    )
}

/// How many vault fields were set on `contact` (INT-027, PRIV-001).
async fn fields_set(pool: &PgPool, contact: Uuid) -> String {
    let sql = format!(
        "SELECT count(*)::text AS v FROM ops.audit_log WHERE subject_id = '{contact}' \
         AND action = '{CONTACT_FIELDS_SET}'"
    );
    text(pool, sql).await
}

/// TD-INT-43 (INT-027, PRIV-005, PRIV-008, PRIV-001): with the question open, the chat answer
/// sets the name and address on her own vault contact, keeps no description or intake answer
/// holding its words and is not asked again; a later message is kept as before, without her
/// vault name (TD-AI-30, AI-003).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_43_the_chat_answer_lands_on_her_vault_contact(pool: PgPool) {
    let chat = Chat::verified(&pool, 1).await;
    ask(&pool, &chat).await;
    let before = kept(&pool, chat.request).await;
    let page = chat.say(ANSWER).await;
    for needle in ["Maria", "Sample"] {
        assert!(!page.contains(needle), "the page shows no {needle}");
    }
    assert_eq!(
        fields_set(&pool, chat.contact).await,
        "2",
        "name and address set"
    );
    let mut db = pool.acquire().await.expect("db");
    let found = find_by_phone(&mut db, &vault(), &chat.number).await;
    assert_eq!(
        found.expect("find"),
        Some(chat.contact),
        "the contact the release reads"
    );
    drop(db);
    let after = kept(&pool, chat.request).await;
    for needle in ["Maria", "Sample"] {
        assert!(!after.contains(needle), "kept words hold {needle}: {after}");
    }
    assert!(
        after.contains("address_contact"),
        "the question answered: {after}"
    );
    assert_ne!(before, after);
    let page = chat.say("Maria Sample's sink leaks").await;
    assert!(page.contains(TEXTED), "the text path took it");
    assert_eq!(
        fields_set(&pool, chat.contact).await,
        "2",
        "a later message is no answer"
    );
    let asked = format!(
        "SELECT count(*)::text AS v FROM messaging.outbox WHERE '{}' = ANY(subject_ids) \
         AND template_id = '{NAME_ADDRESS_TEMPLATE}'",
        chat.request
    );
    assert_eq!(text(&pool, asked).await, "1", "not asked again");
    let later = kept(&pool, chat.request).await;
    assert!(
        later.contains("sink leaks"),
        "a later message is kept: {later}"
    );
    assert!(!later.contains("Maria"), "her vault name removed: {later}");
}

/// TD-INT-43 (INT-038, INT-027): an age under 13 typed while the question is open shows
/// INT-038's reply and deletes the chat session row with the request's data.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_43_an_age_under_13_deletes_the_session_row(pool: PgPool) {
    let chat = Chat::verified(&pool, 2).await;
    ask(&pool, &chat).await;
    let page = chat.say("I'm 11-years-old").await;
    let reply = render(UNDER_18_TEMPLATE, Language::En, false, &[]).expect("template");
    let first = reply.body.split('\'').next().expect("its words");
    assert!(page.contains(first), "INT-038's reply on the page");
    let sessions = format!(
        "SELECT count(*)::text AS v FROM intake.web_chat_session WHERE contact_id = '{}'",
        chat.contact
    );
    assert_eq!(text(&pool, sessions).await, "0", "the session row deleted");
    assert_eq!(
        fields_set(&pool, chat.contact).await,
        "0",
        "no answer stored"
    );
}

/// TD-INT-43 (TXT-031, INT-027): "LIST" typed while the question is open keeps its LIST path:
/// nothing goes to the vault and the question stays open.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_43_list_keeps_its_path_while_the_question_is_open(pool: PgPool) {
    let chat = Chat::verified(&pool, 3).await;
    ask(&pool, &chat).await;
    let page = chat.say("LIST").await;
    assert!(page.contains(TEXTED), "answered by text");
    assert_eq!(
        fields_set(&pool, chat.contact).await,
        "0",
        "no name or address"
    );
    let after = kept(&pool, chat.request).await;
    assert!(!after.contains("address_contact"), "still open: {after}");
}

/// TD-AI-30 (AI-020, AI-003): a name she stated in an earlier chat turn is removed from a later
/// turn's kept words; before the code a message is scrubbed with no vault name, as today.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ai_30_a_stated_name_is_removed_from_a_later_turn(pool: PgPool) {
    let chat = Chat::verified(&pool, 4).await;
    chat.say("my name is Dana").await;
    chat.say("Dana's sink leaks").await;
    let later = kept(&pool, chat.request).await;
    assert!(
        later.contains("sink leaks"),
        "the later turn is kept: {later}"
    );
    assert!(!later.contains("Dana"), "the stated name removed: {later}");
    let app = router(&pool).await;
    let (page, cookie) = send(&app, "GET", "", String::new()).await;
    let token = page.split("name=\"csrf\" value=\"").nth(1);
    let token = token.and_then(|r| r.split('"').next()).expect("token");
    let form = format!(
        "csrf={token}&chat={:032x}&message={}&g=&said=&step=message&words={}",
        0x0649_9999_u64,
        Uuid::new_v4(),
        encode("Dana's sink leaks")
    );
    let (page, _) = send(&app, "POST", &cookie, form).await;
    assert!(
        page.contains("You: Dana&#39;s sink leaks"),
        "no vault name before the code"
    );
}
