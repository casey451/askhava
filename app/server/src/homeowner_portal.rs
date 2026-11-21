//! The served homeowner portal: sign-in by phone and texted code, her home page and the texts-off
//! switch (SEC-008, SEC-014, SEC-034, SEC-042, SEC-046, SEC-048, PRIV-021, CON-035; T307, D-2128,
//! D-3877, D-3967, D-4025).
//!
//! [`routes`] serves `portals`' homeowner portal (T087): `GET` [`HOMEOWNER_SIGNIN_ROUTE`] (the
//! phone form, with a pre-session cookie for its CSRF token), `POST` [`HOMEOWNER_SIGNIN_ROUTE`]
//! (send a code), `POST` [`HOMEOWNER_CODE_ROUTE`] (check it and start her 30-day session), `GET`
//! [`HOMEOWNER_HOME_ROUTE`] (her own record, `HomeFacts::read` with her session's proof) and `POST`
//! [`TEXTS_OFF_ROUTE`] (one suppression entry at once, confirmed on screen). Every answer is
//! `no-store`; every `POST` passes `refuse_cross_site`, then the CSRF form check (both refusals a
//! 403 and a `csrf_rejected` security event), then the realm, then its fields. No session, another
//! realm's or an ended one gets [`signed_out`], a 401 linking to `/h/sign-in` with no detail.
//! SEC-014: a send passes `admit_code_send` (the IP, then her contact when on file, so an unknown
//! number spends the IP limit and sends nothing) and a check `admit_code_try` first; a refusal
//! answers as always and sends or checks nothing. A number not on file gets the same code page.
//! The vault and ID secret are the Twilio webhooks'; the Verify client is the optional
//! [`HomeownerVerify`] part (`RouterParts::with_homeowner_verify`, from [`homeowner_verify`] in
//! `main`); without either a send sends nothing and a check calls nothing and answers the
//! wrong-code page, never a 5xx. `server` names no vault read: her number never leaves `portals`
//! except into the rendered page.
//!
//! The data routes (T561, D-3967, D-3877; PRIV-031, PRIV-011): `GET` [`DOWNLOAD_ROUTE`] answers
//! her "Download my data" file (`download_file` over [`held_file`]: her requests and consent
//! records through `intake::api`, her shown quotes through `quotes::api` with each business's
//! public name, and the words `messaging` still holds for her through `held_words`). `POST`
//! [`DATA_REQUEST_ROUTE`] opens a copy or delete request and texts its code, `POST`
//! [`DATA_RESEND_ROUTE`] texts it again, and `POST` [`DATA_CODE_ROUTE`] acts only on her open
//! request with the right code: a copy answers the same file, a delete the PRIV-011 sentence. Each
//! send passes `admit_code_send` (the IP, then her contact) and each check `admit_code_try`
//! first; a refused or impossible send answers [`not_sent`] and sends nothing.
//!
//! The photo routes (T674, D-4360, D-5082; INT-044, PRIV-014, PRIV-015, PRIV-025, PRIV-028,
//! CARD-013, SEC-019, A11Y-027): `GET` `PHOTO_UPLOAD_ROUTE` shows `portals`' upload page for her
//! own request (`{job}` is the job number; a request that is not hers, or no number, is the one
//! bare 404) and changes nothing. Its `POST` reads the CSRF token from either body first
//! ([`csrf_checked`]): a multipart body is an upload, taken by `privacy::api::take_upload` (format
//! and size checks, metadata stripped, re-encoded here, no outside AI) and shown through a
//! short-lived link by `upload_screen`; a form-encoded body is her answer about the newest
//! unconfirmed photo (`intake::api::press_photo`: `answer=yes` records the `photo_ok` row,
//! `answer=remove` deletes the photo). The body limit is `MAX_PHOTO_BYTES` plus [`FORM_SLACK`],
//! so a file just over the limit still gets the page's own error. A post with neither is the
//! page again as a 400; any post after her card was shown is [`card_out`], a 409. `GET`
//! [`PHOTO_LINK_ROUTE`] answers the opened JPEG of a live token and a bare 404 for any other,
//! with no session: the token in the path is the key (PRIV-028). Without [`PhotoParts`] an
//! upload fails and every photo link is the 404.

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, CodeChannel, IdempotencyKey, LocalStorage, TwilioConfig, VerifyAdapter,
    twilio_clients,
};
use askhava_businesses::api::public_names;
use askhava_core::api::{Clock, Environment, IdSecret, LinkToken, Settings, SystemClock};
use askhava_intake::api::{
    Admission, PhotoPress, card_went_out, consent_records, conversation, conversations_of_contact,
    jobs_of_contact, photo_answer, photo_job_is_hers, press_photo,
};
use askhava_jobs::api::job_by_number;
use askhava_messaging::api::held_words;
use askhava_portals::api::{
    Acted, ButtonForm, CARD_FALLBACK_NAME, CookieJar, CsrfField, CsrfForm, DATA_CODE_ROUTE,
    DATA_REQUEST_ROUTE, DOWNLOAD_ROUTE, DataRequestKind, HOMEOWNER_CODE_ROUTE,
    HOMEOWNER_HOME_ROUTE, HOMEOWNER_SIGNIN_ROUTE, HeldItem, HomeFacts, HomeownerError, OnFile,
    PHOTO_UPLOAD_ROUTE, PortalScreen, RequestError, Section, TEXTS_OFF_ROUTE, TOO_BIG_ERROR,
    UploadScreen, VERIFY_DAILY_THRESHOLD, WebSecurity, admit_code_send, admit_code_try,
    confirm_request, csrf_matches, csrf_token, current_session, download_file, ensure_pre_session,
    finish_sign_in, homeowner_page, no_store, open_request, photo_upload_page, refuse_cross_site,
    resend_request_code, security_event_client, send_sign_in_code, turn_texts_off, upload_screen,
};
use askhava_privacy::api::{
    DEFAULT_MIN_SHORT_SIDE, HomeownerSessionProof, MAX_PHOTO_BYTES, PhotoError, PhotoSeal,
    PhotoTaken, find_by_phone, issue_link, open_link, photo_path, take_upload,
};
use askhava_quotes::api::{page_quotes, shown_quotes};
use axum::Router;
use axum::body::Body;
use axum::extract::{
    DefaultBodyLimit, Extension, FromRef, FromRequest, Multipart, Path, Request, State,
};
use axum::http::StatusCode;
use axum::http::header::{
    CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, LOCATION,
};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::inbound_text::Webhooks;

