//! Twilio's inbound voice webhook for the masked pool numbers (CALL-002 to CALL-005, CALL-025,
//! SEC-016, SEC-017, FEE-022; D-1708, D-1755, D-1836, D-2091 to D-2095).
//!
//! [`routes`] adds one POST route, [`VOICE_PATH`], the voice URL of every pool number. A delivery
//! is taken only when its `X-Twilio-Signature` passed `adapters::api::verify_webhook` over
//! `ASKHAVA_PUBLIC_BASE_URL` plus the path and query; any other gets 403 and one log line. The
//! query says which turn of the call it is: none (the call just came in), `try=N` (the digits of
//! the call's Nth PIN try) or `confirm=N` (the key pressed after the read-back given at try N);
//! Twilio signs the query, so a caller cannot change it. Each turn runs `calls::api::voice_turn`
//! or `voice_confirm`, which find the party, the open lines with their stored PINs, the stored
//! counts and the attestation, count a wrong try once per (CallSid, try) and store the call's
//! start once per CallSid when they answer with the dial. The reply is `calls::api` TwiML: the
//! PIN prompt, the read-back (A attestation only), the dial or a hang-up. It answers only the
//! calling leg. No line ID, phone or PIN is logged or put in a URL (PRIV-016): the confirm URL
//! carries only the try number, and `voice_confirm` connects only the line stored with that try
//! when it was read back, while still open and still the caller's business's (CALL-005, D-2201).
//! T390 (CALL-011, D-3079, D-3169): after a turn answered with the dial, `calls::api::link_after_dial`
//! queues the LINK notice and stores one pending link when the caller was an unlinked phone; its
//! failure is one warn line naming the error variant only and never changes the dial.

use std::sync::Arc;

use askhava_adapters::api::{
    Delivery, MaskedPool, VerifiedWebhook, WebhookVendor, refuse, verify_webhook,
};
use askhava_calls::api::{
    Attestation, CallsError, IncomingCall, JobPin, PinWait, TRIES_PER_CALL, VoiceCall, VoiceKeys,
    VoiceReply, hangup_twiml, link_after_dial, pin_prompt_twiml, readback_twiml, voice_confirm,
    voice_turn,
};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use secrecy::{ExposeSecret, SecretString};

use crate::inbound_text::Webhooks;

/// Where Twilio posts a call to a pool number and each of its turns (the numbers' voice URL).
pub const VOICE_PATH: &str = "/webhooks/twilio/voice";
/// The `ID_SECRET` version the calling-number digest is keyed by (PRIV-024).
const ID_SECRET_VERSION: u16 = 1;

/// What the voice route needs: the shared Twilio webhook state, the masked pool and the PIN wait.
pub struct VoiceHooks {
    /// Pool, clock, public base URL, auth token, `ID_SECRET` and vault (`inbound_text`).
    pub hooks: Arc<Webhooks>,
    /// The pool numbers this route answers for (CALL-017, CALL-021).
    pub pool: MaskedPool,
    /// How long each prompt waits for digits (CALL-013).
    pub wait: PinWait,
}

/// The voice route over `v`, for `build_router` to merge (SEC-016).
pub fn routes(v: Arc<VoiceHooks>) -> Router {
    Router::new().route(VOICE_PATH, post(voice)).with_state(v)
}

/// Which turn of a call a delivery is, from the signed query.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Turn {
    First,
    Try(u8),
    Confirm(u8),
}

impl Turn {
    /// `None` for any query this route never sets.
    fn parse(query: Option<&str>) -> Option<Self> {
        let Some(query) = query.filter(|q| !q.is_empty()) else {
            return Some(Self::First);
        };
        let (name, n) = query.split_once('=')?;
        let n = n
            .parse::<u8>()
            .ok()
            .filter(|n| (1..=TRIES_PER_CALL).contains(n))?;
        match name {
            "try" => Some(Self::Try(n)),
            "confirm" => Some(Self::Confirm(n)),
            _ => None,
        }
    }

    /// The try number this turn is about (0 before any digits).
    fn number(self) -> u8 {
        match self {
            Self::First => 0,
            Self::Try(n) | Self::Confirm(n) => n,
        }
    }
}

