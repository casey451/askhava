//! The test-control router is absent from the release build and locked in test builds (SEC-035, SEC-002; TD-SEC-15).
//!
//! TD-SEC-15 (this task's cases; the settings cases are in `app/core/src/tests/settings.rs`):
//! without the `test-control` feature every `/__test/*` route is 404 under every `APP_ENV`, even
//! with the right bearer, and the clock is the system clock; no manifest turns the feature on, so
//! a release build cannot pick it up by feature unification. With the feature
//! (`cargo test -p askhava-security --test control_router_absent --features askhava-core/test-control`)
//! the router refuses `test` and `live`, a missing or short secret, and any request without the
//! per-run bearer secret, and the delivered-message read comes from the stand-in's sent log. The
//! cases for the other build return early, so both commands pass. Fake values only.

use std::fs;
use std::path::{Path, PathBuf};

use askhava_core::api::{
    Clock, Settings, SettingsError, SystemClock, TEST_CONTROL_COMPILED, TestControl,
    TestControlError,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::RawQuery;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::ServiceExt;

const TEST_LIST: &str =
    "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
const LIVE_LIST: &str =
    "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";
const ROUTES: [(&str, &str); 3] = [
    ("GET", "/__test/clock"),
    ("POST", "/__test/clock/advance"),
    ("GET", "/__test/messages?to=%2B15205550100"),
];

/// A per-run secret built at run time, so no secret-shaped literal sits in the source.
fn secret() -> String {
    format!("fake-run-secret-{}", "0".repeat(24))
}

fn pairs(env: &str, extra: &[(&str, String)]) -> Vec<(String, String)> {
    let mut all = vec![
        ("ASKHAVA_APP_ENV", env.to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".to_owned(),
        ),
        ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST.to_owned()),
        ("ASKHAVA_TEST_CONTROL_SECRET", secret()),
    ];
    if env == "live" {
        all.push(("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST.to_owned()));
        all.push(("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01".to_owned()));
        all.push(("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake".to_owned()));
        all.push(("ASKHAVA_RESEND_DOMAIN", "mail.example.com".to_owned()));
        all.push(("ASKHAVA_STORAGE_BUCKET", "askhava-live".to_owned()));
    }
    for (key, value) in extra {
        all.retain(|(k, _)| k != key);
        all.push((key, value.clone()));
    }
    all.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn control(env: &str, extra: &[(&str, String)]) -> Result<TestControl, TestControlError> {
    let pairs = pairs(env, extra);
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings parse");
    TestControl::from_settings(&settings, pairs)
}

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
) -> (StatusCode, String) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = if method == "POST" {
        request = request.header("content-type", "application/json");
        Body::from(r#"{"seconds":3600}"#)
    } else {
        Body::empty()
    };
    let reply = router
        .clone()
        .oneshot(request.body(body).expect("request builds"))
        .await
        .expect("router answers");
    let status = reply.status();
    let bytes = to_bytes(reply.into_body(), 1 << 20)
        .await
        .expect("body reads");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn every_route(router: &Router, bearer: Option<&str>) -> Vec<StatusCode> {
    let mut statuses = Vec::new();
    for (method, uri) in ROUTES {
        statuses.push(call(router, method, uri, bearer).await.0);
    }
    statuses
}

fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.join("Cargo.toml")];
    for folder in ["app", "tests"] {
        for entry in fs::read_dir(root.join(folder))
            .expect("folder reads")
            .flatten()
        {
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                found.push(manifest);
            }
        }
    }
    found
}

/// TD-SEC-15: only `core` defines the feature and no manifest enables it, so a release
/// build of the binaries never compiles the routes in (SEC-035, D-491).
#[test]
fn td_sec_15_no_manifest_turns_the_feature_on() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for manifest in manifests(&root) {
        let text = fs::read_to_string(&manifest).expect("manifest reads");
        let mentions: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#') && line.contains("test-control"))
            .collect();
        let core = manifest.ends_with("app/core/Cargo.toml");
        let allowed: &[&str] = if core {
            &["test-control = [\"dep:reqwest\"]"]
        } else {
            &[]
        };
        assert_eq!(mentions, allowed, "{}", manifest.display());
    }
}