/// A boxed Verify answer, so the part can be any [`VerifyAdapter`].
type Answer<'a, T> = Pin<Box<dyn Future<Output = Result<T, AdapterError>> + Send + 'a>>;

/// [`VerifyAdapter`] behind a box (its methods return `impl Future`, so it has no `dyn` form).
trait BoxedVerify: Send + Sync {
    fn send<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        to: &'a str,
        by: CodeChannel,
    ) -> Answer<'a, String>;
    fn check<'a>(&'a self, to: &'a str, code: &'a str) -> Answer<'a, bool>;
}

impl<V: VerifyAdapter + Send + Sync> BoxedVerify for V {
    fn send<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        to: &'a str,
        by: CodeChannel,
    ) -> Answer<'a, String> {
        Box::pin(self.send_code(key, to, by))
    }

    fn check<'a>(&'a self, to: &'a str, code: &'a str) -> Answer<'a, bool> {
        Box::pin(self.check_code(to, code))
    }
}

/// The homeowner sign-in's Verify client (SEC-008): Twilio Verify in `main`, a fake in this
/// task's own tests.
pub struct HomeownerVerify(Box<dyn BoxedVerify>);

impl HomeownerVerify {
    /// Codes sent and checked through `verify`.
    #[must_use]
    pub fn new<V: VerifyAdapter + Send + Sync + 'static>(verify: V) -> Self {
        Self(Box::new(verify))
    }
}

impl VerifyAdapter for HomeownerVerify {
    async fn send_code(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.send(idempotency, to, channel).await
    }

    async fn check_code(&self, to: &str, code: &str) -> Result<bool, AdapterError> {
        self.0.check(to, code).await
    }
}

/// The Verify part `main` passes to `RouterParts::with_homeowner_verify`: the Twilio Verify
/// client from `pairs`; `None` in dev and CI without Twilio settings (SEC-008, D-3877).
///
/// # Errors
///
/// Bad Twilio settings in the test or live project. Never a value.
pub fn homeowner_verify<I>(settings: &Settings, pairs: I) -> io::Result<Option<HomeownerVerify>>
where
    I: IntoIterator<Item = (String, String)>,
{
    match TwilioConfig::from_settings(settings, pairs) {
        Ok(config) => {
            let (_, _, verify) = twilio_clients(&config).map_err(io::Error::other)?;
            Ok(Some(HomeownerVerify::new(verify)))
        }
        Err(error) => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other(error)),
        },
    }
}

/// The portal routes' state.
#[derive(Clone)]
struct Portal {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    hooks: Option<Arc<Webhooks>>,
    verify: Option<Arc<HomeownerVerify>>,
    photos: Option<Arc<PhotoParts>>,
}

impl FromRef<Portal> for WebSecurity {
    fn from_ref(state: &Portal) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the SEC-014 limits; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// The path of a data request's "Text me the code again" form (T561, D-3877 (1), D-2348).
pub const DATA_RESEND_ROUTE: &str = "/h/my-data/resend";

/// The portal and data routes over `pool`, with the webhooks' vault and ID secret and `verify`.
pub fn routes(
    pool: PgPool,
    hooks: Option<&Arc<Webhooks>>,
    verify: Option<HomeownerVerify>,
    photos: Option<PhotoParts>,
) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Portal {
        pool,
        clock,
        security: security.clone(),
        hooks: hooks.cloned(),
        verify: verify.map(Arc::new),
        photos: photos.map(Arc::new),
    };
    let upload = Router::new()
        .route(PHOTO_UPLOAD_ROUTE, get(photos_page).post(photos_post))
        .layer(DefaultBodyLimit::max(MAX_PHOTO_BYTES + FORM_SLACK));
    Router::new()
        .route(HOMEOWNER_SIGNIN_ROUTE, get(sign_in_page).post(send))
        .route(HOMEOWNER_CODE_ROUTE, post(check))
        .route(HOMEOWNER_HOME_ROUTE, get(home))
        .route(TEXTS_OFF_ROUTE, post(texts_off))
        .route(DOWNLOAD_ROUTE, get(download))
        .route(DATA_REQUEST_ROUTE, post(data_request))
        .route(DATA_CODE_ROUTE, post(data_code))
        .route(DATA_RESEND_ROUTE, post(data_resend))
        .merge(upload)
        .route(PHOTO_LINK_ROUTE, get(photo))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// Notes the client address (SEC-040) for the limits.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "homeowner portal page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-008, SEC-034: the answer to a request with no homeowner session: a 401 linking to
/// `/h/sign-in`, with no detail.
fn signed_out() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Sign in</title></head><body><main>\
         <h1>Sign in</h1><p><a href=\"{HOMEOWNER_SIGNIN_ROUTE}\">Sign in</a> to see this page.\
         </p></main></body></html>"
    );
    (StatusCode::UNAUTHORIZED, Html(page)).into_response()
}

