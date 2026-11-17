//! T469 (SEC-001, SEC-002, SEC-014, SEC-025, SEC-032, SEC-036, OPS-038; D-3750): the email
//! provider built from settings, served through `ServedSignIn::from_settings` against PostgreSQL
//! and a local recording server shaped as the Resend stand-in (`POST <base>/emails`, 127.0.0.1
//! only). A sign-in request for a member sends one email to that member's own address and none
//! past the per-business cap; `email_provider` refuses `live` outside live, `stand_in` and a
//! stand-in URL in live, each error naming the key and never a value; with no API key in CI no
//! provider is built and no sign-in email goes. Fake data only (`example` addresses, fake keys).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create};
use askhava_core::api::{MIGRATOR, Settings};
use askhava_messaging::api::DailyLimit;
use askhava_portals::api::admit_sign_in_link;
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::business_signin::ServedSignIn;
use askhava_server::email_provider::email_provider;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use tower::ServiceExt;

const HOST: &str = "example.test";
const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";
const API_KEY: &str = "re_fake_key_never_real_0001";
const DB_URL: &str = "postgres://askhava_app@db.invalid/askhava";
const POSTAL: &str = "PO Box 1234, Faketown, AZ 00000";
const MEMBER: &str = "member@x.example";

/// Each email the recording server took: its recipient and plain text.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

fn key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from(DB_URL.to_owned());
    Vault::new(&key(0x53).into(), &key(0x63).into(), &url).expect("vault")
}

fn ip(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, n))
}

/// Fake pairs for `env`, then `extra` (a later pair replaces nothing; keys are distinct).
fn pairs(env: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let (seal, index) = (key(0x53), key(0x63));
    let list = |tag: &str| {
        format!("twilio:AC{tag},stripe:acct_{tag},resend:{tag}.example.com,bucket:{tag}")
    };
    let (test, live) = (list("testfake"), list("livefake"));
    let tag = if env == "live" {
        "livefake"
    } else {
        "testfake"
    };
    let (sid, account, domain) = (
        format!("AC{tag}"),
        format!("acct_{tag}"),
        format!("{tag}.example.com"),
    );
    let base = [
        ("ASKHAVA_APP_ENV", env),
        ("ASKHAVA_DATABASE_URL", DB_URL),
        ("ASKHAVA_ALLOW_LIST_TEST", test.as_str()),
        ("ASKHAVA_ALLOW_LIST_LIVE", live.as_str()),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", sid.as_str()),
        ("ASKHAVA_STRIPE_ACCOUNT_ID", account.as_str()),
        ("ASKHAVA_RESEND_DOMAIN", domain.as_str()),
        ("ASKHAVA_STORAGE_BUCKET", tag),
        ("ASKHAVA_ID_SECRET", SECRET),
        ("ASKHAVA_POSTAL_ADDRESS", POSTAL),
        ("VAULT_KEY", seal.as_str()),
        ("VAULT_INDEX_KEY", index.as_str()),
    ];
    base.iter()
        .chain(extra)
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// A recording server shaped as the Resend stand-in on 127.0.0.1; returns its base URL.
async fn recorder(seen: Seen) -> String {
    async fn emails(State(seen): State<Seen>, body: String) -> String {
        let email: serde_json::Value = serde_json::from_str(&body).expect("json");
        let field = |name: &str| email[name].as_str().unwrap_or_default().to_owned();
        let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.push((field("to"), field("text")));
        format!("{{\"id\":\"re_fake_{}\"}}", seen.len())
    }
    let app = Router::new()
        .route("/resend/emails", post(emails))
        .with_state(seen);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let at = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{at}/resend")
}

/// The assembled router with the sign-in part `from_settings` built over `pairs`.
fn served(pool: &PgPool, pairs: Vec<(String, String)>) -> Router {
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let part = ServedSignIn::from_settings(&settings, pool.clone(), pairs).expect("sign-in part");
    let parts = RouterParts::from((None, None, None, None)).with_sign_in(part);
    build_router(pool.clone(), Router::new(), parts)
}

/// One business with an owner and a member at [`MEMBER`]; returns the business.
async fn business(pool: &PgPool) -> sqlx::types::Uuid {
    let now = Timestamp::now();
    let mut db = pool.acquire().await.expect("db");
    let owner_box = mailbox_for(&mut db, &vault(), "owner@x.example")
        .await
        .expect("box");
    let business = create(&mut db, false, now).await.expect("business");
    let owner = add_owner(&mut db, business, owner_box, now)
        .await
        .expect("owner");
    let acting = Acting {
        business,
        member: owner,
    };
    let member_box = mailbox_for(&mut db, &vault(), MEMBER).await.expect("box");
    let added = add_member(&mut db, acting, member_box, Roles::BOTH, now).await;
    added.expect("member");
    business
}

/// One request from client `n` carrying `cookie`; returns the `Set-Cookie` pairs and the body.
async fn send(
    app: &Router,
    n: u8,
    (method, cookie, form): (&str, &str, &str),
) -> (Vec<String>, String) {
    let mut request = Request::builder()
        .method(method)
        .uri("/b/sign-in")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from(form.to_owned()))
        .expect("request");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(ip(n), 40_000)));
    let response = app.clone().oneshot(request).await.expect("response");
    let cookies = response.headers().get_all(header::SET_COOKIE).iter();
    let cookies = cookies
        .filter_map(|c| c.to_str().ok()?.split(';').next().map(str::to_owned))
        .collect();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (cookies, String::from_utf8_lossy(&body).into_owned())
}

