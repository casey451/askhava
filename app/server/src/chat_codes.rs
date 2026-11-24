//! The web chat's code step and its request checks (T489; CON-025, SEC-042, SEC-048; D-963,
//! D-3144, D-3744, D-3882, D-4044).
//!
//! [`ChatCodes`] implements T103's `intake::api::CodeStep` over `adapters`' Twilio Verify client,
//! [`ChatParts`] implements `site::api::ChatKeys` with T103's `send_code` and `check_code` over the
//! vault and [`ChatCodes`] (D-963), and after the code scrubs the chat's words with her vault
//! name and stated names (`privacy::api::read_inbound_for`, D-2088's error rules) and hands her
//! raw words and the vault to `intake::api::chat_raw_message` (T649, INT-027, AI-003, D-4294);
//! [`chat_keys`] builds it from the settings for `main` to hand to `RouterParts::with_chat_codes` (none in dev and `Ci`, so no code goes), and [`scrubbed`] is
//! the chat's scrubber (`privacy::api::scrub`, which `site` may not call). [`guard`] puts the
//! request checks on `site`'s chat router, since `site` may not call `portals` (D-3882): every
//! response `no-store`; a cross-site `POST` is refused by `portals::api::refuse_cross_site`; a
//! same-site `POST` whose `csrf` field is not the cookie's token is a 403 and one `csrf_rejected`
//! security event. Only a `POST` is checked, so other methods reach the router as they are. Every
//! request gets a pre-session cookie (`ensure_pre_session`) and the page its token as
//! `site::api::ChatCsrf`; a passing `POST` gets the client IP `security_event_client` reads as
//! `site::api::ChatClientIp` for intake's new-request limit (SEC-014, SEC-040, D-4292). The typed number is never logged, stored here or put in an error.
//! [`home_guard`] gives the three home paths the same cookie and token for the home request box,
//! which posts to `/request`, and marks them `no-store` (T593, D-4098).

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use crate::inbound_text::answer_words;
use askhava_adapters::api::{
    CodeChannel, IdempotencyKey, TwilioConfig, TwilioVerify, VerifyAdapter, twilio_clients,
};
use askhava_businesses::api::public_names;
use askhava_core::api::{IdSecret, Settings, SystemClock};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_intake::api::{
    ChatAnswer, ChatError, ChatIn, ChatSettings, Checked, CodeStep, IntakeError, RawReply,
    chat_contact, chat_raw_message, check_code, list_ask, name_address_open, open_request_jobs,
    reads_as_address, send_code,
};
use askhava_portals::api::{
    CookieJar, SecurityEvent, SecurityEventKind, WebSecurity, admit_security_event, csrf_matches,
    csrf_token, ensure_pre_session, no_store, record_security_event, refuse_cross_site,
    security_event_client,
};
use askhava_privacy::api::{
    CommonWords, InboundWords, Known, NameMemory, Vault, VaultError, read_inbound_for, scrub,
};
use askhava_quotes::api::quoted_prices;
use askhava_site::api::{
    ChatClientIp, ChatCsrf, ChatKeys, ChatRead, HOME_PATH, OLD_CATEGORIES_PATH, OLD_EVENTS_PATH,
    REQUEST_PATH,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{FromRequest, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::PgConnection;
use sqlx::types::Uuid;
use sqlx::{Connection, PgPool};

/// The largest chat form read (the chat lines carried forward included).
const FORM_LIMIT: usize = 64 * 1024;

/// D-3144, D-4044: T103's code step over Twilio Verify; the idempotency key is derived from the
/// session's key with `ASKHAVA_ID_SECRET` (SEC-018). No `Debug` (R6).
pub struct ChatCodes {
    verify: TwilioVerify,
    secret: SecretString,
}

impl CodeStep for ChatCodes {
    async fn send(&self, number: &SecretString, key: &str) -> Result<(), ChatError> {
        let key = IdempotencyKey::derive(&self.secret, "verify.web_chat", &[key]);
        let to = number.expose_secret();
        let sent = self.verify.send_code(&key, to, CodeChannel::Sms).await;
        sent.map(|_| ()).map_err(|_| ChatError::CodeStep)
    }

    async fn check(&self, number: &SecretString, code: &str) -> Result<bool, ChatError> {
        let checked = self.verify.check_code(number.expose_secret(), code).await;
        checked.map_err(|_| ChatError::CodeStep)
    }
}

/// D-963, D-3144: the chat's code step: T103's calls over the vault and [`ChatCodes`], with
/// `ASKHAVA_ID_SECRET`. No `Debug` (R6).
pub struct ChatParts {
    code: ChatCodes,
    vault: Vault,
    secret: SecretString,
}

impl ChatKeys for ChatParts {
    fn secret(&self) -> &SecretString {
        &self.secret
    }

    async fn send(
        &self,
        db: &mut PgConnection,
        key: &SecretString,
        number: &SecretString,
        now: Timestamp,
    ) -> Result<bool, ChatError> {
        send_code(db, (&self.vault, &self.code), key, number, now).await
    }

    async fn check(
        &self,
        db: &mut PgConnection,
        key: &SecretString,
        typed: (&SecretString, &str),
        at: (ChatSettings, Timestamp),
    ) -> Result<Checked, ChatError> {
        let parts = (&self.secret, &self.vault, &self.code);
        check_code(db, parts, key, typed, at).await
    }

    /// AI-003, AI-020, INT-027, D-3705, D-4294: as the text route reads, her answer to the open
    /// name-and-address question that reads as an address is only `[removed]` from here on.
    async fn read(
        &self,
        db: &mut PgConnection,
        key: &SecretString,
        raw: &SecretString,
    ) -> Result<ChatRead, ChatError> {
        let Some(contact) = chat_contact(db, key).await? else {
            return Ok(plain(raw, true));
        };
        let mut read = read_for(db, &self.vault, contact, raw).await?;
        let raw = raw.expose_secret();
        if reads_as_address(raw) && name_address_open(db, contact).await? {
            read.words = answer_words(&read.words, true).to_owned();
        }
        Ok(read)
    }

    async fn turn(
        &self,
        db: &mut PgConnection,
        key: &SecretString,
        (msg, raw): (&ChatIn<'_>, &SecretString),
        intake: IntakeSettings,
    ) -> Result<ChatAnswer, ChatError> {
        let names = list_names(db, key, msg.words).await?;
        let raw = RawReply {
            raw,
            vault: &self.vault,
        };
        let said = (msg, (raw, names.as_slice()));
        chat_raw_message(db, (&self.secret, key), said, intake).await
    }
}

/// AI-003, AI-020, PRIV-026, D-2088, D-4294: `raw` read for vault `contact` as the SMS path's
/// `read` does, in its own transaction: her vault name and stated names removed, a newly stated
/// name sealed. A vault database or audit error is [`ChatError::Vault`] (nothing kept); any
/// other vault error falls back to the empty-context scrub, whose turn stops before intake.
async fn read_for(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    raw: &SecretString,
) -> Result<ChatRead, ChatError> {
    let words = InboundWords {
        emergency: &[],
        opt_out: &[],
        help: &[],
        start: &[],
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    match read_inbound_for(&mut tx, vault, contact, raw.expose_secret(), &words).await {
        Ok(read) => {
            tx.commit().await.map_err(|_| IntakeError::Database)?;
            let words = read.text.as_str().to_owned();
            Ok(ChatRead { words, go_on: true })
        }
        Err(VaultError::Database | VaultError::Audit) => Err(ChatError::Vault),
        Err(_) => Ok(plain(raw, false)),
    }
}

/// `raw` with the empty-context scrub ([`scrubbed`]), the turn going on when `go_on`.
fn plain(raw: &SecretString, go_on: bool) -> ChatRead {
    let words = scrubbed(raw.expose_secret());
    ChatRead { words, go_on }
}

/// TXT-031, D-1310, D-4294: for a LIST message after the code, the public names of every
/// business that quoted a job of her open request, as the text route reads them; else none.
async fn list_names(
    db: &mut PgConnection,
    key: &SecretString,
    words: &str,
) -> Result<Vec<(Uuid, String)>, ChatError> {
    if list_ask(words).is_none() {
        return Ok(Vec::new());
    }
    let Some(contact) = chat_contact(db, key).await? else {
        return Ok(Vec::new());
    };
    let jobs = open_request_jobs(db, contact).await?.unwrap_or_default();
    let mut quoted: Vec<Uuid> = Vec::new();
    for job in jobs {
        let prices = quoted_prices(db, job)
            .await
            .map_err(|_| IntakeError::Database)?;
        quoted.extend(prices.iter().map(|p| p.business));
    }
    quoted.sort_unstable();
    quoted.dedup();
    let names = public_names(db, &quoted).await;
    Ok(names.map_err(|_| IntakeError::Database)?)
}

/// AI-002, PRIV-001: `raw` with names, numbers, emails, links and street numbers removed (no
/// vault name: the chat before its code, or with no keys; after it [`ChatParts`] reads with her
/// vault name, D-4294).
#[must_use]
pub fn scrubbed(raw: &str) -> String {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(raw, &known, &mut NameMemory::default())
        .as_str()
        .to_owned()
}

/// D-3144, D-3744: the chat's code-step keys from `settings` and `pairs`: the vault, the Verify
/// client and `ASKHAVA_ID_SECRET`; `None` where the vault is not configured (dev and `Ci`), as
/// [`crate::pick_code_keys`] does.
///
/// # Errors
///
/// A missing or bad key, named, never a value.
pub fn chat_keys<I>(settings: &Settings, pairs: I) -> io::Result<Option<ChatParts>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let Some(vault) = crate::vault_from(settings, pairs.clone())? else {
        return Ok(None);
    };
    let secret = pairs
        .iter()
        .find(|(k, v)| k == "ASKHAVA_ID_SECRET" && !v.trim().is_empty())
        .map(|(_, v)| SecretString::from(v.trim().to_owned()))
        .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
    IdSecret::new(1, secret.clone()).map_err(io::Error::other)?;
    let twilio = TwilioConfig::from_settings(settings, pairs).map_err(io::Error::other)?;
    let (_, _, verify) = twilio_clients(&twilio).map_err(io::Error::other)?;
    let code = ChatCodes {
        verify,
        secret: secret.clone(),
    };
    Ok(Some(ChatParts {
        code,
        vault,
        secret,
    }))
}

/// SEC-042, SEC-048, SEC-034, D-3882: `chat` behind the token check, `no-store` and the
/// cross-site refusal (outermost), recording refusals over `pool`.
pub fn guard(chat: Router, pool: PgPool) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::new(SystemClock));
    chat.layer(from_fn_with_state(pool, check_token))
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// SEC-042, SEC-048: a `POST` whose `csrf` field is not the cookie's token is a 403 with one
/// `csrf_rejected` event; every request passes on with a pre-session cookie and its token, and a
/// passing `POST` with its client IP as `site::api::ChatClientIp` (SEC-014, SEC-040, D-4292),
/// unless that IP is unspecified (no peer and no edge header), so such posts share no bucket.
async fn check_token(State(pool): State<PgPool>, request: Request, next: Next) -> Response {
    let jar = CookieJar::from_headers(request.headers());
    let mut request = request;
    if request.method() == Method::POST {
        let ip = security_event_client(&request);
        let (parts, body) = request.into_parts();
        let Ok(bytes) = to_bytes(body, FORM_LIMIT).await else {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        };
        if !csrf_matches(&jar, &posted_csrf(bytes.clone()).await) {
            return refuse(&pool, ip, &parts.headers).await;
        }
        request = Request::from_parts(parts, Body::from(bytes));
        if !ip.is_unspecified() {
            request.extensions_mut().insert(ChatClientIp(ip));
        }
    }
    with_token(jar, request, next).await
}

/// SEC-042: `request` passed on with a pre-session cookie (set when `jar` has none) and its token
/// as `site::api::ChatCsrf`.
async fn with_token(jar: CookieJar, mut request: Request, next: Next) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        tracing::error!("web-chat pre-session cookie failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let token = csrf_token(&jar).unwrap_or_default();
    request.extensions_mut().insert(ChatCsrf(token));
    (jar, next.run(request).await).into_response()
}

/// SEC-042, WEB-009, WEB-012, T593, D-4098: `pages` with [`guard`]'s pre-session cookie and token
/// on a `GET` of the three home paths (`/`, `/categories`, `/events`), whose request box posts to
/// `/request`; each such answer is `no-store`, since it holds a per-visitor token. Every other
/// path and method passes as it is.
pub fn home_guard(pages: Router) -> Router {
    pages.layer(from_fn(home_token))
}

/// [`home_guard`]'s layer.
async fn home_token(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let home = [HOME_PATH, OLD_CATEGORIES_PATH, OLD_EVENTS_PATH].contains(&path);
    if !home || !matches!(*request.method(), Method::GET | Method::HEAD) {
        return next.run(request).await;
    }
    let jar = CookieJar::from_headers(request.headers());
    let mut response = with_token(jar, request, next).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

/// The posted form's `csrf` field, empty when absent or when the body is no form. The rebuilt
/// request is a `POST`: axum's `Form` reads a `GET`'s fields from the query string (SEC-042, T595).
async fn posted_csrf(bytes: axum::body::Bytes) -> String {
    let form = Request::builder()
        .method(Method::POST)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(bytes));
    let Ok(form) = form else {
        return String::new();
    };
    match axum::Form::<Vec<(String, String)>>::from_request(form, &()).await {
        Ok(axum::Form(fields)) => fields
            .into_iter()
            .find(|(k, _)| k == "csrf")
            .map(|(_, v)| v)
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// SEC-048: the 403 for a wrong-token `POST`, with its security event (request ID and route, no
/// actor: there is no session on this page) while the per-minute cap admits it.
async fn refuse(pool: &PgPool, ip: IpAddr, headers: &HeaderMap) -> Response {
    let (kind, now) = (SecurityEventKind::CsrfRejected, Timestamp::now());
    if admit_security_event(kind, ip, REQUEST_PATH, now) {
        let request_id = headers
            .get("x-request-id")
            .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
        let event = SecurityEvent {
            kind,
            route: REQUEST_PATH,
            request_id,
            actor_ref: None,
        };
        let stored = match pool.acquire().await {
            Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
            Err(_) => false,
        };
        if !stored {
            tracing::warn!("web-chat security event not stored");
        }
    }
    StatusCode::FORBIDDEN.into_response()
}