/// `screen` with the jar's CSRF token, answered with `status`.
fn shown(jar: &CookieJar, screen: &PortalScreen, status: StatusCode) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    match homeowner_page(screen, &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("homeowner page"),
    }
}

/// The code page for the number as typed; `failed` after a wrong or refused code.
fn code_page(jar: &CookieJar, phone: &str, failed: bool) -> Response {
    let phone = SecretString::from(phone.to_owned());
    let status = if failed {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    shown(jar, &PortalScreen::Code { phone, failed }, status)
}

/// `GET` [`HOMEOWNER_SIGNIN_ROUTE`] (SEC-008, SEC-042): the phone form, with a pre-session cookie
/// for its CSRF token. Changes nothing stored.
async fn sign_in_page(jar: CookieJar) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return failed("pre-session");
    };
    let screen = PortalScreen::SignIn {
        phone: SecretString::from(String::new()),
        failed: false,
    };
    let page = shown(&jar, &screen, StatusCode::OK);
    (jar, page).into_response()
}

/// The sign-in forms: the CSRF token, the typed number and, on the code step, the code. A missing
/// field reads as empty, so the CSRF check is reached first; any other field fails the parse.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    code: String,
}

impl CsrfField for SignInForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// Whether `admitted` let the request through; an error refuses (D-1951).
fn passed<E>(admitted: Result<Admission, E>) -> bool {
    matches!(admitted, Ok(Admission::Admitted))
}

/// The contact holding `phone`, `None` for none, no vault or a number that is not a US number;
/// a failed lookup is logged once, without the number, and read as none.
async fn on_file(state: &Portal, phone: &str) -> Option<Uuid> {
    let hooks = state.hooks.as_deref()?;
    let mut db = state.pool.acquire().await.ok()?;
    match find_by_phone(&mut db, &hooks.vault, phone).await {
        Ok(contact) => contact,
        Err(error) => {
            tracing::debug!(%error, "homeowner sign-in lookup found no contact");
            None
        }
    }
}

/// `POST` [`HOMEOWNER_SIGNIN_ROUTE`] (SEC-008, SEC-014): after the per-IP and per-number send
/// limits, one code to a number on file; every admitted or refused number gets the same code page.
async fn send(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<SignInForm>,
) -> Response {
    if askhava_privacy::api::normalize_phone(&form.phone).is_err() {
        let phone = SecretString::from(form.phone);
        let screen = PortalScreen::SignIn {
            phone,
            failed: true,
        };
        return shown(&jar, &screen, StatusCode::BAD_REQUEST);
    }
    let now = state.clock.now();
    let contact = on_file(&state, &form.phone).await;
    let threshold = i32::try_from(VERIFY_DAILY_THRESHOLD).unwrap_or(i32::MAX);
    let limit = admit_code_send(&state.pool, (ip, contact), (now, threshold)).await;
    let parts = state.hooks.clone().zip(state.verify.clone());
    if let (true, Some(parts)) = (passed(limit), parts) {
        tokio::spawn(send_later(
            state.pool.clone(),
            parts,
            form.phone.clone(),
            now,
        ));
    }
    code_page(&jar, &form.phone, false)
}

/// TD-PICK-36, D-1954, D-4013: sends the sign-in code off the request, so the code page never
/// waits on the challenge write or Verify and its timing does not show whether a number is on
/// file; a failure is logged once, without the number.
async fn send_later(
    pool: PgPool,
    (hooks, verify): (Arc<Webhooks>, Arc<HomeownerVerify>),
    phone: String,
    now: Timestamp,
) {
    let keys = (&hooks.vault, verify.as_ref(), &hooks.id_secret);
    if let Err(error) = send_sign_in_code(&pool, keys, &phone, now).await {
        tracing::warn!(%error, "homeowner sign-in code not sent");
    }
}

/// `POST` [`HOMEOWNER_CODE_ROUTE`] (SEC-008, SEC-014, SEC-020): after the per-IP try limit, checks
/// the code; a right one starts her 30-day session and goes to her home (303).
async fn check(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<SignInForm>,
) -> Response {
    let now = state.clock.now();
    let parts = state.hooks.as_deref().zip(state.verify.as_deref());
    let admitted = passed(admit_code_try(&state.pool, (ip, None), now).await);
    let Some((hooks, verify)) = parts.filter(|_| admitted) else {
        return code_page(&jar, &form.phone, true);
    };
    let typed = (jar.clone(), form.phone.as_str(), form.code.as_str());
    match finish_sign_in(&state.pool, (&hooks.vault, verify), typed, now).await {
        Ok(jar) => {
            let to = HeaderValue::from_static(HOMEOWNER_HOME_ROUTE);
            (jar, (StatusCode::SEE_OTHER, [(LOCATION, to)])).into_response()
        }
        Err(HomeownerError::Database) => failed("sign-in check"),
        Err(HomeownerError::Refused | HomeownerError::Verify) => code_page(&jar, &form.phone, true),
    }
}

