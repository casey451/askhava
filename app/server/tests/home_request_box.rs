//! T593 (WEB-009, WEB-012, CON-025, AI-021, A11Y-035, SEC-042, D-1515, D-3331, D-4098): the home
//! request box served by the assembled router against PostgreSQL. A hero `POST /request` (the
//! `words` field, no chat key) lands on the CON-025 start screen with her words in the chat box,
//! escaped and cut to `WORDS_MAX_CHARS`, writes no row in `intake`, `privacy`, `ai`, `messaging`,
//! `jobs` or `core`'s task table, and puts her words in no log line; a token-less or cross-site hero post
//! is refused (403); the hero post's answer and the three home paths are `no-store`; and `GET /`
//! holds every non-fixed home block of `docs/copy/site-copy.md` as today's market renders it, the
//! kicker read from `markets.yaml`. Fake words only: no name, phone or address.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use askhava_core::api::MIGRATOR;
use askhava_ops::api::STREAM_KIND;
use askhava_server::{RouterParts, build_router};
use askhava_site::api::WORDS_MAX_CHARS;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use sqlx::PgPool;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const PRE: &str = "__Host-askhava-pre";
/// Fake job words with no name, number or address, and a marker no log line may hold.
const MARKER: &str = "fake-marker-t593 the pool pump hums but no water moves";
/// The three paths that answer with the home page (WEB-012).
const HOMES: [&str; 3] = ["/", "/categories", "/events"];
/// The schemas whose rows a hero post must not change, beside `core`'s task table ([`TASKS`]).
const SCHEMAS: [&str; 5] = ["intake", "privacy", "ai", "messaging", "jobs"];
/// `core`'s task table, as schema and table names.
const TASKS: (&str, &str) = ("core", "task");
/// The repository root, a run-time base; the site copy is `docs/copy/site-copy.md` under it.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// One answer: status, body, whether it is `no-store` and the pre-session cookie it set.
struct Reply {
    status: u16,
    text: String,
    no_store: bool,
    pre: Option<String>,
}

fn app(pool: &PgPool) -> Router {
    build_router(
        pool.clone(),
        Router::new(),
        RouterParts::from((None, None, None, None)),
    )
}

/// Sends `method` to `path` with `cookie`, `Sec-Fetch-Site: site` and `form`.
async fn send(
    app: &Router,
    (method, path): (&str, &str),
    (cookie, site): (&str, &str),
    form: &str,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", site)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from(form.to_owned()))
        .expect("request");
    let at = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 93)), 40_000);
    request.extensions_mut().insert(ConnectInfo(at));
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let control = response.headers().get(header::CACHE_CONTROL);
    let no_store = control.is_some_and(|v| v.to_str().is_ok_and(|v| v.contains("no-store")));
    let pre = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|c| c.to_str().ok()?.split(';').next().map(str::to_owned))
        .find(|pair| pair.starts_with(&format!("{PRE}=")));
    let body = to_bytes(response.into_body(), 1 << 21).await.expect("body");
    let text = String::from_utf8_lossy(&body).into_owned();
    Reply {
        status,
        text,
        no_store,
        pre,
    }
}

/// Opens home path `path`: its pre-session cookie pair, its request box's token and the page.
async fn opened(app: &Router, path: &str) -> (String, String, Reply) {
    let page = send(app, ("GET", path), ("", "same-origin"), "").await;
    let cookie = page.pre.clone().expect("a pre-session cookie");
    let token = page.text.split("name=\"csrf\" value=\"").nth(1);
    let token = token.and_then(|r| r.split('"').next()).expect("token");
    (cookie, token.to_owned(), page)
}

