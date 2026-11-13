//! Twilio's status callbacks about the dialed leg of a masked call (CALL-007, CALL-008, SEC-016,
//! SEC-017, PRIV-016, FEE-022, TXT-030; D-1814, D-1836, D-3118).
//!
//! [`routes`] adds one POST route, [`STATUS_PATH`], the URL `calls::api::dial_twiml` names on the
//! dialed `<Number>` (`statusCallbackEvent="initiated completed"`). A delivery is taken only when
//! its `X-Twilio-Signature` passed `adapters::api::verify_webhook` over `ASKHAVA_PUBLIC_BASE_URL`
//! plus the path (`inbound_voice::verified`); any other gets 403 and changes nothing. [`routes`]
//! puts the route under `webhook_refusals`' layer itself, so each 403 is a capped signature event
//! and `build_router` keeps its four `refused(...)` calls (SEC-048, D-3525).
//!
//! The line is the one T262's voice webhook stored with the parent call's start
//! (`calls::api::call_start` by `ParentCallSid`); when that write was lost, `dialed_line` finds it
//! from the pool number dialed from and her number dialed (vault lookup only). Then, in one
//! transaction: every taken event records the dialed SID on that start
//! (`call_start_confirmed`, creating it at the callback's time when it was lost); a final event
//! also logs the call (`finished_call`, `log_finished_call`) as placed by the business, since the
//! voice route dials only for the line's business (D-2093), and, when it was newly logged, queues
//! one `picks.unreachable_step` `business_call` check for the job's paid pick of that business.
//! The start keeps its first dialed SID and the log one row per CallSid, so a repeated or
//! out-of-order callback changes nothing more (SEC-017). A database failure is 500 so Twilio
//! retries; anything else 200. No phone number or SID is logged (PRIV-016).

use std::sync::Arc;

use askhava_calls::api::{
    Caller, CallsError, call_start, call_start_confirmed, dialed_line, finished_call, line_parties,
    log_finished_call,
};
use askhava_core::api::enqueue;
use askhava_picks::api::{Step, accepted_pick, step_task};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::inbound_text::Webhooks;
use crate::inbound_voice::verified;
use crate::webhook_refusals::{self, RefusalLog};

/// Where Twilio posts the dialed leg's status callbacks: `calls`' `DIALED_STATUS_PATH`, written
/// out so the route-table scan reads it (TD-SEC-01).
pub const STATUS_PATH: &str = "/webhooks/twilio/voice/dialed-status";

/// The status-callback route over the shared Twilio webhook state, under the refusal layer that
/// records each signature refusal in `refusals` (SEC-016, SEC-048, D-3525).
pub fn routes(h: Arc<Webhooks>, refusals: RefusalLog) -> Router {
    let router = Router::new().route(STATUS_PATH, post(status)).with_state(h);
    webhook_refusals::layer(router, refusals)
}

/// Which callback a delivery is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    /// The dialed leg was placed.
    Initiated,
    /// The dialed leg ended: completed, busy, no answer, failed or canceled.
    Final,
}

impl Event {
    /// The event a `CallStatus` names; `None` for one this route does not act on.
    fn parse(word: &str) -> Option<Self> {
        match word {
            "queued" | "initiated" => Some(Self::Initiated),
            "completed" | "busy" | "no-answer" | "failed" | "canceled" => Some(Self::Final),
            _ => None,
        }
    }
}

/// Why a callback was not applied; logged by variant only (PRIV-016).
#[derive(Debug)]
enum Failed {
    /// A `calls` step or the database failed.
    Calls,
    /// Reading the pick failed.
    Picks,
    /// Queuing the check failed.
    Queue,
}

impl From<CallsError> for Failed {
    fn from(_: CallsError) -> Self {
        Self::Calls
    }
}

/// SEC-016, SEC-017: one dialed-leg status callback.
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
    match apply(&h, hook.params()).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "twilio dialed-leg status step failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Whether `sid` is a Twilio call SID: `CA` and 32 letters or digits.
fn is_call_sid(sid: &str) -> bool {
    sid.strip_prefix("CA")
        .is_some_and(|t| t.len() == 32 && t.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// CALL-008, FEE-022, SEC-017: records the dialed leg on its start and, for a final event, logs
/// the call and queues its check, all in one transaction; a callback naming no known line, no
/// valid SIDs or an event this route does not act on changes nothing.
async fn apply(h: &Webhooks, params: &[(String, String)]) -> Result<(), Failed> {
    let get = |key: &str| {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let (Some(sid), Some(parent)) = (get("CallSid"), get("ParentCallSid")) else {
        return Ok(());
    };
    let event = get("CallStatus").and_then(Event::parse);
    let Some(event) = event.filter(|_| is_call_sid(sid) && is_call_sid(parent)) else {
        return Ok(());
    };
    let mut db = h.pool.acquire().await.map_err(|_| CallsError::Database)?;
    let ends = (get("From"), get("To"));
    let Some(line) = line_of(&mut db, h, parent, ends).await? else {
        tracing::warn!("twilio dialed-leg status names no known line; nothing was logged");
        return Ok(());
    };
    let now = h.clock.now();
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    call_start_confirmed(&mut tx, line, parent, sid, now).await?;
    if event == Event::Final {
        log_and_check(&mut tx, line, params, now).await?;
    }
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(())
}

/// The line of the call with parent CallSid `parent`: the one its stored start names, else, when
/// that write was lost, the one `dialed_line` finds from the leg's `From` (the pool number) and
/// `To` (her number, used for the vault lookup only) (D-1836).
async fn line_of(
    db: &mut PgConnection,
    h: &Webhooks,
    parent: &str,
    (from, to): (Option<&str>, Option<&str>),
) -> Result<Option<Uuid>, CallsError> {
    if let Some((line, _)) = call_start(db, parent).await? {
        return Ok(Some(line));
    }
    let (Some(from), Some(to)) = (from, to) else {
        return Ok(None);
    };
    dialed_line(db, &h.vault, from, &SecretString::from(to.to_owned())).await
}

/// CALL-007, CALL-008, TXT-030, D-1814: logs the finished call once per CallSid as the business's
/// and, only when it was newly logged, queues one `business_call` check for the job's paid or free
/// pick of the line's business.
async fn log_and_check(
    tx: &mut PgConnection,
    line: Uuid,
    params: &[(String, String)],
    now: Timestamp,
) -> Result<(), Failed> {
    let Some(call) = finished_call(line, Caller::Business, params, now) else {
        return Ok(());
    };
    if !log_finished_call(tx, &call).await? {
        return Ok(());
    }
    let Some((job, business)) = line_parties(tx, line).await? else {
        return Ok(());
    };
    let pick = accepted_pick(tx, job).await.map_err(|_| Failed::Picks)?;
    let Some(pick) = pick.filter(|p| p.business == business) else {
        return Ok(());
    };
    let task = step_task(pick.pick, Step::BusinessCall, now);
    enqueue(tx, &task).await.map_err(|_| Failed::Queue)?;
    Ok(())
}
