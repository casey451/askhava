//! TD-SEC-74 (SEC-014, SEC-040; T646, D-4292, D-4880): the web chat's new-request limit is keyed
//! on the client IP `portals::api::security_event_client` reads from the edge's header, never on
//! the socket peer. The assembled router is served with connect info from one peer address (the
//! host's edge), under the client-IP policy as the app sets it: no `configure_security_events`
//! call here, so the default policy reads `core::api::DEFAULT_CLIENT_IP_HEADER` (`x-real-ip`),
//! as `main` does with `ASKHAVA_CLIENT_IP_HEADER` unset. Each chat runs its job message, number
//! and code steps against a local fake Verify that approves every code. Verified chats from
//! eleven different clients all open; ten from one client open and its eleventh is refused with
//! the new-request limit's answer. Fake data only: 555-01xx numbers, documentation-range IPs.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_core::api::{DEFAULT_CLIENT_IP_HEADER, MIGRATOR, Settings};
use askhava_server::chat_codes::chat_keys;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sqlx::PgPool;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const PATH: &str = "/request";
const PRE: &str = "__Host-askhava-pre";
/// The one socket peer every request comes from: the host's edge (documentation range).
const EDGE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
/// Fake job words with no name, number or address.
const WORDS: &str = "the+kitchen+sink+drips+under+the+cabinet";
/// The fake Verify's answer to both a send and a check: every code is right.
const APPROVED: &str = r#"{"sid":"VE00000000000000000000000000000074","valid":true}"#;
/// The page's answer for an opened request, and the new-request limit's refusal (escape-free).
const OPENED: &str = "got your request";
const REFUSED: &str = "take new requests right now";

/// The assembled router with the chat's code keys over a fake Verify, and its one cookie pair
/// and token.
struct World {
    app: Router,
    cookie: String,
    token: String,
}

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

/// Dev settings with the vault, `ID_SECRET` and Twilio Verify at `verify`. Fake values.
fn pairs(verify: &str) -> Vec<(String, String)> {
    let fixed = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-not-a-real-key-0074",
        ),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", verify),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ];
    let keys = [
        ("VAULT_KEY", vault_key(0x51)),
        ("VAULT_INDEX_KEY", vault_key(0x52)),
    ];
    fixed
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .chain(keys.into_iter().map(|(k, v)| (k.to_owned(), v)))
        .collect()
}

async fn world(pool: &PgPool) -> World {
    let pairs = pairs(&fake_verify().await);
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let keys = chat_keys(&settings, pairs).expect("chat keys");
    assert!(keys.is_some(), "the chat's code keys are configured");
    let parts = RouterParts::from((None, None, None, None)).with_chat_codes(keys);
    let app = build_router(pool.clone(), Router::new(), parts);
    let page = send(&app, "GET", "", "", None).await;
    let cookie = page.1.expect("a pre-session cookie");
    let token = page.0.split("name=\"csrf\" value=\"").nth(1);
    let token = token.and_then(|rest| rest.split('"').next());
    World {
        app,
        cookie,
        token: token.expect("token").to_owned(),
    }
}

/// Sends `method` to `/request` from the one edge peer with `cookie`, `form` and, when given,
/// `client` in the edge's client-IP header: the page and the pre-session cookie it set.
async fn send(
    app: &Router,
    method: &str,
    cookie: &str,
    form: &str,
    client: Option<IpAddr>,
) -> (String, Option<String>) {
    let mut request = Request::builder()
        .method(method)
        .uri(PATH)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie);
    if let Some(client) = client {
        request = request.header(DEFAULT_CLIENT_IP_HEADER, client.to_string());
    }
    let mut request = request.body(Body::from(form.to_owned())).expect("request");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(EDGE, 40_000)));
    let response = app.clone().oneshot(request).await.expect("response");
    assert!(response.status().as_u16() < 400, "{}", response.status());
    let pre = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|c| c.to_str().ok()?.split(';').next().map(str::to_owned))
        .find(|pair| pair.starts_with(&format!("{PRE}=")));
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (String::from_utf8_lossy(&body).into_owned(), pre)
}

/// Chat `n` from `client`: its job message, its fake number (555-01xx) and a code; the last page.
async fn chat(w: &World, n: u16, client: IpAddr) -> String {
    let key = format!("{:032x}", 0x7400 + u32::from(n));
    let number = format!("928555{:04}", 100 + n);
    let mut page = String::new();
    for (i, step) in ["message", "number", "code"].into_iter().enumerate() {
        let message = format!("00000000-0000-4000-8000-0000{:04}{:04}", n, i);
        let form = format!(
            "csrf={}&chat={key}&message={message}&g=&said=&step={step}&words={WORDS}\
             &number={number}&code=123456",
            w.token
        );
        page = send(&w.app, "POST", &w.cookie, &form, Some(client)).await.0;
    }
    page
}

/// Client `n` in a documentation range, never the edge.
fn client(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(198, 51, 100, n))
}

/// TD-SEC-74: eleven verified chats from eleven different clients, all through the one edge
/// peer, all open: had the limit counted the peer, the eleventh would have been refused.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_74_eleven_clients_behind_one_edge_all_open(pool: PgPool) {
    let w = world(&pool).await;
    for n in 1..=11 {
        let page = chat(&w, n.into(), client(n)).await;
        assert!(page.contains(OPENED), "client {n} was not opened");
    }
}

/// TD-SEC-74: ten verified chats from one client open, and that client's eleventh is refused
/// with the new-request limit's answer, while another client behind the same edge still opens.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_74_one_clients_eleventh_request_is_refused(pool: PgPool) {
    let w = world(&pool).await;
    let one = client(200);
    for n in 1..=10 {
        let page = chat(&w, n, one).await;
        assert!(page.contains(OPENED), "request {n} was not opened");
    }
    let eleventh = chat(&w, 11, one).await;
    assert!(eleventh.contains(REFUSED), "the eleventh was not refused");
    assert!(!eleventh.contains(OPENED));
    assert!(chat(&w, 12, client(201)).await.contains(OPENED));
}
