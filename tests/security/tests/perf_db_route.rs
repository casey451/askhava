//! `GET /__test/perf/db` exists only in a test-control build and only behind the bearer (SEC-035; TD-OPS-41, D-3292, D-3298).
//!
//! TD-OPS-41 (the route cases; the counter, bucket and log cases are in
//! `tests/integration/tests/db_metrics.rs`): in the default build, what `Ci` runs, the route is
//! 404 under every `APP_ENV`, even with the right bearer, so the release build has no such route.
//! With the feature (`cargo test -p askhava-security --test perf_db_route --features
//! askhava-core/test-control`) it answers the numbers-only snapshot with the per-run bearer,
//! 404 without it or with a wrong one, and the router refuses `test` and `live`. As in
//! `control_router_absent.rs`, the cases for the other build return early, so both commands pass.
//! Fake values only.

use askhava_core::api::{Settings, TEST_CONTROL_COMPILED, TestControl, TestControlError};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

const ROUTE: &str = "/__test/perf/db";
const TEST_LIST: &str =
    "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
const LIVE_LIST: &str =
    "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";

/// A per-run secret built at run time, so no secret-shaped literal sits in the source.
fn secret() -> String {
    format!("fake-perf-secret-{}", "0".repeat(24))
}

fn pairs(env: &str) -> Vec<(String, String)> {
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
    all.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn control(env: &str) -> Result<TestControl, TestControlError> {
    let pairs = pairs(env);
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings parse");
    TestControl::from_settings(&settings, pairs)
}

/// The router merged into an empty app, as `askhava_server::build_router` merges it.
async fn get(control: &TestControl, bearer: Option<&str>) -> (StatusCode, String) {
    let mut request = Request::builder().method("GET").uri(ROUTE);
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let reply = Router::new()
        .merge(control.router())
        .oneshot(request.body(Body::empty()).expect("request builds"))
        .await
        .expect("router answers");
    let status = reply.status();
    let bytes = to_bytes(reply.into_body(), 1 << 20)
        .await
        .expect("body reads");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// TD-OPS-41: the default build has no `/__test/perf/db` under any `APP_ENV`, bearer or not (SEC-035).
#[tokio::test]
async fn td_ops_41_release_build_has_no_perf_route() {
    if TEST_CONTROL_COMPILED {
        return;
    }
    for env in ["dev", "ci", "test", "live"] {
        let control = control(env).expect("the release build never refuses");
        for bearer in [None, Some(secret())] {
            let (status, _) = get(&control, bearer.as_deref()).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{env}");
        }
    }
}

/// TD-OPS-41: with the bearer the route answers the numbers-only snapshot (D-3298).
#[tokio::test]
async fn td_ops_41_perf_route_answers_the_snapshot_with_the_bearer() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    let control = control("ci").expect("ci starts");
    let (status, body) = get(&control, Some(&secret())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let snapshot: Value = serde_json::from_str(&body).expect("the snapshot is JSON");
    for key in ["statements", "acquires", "wait_sum_us", "wait_max_us"] {
        assert!(snapshot[key].is_u64(), "{key} is a count: {body}");
    }
    let buckets = snapshot["buckets"].as_array().expect("buckets");
    assert_eq!(buckets.len(), 16, "15 bounds and the bucket above 5 s");
    assert_eq!(buckets[0]["le_us"], 100);
    assert_eq!(buckets[14]["le_us"], 5_000_000);
    assert!(buckets[15]["le_us"].is_null());
    assert!(snapshot.get("pool").is_some(), "{body}");
    assert_eq!(snapshot["acquire_timeouts"], "not exposed");
}

/// TD-OPS-41: no bearer, a wrong bearer or an empty one gets the same 404 as an absent route (SEC-035).
#[tokio::test]
async fn td_ops_41_perf_route_is_404_without_the_bearer() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    let control = control("ci").expect("ci starts");
    let wrong = format!("{}1", &secret()[1..]);
    for bearer in [None, Some(wrong.as_str()), Some("")] {
        let (status, body) = get(&control, bearer).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.contains("statements"), "{body}");
    }
}

/// TD-OPS-41: a test-control build refuses `test` and `live`, so the route never serves there (SEC-035).
#[test]
fn td_ops_41_perf_route_is_refused_in_test_and_live() {
    if !TEST_CONTROL_COMPILED {
        return;
    }
    for env in ["test", "live"] {
        assert_eq!(
            control(env).err(),
            Some(TestControlError::WrongEnvironment),
            "{env}"
        );
    }
}
