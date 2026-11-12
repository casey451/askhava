//! TD-SEC-62 (T362, D-2633, D-3106; PICK-009, A11Y-033, SEC-012): with the built files' folder
//! set to a temporary folder holding a fake `portals/quote_page.js` (`Ci` builds no `.js`),
//! `build_router` serves `/static/portals/quote_page.js` as JavaScript under the security headers;
//! plain and encoded traversal, a directory and an unknown crate answer 404; `portals`' own
//! stylesheet route still answers; the folder setting is read as `server` reads it. Fake data only.

use std::path::PathBuf;

use askhava_core::api::Settings;
use askhava_server::request_timeout::RequestTimeout;
use askhava_server::static_files::{StaticRoot, content_hashed, image_dir};
use askhava_server::{Hooks, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const FAKE_SCRIPT: &str = "export const fakeT362 = 1;\n";

/// A fresh temporary folder: `portals/quote_page.js`, a hashed copy, a `portals/inner/` folder
/// and a file beside `portals/` that no path may reach.
fn fake_folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("askhava-t362-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("portals").join("inner")).expect("folders");
    std::fs::write(root.join("portals").join("quote_page.js"), FAKE_SCRIPT).expect("script");
    std::fs::write(root.join("portals").join("app.0123abcd.js"), FAKE_SCRIPT).expect("hashed");
    std::fs::write(root.join("outside.txt"), "fake outside T362").expect("outside");
    root
}

fn app(root: PathBuf) -> Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("a lazy pool");
    let hooks: Hooks = (None, None, None, None);
    let parts = (hooks, RequestTimeout::DEFAULT, StaticRoot::Built(root));
    build_router(pool, Router::new(), parts)
}

async fn get(app: &Router, path: &str) -> Response {
    let request = Request::get(path).body(Body::empty()).expect("request");
    app.clone().oneshot(request).await.expect("infallible")
}

fn header<'a>(response: &'a Response, name: &str) -> &'a str {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

