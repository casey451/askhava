//! TD-CON-36 (INT-038, CON-008, CON-010, CON-035, CON-022, TXT-002, TXT-041; T645, D-4292,
//! D-4473, D-4726): the web chat at `/request`, served by the assembled router against
//! PostgreSQL, honours an opt-out word or phrase typed after the code (the session's number listed
//! at once with channel `web_chat`, CON-009's words on the page, no text, no job change), stops a
//! sender who states an age under 18 before and after the code (INT-038's reply; after the code
//! its text queued as on the text path and the request closed; under 13 the request's data and
//! the session row deleted, the consent record kept, on an app-role pool), and keeps the first
//! message's language for the consent text and TXT-041's line. The page's turn has no code keys,
//! so each verified session is seeded through `intake::api::check_code` with a fake code step
//! (the owner pool). Fake data only: 555-01xx numbers and made-up job words.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use askhava_core::api::{Environment, MIGRATOR, ManualClock};
use askhava_intake::api::rules::{
    SAFETY_TEMPLATE, Settings as IntakeSettings, UNDER_18_TEMPLATE, consent_template,
};
use askhava_intake::api::{
    ChatError, ChatSettings, Checked, CodeStep, SPANISH_NOT_YET_TEMPLATE, check_code, send_code,
    start_chat,
};
use askhava_messaging::api::{CONFIRMATION_TEMPLATE, Language, render};
use askhava_privacy::api::Vault;
use askhava_server::inbound_text::Webhooks;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const PATH: &str = "/request";
const PRE: &str = "__Host-askhava-pre";
/// Made-up job words for the first message before the code.
const FIRST: &str = "the kitchen sink drips under the cabinet";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x36), &key(0x63), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn settings(spanish: bool) -> IntakeSettings {
    IntakeSettings {
        spanish_conversations_on: spanish,
        ..IntakeSettings::default()
    }
}

/// A code step that sends nothing and takes any code (the page's real one is Twilio Verify).
struct FakeCode;

impl CodeStep for FakeCode {
    async fn send(&self, _: &SecretString, _: &str) -> Result<(), ChatError> {
        Ok(())
    }

    async fn check(&self, _: &SecretString, _: &str) -> Result<bool, ChatError> {
        Ok(true)
    }
}

/// The assembled router over `pool` with the intake switches `spanish` sets.
fn router(pool: &PgPool, spanish: bool) -> Router {
    let hooks = Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(Timestamp::now())),
        public_base: "https://hooks.example.test".to_owned(),
        auth_token: SecretString::from("fake_twilio_auth_token"),
        id_secret: secret(),
        vault: vault(),
        environment: Environment::Test,
        intake: settings(spanish),
    });
    build_router(
        pool.clone(),
        Router::new(),
        RouterParts::from((Some(hooks), None, None, None)),
    )
}

/// One browser's chat: its cookie, form token and chat key.
struct Chat {
    app: Router,
    cookie: String,
    token: String,
    key: String,
}

async fn send(app: &Router, method: &str, cookie: &str, form: String) -> (u16, String) {
    let mut request = Request::builder()
        .method(method)
        .uri(PATH)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from(form))
        .expect("request");
    let at = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 36)), 40_000);
    request.extensions_mut().insert(ConnectInfo(at));
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|c| c.to_str().ok()?.split(';').next().map(str::to_owned))
        .find(|pair| pair.starts_with(&format!("{PRE}=")))
        .unwrap_or_default();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let text = String::from_utf8_lossy(&body).into_owned();
    (
        status,
        if method == "GET" {
            cookie + "\n" + &text
        } else {
            text
        },
    )
}