/// SEC-046: her session's proof, else the signed-out answer.
async fn signed_in(
    state: &Portal,
    jar: &CookieJar,
) -> Result<HomeownerSessionProof, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match current_session(&mut db, jar, state.clock.now()).await {
        Ok(session) => session
            .and_then(|s| s.homeowner_proof(crate::session_minter()))
            .ok_or_else(|| Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("session"))),
    }
}

/// CON-026: whether her latest request's texts are in Spanish; `false` with no request.
async fn spanish(db: &mut PgConnection, contact: Uuid) -> Result<bool, &'static str> {
    let ids = conversations_of_contact(db, contact).await;
    let Some(latest) = ids.map_err(|_| "requests")?.pop() else {
        return Ok(false);
    };
    let row = conversation(db, latest).await.map_err(|_| "request")?;
    Ok(row.is_some_and(|r| r.language == "es"))
}

/// PRIV-021: her home page, read through her own proof alone; no number on file is signed out.
async fn home_page(state: &Portal, jar: &CookieJar, proof: &HomeownerSessionProof) -> Response {
    let Some(hooks) = state.hooks.as_deref() else {
        return failed("vault");
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let her = match OnFile::read(&mut db, &hooks.vault, proof).await {
        Ok(Some(her)) => her,
        Ok(None) => return signed_out(),
        Err(_) => return failed("her record"),
    };
    let spanish = match spanish(&mut db, her.contact()).await {
        Ok(spanish) => spanish,
        Err(what) => return failed(what),
    };
    match HomeFacts::read(&mut db, &hooks.vault, proof, spanish).await {
        Ok(Some(facts)) => shown(jar, &PortalScreen::Home(facts), StatusCode::OK),
        Ok(None) => signed_out(),
        Err(_) => failed("home facts"),
    }
}

/// `GET` [`HOMEOWNER_HOME_ROUTE`] (PRIV-021, SEC-034, SEC-046): her own record. Changes nothing.
async fn home(State(state): State<Portal>, jar: CookieJar) -> Response {
    match signed_in(&state, &jar).await {
        Ok(proof) => home_page(&state, &jar, &proof).await,
        Err(answer) => *answer,
    }
}

/// `POST` [`TEXTS_OFF_ROUTE`] (CON-035, PRIV-021): puts her number on the suppression list at once
/// and shows her home with the on-screen confirmation; no text is sent.
async fn texts_off(
    State(state): State<Portal>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let proof = match signed_in(&state, &jar).await {
        Ok(proof) => proof,
        Err(answer) => return *answer,
    };
    let Some(hooks) = state.hooks.as_deref() else {
        return failed("vault");
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let her = match OnFile::read(&mut db, &hooks.vault, &proof).await {
        Ok(Some(her)) => her,
        Ok(None) => return signed_out(),
        Err(_) => return failed("her record"),
    };
    let now = state.clock.now();
    if turn_texts_off(&mut db, &hooks.vault, &her, now)
        .await
        .is_err()
    {
        return failed("texts off");
    }
    drop(db);
    home_page(&state, &jar, &proof).await
}

/// SEC-046: the number on file read through her session's proof, else the signed-out answer.
async fn her(state: &Portal, jar: &CookieJar) -> Result<OnFile, Box<Response>> {
    let proof = signed_in(state, jar).await?;
    let Some(hooks) = state.hooks.as_deref() else {
        return Err(Box::new(failed("vault")));
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match OnFile::read(&mut db, &hooks.vault, &proof).await {
        Ok(Some(her)) => Ok(her),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("her record"))),
    }
}

/// The Arizona day of `at` (UTC-7 all year, no daylight saving), as "November 1, 2026".
fn arizona_day(at: Timestamp) -> String {
    let arizona = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    at.to_zoned(arizona).strftime("%B %-d, %Y").to_string()
}

/// One held record of `contact`'s for the file.
fn item(contact: Uuid, section: Section, at: Timestamp, words: String) -> HeldItem {
    HeldItem {
        contact,
        section,
        at,
        words,
    }
}

/// PRIV-031: her requests (`intake::api::conversations_of_contact`) and each one's consent
/// records (`consent_records`).
async fn request_items(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Vec<HeldItem>, &'static str> {
    let mut items = Vec::new();
    for id in conversations_of_contact(db, contact)
        .await
        .map_err(|_| "requests")?
    {
        let Some(row) = conversation(db, id).await.map_err(|_| "request")? else {
            continue;
        };
        let language = if row.language == "es" {
            "Spanish"
        } else {
            "English"
        };
        let state = row.closed_at.map_or_else(
            || "still open".to_owned(),
            |at| format!("closed {}", arizona_day(at)),
        );
        let words = format!("Job request, texts in {language}, {state}");
        items.push(item(contact, Section::Requests, row.opened_at, words));
        for record in consent_records(db, id).await.map_err(|_| "consent")? {
            let by = if record.channel == "web_chat" {
                "web chat"
            } else {
                "replying to a text"
            };
            let version = record.consent_version;
            let words = format!("You agreed to our texts by {by} (wording {version})");
            items.push(item(contact, Section::Consent, record.consented_at, words));
        }
    }
    Ok(items)
}

/// PRIV-031: the quotes shown on her jobs (`quotes::api::shown_quotes`), each with its business's
/// public name, time and wording (`page_quotes`, `businesses::api::public_names`).
async fn quote_items(db: &mut PgConnection, contact: Uuid) -> Result<Vec<HeldItem>, &'static str> {
    let mut items = Vec::new();
    for job in jobs_of_contact(db, contact).await.map_err(|_| "jobs")? {
        let shown = shown_quotes(db, job).await.map_err(|_| "shown quotes")?;
        let quotes = page_quotes(db, job).await.map_err(|_| "quotes")?;
        let quotes: Vec<_> = (quotes.into_iter())
            .filter(|q| !q.withdrawn && shown.contains(&q.business))
            .collect();
        let ids: Vec<Uuid> = quotes.iter().map(|q| q.business).collect();
        let names = public_names(db, &ids).await.map_err(|_| "business names")?;
        for quote in quotes {
            let name = (names.iter().find(|(id, _)| *id == quote.business))
                .map_or(CARD_FALLBACK_NAME[0], |(_, name)| name.as_str());
            let words = format!("Quote from {name}: {}", quote.original);
            items.push(item(contact, Section::Quotes, quote.received_at, words));
        }
    }
    Ok(items)
}