/// `raw` form-encoded: every byte but ASCII letters and digits as `%XX`.
fn encoded(raw: &str) -> String {
    raw.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The hero form: `csrf` (no field when `None`) and her words as `words`.
fn hero(csrf: Option<&str>, job: &str) -> String {
    let job = format!("words={}", encoded(job));
    csrf.map_or(job.clone(), |t| format!("csrf={t}&{job}"))
}

/// The text of the chat box (`chat-words`) in `html`, with HTML escapes undone.
fn chat_box(html: &str) -> String {
    let rest = html
        .split("id=\"chat-words\"")
        .nth(1)
        .expect("the chat box");
    let inner = rest.split_once('>').expect("its tag").1;
    unescaped(inner.split("</textarea>").next().expect("its end"))
}

fn unescaped(text: &str) -> String {
    [
        ("&lt;", "<"),
        ("&#60;", "<"),
        ("&gt;", ">"),
        ("&#62;", ">"),
        ("&quot;", "\""),
        ("&#34;", "\""),
        ("&#39;", "'"),
        ("&amp;", "&"),
        ("&#38;", "&"),
    ]
    .iter()
    .fold(text.to_owned(), |t, (from, to)| t.replace(from, to))
}

/// Every row count in the five schemas, by table (each count run by `query_to_xml`, so the SQL
/// is one checked statement), then the task table's ([`tasks`]).
async fn counts(pool: &PgPool) -> Vec<(String, i64)> {
    let schemas: Vec<String> = SCHEMAS.map(str::to_owned).to_vec();
    let rows = sqlx::query!(
        r#"SELECT (table_schema || '.' || table_name)::text AS "name!",
             (xpath('/row/n/text()', query_to_xml(
                 format('SELECT count(*) AS n FROM %I.%I', table_schema, table_name),
                 false, true, '')))[1]::text::bigint AS "n!"
           FROM information_schema.tables
           WHERE table_type = 'BASE TABLE' AND table_schema::text = ANY($1)
           ORDER BY 1"#,
        &schemas,
    )
    .fetch_all(pool)
    .await
    .expect("counts");
    assert!(rows.len() > 20, "the five schemas' tables were found");
    let mut all: Vec<(String, i64)> = rows.into_iter().map(|r| (r.name, r.n)).collect();
    all.push(("task table (not request log)".to_owned(), tasks(pool).await));
    all
}

/// The task table's rows other than OPS-011's request-log lines, which every request queues (route
/// template and status, never a body) from a background writer at its own pace.
async fn tasks(pool: &PgPool) -> i64 {
    sqlx::query_scalar!(
        r#"SELECT (xpath('/row/n/text()', query_to_xml(
               format('SELECT count(*) AS n FROM %I.%I WHERE kind <> %L', $1::text, $2::text, $3::text),
               false, true, '')))[1]::text::bigint AS "n!""#,
        TASKS.0,
        TASKS.1,
        STREAM_KIND
    )
    .fetch_one(pool)
    .await
    .expect("tasks")
}