/// TD-SEC-62 (PICK-009, A11Y-033, SEC-012): the quote page's script answers 200 as JavaScript,
/// byte for byte, with every security header and a revalidated cache.
#[tokio::test]
async fn td_sec_62_the_quote_page_script_is_served_as_javascript_under_the_headers() {
    let app = app(fake_folder("served"));
    let response = get(&app, "/static/portals/quote_page.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(header(&response, "content-type").contains("javascript"));
    assert_eq!(header(&response, "x-content-type-options"), "nosniff");
    assert!(header(&response, "content-security-policy").contains("object-src 'none'"));
    assert!(header(&response, "content-security-policy").contains("frame-ancestors 'none'"));
    assert!(header(&response, "strict-transport-security").contains("includeSubDomains"));
    assert!(header(&response, "permissions-policy").contains("camera=()"));
    assert!(!header(&response, "x-request-id").is_empty());
    assert_eq!(header(&response, "cache-control"), "no-cache");
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    assert_eq!(body.as_ref(), FAKE_SCRIPT.as_bytes());
}

/// TD-SEC-62 (SEC-012): plain and encoded traversal, a directory (with and without its slash)
/// and an unknown crate answer 404 under the security headers, never the file outside.
#[tokio::test]
async fn td_sec_62_traversal_directories_and_unknown_crates_answer_404() {
    let app = app(fake_folder("refused"));
    let refused = [
        "/static/../Cargo.toml",
        "/static/portals/../outside.txt",
        "/static/portals/%2e%2e/outside.txt",
        "/static/portals/%2E%2E%2Foutside.txt",
        "/static/portals/..%5Coutside.txt",
        "/static/portals",
        "/static/portals/",
        "/static/portals/inner",
        "/static/portals/inner/",
        "/static/nosuchcrate/quote_page.js",
        "/static/outside.txt",
        "/static/portals/missing.js",
    ];
    for path in refused {
        let response = get(&app, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            header(&response, "x-content-type-options"),
            "nosniff",
            "{path}"
        );
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        assert!(
            !String::from_utf8_lossy(&body).contains("fake outside"),
            "{path}"
        );
    }
}

/// TD-SEC-62 (D-3106): `portals`' stylesheet route keeps its own answer beside the mount, a
/// write method is refused, and only a found content-hashed name is cached for a year.
#[tokio::test]
async fn td_sec_62_the_stylesheet_route_stays_and_only_hashed_names_cache_long() {
    let app = app(fake_folder("cache"));
    let css = get(&app, "/static/portals/feedback.css").await;
    assert_eq!(css.status(), StatusCode::OK);
    assert!(header(&css, "content-type").starts_with("text/css"));
    let hashed = get(&app, "/static/portals/app.0123abcd.js").await;
    assert_eq!(hashed.status(), StatusCode::OK);
    assert_eq!(
        header(&hashed, "cache-control"),
        "public, max-age=31536000, immutable"
    );
    let missing = get(&app, "/static/portals/gone.0123abcd.js").await;
    assert_eq!(header(&missing, "cache-control"), "no-cache");
    let post = Request::post("/static/portals/quote_page.js")
        .body(Body::empty())
        .expect("request");
    let post = app.clone().oneshot(post).await.expect("infallible");
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(content_hashed("/static/portals/quote_page.9f8e7d6c5b.js"));
    assert!(!content_hashed("/static/portals/quote_page.js"));
    assert!(!content_hashed("/static/portals/.0123abcd.js"));
    assert!(!content_hashed("/static/portals/quote_page.notahash.js"));
}

/// Fake allow-lists and accounts, so test and live start (SEC-036).
const TEST_LIST: &str =
    "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
const LIVE_LIST: &str =
    "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";

fn settings(env: &str, dir: Option<&str>) -> Settings {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", env),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
    ];
    if env == "live" {
        pairs.extend([
            ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
            ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01"),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake"),
            ("ASKHAVA_RESEND_DOMAIN", "mail.example.com"),
            ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
        ]);
    }
    if let Some(dir) = dir {
        pairs.push(("ASKHAVA_STATIC_DIR", dir));
    }
    let pairs = pairs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned()));
    Settings::from_pairs(pairs).expect("settings")
}

/// TD-SEC-62 (D-2633): `ASKHAVA_STATIC_DIR` wins; unset, test and live read the image's folder
/// and dev and CI the repository's `app/<crate>/static/`.
#[test]
fn td_sec_62_the_folder_comes_from_the_setting_or_the_environment() {
    let set = StaticRoot::from_settings(&settings("ci", Some(" /tmp/fake-static ")));
    assert_eq!(
        set.folder("portals"),
        PathBuf::from("/tmp/fake-static/portals")
    );
    for env in ["test", "live"] {
        let root = StaticRoot::from_settings(&settings(env, None));
        assert_eq!(root.folder("portals"), image_dir().join("portals"), "{env}");
    }
    for env in ["dev", "ci"] {
        let root = StaticRoot::from_settings(&settings(env, None));
        assert_eq!(
            root.folder("portals"),
            PathBuf::from("./app/portals/static"),
            "{env}"
        );
    }
    assert_eq!(StaticRoot::default(), StaticRoot::Built(image_dir()));
}

/// TD-SEC-62 (D-3242): the image copies the built files to the folder `image_dir()` names, so a
/// package rename that moves `image_dir()` away from the Dockerfile's `COPY` line fails here. Read
/// at run time, not compiled in: an `include_str!` would need the Dockerfile in `Ci`'s Rust path
/// rules, which are protected (D-1867).
#[test]
fn td_sec_62_the_image_folder_is_where_the_dockerfile_copies_the_built_files() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Dockerfile");
    let dockerfile = std::fs::read_to_string(path).expect("the Dockerfile");
    let copy = format!("COPY --from=browser /out/static {}", image_dir().display());
    assert!(
        dockerfile.lines().any(|line| line.trim() == copy),
        "the Dockerfile has no line `{copy}`"
    );
}
