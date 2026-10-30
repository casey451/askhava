//! Logs and Sentry events hold no contact value (PRIV-016, PRIV-017, PRIV-028; TD-PRIV-13).
//!
//! The sentinels are the values of `tests/fixtures` (T010, D-109), repeated
//! here because `core` is layer 0 and depends on no other first-party crate.
//! Log output is captured from the real JSON layer into a buffer.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::routing::get;
use sentry::protocol::{
    Breadcrumb, Context, Event, Exception, Log, LogLevel, Request as SentryRequest, TraceContext,
    User,
};
use serde::Deserialize;
use serde_json::Value;
use tower::ServiceExt;
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;

use crate::logging::{
    REDACTED, Redacted, json_layer, request_span, scrub_event, scrub_text, sentry_options,
};
use crate::settings::Environment;

const SENTINEL_NAME: &str = "Zelda Canary";
const SENTINEL_PHONE: &str = "+19285550199";
const SENTINEL_EMAIL: &str = "canary@example.com";
const SENTINEL_STREET: &str = "1 Canary Way";
const FAKE_TOKEN: &str = "FAKEtoken0123456789";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        )
        .unwrap()
    }

    fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync {
        let writer = self.clone();
        tracing_subscriber::registry().with(json_layer(move || writer.clone()))
    }
}

fn assert_no_sentinel(text: &str) {
    for sentinel in [
        SENTINEL_NAME,
        SENTINEL_PHONE,
        SENTINEL_EMAIL,
        SENTINEL_STREET,
        "9285550199",
    ] {
        assert!(!text.contains(sentinel), "a sentinel reached the output");
    }
}

/// TD-PRIV-13; PRIV-016, D-492: every log line is one JSON object with `message` and `level` at the top level.
#[test]
fn every_log_line_is_one_json_object() {
    let capture = Capture::default();
    tracing::subscriber::with_default(capture.subscriber(), || {
        let span = tracing::info_span!("request", request_id = "req-1", route = "/q/{token}");
        let _entered = span.enter();
        tracing::info!(attempt = 2, "quote page shown\nsecond line");
        tracing::warn!("slow reply");
    });
    let text = capture.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let object: Value = serde_json::from_str(line).unwrap();
        assert!(object["message"].is_string() && object["level"].is_string());
        assert_eq!(object["spans"][0]["request_id"], "req-1");
    }
}

/// TD-PRIV-13; PRIV-016, PRIV-028: a `Redacted` value logged with `?` or `%` prints only the marker.
#[test]
fn redacted_values_never_reach_a_log() {
    let phone = Redacted::new(SENTINEL_PHONE.to_owned());
    let name = Redacted::new(SENTINEL_NAME.to_owned());
    let capture = Capture::default();
    tracing::subscriber::with_default(capture.subscriber(), || {
        tracing::info!(phone = ?phone, name = %name, "contact saved");
        tracing::error!("failed for {phone} and {name:?}");
    });
    let text = capture.text();
    assert_no_sentinel(&text);
    assert!(text.contains(REDACTED));
    assert_eq!(phone.expose(), SENTINEL_PHONE);
}