/// AI-021, A11Y-035, CON-025: a hero post writes no row, queues no task and lands on the start
/// screen (the consent text and the 18-or-older line above the chat box) with her words in the
/// chat box, escaped; a planted `<script>` comes back as text; one character over the maximum
/// comes back cut to it; the answer is `no-store` and sets no cookie beyond the pre-session one.
#[sqlx::test(migrator = "MIGRATOR")]
async fn t593_hero_post_stores_nothing_and_fills_the_chat_box(pool: PgPool) {
    let app = app(&pool);
    let (cookie, token, _) = opened(&app, "/").await;
    let before = counts(&pool).await;
    let planted = format!("{MARKER} <script>alert(1)</script> & \"quotes\"");
    let reply = send(
        &app,
        ("POST", "/request"),
        (&cookie, "same-origin"),
        &hero(Some(&token), &planted),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert!(reply.no_store, "the hero post's answer is no-store");
    assert!(
        reply.pre.is_none(),
        "no new cookie: the pre-session one is already set"
    );
    assert_eq!(
        counts(&pool).await,
        before,
        "no row written, no task queued"
    );
    assert_eq!(chat_box(&reply.text), planted);
    assert!(!reply.text.contains("<script>alert"), "the script is text");
    let brand = askhava_core::api::brand().display_name();
    let age = reply
        .text
        .find(&format!("You must be 18 or older to use {brand}."));
    let chat = reply.text.find("id=\"chat-words\"").expect("chat box");
    assert!(
        age.is_some_and(|at| at < chat),
        "the 18-or-older line is above the chat box"
    );
    assert!(
        reply
            .text
            .find("data-fixed=\"WEB-014\"")
            .is_some_and(|at| at < chat),
        "consent first"
    );
    assert!(
        reply
            .text
            .contains(&format!("maxlength=\"{WORDS_MAX_CHARS}\""))
    );
    let long = "a".repeat(WORDS_MAX_CHARS + 1);
    let cut = send(
        &app,
        ("POST", "/request"),
        (&cookie, "same-origin"),
        &hero(Some(&token), &long),
    )
    .await;
    assert_eq!(
        chat_box(&cut.text),
        "a".repeat(WORDS_MAX_CHARS),
        "cut to the maximum"
    );
    assert_eq!(counts(&pool).await, before, "still no row");
}

/// SEC-042, WEB-012: a token-less and a cross-site hero post are refused (403, no words back);
/// a hero post from `/categories` lands on the start screen like one from `/`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn t593_hero_post_needs_its_token_and_works_from_old_paths(pool: PgPool) {
    let app = app(&pool);
    let (cookie, token, _) = opened(&app, "/").await;
    for (case, form, site) in [
        ("token-less", hero(None, MARKER), "same-origin"),
        ("cross-site", hero(Some(&token), MARKER), "cross-site"),
    ] {
        let reply = send(&app, ("POST", "/request"), (&cookie, site), &form).await;
        assert_eq!(reply.status, 403, "{case}");
        assert!(!reply.text.contains(MARKER), "{case}: no words back");
    }
    let (cookie, token, _) = opened(&app, "/categories").await;
    let reply = send(
        &app,
        ("POST", "/request"),
        (&cookie, "same-origin"),
        &hero(Some(&token), MARKER),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_eq!(chat_box(&reply.text), MARKER);
}

/// The text of every log event and span field captured.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn push(&self, line: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The binary's one global subscriber, installed on first use. A global one, not a thread's
/// default: a callsite another test's thread registers first would otherwise stay cached as
/// "never" for this thread. Every test of this file posts only fake words, and none may log them.
fn captured() -> Lines {
    static LINES: OnceLock<Lines> = OnceLock::new();
    LINES
        .get_or_init(|| {
            let lines = Lines::default();
            tracing::subscriber::set_global_default(lines.clone()).expect("one global subscriber");
            lines
        })
        .clone()
}

/// One event's or span's fields as text.
#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl tracing::Subscriber for Lines {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        span.record(&mut fields);
        self.push(fields.0);
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        self.push(fields.0);
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.push(fields.0);
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// PRIV-016, AI-021: under a captured subscriber, the marker posted through the hero form, a
/// token-less post and a cross-site post appears in no log line (the refusals did log).
#[sqlx::test(migrator = "MIGRATOR")]
async fn t593_her_words_reach_no_log_line(pool: PgPool) {
    let lines = captured();
    let app = app(&pool);
    let (cookie, token, _) = opened(&app, "/").await;
    let posts = [
        (hero(Some(&token), MARKER), "same-origin", 200),
        (hero(None, MARKER), "same-origin", 403),
        (hero(Some(&token), MARKER), "cross-site", 403),
    ];
    for (form, site, status) in posts {
        let reply = send(&app, ("POST", "/request"), (&cookie, site), &form).await;
        assert_eq!(reply.status, status, "{site}");
    }
    let all = lines.all();
    assert!(!all.is_empty(), "the subscriber captured the requests");
    let marker = MARKER.split(' ').next().expect("marker word");
    for line in &all {
        assert!(!line.contains(marker), "her words in a log line: {line}");
        assert!(
            !line.contains(&encoded(MARKER)),
            "her encoded words in a log line: {line}"
        );
    }
}

/// The page's text: tags dropped, escapes undone, whitespace collapsed.
fn page_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = unescaped(&text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every line of every `## Home ·` block of the site copy not marked `fixed`.
fn home_lines() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    let copy = std::fs::read_to_string(std::path::Path::new(ROOT).join("docs/copy/site-copy.md"))
        .expect("site copy");
    for line in copy.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            let home = head.starts_with("Home ·") && !head.contains("fixed)");
            current = home.then(|| head.to_owned());
        } else if let (Some(head), Some(copy)) = (&current, line.strip_prefix("> ")) {
            out.push((head.clone(), copy.trim().to_owned()));
        }
    }
    out
}

/// WEB-012, D-3331, D-4098, D-1515: `/`, `/categories` and `/events` are `no-store`; `GET /`
/// holds every non-fixed home block verbatim as today's market renders it, the kicker reads the
/// market's name and state from `markets.yaml`, and the October 28 draft is gone.
#[sqlx::test(migrator = "MIGRATOR")]
async fn t593_home_paths_are_no_store_and_hold_the_copy(pool: PgPool) {
    let app = app(&pool);
    for path in HOMES {
        let page = send(&app, ("GET", path), ("", "same-origin"), "").await;
        assert_eq!(page.status, 200, "{path}");
        assert!(page.no_store, "{path} is no-store");
    }
    let (_, _, page) = opened(&app, "/").await;
    let text = page_text(&page.text);
    let lines = home_lines();
    assert!(lines.len() >= 15, "the home blocks were read");
    for (head, copy) in lines {
        assert!(text.contains(&copy), "{head}: {copy}");
    }
    let markets = askhava_catalog::api::markets().expect("markets");
    let market = markets.markets().first().expect("one market");
    let state = market.state_name.as_deref().expect("state_name");
    let kicker = format!("<p class=\"kicker\">{}, {state}</p>", market.name);
    assert!(page.text.contains(&kicker), "the kicker from markets.yaml");
    for gone in [
        "For homes in Lake Havasu City",
        "Get quotes from Havasu businesses without giving out your number.",
    ] {
        assert!(!text.contains(gone), "{gone}");
    }
}