async fn voice(
    State(v): State<Arc<VoiceHooks>>,
    uri: Uri,
    head: HeaderMap,
    body: Bytes,
) -> Response {
    let hook = match verified(&v.hooks, &uri, &head, &body) {
        Ok(hook) => hook,
        Err(code) => return code.into_response(),
    };
    let Some(turn) = Turn::parse(uri.query()) else {
        return twiml(hangup_twiml());
    };
    match answer(&v, &hook, turn).await {
        Ok(xml) => twiml(xml),
        Err(CallsError::Database) => {
            tracing::error!("twilio voice webhook database error");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::warn!(error = ?e, "twilio voice webhook step failed; the call was ended");
            twiml(hangup_twiml())
        }
    }
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

/// The carrier's attestation from Twilio's `StirVerstat` (`TN-Validation-Passed-A` is A).
fn attestation(verstat: Option<&str>) -> Attestation {
    let a = verstat.is_some_and(|s| s.starts_with("TN-Validation-Passed-") && s.ends_with("-A"));
    Attestation::parse(a.then_some("A"))
}

/// The TwiML for one checked turn (CALL-002 to CALL-005, CALL-025).
async fn answer(v: &VoiceHooks, hook: &VerifiedWebhook, turn: Turn) -> Result<String, CallsError> {
    let field = |name: &str| {
        let found = hook.params().iter().find(|(k, _)| k == name);
        found.map(|(_, value)| value.as_str())
    };
    let (Some(sid), Some(to), Some(from)) = (field("CallSid"), field("To"), field("From")) else {
        return Ok(hangup_twiml());
    };
    if !v.pool.numbers().iter().any(|n| n.as_str() == to) {
        return Ok(hangup_twiml());
    }
    let caller = SecretString::from(from.to_owned());
    let call = VoiceCall {
        call: IncomingCall {
            pool_number: to,
            caller: &caller,
        },
        call_sid: sid,
        attestation: attestation(field("StirVerstat")),
        try_no: turn.number(),
        entered: matches!(turn, Turn::Try(_))
            .then(|| field("Digits").and_then(JobPin::parse))
            .flatten(),
    };
    let keys = VoiceKeys {
        id_secret: &v.hooks.id_secret,
        version: ID_SECRET_VERSION,
        pool: v.pool.numbers(),
    };
    let mut db = v
        .hooks
        .pool
        .acquire()
        .await
        .map_err(|_| CallsError::Database)?;
    let (vault, now) = (&v.hooks.vault, v.hooks.clock.now());
    let reply = match turn {
        Turn::Confirm(_) => {
            let pressed = field("Digits") == Some("1");
            voice_confirm(&mut db, vault, &keys, &call, (pressed, now)).await?
        }
        Turn::First | Turn::Try(_) => voice_turn(&mut db, vault, &keys, &call, now).await?,
    };
    if matches!(reply, VoiceReply::Dial(_)) {
        after_dial(&mut db, vault, (sid, &caller), now).await;
    }
    Ok(render(v.wait, turn, reply))
}

/// CALL-011, D-3079: the LINK step after a dial; a failure is one log line naming the error variant
/// only (no number, line or PIN) and the dial still answers.
async fn after_dial(
    db: &mut sqlx::PgConnection,
    vault: &askhava_privacy::api::Vault,
    (sid, caller): (&str, &SecretString),
    now: jiff::Timestamp,
) {
    if let Err(e) = link_after_dial(db, vault, sid, caller, now).await {
        tracing::warn!(error = ?e, "twilio voice link step failed; the call was dialed");
    }
}

/// The reply's TwiML; a prompt posts the next try, a read-back its confirm.
fn render(wait: PinWait, turn: Turn, reply: VoiceReply) -> String {
    let n = turn.number();
    let next = n.saturating_add(1);
    let prompt = || {
        if next > TRIES_PER_CALL {
            return hangup_twiml();
        }
        pin_prompt_twiml(wait, &format!("{VOICE_PATH}?try={next}"))
    };
    match reply {
        VoiceReply::Dial(xml) => xml.expose_secret().to_owned(),
        VoiceReply::AskPin => prompt(),
        VoiceReply::ReadBack(words) => {
            let action = format!("{VOICE_PATH}?confirm={n}");
            readback_twiml(&words, wait, &action)
                .map_or_else(prompt, |x| x.expose_secret().to_owned())
        }
        VoiceReply::HangUp => hangup_twiml(),
    }
}

/// A `text/xml` 200 with `body`.
fn twiml(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/xml")], body).into_response()
}