/// PRIV-031: her "Download my data" file: only her own items, on one connection held for the reads.
async fn held_file(state: &Portal, her: &OnFile) -> Result<String, &'static str> {
    let mut db = state.pool.acquire().await.map_err(|_| "connection")?;
    let contact = her.contact();
    let mut items = request_items(&mut db, contact).await?;
    items.extend(quote_items(&mut db, contact).await?);
    let held = held_words(&mut db, contact).await.map_err(|_| "messages")?;
    let messages = held
        .into_iter()
        .map(|(at, words)| item(contact, Section::Messages, at, words));
    items.extend(messages);
    Ok(download_file(her, &items))
}

/// Her file as a JSON download, or the failed answer.
async fn file_answer(state: &Portal, her: &OnFile) -> Response {
    match held_file(state, her).await {
        Ok(file) => {
            let kind = (CONTENT_TYPE, "application/json; charset=utf-8");
            let saved = (CONTENT_DISPOSITION, "attachment; filename=\"my-data.json\"");
            (StatusCode::OK, [kind, saved], file).into_response()
        }
        Err(what) => failed(what),
    }
}

/// `GET` [`DOWNLOAD_ROUTE`] (PRIV-031, SEC-034, SEC-046): her own file. Changes nothing.
async fn download(State(state): State<Portal>, jar: CookieJar) -> Response {
    match her(&state, &jar).await {
        Ok(her) => file_answer(&state, &her).await,
        Err(answer) => *answer,
    }
}

/// The data-request forms: the CSRF token, the kind (`copy` or `delete`), the request's ID and,
/// on the code step, the code. A missing field reads as empty, so CSRF and the realm come first.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DataForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    request: String,
    #[serde(default)]
    code: String,
}

impl CsrfField for DataForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl DataForm {
    /// The kind asked for; `None` for any other word.
    fn kind(&self) -> Option<DataRequestKind> {
        match self.kind.as_str() {
            "copy" => Some(DataRequestKind::Copy),
            "delete" => Some(DataRequestKind::Delete),
            _ => None,
        }
    }

    /// The request's ID; `None` when it is not a number.
    fn request(&self) -> Option<i64> {
        self.request.parse().ok()
    }
}

/// A short page with `title`, `heading` and the paragraph `line` (no detail, never a value).
fn note(status: StatusCode, (title, heading): (&str, &str), line: &str) -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>{title}</title></head><body><main>\
         <h1>{heading}</h1><p>{line}</p></main></body></html>"
    );
    (status, Html(page)).into_response()
}

/// SEC-014: the answer when no code was sent (a limit refused it, or Verify is not set up or
/// failed): a 429 linking back to her page; nothing was sent.
fn not_sent() -> Response {
    let line = format!(
        "Nothing has changed. Wait a few minutes and try again, or <a \
         href=\"{HOMEOWNER_HOME_ROUTE}\">go back to your page</a>."
    );
    let words = ("Code not sent", "We couldn't text your code");
    note(StatusCode::TOO_MANY_REQUESTS, words, &line)
}

/// PRIV-031: the answer for a request that is not hers or has ended: a 404 linking to her page.
fn ended() -> Response {
    let line = format!(
        "Delete and copy requests close after 30 days, and each code works once. <a \
         href=\"{HOMEOWNER_HOME_ROUTE}\">Go back to your page</a> to start a new one."
    );
    note(
        StatusCode::NOT_FOUND,
        ("Request closed", "This data request has closed"),
        &line,
    )
}