/// Asks for a sign-in link for `address` from client `n`, as a browser does.
async fn ask(app: &Router, address: &str, n: u8) {
    let (cookies, page) = send(app, n, ("GET", "", "")).await;
    let csrf = page.split("name=\"csrf\" value=\"").nth(1);
    let csrf = csrf.and_then(|r| r.split('"').next()).unwrap_or_default();
    let form = format!("csrf={csrf}&email={address}");
    let (_, page) = send(app, n, ("POST", &cookies.join("; "), &form)).await;
    assert!(page.contains("Check your email"), "the same reply always");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_member_gets_one_sign_in_email_and_none_past_the_cap(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let extra = [
        ("ASKHAVA_RESEND_API_KEY", API_KEY),
        ("ASKHAVA_RESEND_BASE_URL", base.as_str()),
    ];
    let app = served(&pool, pairs("ci", &extra));
    let business = business(&pool).await;
    ask(&app, MEMBER, 1).await;
    let sent = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(sent.len(), 1, "one email per admitted request");
    assert_eq!(sent[0].0, MEMBER, "to the member's own address");
    assert!(sent[0].1.contains("/b/sign-in/"), "it carries the link");
    let now = Timestamp::now();
    for n in 100..=250 {
        let admitted = admit_sign_in_link(&pool, (ip(n), Some(business)), now).await;
        if matches!(admitted, Ok(askhava_intake::api::Admission::Refused(_))) {
            break;
        }
    }
    ask(&app, MEMBER, 2).await;
    let after = seen.lock().unwrap_or_else(PoisonError::into_inner).len();
    assert_eq!(after, 1, "no email past the per-business cap");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn no_key_in_ci_builds_no_provider_and_sends_nothing(pool: PgPool) {
    let seen = Seen::default();
    let base = recorder(Arc::clone(&seen)).await;
    let extra = [("ASKHAVA_RESEND_BASE_URL", base.as_str())];
    let pairs = pairs("ci", &extra);
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let built = email_provider(&settings, pairs.clone()).expect("no error in CI");
    assert!(built.is_none(), "no key: no provider");
    let app = served(&pool, pairs);
    business(&pool).await;
    ask(&app, MEMBER, 1).await;
    assert!(
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "no email"
    );
}

#[test]
fn the_provider_refuses_a_wrong_mode_or_url_naming_only_the_key() {
    let stand_in = "http://127.0.0.1:4010/resend";
    let cases = [
        (
            "ci",
            vec![("ASKHAVA_ADAPTER_MODE_RESEND", "live")],
            "ASKHAVA_ADAPTER_MODE_RESEND",
        ),
        (
            "test",
            vec![("ASKHAVA_ADAPTER_MODE_RESEND", "live")],
            "ASKHAVA_ADAPTER_MODE_RESEND",
        ),
        (
            "live",
            vec![("ASKHAVA_ADAPTER_MODE_RESEND", "stand_in")],
            "ASKHAVA_ADAPTER_MODE_RESEND",
        ),
        (
            "live",
            vec![("ASKHAVA_RESEND_BASE_URL", stand_in)],
            "ASKHAVA_RESEND_BASE_URL",
        ),
        (
            "ci",
            vec![("ASKHAVA_ADAPTER_MODE_RESEND", "sandbox")],
            "ASKHAVA_ADAPTER_MODE_RESEND",
        ),
    ];
    for (env, mut extra, named) in cases {
        extra.push(("ASKHAVA_RESEND_API_KEY", API_KEY));
        let pairs = pairs(env, &extra);
        let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
        let error = email_provider(&settings, pairs).err().expect("refused");
        let text = error.to_string();
        assert!(text.contains(named), "{env}: names {named}: {text}");
        for value in [API_KEY, stand_in, "stand_in", "sandbox", SECRET] {
            assert!(!text.contains(value), "{env}: no value in the error");
        }
    }
}

#[test]
fn a_missing_key_stops_test_and_live_and_the_limits_default() {
    for env in ["test", "live"] {
        let pairs = pairs(env, &[]);
        let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
        let error = email_provider(&settings, pairs).err().expect("refused");
        assert!(
            error.to_string().contains("ASKHAVA_RESEND_API_KEY"),
            "{env}"
        );
    }
    let pairs = pairs("test", &[("ASKHAVA_RESEND_API_KEY", API_KEY)]);
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let provider = email_provider(&settings, pairs)
        .expect("built")
        .expect("some");
    assert_eq!(
        provider.daily_limit(),
        DailyLimit::new(100, 90).expect("limit")
    );
    assert_eq!(provider.monthly_limit(), 3_000);
    let bad = self::pairs(
        "ci",
        &[
            ("ASKHAVA_RESEND_API_KEY", API_KEY),
            ("ASKHAVA_EMAIL_DAILY_LIMIT", "0"),
        ],
    );
    let settings = Settings::from_pairs(bad.clone()).expect("fake settings");
    let error = email_provider(&settings, bad).err().expect("refused");
    assert!(error.to_string().contains("ASKHAVA_EMAIL_DAILY_LIMIT"));
}