/// Opens the page for chat key `n` (32 hex digits).
async fn open(app: &Router, n: u32) -> Chat {
    let (_, page) = send(app, "GET", "", String::new()).await;
    let (cookie, page) = page.split_once('\n').expect("a cookie line");
    let token = page.split("name=\"csrf\" value=\"").nth(1);
    let token = token.and_then(|r| r.split('"').next()).expect("token");
    Chat {
        app: app.clone(),
        cookie: cookie.to_owned(),
        token: token.to_owned(),
        key: format!("{:032x}", 0xc036_0000_u64 + u64::from(n)),
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

impl Chat {
    /// Posts `words` as one chat message and answers the page.
    async fn say(&self, words: &str) -> String {
        let form = format!(
            "csrf={}&chat={}&message={}&g=&said=&step=message&words={}",
            self.token,
            self.key,
            Uuid::new_v4(),
            encode(words)
        );
        let (status, page) = send(&self.app, "POST", &self.cookie, form).await;
        assert_eq!(status, 200, "{words}: status");
        page
    }

    /// Passes the code for `number` (seeded on the owner `pool`); the request and texts queued.
    async fn verify(
        &self,
        pool: &PgPool,
        number: &str,
        intake: IntakeSettings,
    ) -> (Uuid, Vec<&'static str>) {
        let mut conn = pool.acquire().await.expect("connection");
        let (key, number) = (
            SecretString::from(self.key.clone()),
            SecretString::from(number.to_owned()),
        );
        let (vault, now) = (vault(), Timestamp::now());
        start_chat(&mut conn, &key, None, now)
            .await
            .expect("session");
        let sent = send_code(&mut conn, (&vault, &FakeCode), &key, &number, now).await;
        assert_eq!(sent, Ok(true), "code sent");
        let chat = ChatSettings {
            language: Language::En,
            settings: intake,
            ip: None,
        };
        let keys = (&secret(), &vault, &FakeCode);
        let checked = check_code(&mut conn, keys, &key, (&number, "123456"), (chat, now)).await;
        match checked.expect("check") {
            Checked::Verified(id, queued) => (id, queued.iter().map(|q| q.template).collect()),
            other => panic!("not verified: {other:?}"),
        }
    }
}

/// The lines the page shows as ours, unescaped.
fn ours(page: &str) -> Vec<String> {
    let brand = format!("{}: ", askhava_core::api::brand().display_name());
    page.split("<li class=\"said-ours\">")
        .skip(1)
        .filter_map(|li| li.split("</li>").next())
        .map(|li| {
            li.strip_prefix(brand.as_str())
                .unwrap_or(li)
                .replace("&#39;", "'")
                .replace("&#x27;", "'")
                .replace("&#34;", "\"")
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        })
        .collect()
}

fn asks_number(page: &str) -> bool {
    page.contains("id=\"chat-number-hint\"")
}

fn body(template: &str, language: Language, spanish_on: bool) -> String {
    render(template, language, spanish_on, &[])
        .expect("template")
        .body
}

/// The one text value `v` of `sql` (one row), which reads other modules' tables, run through
/// `query_to_xml` as `home_request_box.rs` does: `server` names no other schema in a query macro.
async fn value(pool: &PgPool, sql: String) -> String {
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

async fn count(pool: &PgPool, from: &str) -> i64 {
    let v = value(pool, format!("SELECT count(*) AS v FROM {from}")).await;
    v.parse().expect("a count")
}

const OUTBOX: &str = "messaging.outbox";

/// The session's listings: channel and `since` in seconds (CON-022).
async fn listed(pool: &PgPool, key: &str) -> Vec<(String, i64)> {
    let sql = format!(
        "SELECT string_agg(e.channel || ' ' || extract(epoch FROM s.since)::bigint, ',') AS v \
         FROM intake.web_chat_session w \
         JOIN privacy.contact_index i ON i.contact_id = w.contact_id \
         JOIN messaging.suppression s ON s.index_version = i.key_version AND s.digest = i.digest \
         JOIN messaging.optout_event e ON e.id = s.event_id \
         WHERE w.key_digest = sha256(convert_to('{key}', 'UTF8'))"
    );
    let v = value(pool, sql).await;
    v.split(',')
        .filter(|row| !row.is_empty())
        .map(|row| {
            let (channel, since) = row.split_once(' ').expect("a listing");
            (channel.to_owned(), since.parse().expect("seconds"))
        })
        .collect()
}

/// The request's state: closed, its first job and its language.
async fn request(pool: &PgPool, id: Uuid) -> (bool, String, String) {
    let sql = format!(
        "SELECT (closed_at IS NOT NULL)::text || ' ' || coalesce(job_id::text, '-') || ' ' || \
         language AS v FROM intake.conversation WHERE id = '{id}'"
    );
    let v = value(pool, sql).await;
    let mut parts = v.split(' ').map(str::to_owned);
    let mut next = || parts.next().expect("a request");
    (next() == "true", next(), next())
}

/// The texts queued about request `id`: template and body.
async fn texts(pool: &PgPool, id: Uuid) -> Vec<(String, String)> {
    let sql = format!(
        "SELECT string_agg(template_id || '~~' || body, '##' ORDER BY id) AS v \
         FROM messaging.outbox WHERE '{id}' = ANY(subject_ids)"
    );
    let v = value(pool, sql).await;
    v.split("##")
        .filter(|row| !row.is_empty())
        .map(|row| {
            let (template, body) = row.split_once("~~").expect("a text");
            (template.to_owned(), body.to_owned())
        })
        .collect()
}

/// TD-CON-36 (CON-008, CON-010, CON-035, CON-022): after the code "STOP", "cancel", "Stop texting
/// me" and "parar" each list the session's number at once with channel `web_chat` and its time,
/// show CON-009's words, queue no text and change no job; "can stop by at 3" is a normal message.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_36_an_opt_out_after_the_code_lists_the_number_at_once(pool: PgPool) {
    let app = router(&pool, false);
    let cases = [
        ("STOP", false),
        ("cancel", false),
        ("Stop texting me", false),
        ("parar", true),
    ];
    for (n, (words, spanish)) in (1u32..).zip(cases) {
        let chat = open(&app, n).await;
        chat.say(FIRST).await;
        let (id, _) = chat
            .verify(&pool, &format!("+1928555010{n}"), settings(false))
            .await;
        let (before, state) = (count(&pool, OUTBOX).await, request(&pool, id).await);
        let at = Timestamp::now().as_second();
        let page = chat.say(words).await;
        let language = if spanish { Language::Es } else { Language::En };
        let confirm = body(CONFIRMATION_TEMPLATE, language, false);
        assert_eq!(
            ours(&page),
            vec![confirm],
            "{words}: CON-009's words on the page"
        );
        let rows = listed(&pool, &chat.key).await;
        assert!(!rows.is_empty(), "{words}: the number is listed");
        for (channel, since) in rows {
            assert_eq!(channel, "web_chat", "{words}: channel");
            assert!(
                (at - 1..=Timestamp::now().as_second() + 1).contains(&since),
                "{words}: at once"
            );
        }
        let after = count(&pool, OUTBOX).await;
        assert_eq!(after, before, "{words}: no text queued");
        assert_eq!(
            request(&pool, id).await,
            state,
            "{words}: no request or job change"
        );
    }
    let chat = open(&app, 9).await;
    chat.say(FIRST).await;
    let (id, _) = chat.verify(&pool, "+19285550109", settings(false)).await;
    let page = chat.say("can stop by at 3").await;
    assert!(ours(&page).is_empty(), "a normal message shows no opt-out");
    assert!(listed(&pool, &chat.key).await.is_empty(), "nothing listed");
    assert!(!request(&pool, id).await.0, "the request stays open");
}

/// TD-CON-36 (INT-038, INT-002, CON-008): before the code, an under-18 sender gets INT-038's reply
/// and no number question; "STOP" shows CON-009's words, lists nothing and asks no number; a
/// message about someone else's age is normal; a 911-list match gets the INT-004 text first.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_36_before_the_code_age_and_stop_are_answered_on_the_page(pool: PgPool) {
    let app = router(&pool, false);
    let page = open(&app, 1).await.say("I'm 15 and my sink leaks").await;
    assert_eq!(
        ours(&page),
        vec![body(UNDER_18_TEMPLATE, Language::En, false)]
    );
    assert!(!asks_number(&page), "no number question after INT-038");
    let page = open(&app, 2).await.say("STOP").await;
    assert_eq!(
        ours(&page),
        vec![body(CONFIRMATION_TEMPLATE, Language::En, false)]
    );
    assert!(!asks_number(&page), "no number question after STOP");
    let listings = count(&pool, "messaging.suppression").await;
    assert_eq!(listings, 0, "nothing listed before the code");
    let page = open(&app, 3)
        .await
        .say("my 15-year-old broke the door")
        .await;
    assert!(
        ours(&page).is_empty(),
        "someone else's age is a normal request"
    );
    assert!(asks_number(&page), "it asks the number");
    let page = open(&app, 4).await.say("I'm 15 and I smell gas").await;
    let shown = ours(&page);
    assert_eq!(
        shown.first(),
        Some(&body(SAFETY_TEMPLATE, Language::En, false)),
        "INT-004 first"
    );
}

/// TD-CON-36 (INT-038): after the code "I am 16" shows INT-038's reply, queues the same INT-038
/// text the text path queues and closes the request with no card.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_36_an_under_18_age_after_the_code_closes_the_request(pool: PgPool) {
    let app = router(&pool, false);
    let chat = open(&app, 1).await;
    chat.say(FIRST).await;
    let (id, _) = chat.verify(&pool, "+19285550111", settings(false)).await;
    let page = chat.say("I am 16").await;
    let reply = body(UNDER_18_TEMPLATE, Language::En, false);
    assert_eq!(
        ours(&page),
        vec![reply.clone()],
        "INT-038's reply on the page"
    );
    let queued = texts(&pool, id).await;
    assert!(
        queued.contains(&(UNDER_18_TEMPLATE.to_owned(), reply)),
        "the INT-038 text"
    );
    assert!(request(&pool, id).await.0, "the request closed");
    let cards = count(&pool, "messaging.outbox WHERE template_id LIKE '%card%'").await;
    assert_eq!(cards, 0, "no card");
}