/// TD-SEC-15: without the feature every `/__test/*` route is 404 under every `APP_ENV`,
/// even with the right bearer, and the clock is the system clock (SEC-035).
#[tokio::test]
async fn td_sec_15_release_build_has_no_control_routes_under_any_app_env() {
    if TEST_CONTROL_COMPILED {
        return;
    }
    for env in ["dev", "ci", "test", "live"] {
        let control = control(env, &[]).expect("the release build never refuses");
        let router = control.router();
        for bearer in [None, Some(secret())] {
            let statuses = every_route(&router, bearer.as_deref()).await;
            assert_eq!(statuses, [StatusCode::NOT_FOUND; 3], "{env}");
        }
        let drift = control.clock().now().duration_since(SystemClock.now());
        assert!(
            drift.abs().as_secs() < 5,
            "{env}: the clock is not the system clock"
        );
    }
}

/// TD-SEC-15: a test-control build refuses `test` and `live` and a missing or short secret (SEC-035, SEC-002).
#[test]
fn td_sec_15_control_build_refuses_live_test_and_a_weak_secret() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    for env in ["test", "live"] {
        assert_eq!(
            control(env, &[]).err(),
            Some(TestControlError::WrongEnvironment)
        );
    }
    let short = Some(TestControlError::Settings(SettingsError::Invalid(
        "ASKHAVA_TEST_CONTROL_SECRET",
    )));
    let missing = Some(TestControlError::Settings(SettingsError::Missing(
        "ASKHAVA_TEST_CONTROL_SECRET",
    )));
    let key = "ASKHAVA_TEST_CONTROL_SECRET";
    assert_eq!(control("ci", &[(key, "short".to_owned())]).err(), short);
    assert_eq!(control("ci", &[(key, String::new())]).err(), missing);
}

/// TD-SEC-15: in CI the router refuses a request without the per-run bearer secret (SEC-035).
#[tokio::test]
async fn td_sec_15_control_router_needs_the_per_run_bearer() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    let router = control("ci", &[]).expect("ci starts").router();
    let wrong = format!("{}1", &secret()[1..]);
    for bearer in [None, Some(wrong.as_str()), Some("")] {
        let statuses = every_route(&router, bearer).await;
        assert_eq!(statuses, [StatusCode::NOT_FOUND; 3], "a bad bearer passed");
    }
    let (status, _) = call(&router, "GET", "/__test/clock", Some(&secret())).await;
    assert_eq!(status, StatusCode::OK);
}

/// SEC-035: an advance moves the clock both processes read when they share the clock file.
#[tokio::test]
async fn td_sec_15_clock_advance_reaches_every_process_on_the_file() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    let file = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("t020-clock-offset");
    let _ = fs::remove_file(&file);
    let extra = [(
        "ASKHAVA_TEST_CONTROL_CLOCK_FILE",
        file.display().to_string(),
    )];
    let web = control("ci", &extra).expect("web starts");
    let worker = control("ci", &extra).expect("worker starts");
    let (status, _) = call(
        &web.router(),
        "POST",
        "/__test/clock/advance",
        Some(&secret()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ahead = worker.clock().now().duration_since(SystemClock.now());
    assert!(
        (3_595..=3_605).contains(&ahead.as_secs()),
        "worker clock not advanced"
    );
    let _ = fs::remove_file(&file);
}

/// SEC-035, D-491: delivered messages come from the stand-in's sent log, filtered by `To`.
#[tokio::test]
async fn td_sec_15_messages_come_from_the_stand_in_sent_log() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds");
    let port = listener.local_addr().expect("has an address").port();
    let path = format!(
        "/twilio/2010-04-01/Accounts/AC{}/Messages.json",
        "0".repeat(32)
    );
    let fake = Router::new().route(
        &path,
        get(|RawQuery(query): RawQuery| async move {
            let query = query.unwrap_or_default();
            format!(r#"{{"messages":[{{"sid":"SMfake01","query":"{query}"}}],"page":0}}"#)
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, fake).await });
    let base = format!("http://127.0.0.1:{port}/twilio");
    let router = control("ci", &[("ASKHAVA_TWILIO_BASE_URL", base)])
        .expect("ci starts")
        .router();
    let (status, body) = call(&router, "GET", ROUTES[2].1, Some(&secret())).await;
    server.abort();
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""sid":"SMfake01""#), "{body}");
    assert!(body.contains("To=%2B15205550100"), "{body}");
    assert!(
        !body.contains("page"),
        "only the messages are passed on: {body}"
    );
}
