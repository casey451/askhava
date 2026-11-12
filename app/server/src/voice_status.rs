//! Twilio's callbacks about the calls the app places: their progress and the safety call's key
//! press (CALL-022, OPS-028, CON-035, SEC-016; D-2762, D-2771, D-3231).
//!
//! [`routes`] adds two POST routes. A delivery is taken only when its `X-Twilio-Signature` passed
//! `adapters::api::verify_webhook` over `ASKHAVA_PUBLIC_BASE_URL` plus the path and query; any
//! other gets 403 and one log line. Twilio signs the query, which `calls` set when it placed the
//! call, so nobody can change it.
//!
//! - [`STATUS_PATH`]: every status and answering-machine-detection callback. A safety call's
//!   query is `calls::api::SafetyCall::query`; `calls::api::safety_call_status` decides the call
//!   once, by the detection result or a final no-answer, and acknowledges Casey's alert when a
//!   person answered; every other callback (`initiated`, `ringing`, `answered`, a `completed`
//!   after a decision) changes nothing. An owner-bridge callback (`bridge=<alert>`) changes
//!   nothing. A failed step keeps nothing and is 500, so a repeat can still decide; else 200.
//! - [`KEY_PATH`]: the safety call's `Gather` action. `calls::api::safety_key_once` takes the key
//!   once per call ("9" lists her number, CON-035) and the answer is hang-up TwiML.
//!
//! No phone, key or body is logged; a failure logs the error variant only (PRIV-016).

use std::sync::Arc;

use askhava_adapters::api::{Delivery, VerifiedWebhook, WebhookVendor, refuse, verify_webhook};
use askhava_calls::api::{
    CallsError, SAFETY_KEY_PATH, SAFETY_STATUS_PATH, SafetyCall, hangup_twiml, safety_call_status,
    safety_key_once,
};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use crate::inbound_text::Webhooks;

/// Where Twilio posts a placed call's progress and detection result.
pub const STATUS_PATH: &str = SAFETY_STATUS_PATH;
/// Where the safety call's `Gather` posts the key pressed.
pub const KEY_PATH: &str = SAFETY_KEY_PATH;

/// The two routes over the shared Twilio webhook state, for `build_router` to merge (SEC-016).
pub fn routes(h: Arc<Webhooks>) -> Router {
    Router::new()
        .route(STATUS_PATH, post(status))
        .route(KEY_PATH, post(key))
        .with_state(h)
}

/// OPS-028, D-2771: one status or detection callback.
async fn status(
    State(h): State<Arc<Webhooks>>,
    uri: Uri,
    head: HeaderMap,
    body: Bytes,
) -> Response {
    let hook = match verified(&h, &uri, &head, &body) {
        Ok(hook) => hook,
        Err(code) => return code.into_response(),
    };
    let Some(call) = uri.query().and_then(SafetyCall::from_query) else {
        return StatusCode::OK.into_response();
    };
    let fields = (field(&hook, "CallStatus"), field(&hook, "AnsweredBy"));
    let Ok(mut db) = h.pool.acquire().await else {
        tracing::error!("twilio voice status database error");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match safety_call_status(&mut db, &call, fields, h.clock.now()).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "twilio voice status step failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// CALL-022, CON-035, D-2771: the safety call's key press; always answered with a hang-up.
async fn key(State(h): State<Arc<Webhooks>>, uri: Uri, head: HeaderMap, body: Bytes) -> Response {
    let hook = match verified(&h, &uri, &head, &body) {
        Ok(hook) => hook,
        Err(code) => return code.into_response(),
    };
    let call = uri.query().and_then(SafetyCall::from_query);
    if let (Some(call), Some(digits)) = (call, field(&hook, "Digits")) {
        let taken = match h.pool.acquire().await {
            Ok(mut db) => safety_key_once(&mut db, &h.vault, &call, digits, h.clock.now()).await,
            Err(_) => Err(CallsError::Database),
        };
        if let Err(e) = taken {
            tracing::error!(error = ?e, "twilio safety key step failed");
        }
    }
    ([(header::CONTENT_TYPE, "text/xml")], hangup_twiml()).into_response()
}

/// The value of the form field `name`, if sent.
fn field<'a>(hook: &'a VerifiedWebhook, name: &str) -> Option<&'a str> {
    let found = hook.params().iter().find(|(k, _)| k == name);
    found.map(|(_, value)| value.as_str())
}

/// The delivery checked against the URL Twilio signed: the public base, path and query (SEC-016).
fn verified(
    h: &Webhooks,
    uri: &Uri,
    head: &HeaderMap,
    body: &[u8],
) -> Result<VerifiedWebhook, StatusCode> {
    let tail = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
    let url = format!("{}{tail}", h.public_base);
    let pairs: Vec<(&str, &str)> = head
        .iter()
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.as_str(), v)))
        .collect();
    let delivery = Delivery {
        url: &url,
        body,
        headers: &pairs,
    };
    let now = h.clock.now().as_second();
    verify_webhook(WebhookVendor::Twilio, &h.auth_token, delivery, now).map_err(|e| {
        StatusCode::from_u16(refuse(WebhookVendor::Twilio, e)).unwrap_or(StatusCode::FORBIDDEN)
    })
}