/// TD-CON-36 (INT-038, PRIV-007, D-4726): after the code "I'm 11", on a router whose every
/// connection is `askhava_app`, deletes the request's data except the consent record, and the
/// chat session row, in the same transaction.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_36_an_under_13_age_deletes_the_request_data_and_the_session(pool: PgPool) {
    let app_pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query!("SET ROLE askhava_app")
                    .execute(conn)
                    .await
                    .map(|_| ())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    let app = router(&app_pool, false);
    let chat = open(&app, 1).await;
    chat.say(FIRST).await;
    let (id, _) = chat.verify(&pool, "+19285550113", settings(false)).await;
    let page = chat.say("I'm 11").await;
    assert_eq!(
        ours(&page),
        vec![body(UNDER_18_TEMPLATE, Language::En, false)]
    );
    assert!(request(&pool, id).await.0, "the request closed");
    let of = |table: &str| format!("intake.{table} WHERE conversation_id = '{id}'");
    assert_eq!(
        count(&pool, &of("consent_record")).await,
        1,
        "the consent record kept"
    );
    assert_eq!(
        count(&pool, &of("intake_answer")).await,
        0,
        "its answers deleted"
    );
    assert_eq!(
        count(&pool, &of("web_chat_session")).await,
        0,
        "the session row deleted"
    );
}