/// The code step of request `request`; a 400 with the error summary when `failed`.
fn request_screen(
    jar: &CookieJar,
    (kind, request): (DataRequestKind, i64),
    failed: bool,
) -> Response {
    let status = if failed {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    let screen = PortalScreen::Request {
        kind,
        request,
        failed,
    };
    shown(jar, &screen, status)
}

/// SEC-014, D-1951: the Verify parts when the per-IP and per-contact send limits admit one more
/// code to her; `None` (send nothing) when refused or not set up.
async fn admitted_send(
    state: &Portal,
    ip: IpAddr,
    her: &OnFile,
) -> Option<(Arc<Webhooks>, Arc<HomeownerVerify>)> {
    let threshold = i32::try_from(VERIFY_DAILY_THRESHOLD).unwrap_or(i32::MAX);
    let at = (state.clock.now(), threshold);
    let limit = admit_code_send(&state.pool, (ip, Some(her.contact())), at).await;
    let parts = state.hooks.clone().zip(state.verify.clone());
    parts.filter(|_| passed(limit))
}

/// `POST` [`DATA_REQUEST_ROUTE`] (PRIV-031, SEC-014): opens her copy or delete request and texts
/// its code after the send limits; the code step follows.
async fn data_request(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<DataForm>,
) -> Response {
    let her = match her(&state, &jar).await {
        Ok(her) => her,
        Err(answer) => return *answer,
    };
    let Some(kind) = form.kind() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some((hooks, verify)) = admitted_send(&state, ip, &her).await else {
        return not_sent();
    };
    let keys = (verify.as_ref(), &hooks.id_secret);
    match open_request(&state.pool, keys, &her, (kind, state.clock.now())).await {
        Ok(request) => request_screen(&jar, (kind, request.id), false),
        Err(RequestError::Verify) => not_sent(),
        Err(_) => failed("data request"),
    }
}

/// `POST` [`DATA_RESEND_ROUTE`] (PRIV-031, SEC-014, D-2348): texts her open request's code again
/// after the send limits.
async fn data_resend(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<DataForm>,
) -> Response {
    let her = match her(&state, &jar).await {
        Ok(her) => her,
        Err(answer) => return *answer,
    };
    let Some(id) = form.request() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some((hooks, verify)) = admitted_send(&state, ip, &her).await else {
        return not_sent();
    };
    let keys = (verify.as_ref(), &hooks.id_secret);
    match resend_request_code(&state.pool, keys, &her, (id, state.clock.now())).await {
        Ok(request) => request_screen(&jar, (request.kind, request.id), false),
        Err(RequestError::NoRequest) => ended(),
        Err(RequestError::Verify) => not_sent(),
        Err(_) => failed("data resend"),
    }
}

/// `POST` [`DATA_CODE_ROUTE`] (PRIV-031, PRIV-011, SEC-014): after the per-IP try limit, acts on
/// her open request only with its right code: a copy answers her file, a delete the PRIV-011
/// sentence; anything else changes nothing.
async fn data_code(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<DataForm>,
) -> Response {
    let her = match her(&state, &jar).await {
        Ok(her) => her,
        Err(answer) => return *answer,
    };
    let (Some(kind), Some(id)) = (form.kind(), form.request()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let now = state.clock.now();
    let admitted = passed(admit_code_try(&state.pool, (ip, None), now).await);
    let Some(verify) = state.verify.as_deref().filter(|_| admitted) else {
        return request_screen(&jar, (kind, id), true);
    };
    match confirm_request(&state.pool, verify, &her, (id, &form.code, now)).await {
        Ok(Acted::Copy) => file_answer(&state, &her).await,
        Ok(Acted::Deleted(sentence)) => note(
            StatusCode::OK,
            ("Data deleted", "Your data is deleted"),
            sentence,
        ),
        Err(RequestError::NoRequest) => ended(),
        Err(RequestError::Database) => failed("data code"),
        Err(RequestError::WrongCode | RequestError::NoLiveCode | RequestError::Verify) => {
            request_screen(&jar, (kind, id), true)
        }
    }
}

/// The path of a photo's short-lived link: the token in the path only (PRIV-014, PRIV-028).
pub const PHOTO_LINK_ROUTE: &str = "/photos/{token}";
/// The room the upload's body limit leaves past `MAX_PHOTO_BYTES` for the form's own parts, so a
/// file just over the limit reaches `take_upload` and gets the page's error (PRIV-025).
const FORM_SLACK: usize = 64 * 1024;
const STORAGE_BASE_URL: &str = "ASKHAVA_STORAGE_BASE_URL";

/// The photo routes' keys (T674; PRIV-014, PRIV-023): the seal under `VAULT_KEY`, the photo
/// storage and the `ID_SECRET` for storage keys and link tokens. No `Debug`: nothing prints.
pub struct PhotoParts {
    seal: PhotoSeal,
    storage: LocalStorage,
    id_secret: SecretString,
    link: IdSecret,
}

impl PhotoParts {
    /// The parts over `storage`, sealing under `vault_key` and keyed by `id_secret`.
    ///
    /// # Errors
    ///
    /// A bad vault key or a weak ID secret; the message names no value.
    pub fn new(
        vault_key: &SecretString,
        storage: LocalStorage,
        id_secret: SecretString,
    ) -> io::Result<Self> {
        let seal = PhotoSeal::new(vault_key).map_err(io::Error::other)?;
        let link = IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
        Ok(Self {
            seal,
            storage,
            id_secret,
            link,
        })
    }
}

/// The photo parts from `VAULT_KEY`, `ASKHAVA_STORAGE_BASE_URL` and `ASKHAVA_ID_SECRET` in
/// `pairs` (a blank value counts as unset); `None`, serving no upload and no photo link, when
/// any of the three is absent.
///
/// # Errors
///
/// A storage URL that is invalid or a stand-in in live, a bad vault key or a weak ID secret; the
/// message names the key, never a value.
pub fn photo_parts<I>(settings: &Settings, pairs: I) -> io::Result<Option<PhotoParts>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let (mut key, mut base, mut secret) = (None, None, None);
    for (name, value) in pairs.into_iter().filter(|(_, v)| !v.trim().is_empty()) {
        match name.as_str() {
            "VAULT_KEY" => key = Some(SecretString::from(value)),
            STORAGE_BASE_URL => base = Some(value.trim().to_owned()),
            "ASKHAVA_ID_SECRET" => secret = Some(SecretString::from(value.trim().to_owned())),
            _ => {}
        }
    }
    let (Some(key), Some(base), Some(secret)) = (key, base, secret) else {
        return Ok(None);
    };
    askhava_core::api::check_base_url(settings.environment(), STORAGE_BASE_URL, &base)
        .map_err(io::Error::other)?;
    let storage = LocalStorage::new(&base)
        .map_err(|_| io::Error::other("invalid ASKHAVA_STORAGE_BASE_URL"))?;
    PhotoParts::new(&key, storage, secret).map(Some)
}

/// SEC-019: the one not-found answer, with no detail, for a request that is not hers, an unknown
/// number and a dead or unknown photo token.
fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// INT-044, CARD-013 (D-5082): the answer to any photo post once her card was shown: a 409
/// linking to her page; nothing changed, since a shown card keeps its photo IDs.
fn card_out() -> Response {
    let line = format!(
        "Your request is past the photo step, so its photos can't change now. <a \
         href=\"{HOMEOWNER_HOME_ROUTE}\">Go back to your page</a>."
    );
    let words = ("Photos closed", "Photos can't change now");
    note(StatusCode::CONFLICT, words, &line)
}

/// SEC-019, SEC-046: the job whose number is `number` when a request of the signed-in homeowner
/// opened it (`photo_job_is_hers`, D-5083); the signed-out answer with no session, else [`not_found`].
async fn her_job(state: &Portal, jar: &CookieJar, number: &str) -> Result<Uuid, Box<Response>> {
    let her = her(state, jar).await?;
    let Ok(number) = number.parse::<i64>() else {
        return Err(Box::new(not_found()));
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    let Ok(job) = job_by_number(&mut db, number).await else {
        return Err(Box::new(failed("job number")));
    };
    let Some(job) = job else {
        return Err(Box::new(not_found()));
    };
    match photo_job_is_hers(&mut db, her.contact(), job).await {
        Ok(true) => Ok(job),
        Ok(false) => Err(Box::new(not_found())),
        Err(_) => Err(Box::new(failed("her jobs"))),
    }
}

/// INT-044, PRIV-015, A11Y-027: the upload page of request `number` in `screen`, as `status`.
fn upload_page(
    jar: &CookieJar,
    number: &str,
    screen: &UploadScreen,
    status: StatusCode,
) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    match photo_upload_page(number, &[], screen, &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("photo upload page"),
    }
}

/// `GET` `PHOTO_UPLOAD_ROUTE` (INT-044, PRIV-015, SEC-019, SEC-034): the upload form for her own
/// request, whatever its status. Changes nothing.
async fn photos_page(
    State(state): State<Portal>,
    jar: CookieJar,
    Path(number): Path<String>,
) -> Response {
    match her_job(&state, &jar, &number).await {
        Ok(_) => upload_page(&jar, &number, &UploadScreen::Ask(None), StatusCode::OK),
        Err(answer) => *answer,
    }
}

/// One photo post, from either body: the CSRF token, her answer (`yes` or `remove`) and the
/// uploaded file; `over` when the file ran past the body limit.
#[derive(Default, serde::Deserialize)]
struct PhotoPost {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    answer: String,
    #[serde(skip)]
    photo: Option<Vec<u8>>,
    #[serde(skip)]
    over: bool,
}

impl CsrfField for PhotoPost {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The fields of a multipart upload; a file past the body limit is `over`, never kept.
async fn upload_fields(mut parts: Multipart) -> PhotoPost {
    let mut post = PhotoPost::default();
    while let Ok(Some(field)) = parts.next_field().await {
        match field.name().map(str::to_owned).as_deref() {
            Some("csrf") => post.csrf = field.text().await.unwrap_or_default(),
            Some("photo") => match field.bytes().await {
                Ok(bytes) => post.photo = Some(bytes.to_vec()),
                Err(_) => post.over = true,
            },
            _ => {}
        }
    }
    post
}

/// SEC-042, SEC-048: the post with its CSRF token checked before anything else. A form-encoded
/// body goes through `CsrfForm`; a multipart body's token is checked the same way, and a wrong
/// one is refused by `CsrfForm` itself over the same request with an empty token, so either
/// refusal is the one 403 and one `csrf_rejected` event naming the route template.
async fn csrf_checked(state: &Portal, request: Request) -> Result<PhotoPost, Box<Response>> {
    let kind = request.headers().get(CONTENT_TYPE);
    let kind = kind.and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !kind.starts_with("multipart/form-data") {
        let form = CsrfForm::<PhotoPost>::from_request(request, state).await;
        return form.map(|CsrfForm(post)| post).map_err(Box::new);
    }
    let (parts, body) = request.into_parts();
    let request = Request::from_parts(parts.clone(), body);
    let fields = Multipart::from_request(request, state).await;
    let post = upload_fields(fields.map_err(|e| Box::new(e.into_response()))?).await;
    if csrf_matches(&CookieJar::from_headers(&parts.headers), &post.csrf) {
        return Ok(post);
    }
    let mut refused = Request::from_parts(parts, Body::from("csrf="));
    let form = HeaderValue::from_static("application/x-www-form-urlencoded");
    refused.headers_mut().insert(CONTENT_TYPE, form);
    refused.headers_mut().remove(CONTENT_LENGTH);
    match CsrfForm::<ButtonForm>::from_request(refused, state).await {
        Ok(_) => Err(Box::new(StatusCode::FORBIDDEN.into_response())),
        Err(refusal) => Err(Box::new(refusal)),
    }
}

/// INT-044, PRIV-014, PRIV-025, CARD-013: takes her upload for `job` through `take_upload` (the
/// on-server checks, no outside AI) and gives the screen: a stored photo's preview through a
/// fresh short-lived link, else the form with the file's error.
async fn taken(
    state: &Portal,
    db: &mut PgConnection,
    (job, file): (Uuid, Vec<u8>),
) -> Result<UploadScreen, Box<Response>> {
    let Some(photos) = state.photos.as_deref() else {
        return Err(Box::new(failed("photo keys")));
    };
    let now = state.clock.now();
    let keys = (&photos.seal, &photos.storage, &photos.id_secret);
    let taken = take_upload(db, keys, (job, file), DEFAULT_MIN_SHORT_SIDE, now).await;
    let taken = taken.map_err(|_| Box::new(failed("photo upload")))?;
    let PhotoTaken::Stored(photo) = taken else {
        return Ok(upload_screen(taken, String::new));
    };
    let token = issue_link(db, &photos.link, photo, now).await;
    let token = token.map_err(|_| Box::new(failed("photo link")))?;
    Ok(upload_screen(taken, || photo_path(&token)))
}

/// What `post` asks of `job`, before her card was shown: an upload's screen, the form again
/// after an answer, or `None` for a post with neither a file nor an answer.
async fn acted(
    state: &Portal,
    db: &mut PgConnection,
    (job, post): (Uuid, PhotoPost),
) -> Result<Option<UploadScreen>, Box<Response>> {
    if post.over {
        return Ok(Some(UploadScreen::Ask(Some(TOO_BIG_ERROR))));
    }
    if let Some(file) = post.photo {
        return taken(state, db, (job, file)).await.map(Some);
    }
    if post.answer.is_empty() {
        return Ok(None);
    }
    let answer = photo_answer(&post.answer);
    match press_photo(db, job, answer, state.clock.now()).await {
        Ok(PhotoPress::CardOut) => Err(Box::new(card_out())),
        Ok(_) => Ok(Some(UploadScreen::Ask(None))),
        Err(_) => Err(Box::new(failed("photo answer"))),
    }
}

/// `POST` `PHOTO_UPLOAD_ROUTE` (INT-044, PRIV-014, PRIV-025, CARD-013, SEC-019, SEC-042): after
/// the CSRF token and her own request, a multipart body is her upload and a form-encoded one her
/// "Send this photo" or "Take it off"; a post with neither is the page as a 400, a refused file
/// the page with its error as a 400, and any post after her card was shown [`card_out`].
async fn photos_post(
    State(state): State<Portal>,
    jar: CookieJar,
    Path(number): Path<String>,
    request: Request,
) -> Response {
    let post = match csrf_checked(&state, request).await {
        Ok(post) => post,
        Err(refusal) => return *refusal,
    };
    let job = match her_job(&state, &jar, &number).await {
        Ok(job) => job,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let nothing = post.photo.is_none() && !post.over && post.answer.is_empty();
    match card_went_out(&mut db, job).await {
        Ok(true) if !nothing => return card_out(),
        Ok(_) => {}
        Err(_) => return failed("card state"),
    }
    let screen = match acted(&state, &mut db, (job, post)).await {
        Ok(screen) => screen,
        Err(answer) => return *answer,
    };
    let bad = !matches!(
        screen,
        Some(UploadScreen::Preview(_) | UploadScreen::Ask(None))
    );
    let status = if bad {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    upload_page(
        &jar,
        &number,
        &screen.unwrap_or(UploadScreen::Ask(None)),
        status,
    )
}

/// `GET` [`PHOTO_LINK_ROUTE`] (PRIV-014, PRIV-028, SEC-034): the opened photo of a live token as
/// a JPEG, with its `privacy.photo_opened` audit row; a dead, unknown or malformed token, and
/// any token without the photo parts, is [`not_found`]. No session opens or is needed.
async fn photo(State(state): State<Portal>, Path(token): Path<String>) -> Response {
    let (Some(photos), Ok(token)) = (state.photos.as_deref(), LinkToken::parse(&token)) else {
        return not_found();
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let keys = (&photos.seal, &photos.storage);
    match open_link(&mut db, keys, &token, state.clock.now()).await {
        Ok(photo) => {
            let kind = [(CONTENT_TYPE, HeaderValue::from_static("image/jpeg"))];
            (kind, photo.as_bytes().to_vec()).into_response()
        }
        Err(PhotoError::LinkDead) => not_found(),
        Err(_) => failed("photo"),
    }
}
