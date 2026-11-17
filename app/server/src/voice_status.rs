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
//!   once per call ("9" lists her number, CON-035) and the answer is hang-up TwiML. When that step
//!   fails, `calls::api::queue_safety_key_retry` queues it (still hang-up TwiML). Twilio retries no
//!   voice action on a non-2xx answer (it speaks an error or fetches the fallback URL; the stand-in
//!   models no `Gather` or `Redirect`, D-2763), so when the queue fails too the answer is 200 with
//!   a short `<Pause>` and a POST `<Redirect>` back here carrying `Digits` and `attempt` in the
//!   query (the redirect posts no `Digits` field); both are stripped before
//!   `SafetyCall::from_query`. After [`KEY_ATTEMPTS`] redirects it says [`STOP_LINE`], hangs up and
//!   raises `calls::api::SAFETY_KEY_LOST_ALERT` if it can (D-4051, D-4088).
//!
//! No phone, key or body is logged; a failure logs the error variant only (PRIV-016).

use std::sync::Arc;

use askhava_adapters::api::VerifiedWebhook;
use askhava_calls::api::{
    SafetyCall, hangup_twiml, queue_safety_key_retry, safety_call_status, safety_key_lost,
    safety_key_once,
};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use crate::inbound_text::{Webhooks, verified};

/// Where Twilio posts a placed call's progress and detection result: `calls`'
/// `SAFETY_STATUS_PATH`, written out so the route-table scan reads it (TD-SEC-01).
pub const STATUS_PATH: &str = "/webhooks/twilio/voice/status";
/// Where the safety call's `Gather` posts the key pressed: `calls`' `SAFETY_KEY_PATH`.
pub const KEY_PATH: &str = "/webhooks/twilio/voice/safety-key";
/// The most redirects back to [`KEY_PATH`] after the key step and its queue both failed.
pub const KEY_ATTEMPTS: u32 = 3;
/// Said after the last redirect, before the hang-up (D-4051).
pub const STOP_LINE: &str = "To stop texts, reply STOP to any message from us.";

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

/// CALL-022, CON-035, D-2771, D-4051: the safety call's key press; hang-up TwiML once the key was
/// taken or queued, else a bounded redirect back here, then the STOP line.
async fn key(State(h): State<Arc<Webhooks>>, uri: Uri, head: HeaderMap, body: Bytes) -> Response {
    let hook = match verified(&h, &uri, &head, &body) {
        Ok(hook) => hook,
        Err(code) => return code.into_response(),
    };
    let (query, attempt, carried) = key_query(uri.query().unwrap_or_default());
    let call = SafetyCall::from_query(&query);
    let digits = field(&hook, "Digits").or(carried);
    let (Some(call), Some(digits)) = (call, digits) else {
        return twiml(hangup_twiml());
    };
    if take_key(&h, &call, digits).await || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return twiml(hangup_twiml());
    }
    if attempt < KEY_ATTEMPTS {
        let next = attempt + 1;
        let query = query.replace('&', "&amp;");
        let url = format!(
            "{}{KEY_PATH}?{query}&amp;Digits={digits}&amp;attempt={next}",
            h.public_base
        );
        return twiml(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Pause length=\"2\"/>\
             <Redirect method=\"POST\">{url}</Redirect></Response>"
        ));
    }
    if let Ok(mut db) = h.pool.acquire().await
        && let Err(e) = safety_key_lost(&mut db, &call).await
    {
        tracing::error!(error = ?e, "twilio safety key alert failed");
    }
    twiml(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Say>{STOP_LINE}</Say><Hangup/>\
         </Response>"
    ))
}

/// CON-035, D-4051: takes the key once, or queues it when the step fails; false when neither held.
async fn take_key(h: &Webhooks, call: &SafetyCall, digits: &str) -> bool {
    let now = h.clock.now();
    let Ok(mut db) = h.pool.acquire().await else {
        tracing::error!("twilio safety key database error");
        return false;
    };
    let Err(e) = safety_key_once(&mut db, &h.vault, call, digits, now).await else {
        return true;
    };
    tracing::error!(error = ?e, "twilio safety key step failed");
    match queue_safety_key_retry(&mut db, call, digits, now).await {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(error = ?e, "twilio safety key queue failed");
            false
        }
    }
}

/// `query` without a redirect's `attempt` and `Digits`, the attempt (0 when absent, the bound
/// when unreadable) and the `Digits` it carried.
fn key_query(query: &str) -> (String, u32, Option<&str>) {
    let (mut kept, mut attempt, mut digits) = (Vec::new(), 0, None);
    for part in query.split('&') {
        if let Some(n) = part.strip_prefix("attempt=") {
            attempt = n.parse().unwrap_or(KEY_ATTEMPTS);
        } else if let Some(d) = part.strip_prefix("Digits=") {
            digits = Some(d);
        } else {
            kept.push(part);
        }
    }
    (kept.join("&"), attempt, digits)
}

/// `body` as a `text/xml` answer.
fn twiml(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/xml")], body).into_response()
}

/// The value of the form field `name`, if sent.
fn field<'a>(hook: &'a VerifiedWebhook, name: &str) -> Option<&'a str> {
    let found = hook.params().iter().find(|(k, _)| k == name);
    found.map(|(_, value)| value.as_str())
}