/// TD-CON-36 (TXT-002): with Spanish conversations on, a first message in Spanish makes the
/// consent text and the conversation Spanish.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_36_a_spanish_first_message_makes_the_request_spanish(pool: PgPool) {
    let app = router(&pool, true);
    let chat = open(&app, 1).await;
    chat.say("tengo una fuga debajo del fregadero de la cocina")
        .await;
    let (id, queued) = chat.verify(&pool, "+19285550121", settings(true)).await;
    let consent = consent_template(false);
    assert_eq!(queued, vec![consent], "the consent text alone");
    let spanish = body(consent, Language::Es, true);
    assert_eq!(
        texts(&pool, id).await,
        vec![(consent.to_owned(), spanish)],
        "Spanish consent"
    );
    assert_eq!(request(&pool, id).await.2, "es", "a Spanish conversation");
}

/// TD-CON-36 (TXT-041): with Spanish conversations off, a Spanish first message gets the English
/// consent text and then TXT-041's line.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_36_spanish_off_sends_the_txt_041_line_after_the_first_reply(pool: PgPool) {
    let app = router(&pool, false);
    let chat = open(&app, 1).await;
    chat.say("tengo una fuga debajo del fregadero de la cocina")
        .await;
    let (id, queued) = chat.verify(&pool, "+19285550122", settings(false)).await;
    let consent = consent_template(false);
    assert_eq!(
        queued,
        vec![consent, SPANISH_NOT_YET_TEMPLATE],
        "the line after the first reply"
    );
    let line = body(SPANISH_NOT_YET_TEMPLATE, Language::Es, false);
    let all = texts(&pool, id).await;
    assert!(
        all.contains(&(SPANISH_NOT_YET_TEMPLATE.to_owned(), line)),
        "TXT-041's line"
    );
    let english = body(consent, Language::En, false);
    assert!(
        all.contains(&(consent.to_owned(), english)),
        "English consent"
    );
    assert_eq!(request(&pool, id).await.2, "en", "an English conversation");
}