/// TD-PRIV-13; PRIV-028: contact fields deserialize straight into `Redacted<String>`.
#[test]
fn contact_fields_deserialize_into_redacted() {
    #[derive(Debug, Deserialize)]
    struct Form {
        phone: Redacted<String>,
        email: Redacted<String>,
    }
    let body = format!(r#"{{"phone":"{SENTINEL_PHONE}","email":"{SENTINEL_EMAIL}"}}"#);
    let form: Form = serde_json::from_str(&body).unwrap();
    assert_eq!(form.phone.expose(), SENTINEL_PHONE);
    assert_eq!(form.email.expose(), SENTINEL_EMAIL);
    assert_no_sentinel(&format!("{form:?}"));
}

/// TD-PRIV-13; PRIV-028, PRIV-016: the request log for a token route shows the route template only.
#[tokio::test(flavor = "current_thread")]
async fn request_log_shows_route_template_only() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let app = Router::new()
        .route("/q/{token}", get(|| async { "ok" }))
        .layer(TraceLayer::new_for_http().make_span_with(request_span::<Body>));
    let uri = format!("/q/{FAKE_TOKEN}?email={SENTINEL_EMAIL}");
    let request = Request::get(uri)
        .header("x-request-id", "req-42")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert!(response.status().is_success());
    let text = capture.text();
    assert!(!text.is_empty());
    assert!(text.contains(r#""route":"/q/{token}""#) && text.contains(r#""request_id":"req-42""#));
    assert!(!text.contains(FAKE_TOKEN));
    assert_no_sentinel(&text);
}

/// TD-PRIV-13; PRIV-016: a request ID that is not short letters, digits and dashes is not logged.
#[test]
fn unsafe_request_id_is_not_logged() {
    let capture = Capture::default();
    tracing::subscriber::with_default(capture.subscriber(), || {
        let request = Request::get("/")
            .header("x-request-id", SENTINEL_EMAIL)
            .body(())
            .unwrap();
        let span = request_span(&request);
        let _entered = span.enter();
        tracing::info!("handled");
    });
    let text = capture.text();
    assert!(text.contains(r#""request_id":"invalid""#) && text.contains(r#""route":"unmatched""#));
    assert_no_sentinel(&text);
}

fn event_full_of_contact_data() -> Event<'static> {
    let mut event = Event {
        message: Some(format!("panic for {SENTINEL_PHONE}")),
        user: Some(User {
            email: Some(SENTINEL_EMAIL.to_owned()),
            ..Default::default()
        }),
        request: Some(SentryRequest {
            query_string: Some(format!("token={FAKE_TOKEN}")),
            ..Default::default()
        }),
        transaction: Some(format!("GET /q/{FAKE_TOKEN}")),
        ..Default::default()
    };
    event.exception.values.push(Exception {
        ty: "panic".to_owned(),
        value: Some(format!(
            "could not text +1 (928) 555-0199 or {SENTINEL_EMAIL}"
        )),
        ..Default::default()
    });
    event
        .extra
        .insert("street".to_owned(), SENTINEL_STREET.into());
    event
        .tags
        .insert("to".to_owned(), SENTINEL_PHONE.to_owned());
    let fields = [("name".to_owned(), Value::from(SENTINEL_NAME))];
    event.contexts.insert(
        "Rust Tracing Fields".to_owned(),
        Context::Other(fields.into_iter().collect()),
    );
    let trace = TraceContext {
        description: Some(format!("GET /q/{FAKE_TOKEN}")),
        data: [("to".to_owned(), Value::from(SENTINEL_PHONE))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    event
        .contexts
        .insert("trace".to_owned(), Context::Trace(Box::new(trace)));
    event.breadcrumbs.values.push(Breadcrumb {
        message: Some(SENTINEL_EMAIL.to_owned()),
        ..Default::default()
    });
    event
}

/// TD-PRIV-13; PRIV-017: the Sentry scrubber leaves no sentinel and no token in an event.
#[test]
fn sentry_event_leaves_with_no_sentinel() {
    let scrubbed = scrub_event(event_full_of_contact_data());
    let json = serde_json::to_string(&scrubbed).unwrap();
    assert_no_sentinel(&json);
    assert!(!json.contains(FAKE_TOKEN));
    assert!(scrubbed.user.is_none() && scrubbed.request.is_none());
    assert_eq!(scrubbed.message.as_deref(), Some("panic for [redacted]"));
    assert!(matches!(
        scrubbed.contexts.get("trace"),
        Some(Context::Trace(_))
    ));
}

/// PRIV-017, D-690: Sentry's options drop every log record, so no log body is forwarded.
#[test]
fn sentry_options_drop_every_log_record() {
    let dsn = "https://fakepublic@example.com/1".parse().unwrap();
    let options = sentry_options(dsn, Environment::Ci);
    assert!(!options.send_default_pii);
    let log = Log {
        level: LogLevel::Error,
        body: format!("could not text {SENTINEL_PHONE}"),
        trace_id: None,
        timestamp: std::time::SystemTime::UNIX_EPOCH,
        severity_number: None,
        attributes: Default::default(),
    };
    let before_send_log = options.before_send_log.as_ref().unwrap();
    assert!(before_send_log(log).is_none());
}

/// PRIV-017: the scrubber keeps ordinary words and short numbers.
#[test]
fn scrubber_keeps_ordinary_text() {
    assert_eq!(
        scrub_text("job 42 failed after 3 tries"),
        "job 42 failed after 3 tries"
    );
    assert_eq!(scrub_text("call 928-555-0199 now"), "call [redacted] now");
    assert_eq!(
        scrub_text("mail canary@example.com, then stop"),
        "mail [redacted] then stop"
    );
}
