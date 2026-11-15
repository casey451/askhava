//! The served business sign-up (BIZ-001, BIZ-016, BIZ-021, BIZ-030, CON-019, SEC-014, SEC-015,
//! SEC-019, SEC-034, SEC-042, SEC-048; T302, T459, D-2128, D-3641, D-3676).
//!
//! [`ServedSignUp`] holds the sign-up routes `build_router` merges through
//! `RouterParts::with_sign_up`, each without a session. `GET` [`SIGNUP_ROUTE`] sets the
//! pre-session `__Host-` cookie (`ensure_pre_session`), puts that cookie's CSRF token in the form
//! (SEC-042) and seals the form's shown channels for that token with `ASKHAVA_ID_SECRET`
//! (`sealed_signup_page`), so a later post can prove the wording it saw; the E-SIGN box is never
//! ticked on the served form (CON-019). The category list is the stored switched-on categories,
//! labelled by the shipped class map (`stored_category_list`, BIZ-021); the fee box comes from the
//! Confirm window setting (BIZ-030).
//!
//! `POST` [`SIGNUP_FORM_ROUTE`] (T459) runs, in order: `admit_sign_up` (D-1951); the cross-site
//! and CSRF refusal, a `csrf_rejected` security event (SEC-048); the consent from `ChannelPost`
//! only when its seal verifies, the wording always `consent_wording` (CON-019); the names screened
//! (D-2943); the mailbox minted by `mailbox_for` (D-3262); `submit_signup` (BIZ-001); the names,
//! the pest attestation and agency license (D-2711, D-2712); the ROC number bound once by
//! `check_license`; then `signup_record` read in this request decides the match and flag
//! (`record_match`, BIZ-016) and the screen: the masked contact, the saved-sign-up screen for
//! `NoContact` (needs review), or a bare 503 with `Retry-After` for `Unreadable` and `Unbound`.
//! The record is never stored, logged or put in an error, and every answer is `no-store`
//! (SEC-034). The sign-up is bound to the browser by the [`SIGNUP_COOKIE`] (SEC-019, D-3641),
//! checked by [`bound_signup`].
//!
//! T460 (BIZ-002, BIZ-026, TXT-038, D-3779): the identity steps ([`Ask`], `Form::step`, D-3769)
//! act only for the cookie's sign-up while it is open and `signup_progress` allows the step; the
//! record contact is read in that request, never stored or logged; codes go through
//! [`IdentityCodes`].
//!
//! T462 (BIZ-042, SEC-004, SEC-034, SEC-042, D-462, D-3676): [`INBOX_CHECK_ROUTE`] is served after
//! activation behind `business_principal`, not the binding cookie; it keeps the Business door, so
//! no business person gets the 401 sign-in answer. Its `GET` shows `email_check_page` for the
//! person's own `Acting` and `?app=`; its `POST` (CSRF-checked, cross-site refused, each a
//! `csrf_rejected` event) sends the sample card or stores the answer. No sample pick step here.
//!
//! T461 (BIZ-016, BIZ-019, BIZ-040, BIZ-058, SEC-014, D-3676, D-3743, D-3830): after the identity
//! code, [`SERVICE_AREA_ROUTE`] saves the base ZIP and radius and sends the first email code;
//! [`EMAIL_CHECK_ROUTE`] takes it (`confirm_mailbox`, which settles the sign-up) and
//! [`EMAIL_RESEND_ROUTE`] sends a new one. Each send is admitted first under `CODE_SENDS_PER_IP`
//! and [`SIGNUP_EMAIL_CODES_PER_BUSINESS`]; a refused send opens and sends nothing. The code goes
//! only to the sign-up mailbox through [`SignupMail`] and is never logged or stored here. An
//! activating code clears the [`SIGNUP_COOKIE`], emails the owner's sign-in link and shows the
//! done screen; a flagged one shows `Screen::Review` and keeps the cookie.

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, Charge, CodeChannel, EmailAdapter, IdempotencyKey, LicenseLists, LicenseRecords,
    PaymentIntent, PaymentsAdapter, ResendEmail, StripePayments, TwilioConfig, VerifyAdapter,
    twilio_clients,
};
use askhava_businesses::api::{
    Acting, InboxAnswer, ServiceArea, confirm_mailbox, email_check as inbox_facts, join_facts,
    record_inbox_answer, save_service_area, send_sample_card, service_area, signup_mailbox,
    start_signup_check,
};
use askhava_businesses::api::{
    ConsentChannel, ContactKind, Flag, MaskedContact, NameRefusal, RecordFound, SignUpForm,
    SignupError, SignupRecord, SignupSettings, Step, business as stored_business, check_code,
    next_step, number_is_old, record_match, save_business_name, screen_names, send_code_seen,
    signup_progress, signup_record, stored_category_list, stored_enabled, waiting_on_record,
    waitlisted,
};
use askhava_catalog::api::category as catalog_category;
use askhava_core::api::{
    Clock, Environment, IdSecret, LinkKind, LinkSalt, Settings, SystemClock, link_token,
};
use askhava_intake::api::{Admission, Key, Limit, admit};
use askhava_licensing::api::{
    Agency, ClassMap, DailyCheck, ImportError, PestAttestation, check_license,
    record_agency_license, record_pest_attestation,
};
use askhava_messaging::api::MailboxCodeSender;
use askhava_ops::api::Actor;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    AreaEntered, CODE_SEND_ROUTE, ChannelPost, CodeProblem, CookieJar, Entered,
    IDENTITY_CHECK_ROUTE, LETTER_ROUTE, NUMBER_OLD_ROUTE, PageLanguage, Parts, SIGNUP_FORM_ROUTE,
    SIGNUP_ROUTE, Screen, SecurityEvent, SecurityEventKind, SignupConsent, SignupRefusal,
    admit_security_event, admit_sign_up, csrf_matches, csrf_token, ensure_pre_session, fee_numbers,
    is_cross_site, record_security_event, refusal_problem, sealed_signup_page,
    security_event_client, submit_signup,
};
use askhava_portals::api::{
    CODE_SENDS_PER_IP, CsrfField, CsrfForm, DoneFacts, EMAIL_CHECK_ROUTE, EMAIL_RESEND_ROUTE,
    MailApp, SERVICE_AREA_ROUTE, SIGNIN_ROUTE, SignInAddress, WebSecurity, business_principal,
    done_facts, email_check_page, no_store as no_store_layer, refuse_cross_site, send_sign_in_link,
};
use askhava_privacy::api::{
    BusinessContactError, BusinessPurpose, Vault, mailbox_for, read_business_mailbox,
};
use axum::Router;
use axum::extract::{FromRef, FromRequest, Query, Request, State};
use axum::http::header::{CACHE_CONTROL, HOST, HeaderValue, LOCATION, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use super::business_contacts::ContactDirectory;
use super::business_signin::{NoVerify, SignInMailer};

/// TXT-035: business texting is off, so the form offers email notices alone and the E-SIGN box
/// names email only (BIZ-036, CON-019).
const TEXTING_ON: bool = false;

/// SEC-019, D-3641: the cookie binding a browser to its sign-up: `<sign-up ID>.<salt>.<MAC>`.
pub const SIGNUP_COOKIE: &str = "__Host-askhava-signup";

/// The binding cookie's life: 30 days, so the wait for an identity letter fits (D-3641).
const SIGNUP_COOKIE_SECS: i64 = 30 * 24 * 60 * 60;

/// LIC-008: how long a business waits before trying again after an unreadable record.
const TRY_AGAIN_SECS: &str = "30";

/// SEC-014, SEC-025, PICK-055 (T461): sign-up email code sends per business, 5 per Arizona day,
/// counted after `CODE_SENDS_PER_IP`, so the cookie's holder cannot flood the address.
pub const SIGNUP_EMAIL_CODES_PER_BUSINESS: Limit =
    Limit::daily("signup.email_codes_per_business", 5);

/// What the form `POST` calls besides the pool (T459), each built from settings by
/// [`ServedSignUp::from_settings`].
pub struct StartAdapters {
    /// `ASKHAVA_ID_SECRET` as text: `submit_signup`'s idempotency key input (SEC-018).
    pub id_secret: SecretString,
    /// The vault for the sign-up mailbox (D-3262) and the text phone's link (`ContactDirectory`);
    /// `None` in dev and CI without keys, where the form `POST` stores nothing.
    pub vault: Option<(Vault, ContactDirectory)>,
    /// The Stripe client for the customer (BIZ-001); `None` in dev and CI without it, where
    /// `customer_ready` stays false for `ensure_customer` to retry.
    pub stripe: Option<StripePayments>,
    /// The license records on `ASKHAVA_LICENSE_BASE_URL` (D-2954); `None` in dev and CI without
    /// it, where every record read is unreadable.
    pub records: Option<LicenseLists>,
    /// The saved-sign-up screen's list delay (BIZ-026).
    pub settings: SignupSettings,
    /// TXT-038: the Verify client the identity codes are sent and checked through (T460).
    pub verify: IdentityCodes,
    /// BIZ-016, BIZ-040: the email code and the owner's sign-in link (T461).
    pub mail: SignupMail,
}

/// A boxed Verify answer, so [`IdentityCodes`] can hold any Verify client.
type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, AdapterError>> + Send + 'a>>;

/// [`VerifyAdapter`] in a form that can sit behind `dyn`.
trait Codes: Send + Sync {
    fn send<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        to: &'a str,
        by: CodeChannel,
    ) -> Pending<'a, String>;
    fn check<'a>(&'a self, to: &'a str, code: &'a str) -> Pending<'a, bool>;
}

impl<V: VerifyAdapter + Send + Sync> Codes for V {
    fn send<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        to: &'a str,
        by: CodeChannel,
    ) -> Pending<'a, String> {
        Box::pin(self.send_code(key, to, by))
    }

    fn check<'a>(&'a self, to: &'a str, code: &'a str) -> Pending<'a, bool> {
        Box::pin(self.check_code(to, code))
    }
}

/// BIZ-002, TXT-038: the identity steps' Verify client, whichever one it is: Twilio's, the dev
/// and CI stand-in `NoVerify` (every send and check fails) or a test's fake.
pub struct IdentityCodes(Box<dyn Codes>);

impl IdentityCodes {
    /// The identity steps' codes through `verify`.
    #[must_use]
    pub fn new<V: VerifyAdapter + Send + Sync + 'static>(verify: V) -> Self {
        Self(Box::new(verify))
    }
}

impl VerifyAdapter for IdentityCodes {
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

/// What the routes read: the pool, the sealing key, the fee box's settings and the adapters.
struct Form {
    pool: PgPool,
    secret: IdSecret,
    confirm: ConfirmSettings,
    start: StartAdapters,
}

/// The served sign-up routes (T302: the form page; T459: the form `POST`; T460: the identity
/// steps; T462: the inbox check behind the business session).
pub struct ServedSignUp(Router);

impl ServedSignUp {
    /// The form page and its `POST` over `pool`, sealing and binding with `secret` (CON-019,
    /// SEC-019, D-3602), showing the fee box with `confirm`'s window (BIZ-030) and storing the
    /// sign-up through `start`.
    #[must_use]
    pub fn new(
        pool: PgPool,
        secret: IdSecret,
        confirm: ConfirmSettings,
        start: StartAdapters,
    ) -> Self {
        let inbox_pool = pool.clone();
        let state = Arc::new(Form {
            pool,
            secret,
            confirm,
            start,
        });
        let routes = Router::new()
            .route(SIGNUP_ROUTE, get(form_page))
            .route(SIGNUP_FORM_ROUTE, post(start_post))
            .route(CODE_SEND_ROUTE, post(code_send))
            .route(NUMBER_OLD_ROUTE, post(number_old))
            .route(LETTER_ROUTE, post(letter))
            .route(IDENTITY_CHECK_ROUTE, post(identity_check))
            .route(SERVICE_AREA_ROUTE, post(area_post))
            .route(EMAIL_CHECK_ROUTE, post(email_check))
            .route(EMAIL_RESEND_ROUTE, post(email_resend))
            .with_state(state);
        Self(routes.merge(inbox_routes(inbox_pool)))
    }

    /// The part `main` passes to `build_router`: `ASKHAVA_ID_SECRET`, the Confirm window, the
    /// vault, the Stripe client, the license records and the Verify client from `pairs` (D-2954,
    /// D-3055, TXT-038).
    ///
    /// # Errors
    ///
    /// A missing or weak `ASKHAVA_ID_SECRET`; a bad setting; in test or live a missing vault,
    /// Stripe, Twilio or `ASKHAVA_LICENSE_BASE_URL` setting. Never a value.
    pub fn from_settings<I>(pool: PgPool, pairs: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        Self::built(pool, pairs, (None, None))
    }

    /// As [`ServedSignUp::from_settings`], the identity codes going through `verify` instead
    /// (route tests' fake Verify, TXT-038).
    ///
    /// # Errors
    ///
    /// As [`ServedSignUp::from_settings`].
    pub fn with_verify<I, V>(pool: PgPool, pairs: I, verify: V) -> io::Result<Self>
    where
        I: IntoIterator<Item = (String, String)>,
        V: VerifyAdapter + Send + Sync + 'static,
    {
        Self::built(pool, pairs, (Some(IdentityCodes::new(verify)), None))
    }

    /// As [`ServedSignUp::with_verify`], the email code and sign-in link going through `mail`
    /// (route tests' recording provider, BIZ-040).
    ///
    /// # Errors
    ///
    /// As [`ServedSignUp::from_settings`].
    pub fn with_mail<I, V>(pool: PgPool, pairs: I, verify: V, mail: SignupMail) -> io::Result<Self>
    where
        I: IntoIterator<Item = (String, String)>,
        V: VerifyAdapter + Send + Sync + 'static,
    {
        Self::built(pool, pairs, (Some(IdentityCodes::new(verify)), Some(mail)))
    }

    fn built<I>(
        pool: PgPool,
        pairs: I,
        (verify, mail): (Option<IdentityCodes>, Option<SignupMail>),
    ) -> io::Result<Self>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let pairs: Vec<(String, String)> = pairs.into_iter().collect();
        let raw = pairs
            .iter()
            .find(|(k, v)| k == "ASKHAVA_ID_SECRET" && !v.trim().is_empty())
            .map(|(_, v)| SecretString::from(v.trim().to_owned()))
            .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
        let secret = IdSecret::new(1, raw.clone()).map_err(io::Error::other)?;
        let confirm = ConfirmSettings::from_pairs(pairs.clone()).map_err(io::Error::other)?;
        let mail = match mail {
            Some(mail) => mail,
            None => unsent_mail(&pool, &pairs)?,
        };
        let start = start_adapters(raw, &pairs, (verify, mail))?;
        Ok(Self::new(pool, secret, confirm, start))
    }

    /// The routes, state applied, ready to merge.
    pub fn into_router(self) -> Router {
        self.0
    }
}

/// The form `POST`'s adapters from `pairs`, required in test and live as their own parts require
/// them (D-2954, D-3262).
fn start_adapters(
    id_secret: SecretString,
    pairs: &[(String, String)],
    (verify, mail): (Option<IdentityCodes>, SignupMail),
) -> io::Result<StartAdapters> {
    let settings = Settings::from_pairs(pairs.to_vec()).map_err(io::Error::other)?;
    let vault = super::vault_from(&settings, pairs.to_vec())?;
    let linked = super::vault_from(&settings, pairs.to_vec())?.map(ContactDirectory::new);
    let records = match settings.license_base_url() {
        Some(url) => Some(LicenseLists::new(url).map_err(io::Error::other)?),
        None => match settings.environment() {
            Environment::Dev | Environment::Ci => None,
            Environment::Test | Environment::Live => {
                return Err(io::Error::other(
                    "the sign-up needs ASKHAVA_LICENSE_BASE_URL",
                ));
            }
        },
    };
    Ok(StartAdapters {
        id_secret,
        vault: vault.zip(linked),
        stripe: super::stripe_client(&settings, pairs)?,
        records,
        settings: SignupSettings::from_pairs(pairs.to_vec()).map_err(io::Error::other)?,
        verify: match verify {
            Some(verify) => verify,
            None => identity_codes(&settings, pairs)?,
        },
        mail,
    })
}

/// D-3186: the app builds no Resend client yet, so the email code and the sign-in link are
/// stored but not emailed, as `ServedSignIn::from_settings` builds its mailer.
fn unsent_mail(pool: &PgPool, pairs: &[(String, String)]) -> io::Result<SignupMail> {
    let settings = Settings::from_pairs(pairs.to_vec()).map_err(io::Error::other)?;
    let vault = super::vault_from(&settings, pairs.to_vec())?.map(Arc::new);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let link = SignInMailer::<ResendEmail>::new((pool.clone(), clock), vault.clone(), None);
    Ok(SignupMail::new(vault, None, link))
}

/// A boxed email send's answer; the error carries no value (R5).
type Sent<'a> = Pin<Box<dyn Future<Output = Result<(), ()>> + Send + 'a>>;

/// The two emails sign-up sends, in a form that can sit behind `dyn`.
trait Mails: Send + Sync {
    fn code<'a>(
        &'a self,
        db: &'a mut PgConnection,
        to: (Uuid, &'a SecretString),
        now: Timestamp,
    ) -> Sent<'a>;
    fn link<'a>(&'a self, db: &'a mut PgConnection, to: SignInAddress, now: Timestamp) -> Sent<'a>;
}

/// The vault the code's address is opened in, the code mailer and the sign-in mailer.
struct Mailers<E> {
    vault: Option<Arc<Vault>>,
    code: Option<MailboxCodeSender<E>>,
    link: SignInMailer<E>,
}

impl<E: EmailAdapter + Send + Sync + 'static> Mails for Mailers<E> {
    fn code<'a>(
        &'a self,
        db: &'a mut PgConnection,
        (mailbox, code): (Uuid, &'a SecretString),
        now: Timestamp,
    ) -> Sent<'a> {
        Box::pin(async move {
            let (Some(vault), Some(sender)) = (&self.vault, &self.code) else {
                return Ok(());
            };
            let purpose = BusinessPurpose::AccountEmail;
            let address = read_business_mailbox(db, vault, purpose, mailbox).await;
            let address = address.map_err(|_| ())?;
            let sent = sender.send(db, (mailbox, &address), code, now).await;
            sent.map(|_| ()).map_err(|_| ())
        })
    }

    fn link<'a>(&'a self, db: &'a mut PgConnection, to: SignInAddress, now: Timestamp) -> Sent<'a> {
        Box::pin(async move {
            let sent = send_sign_in_link(db, &self.link, to, now).await;
            sent.map_err(|_| ())
        })
    }
}

/// BIZ-016, BIZ-040, SEC-004, D-3743: what sign-up emails: the mailbox code, opened under
/// `AccountEmail` and sent by `MailboxCodeSender` to the sign-up mailbox only, and the owner's
/// normal sign-in link through `send_sign_in_link` and the server's [`SignInMailer`].
pub struct SignupMail(Box<dyn Mails>);

impl SignupMail {
    /// Codes opened in `vault` and sent by `code`, links sent by `link`; with no vault or no
    /// code sender no code is emailed (D-3186).
    #[must_use]
    pub fn new<E: EmailAdapter + Send + Sync + 'static>(
        vault: Option<Arc<Vault>>,
        code: Option<MailboxCodeSender<E>>,
        link: SignInMailer<E>,
    ) -> Self {
        Self(Box::new(Mailers { vault, code, link }))
    }
}

/// TXT-038, D-3216: the Twilio Verify client, or in dev and CI without Twilio settings the
/// `NoVerify` stand-in, as `ServedSignIn::from_settings` builds its own.
fn identity_codes(settings: &Settings, pairs: &[(String, String)]) -> io::Result<IdentityCodes> {
    match TwilioConfig::from_settings(settings, pairs.to_vec()) {
        Ok(config) => {
            let (_, _, verify) = twilio_clients(&config).map_err(io::Error::other)?;
            Ok(IdentityCodes::new(verify))
        }
        Err(error) => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(IdentityCodes::new(NoVerify)),
            Environment::Test | Environment::Live => Err(io::Error::other(error)),
        },
    }
}

/// BIZ-021, CON-019, SEC-042: `GET` [`SIGNUP_ROUTE`], the form with the box unticked, the
/// pre-session cookie and its CSRF token, and the shown channels sealed for that token. Changes
/// nothing stored; any failure is a bare 500 logged once without values.
async fn form_page(State(form): State<Arc<Form>>, jar: CookieJar) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return failure("pre-session cookie");
    };
    let Some(csrf) = csrf_token(&jar) else {
        return failure("csrf token");
    };
    let Some(categories) = categories(&form.pool).await else {
        return failure("category list");
    };
    match form.page(Screen::Form(&categories), &csrf) {
        Some(html) => (jar, no_store(), Html(html)).into_response(),
        None => failure("sign-up template"),
    }
}

/// BIZ-021, LIC-003: the stored switched-on categories in code order, each "Open now" or
/// "Coming later" by the shipped class map; `None` when the database or the map fails.
async fn categories(pool: &PgPool) -> Option<Vec<(String, &'static str)>> {
    let mut conn = pool.acquire().await.ok()?;
    let enabled = stored_enabled(&mut conn).await.ok()?;
    let map = ClassMap::shipped().ok()?;
    let codes: Vec<&str> = enabled.iter().map(String::as_str).collect();
    Some(stored_category_list(map, &enabled, &codes))
}

/// SEC-014, SEC-015, SEC-042, SEC-048, D-1951: `POST` [`SIGNUP_FORM_ROUTE`]. `admit_sign_up`
/// first (a refusal is the bare failure and does nothing else); then a cross-site post or one
/// whose `csrf` field is not the pre-session cookie's token is a 403 and a `csrf_rejected`
/// security event, storing nothing else; then [`Form::submit`].
async fn start_post(State(form): State<Arc<Form>>, request: Request) -> Response {
    let now = Timestamp::now();
    let ip = security_event_client(&request);
    let admitted = admit_sign_up(&form.pool, ip, now).await;
    if !matches!(admitted, Ok(Admission::Admitted)) {
        return failure("sign-up limit");
    }
    match same_site(&form.pool, request, SIGNUP_FORM_ROUTE, now).await {
        Ok((fields, _)) => {
            let posted = Posted(&fields);
            form.submit(&posted, posted.get("csrf"), now).await
        }
        Err(refused) => *refused,
    }
}

/// SEC-042, SEC-048: the posted fields and the cookies of a same-site post to `route` whose `csrf`
/// field is the request cookie's token; else the 403 with its `csrf_rejected` event ([`refuse`]),
/// or a 400 for a body that is no form.
async fn same_site(
    pool: &PgPool,
    request: Request,
    route: &'static str,
    now: Timestamp,
) -> Result<(Vec<(String, String)>, CookieJar), Box<Response>> {
    let ip = security_event_client(&request);
    let headers = request.headers().clone();
    let parsed = axum::Form::<Vec<(String, String)>>::from_request(request, &()).await;
    let Ok(axum::Form(fields)) = parsed else {
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    };
    let jar = CookieJar::from_headers(&headers);
    let host = headers.get(HOST).and_then(|h| h.to_str().ok());
    if is_cross_site(&headers, host) || !csrf_matches(&jar, Posted(&fields).get("csrf")) {
        return Err(Box::new(refuse(pool, (ip, route), &headers, now).await));
    }
    Ok((fields, jar))
}

/// SEC-048: the 403 for a cross-site or wrong-CSRF post to `route`, with its security event
/// (request ID and route, no actor: there is no session) while the per-minute cap admits it.
async fn refuse(
    pool: &PgPool,
    (ip, route): (IpAddr, &'static str),
    headers: &HeaderMap,
    now: Timestamp,
) -> Response {
    let kind = SecurityEventKind::CsrfRejected;
    if admit_security_event(kind, ip, route, now) {
        let request_id = headers
            .get("x-request-id")
            .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
        let event = SecurityEvent {
            kind,
            route,
            request_id,
            actor_ref: None,
        };
        let stored = match pool.acquire().await {
            Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
            Err(_) => false,
        };
        if !stored {
            tracing::warn!("sign-up security event not stored");
        }
    }
    StatusCode::FORBIDDEN.into_response()
}

/// The posted form's fields, read by name.
struct Posted<'a>(&'a [(String, String)]);

impl<'a> Posted<'a> {
    /// The first value posted as `name`, or empty.
    fn get(&self, name: &str) -> &'a str {
        let found = self.0.iter().find(|(n, _)| n == name);
        found.map_or("", |(_, v)| v.as_str())
    }

    /// Every value posted as `name`.
    fn all(&self, name: &str) -> Vec<&'a str> {
        let named = self.0.iter().filter(|(n, _)| n == name);
        named.map(|(_, v)| v.as_str()).collect()
    }

    /// The value posted as `name` when it is not blank.
    fn given(&self, name: &str) -> Option<&'a str> {
        Some(self.get(name)).filter(|v| !v.trim().is_empty())
    }

    /// D-2943: the typed names, numbers and links, kept on the form again.
    fn fill(&self, entered: &mut Entered<'a>) {
        entered.business_name = self.get("business_name");
        entered.trade_name = self.get("trade_name");
        entered.roc_number = self.get("roc_number");
        entered.btr_number = self.get("btr_number");
        entered.city_license_number = self.get("city_license_number");
        entered.google_link = self.get("google_link");
        entered.yelp_link = self.get("yelp_link");
    }
}

impl Form {
    /// CON-019, D-2943, D-3262, BIZ-001: the consent from the verified seal (else the form again
    /// with `CONSENT_AGAIN`), the names screened (else the form again with the reason), the box
    /// ticked (else `CONSENT_MISSING`, nothing minted), the text phone, categories and source
    /// code checked (`checked_form`, nothing minted), the mailbox minted from the typed address,
    /// then `submit_signup`.
    async fn submit(&self, posted: &Posted<'_>, csrf: &str, now: Timestamp) -> Response {
        let channels = ChannelPost::parse(posted.0, TEXTING_ON, (&self.secret, csrf));
        let mut entered = channels.entered();
        posted.fill(&mut entered);
        let chosen = channels.consent_channels();
        let Some(chosen) = chosen.filter(|_| channels.problem().is_none()) else {
            return self.again(&entered, csrf).await;
        };
        let names = (posted.get("business_name"), posted.given("trade_name"));
        if let Err(refused) = screen_names(names.0, names.1, posted.given("city_license_number")) {
            let reason = refused.reason();
            entered.problem = Some((name_field(refused), reason.as_str()));
            return self.again(&entered, csrf).await;
        }
        let ticked = posted.get("consent") == "yes";
        if !ticked {
            return self.refused(&SignupRefusal::NoConsent, entered, csrf).await;
        }
        let (Some((vault, directory)), Ok(map)) = (&self.start.vault, ClassMap::shipped()) else {
            return failure("vault or class map");
        };
        let has_phone = channels.text_phone.is_some();
        let mut form = match checked_form(posted, map, &chosen, has_phone) {
            Ok(form) => form,
            Err(refusal) => return self.refused(&refusal, entered, csrf).await,
        };
        let Some(mailbox_id) = self.mailbox(vault, posted.get("email")).await else {
            return failure("mailbox");
        };
        form.mailbox_id = mailbox_id;
        let consent = SignupConsent {
            ticked,
            channels: &chosen,
            text_phone: channels.text_phone.as_ref().map(|p| (directory, p)),
        };
        let keys = (
            &Customers(self.start.stripe.as_ref()),
            &self.start.id_secret,
            map,
        );
        match submit_signup(&self.pool, keys, &form, consent, now).await {
            Ok(up) => self.stored(up.business, posted, csrf, now).await,
            Err(refusal) => self.refused(&refusal, entered, csrf).await,
        }
    }

    /// D-3262: the mailbox `mailbox_for` mints for `address`; `Some(None)` for an address that
    /// is not one (the sign-up then refuses `NoEmail`), `None` when the vault failed.
    async fn mailbox(&self, vault: &Vault, address: &str) -> Option<Option<Uuid>> {
        let mut db = self.pool.acquire().await.ok()?;
        match mailbox_for(&mut db, vault, address).await {
            Ok(id) => Some(Some(id)),
            Err(BusinessContactError::BadAddress) => Some(None),
            Err(_) => None,
        }
    }

    /// CON-019, A11Y-030: a refusal that stored nothing shows the form again (with its message
    /// when the form can fix it); any other is the bare failure.
    async fn refused(
        &self,
        refusal: &SignupRefusal,
        mut entered: Entered<'_>,
        csrf: &str,
    ) -> Response {
        let fixable = matches!(
            refusal,
            SignupRefusal::NoConsent
                | SignupRefusal::NoTextPhone
                | SignupRefusal::Signup(
                    SignupError::NoEmail | SignupError::BadCategories | SignupError::BadSourceCode
                )
        );
        if !fixable {
            return failure("sign-up store");
        }
        entered.problem = refusal_problem(refusal);
        self.again(&entered, csrf).await
    }

    /// D-2943: the form again with what was typed and the reason, sealed for the same token.
    async fn again(&self, entered: &Entered<'_>, csrf: &str) -> Response {
        let Some(categories) = categories(&self.pool).await else {
            return failure("category list");
        };
        match self.page(Screen::FormAgain(&categories, entered), csrf) {
            Some(html) => (no_store(), Html(html)).into_response(),
            None => failure("sign-up template"),
        }
    }

    /// The screen's page for `csrf`, its shown channels sealed (CON-019).
    fn page(&self, screen: Screen<'_>, csrf: &str) -> Option<String> {
        let parts = Parts {
            texting_on: TEXTING_ON,
            language: PageLanguage::En,
            csrf,
            fees: fee_numbers(self.confirm, WindowRules::default()),
        };
        sealed_signup_page(screen, &parts, &self.secret).ok()
    }

    /// SEC-019, D-3641: after the sign-up is stored: the binding cookie, the sign-up email's
    /// check opened unsent so activation waits for it (BIZ-040, D-1846; its code is dropped and
    /// the email step opens a new one), the names and licenses, then the contact screen, all
    /// `no-store` (SEC-034).
    async fn stored(
        &self,
        business: Uuid,
        posted: &Posted<'_>,
        csrf: &str,
        now: Timestamp,
    ) -> Response {
        let Some(cookie) = binding_cookie(&self.secret, business) else {
            return failure("sign-up binding");
        };
        let Ok(mut db) = self.pool.acquire().await else {
            return failure("database");
        };
        let held = start_signup_check(&mut db, &self.start.id_secret, business, now).await;
        if held.is_err() {
            return failure("email check");
        }
        let mut response = match licenses(&mut db, business, posted).await {
            Ok(roc) => self.contact(&mut db, (business, roc), csrf, now).await,
            Err(()) => failure("names or licenses"),
        };
        response.headers_mut().append(SET_COOKIE, cookie);
        response
    }

    /// BIZ-016, LIC-008, LIC-011, LIC-048, SEC-034, D-2954: one `signup_record` read decides the
    /// match, the flag and the screen; nothing from the record is stored, logged or put in an
    /// error. `Unreadable` and `Unbound` change nothing and answer 503 with `Retry-After`.
    async fn contact(
        &self,
        db: &mut PgConnection,
        (business, roc): (Uuid, Option<DailyCheck>),
        csrf: &str,
        now: Timestamp,
    ) -> Response {
        let records = Records(self.start.records.as_ref());
        let Ok(record) = signup_record(db, &records, business).await else {
            return failure("sign-up record");
        };
        let passed = roc.map(|check| check == DailyCheck::Active);
        let delay = self.start.settings.list_delay_days;
        let html = match &record {
            SignupRecord::Read(found) => {
                let matched = record_match(db, business, passed.or(Some(true)), found.flag(), now);
                if matched.await.is_err() {
                    return failure("record match");
                }
                let screen = found
                    .masked()
                    .map_or(Screen::NumberOld(delay), |to| Screen::Contact(to, None));
                self.page(screen, csrf)
            }
            SignupRecord::NoContact => {
                let flag = Some(Flag::NoRecordContact);
                if record_match(db, business, passed, flag, now).await.is_err() {
                    return failure("record match");
                }
                self.page(Screen::NumberOld(delay), csrf)
            }
            SignupRecord::Unreadable | SignupRecord::Unbound => return try_again(),
        };
        shown(html)
    }
}

/// BIZ-002, BIZ-026: the identity step a `POST` asks for, one per route (D-3602, D-3676).
#[derive(Debug, Clone, Copy)]
enum Ask {
    /// "Send the code" and "Send a new code": the chain's next code (TXT-038).
    Send,
    /// "That number is old" and "Stop, I'll fix my record first" (D-288).
    Old,
    /// "Mail the letter".
    Letter,
    /// The typed identity code.
    Check,
}

impl Ask {
    /// The step's route, the security event's route (SEC-048).
    const fn path(self) -> &'static str {
        match self {
            Self::Send => CODE_SEND_ROUTE,
            Self::Old => NUMBER_OLD_ROUTE,
            Self::Letter => LETTER_ROUTE,
            Self::Check => IDENTITY_CHECK_ROUTE,
        }
    }
}

/// BIZ-026: `POST` [`CODE_SEND_ROUTE`], [`Ask::Send`].
async fn code_send(State(form): State<Arc<Form>>, request: Request) -> Response {
    step_post(form, request, Ask::Send).await
}

/// BIZ-026: `POST` [`NUMBER_OLD_ROUTE`], [`Ask::Old`].
async fn number_old(State(form): State<Arc<Form>>, request: Request) -> Response {
    step_post(form, request, Ask::Old).await
}

/// BIZ-026: `POST` [`LETTER_ROUTE`], [`Ask::Letter`].
async fn letter(State(form): State<Arc<Form>>, request: Request) -> Response {
    step_post(form, request, Ask::Letter).await
}

/// BIZ-002: `POST` [`IDENTITY_CHECK_ROUTE`], [`Ask::Check`].
async fn identity_check(State(form): State<Arc<Form>>, request: Request) -> Response {
    step_post(form, request, Ask::Check).await
}

/// SEC-019, SEC-042, SEC-048, D-3641: an identity step's `POST`. A cross-site or wrong-CSRF post
/// is the 403 with its security event; a missing or bad binding cookie is the bare 403; else
/// [`Form::step`] for the sign-up the cookie names. No posted ID or license number is read.
async fn step_post(form: Arc<Form>, request: Request, ask: Ask) -> Response {
    let now = Timestamp::now();
    let (fields, jar) = match same_site(&form.pool, request, ask.path(), now).await {
        Ok(posted) => posted,
        Err(refused) => return *refused,
    };
    let Some(business) = bound_signup(&jar, &form.secret) else {
        return not_now("binding");
    };
    let Ok(mut db) = form.pool.acquire().await else {
        return failure("database");
    };
    let posted = Posted(&fields);
    let typed = (posted.get("code"), posted.get("csrf"));
    form.step(&mut db, (business, ask), typed, now).await
}

impl Form {
    /// BIZ-002, BIZ-016, BIZ-026, D-3641, D-3779: the business must be `Pending` or
    /// `NeedsReview` (`takes_identity_code`) and its code not done (else the bare 403); its record is read in this request
    /// (`signup_record`, the bound ROC number). `ask` acts only when `signup_progress`'s stored
    /// step allows it: a code send when the chain's next step is a text or email, the letter when
    /// it is the letter, the code check once any step was sent (so an early check takes no try);
    /// "That number is old" (D-178) any time, then the saved screen. While the sign-up waits on
    /// a new record (`waiting_on_record`, D-1712) every other press shows the saved screen and
    /// sends or checks nothing, so no code reaches the number called old. Any other press shows
    /// the stored state's screen and changes nothing.
    async fn step(
        &self,
        db: &mut PgConnection,
        (business, ask): (Uuid, Ask),
        (code, csrf): (&str, &str),
        now: Timestamp,
    ) -> Response {
        match stored_business(db, business).await {
            Ok(b) if b.status.takes_identity_code() => {}
            Ok(_) => return not_now("status"),
            Err(_) => return failure("business"),
        }
        let at = match signup_progress(db, business).await {
            Ok(progress) if !progress.code_done() => progress.step(),
            Ok(_) => return not_now("code done"),
            Err(_) => return failure("sign-up progress"),
        };
        match waiting_on_record(db, business).await {
            Ok(true) if !matches!(ask, Ask::Old) => return shown(self.saved(csrf)),
            Ok(_) => {}
            Err(_) => return failure("waiting on record"),
        }
        let records = Records(self.start.records.as_ref());
        let found = match signup_record(db, &records, business).await {
            Ok(SignupRecord::Read(found)) => found,
            Ok(SignupRecord::NoContact) => return shown(self.saved(csrf)),
            Ok(SignupRecord::Unreadable | SignupRecord::Unbound) => return try_again(),
            Err(_) => return failure("sign-up record"),
        };
        let here = (&found, csrf);
        match (ask, at, next_step(found.contacts(), at)) {
            (Ask::Send, _, Some(Step::Text | Step::Email))
            | (Ask::Letter, _, Some(Step::Letter)) => {
                self.send(db, (business, at), here, now).await
            }
            (Ask::Send, _, Some(Step::Letter)) => self.screen(here, Some(Step::Letter), None),
            (Ask::Old, ..) => {
                let seen = (business, found.seen());
                let old = number_is_old(db, &self.start.id_secret, seen, now).await;
                old.map_or_else(|_| failure("number is old"), |()| shown(self.saved(csrf)))
            }
            (Ask::Check, Some(at), _) => self.check(db, (business, at), here, (code, now)).await,
            _ => self.screen(here, None, at.map(|s| (s, None))),
        }
    }

    /// BIZ-002, TXT-038, D-1756: the chain's next code to the record contact (a letter step is
    /// recorded for the mailing), then the code screen for the step sent.
    async fn send(
        &self,
        db: &mut PgConnection,
        (business, at): (Uuid, Option<Step>),
        (found, csrf): (&RecordFound, &str),
        now: Timestamp,
    ) -> Response {
        let keys = (&self.start.verify, &self.start.id_secret);
        match send_code_seen(db, keys, business, found.seen(), now).await {
            Ok(sent) => self.screen((found, csrf), None, sent.or(at).map(|s| (s, None))),
            Err(_) => failure("identity code send"),
        }
    }

    /// BIZ-002, BIZ-016, A11Y-030: the typed code checked by `check_code` for the stored step;
    /// right goes on to the service area (BIZ-058, T461), wrong shows the code screen again with
    /// `CodeProblem::Refused` (`check_code` answers only right or wrong, so an expired code is
    /// shown as refused).
    async fn check(
        &self,
        db: &mut PgConnection,
        (business, at): (Uuid, Step),
        (found, csrf): (&RecordFound, &str),
        (code, now): (&str, Timestamp),
    ) -> Response {
        let keys = (&self.start.verify, &self.start.id_secret);
        match check_code(db, keys, business, found.contacts(), (code, now)).await {
            Ok(true) => shown(self.page(Screen::ServiceArea(&AreaEntered::default()), csrf)),
            Ok(false) => self.screen((found, csrf), None, Some((at, Some(CodeProblem::Refused)))),
            Err(_) => failure("identity code check"),
        }
    }

    /// BIZ-026, SEC-034, D-3779: the screen for the stored state, the step from `signup_progress`
    /// (`code`: a sent step and its problem), never from `RecordFound::masked`: the code screen
    /// for a sent step; before any, the letter screen (`letter`, or a letter-only record) or the
    /// contact screen with any landline on the record; the saved screen when the record has no
    /// contact for the step. Always `no-store`.
    fn screen(
        &self,
        (found, csrf): (&RecordFound, &str),
        letter: Option<Step>,
        code: Option<(Step, Option<CodeProblem>)>,
    ) -> Response {
        let delay = self.start.settings.list_delay_days;
        let first = letter.or_else(|| next_step(found.contacts(), None));
        let html = match (code, first) {
            (Some((at, problem)), _) => masked_for(found, at)
                .and_then(|to| self.page(Screen::IdentityCode(&to, problem), csrf)),
            (None, Some(Step::Letter)) => masked_for(found, Step::Letter)
                .and_then(|to| self.page(Screen::Letter(&to, delay), csrf)),
            (None, Some(step)) => masked_for(found, step).and_then(|to| {
                let landline = found.masked().filter(|m| m.kind() == ContactKind::Landline);
                self.page(Screen::Contact(&to, landline), csrf)
            }),
            (None, None) => None,
        };
        shown(html.or_else(|| self.saved(csrf)))
    }

    /// BIZ-026: the saved screen with the typical list delay.
    fn saved(&self, csrf: &str) -> Option<String> {
        let delay = self.start.settings.list_delay_days;
        self.page(Screen::NumberOld(delay), csrf)
    }
}

/// BIZ-040, BIZ-058: a step after the identity code, one per route (D-3676).
#[derive(Debug, Clone, Copy)]
enum Later {
    /// The base ZIP and radius, then the first email code.
    Area,
    /// The typed email code.
    Check,
    /// "Send the email again".
    Resend,
}

impl Later {
    /// The step's route, the security event's route (SEC-048).
    const fn path(self) -> &'static str {
        match self {
            Self::Area => SERVICE_AREA_ROUTE,
            Self::Check => EMAIL_CHECK_ROUTE,
            Self::Resend => EMAIL_RESEND_ROUTE,
        }
    }
}

/// BIZ-058: `POST` [`SERVICE_AREA_ROUTE`], [`Later::Area`].
async fn area_post(State(form): State<Arc<Form>>, request: Request) -> Response {
    later_post(form, request, Later::Area).await
}

/// BIZ-040: `POST` [`EMAIL_CHECK_ROUTE`], [`Later::Check`].
async fn email_check(State(form): State<Arc<Form>>, request: Request) -> Response {
    later_post(form, request, Later::Check).await
}

/// BIZ-040: `POST` [`EMAIL_RESEND_ROUTE`], [`Later::Resend`].
async fn email_resend(State(form): State<Arc<Form>>, request: Request) -> Response {
    later_post(form, request, Later::Resend).await
}

/// SEC-019, SEC-042, SEC-048, D-3641: a later step's `POST`, as [`step_post`]: the cross-site or
/// wrong-CSRF 403 with its event, the bare 403 for a missing or bad binding cookie, then
/// [`Form::later`] for the cookie's sign-up and the client IP (SEC-040).
async fn later_post(form: Arc<Form>, request: Request, later: Later) -> Response {
    let now = Timestamp::now();
    let ip = security_event_client(&request);
    let (fields, jar) = match same_site(&form.pool, request, later.path(), now).await {
        Ok(posted) => posted,
        Err(refused) => return *refused,
    };
    let Some(business) = bound_signup(&jar, &form.secret) else {
        return not_now("binding");
    };
    let Ok(mut db) = form.pool.acquire().await else {
        return failure("database");
    };
    let posted = Posted(&fields);
    form.later(&mut db, (business, later), &posted, (ip, now))
        .await
}

impl Form {
    /// BIZ-016, BIZ-058, D-3641, D-3779: the business must be `Pending` or `NeedsReview`, its
    /// identity code done and its email not yet verified (`signup_progress`), else the bare 403.
    /// The service area is taken once; the email steps only once it is saved (else the 403).
    async fn later(
        &self,
        db: &mut PgConnection,
        (business, later): (Uuid, Later),
        posted: &Posted<'_>,
        (ip, now): (IpAddr, Timestamp),
    ) -> Response {
        match stored_business(db, business).await {
            Ok(b) if b.status.takes_identity_code() => {}
            Ok(_) => return not_now("status"),
            Err(_) => return failure("business"),
        }
        match signup_progress(db, business).await {
            Ok(p) if p.code_done() && !p.email_verified() => {}
            Ok(_) => return not_now("order"),
            Err(_) => return failure("sign-up progress"),
        }
        let Ok(area) = service_area(db, business).await else {
            return failure("service area");
        };
        let csrf = posted.get("csrf");
        match (later, area.is_some()) {
            (Later::Area, false) => self.area(db, business, posted, (ip, now)).await,
            (Later::Area, true) => shown(self.page(Screen::EmailCode(None), csrf)),
            (Later::Check | Later::Resend, false) => not_now("no service area"),
            (Later::Resend, true) => self.email_code(db, business, (ip, now), csrf).await,
            (Later::Check, true) => {
                let code = posted.get("code").trim();
                self.email_checked(db, business, (code, csrf), now).await
            }
        }
    }

    /// BIZ-058, BIZ-049, A11Y-030: the base ZIP and radius parsed and saved, then the first
    /// email code; refused fields show the screen again with what was typed.
    async fn area(
        &self,
        db: &mut PgConnection,
        business: Uuid,
        posted: &Posted<'_>,
        (ip, now): (IpAddr, Timestamp),
    ) -> Response {
        let (base_zip, radius, csrf) = (
            posted.get("base_zip"),
            posted.get("radius"),
            posted.get("csrf"),
        );
        let area = match ServiceArea::parse(base_zip, radius) {
            Ok(area) => area,
            Err(problems) => {
                let entered = AreaEntered {
                    base_zip,
                    radius,
                    problems: &problems,
                    carried: &[],
                };
                return shown(self.page(Screen::ServiceArea(&entered), csrf));
            }
        };
        let by = (Actor::Business(business), now);
        if save_service_area(db, business, &area, by).await.is_err() {
            return failure("service area save");
        }
        self.email_code(db, business, (ip, now), csrf).await
    }

    /// BIZ-040, SEC-014: one email code send ([`Form::send_email_code`]), then the email code
    /// screen, the same whether the send was admitted or refused.
    async fn email_code(
        &self,
        db: &mut PgConnection,
        business: Uuid,
        (ip, now): (IpAddr, Timestamp),
        csrf: &str,
    ) -> Response {
        match self.send_email_code(db, business, (ip, now)).await {
            Ok(()) => shown(self.page(Screen::EmailCode(None), csrf)),
            Err(()) => failure("email code send"),
        }
    }

    /// SEC-014, SEC-015, SEC-025, BIZ-040: admitted first under `CODE_SENDS_PER_IP` then
    /// [`SIGNUP_EMAIL_CODES_PER_BUSINESS`] (a refusal or counter error opens and sends nothing,
    /// so the earlier code still works); then `start_signup_check` opens the check and the code
    /// goes to the sign-up mailbox alone. The code is never logged or kept.
    async fn send_email_code(
        &self,
        db: &mut PgConnection,
        business: Uuid,
        (ip, now): (IpAddr, Timestamp),
    ) -> Result<(), ()> {
        let checks = [
            (CODE_SENDS_PER_IP, Key::Ip(ip)),
            (SIGNUP_EMAIL_CODES_PER_BUSINESS, Key::Record(business)),
        ];
        if !matches!(
            admit(&self.pool, &checks, now).await,
            Ok(Admission::Admitted)
        ) {
            tracing::info!("sign-up email code send refused");
            return Ok(());
        }
        let secret = &self.start.id_secret;
        let opened = start_signup_check(db, secret, business, now).await;
        let Some(code) = opened.map_err(|_| ())? else {
            return Ok(());
        };
        let mailbox = signup_mailbox(db, business).await.map_err(|_| ())?;
        let code = SecretString::from(code);
        self.start.mail.0.code(db, (mailbox, &code), now).await
    }

    /// BIZ-040, BIZ-016, A11Y-030: the typed code through `confirm_mailbox`; a wrong or expired
    /// code (`confirm` answers only taken or not) shows `CodeProblem::Refused` and changes
    /// nothing else; a taken one leaves the business `Active` (`receives_cards`,
    /// [`Form::activated`]) or waiting in `NeedsReview` (`Screen::Review`, the cookie kept).
    async fn email_checked(
        &self,
        db: &mut PgConnection,
        business: Uuid,
        (code, csrf): (&str, &str),
        now: Timestamp,
    ) -> Response {
        let Ok(mailbox) = signup_mailbox(db, business).await else {
            return failure("sign-up mailbox");
        };
        let secret = &self.start.id_secret;
        match confirm_mailbox(db, secret, (business, mailbox), code, now).await {
            Ok(true) => {}
            Ok(false) => {
                let refused = Screen::EmailCode(Some(CodeProblem::Refused));
                return shown(self.page(refused, csrf));
            }
            Err(_) => return failure("email code check"),
        }
        let Ok(status) = stored_business(db, business).await.map(|b| b.status) else {
            return failure("business");
        };
        if status.receives_cards() {
            self.activated(db, (business, mailbox), csrf, now).await
        } else if status.takes_identity_code() {
            shown(self.page(Screen::Review, csrf))
        } else {
            failure("status after email")
        }
    }

    /// BIZ-016, SEC-004, D-3743: the owner's sign-in link to the sign-up mailbox (a failed send
    /// is logged once; the owner can ask for one at sign-in), the done screen and the cleared
    /// [`SIGNUP_COOKIE`].
    async fn activated(
        &self,
        db: &mut PgConnection,
        (business, mailbox): (Uuid, Uuid),
        csrf: &str,
        now: Timestamp,
    ) -> Response {
        let to = SignInAddress { business, mailbox };
        if self.start.mail.0.link(db, to, now).await.is_err() {
            tracing::warn!("sign-up sign-in link not sent");
        }
        let Some(facts) = done(db, business).await else {
            return failure("done facts");
        };
        let mut response = shown(self.page(Screen::Done(&facts), csrf));
        let cleared =
            format!("{SIGNUP_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0");
        if let Ok(cleared) = HeaderValue::from_str(&cleared) {
            response.headers_mut().append(SET_COOKIE, cleared);
        }
        response
    }
}

/// BIZ-031: the done screen's facts for an active business, in its first sign-up trade by the
/// catalog's name; `None` when a read fails.
async fn done(db: &mut PgConnection, business: Uuid) -> Option<DoneFacts> {
    let joined = join_facts(db, business).await.ok()??;
    let code = joined.trades().first()?.clone();
    let name = catalog_category(db, &code).await.ok()??.name;
    done_facts(db, business, (&code, &name), (false, true))
        .await
        .ok()
}

/// BIZ-026, PRIV-016: the masked record contact `step` goes to; a letter's shows the record
/// address's town and ZIP, the part after its last comma in licensing's `street, town zip`.
fn masked_for(found: &RecordFound, step: Step) -> Option<MaskedContact> {
    let address = found.seen().address.map(ExposeSecret::expose_secret);
    let place = address.and_then(|a| a.rsplit_once(", "));
    let place = place.and_then(|(_, p)| p.rsplit_once(' '));
    MaskedContact::for_step(found.contacts(), step, place.unwrap_or_default())
}

/// SEC-034: a screen's page, `no-store`; a page that failed to render is the bare failure.
fn shown(html: Option<String>) -> Response {
    html.map_or_else(
        || failure("sign-up template"),
        |h| (no_store(), Html(h)).into_response(),
    )
}

/// LIC-008: an unreadable or unbound record changes nothing: 503 with `Retry-After`.
fn try_again() -> Response {
    let wait = [(RETRY_AFTER, HeaderValue::from_static(TRY_AGAIN_SECS))];
    (StatusCode::SERVICE_UNAVAILABLE, no_store(), wait).into_response()
}

/// SEC-019, D-3641: a step refused for its binding or the business's status: the bare 403,
/// `no-store`, nothing changed or sent, logged once by reason without values.
fn not_now(why: &'static str) -> Response {
    tracing::info!(why, "sign-up step refused");
    (StatusCode::FORBIDDEN, no_store()).into_response()
}

/// D-2943, D-2711, D-2712, LIC-015, LIC-016, LIC-047: the screened names, a pest business's
/// complete attestation, the `btr_number` for a home-inspection or alarm category (the BTR's
/// licenses; never for septic pumping, whose ADEQ license the form has no field for, D-3726) and
/// the ROC number bound once by `check_license` (its result, or none before any ROC list or with
/// no number). An incomplete attestation or a blank agency number records nothing for that agency.
async fn licenses(
    db: &mut PgConnection,
    business: Uuid,
    posted: &Posted<'_>,
) -> Result<Option<DailyCheck>, ()> {
    let names = (posted.get("business_name"), posted.given("trade_name"));
    let city = posted.given("city_license_number");
    save_business_name(db, business, names, city)
        .await
        .map_err(|_| ())?;
    let chosen = posted.all("category");
    let actor = Actor::Business(business);
    let ticks = posted.all("pest_category");
    let pest = PestAttestation::new(
        posted.get("pest_license") == "yes",
        posted.get("pest_number"),
        ticks.contains(&"wood_destroying"),
        ticks.contains(&"weed_control"),
    );
    if let (true, Ok(pest)) = (chosen.contains(&Agency::PestManagement.category()), pest) {
        record_pest_attestation(db, business, &pest, actor)
            .await
            .map_err(|_| ())?;
    }
    let agencies = chosen.iter().filter_map(|c| Agency::for_category(c));
    let btr = |a: &Agency| matches!(a, Agency::HomeInspector | Agency::AlarmBusiness);
    let agencies: Vec<Agency> = agencies.filter(btr).collect();
    if let Some(number) = posted.given("btr_number") {
        for agency in agencies {
            let recorded = record_agency_license(db, business, (agency, number), actor).await;
            recorded.map_err(|_| ())?;
        }
    }
    let Some(roc) = posted.given("roc_number") else {
        return Ok(None);
    };
    match check_license(db, business, roc).await {
        Ok(check) => Ok(Some(check)),
        Err(ImportError::NoList) => Ok(None),
        Err(_) => Err(()),
    }
}

/// CON-019, BIZ-001, BIZ-019, BIZ-053, D-3725: the form's refusals that `submit_signup` would give
/// after the mailbox is minted (no channel, a text box with no phone, a category or source code
/// `waitlisted` refuses), checked first so a refused form leaves no `privacy.business_mailbox`
/// row behind. The mailbox is checked with a placeholder; the caller sets the minted one.
fn checked_form(
    posted: &Posted<'_>,
    map: &ClassMap,
    chosen: &[ConsentChannel],
    has_phone: bool,
) -> Result<SignUpForm, SignupRefusal> {
    if chosen.is_empty() {
        return Err(SignupRefusal::NoConsent);
    }
    if chosen.contains(&ConsentChannel::Text) && !has_phone {
        return Err(SignupRefusal::NoTextPhone);
    }
    let categories = posted.all("category").into_iter().map(str::to_owned);
    let form = SignUpForm {
        categories: categories.collect(),
        mailbox_id: Some(Uuid::nil()),
        heard_from: None,
        g_code: None,
    };
    waitlisted(map, &form).map_err(SignupRefusal::Signup)?;
    Ok(form)
}

/// D-2943: the field a refused name is shown beside.
const fn name_field(refused: NameRefusal) -> &'static str {
    match refused {
        NameRefusal::CityLicense | NameRefusal::CityLicenseScreened(_) => "city_license_number",
        NameRefusal::Missing | NameRefusal::TooLong | NameRefusal::Screened(_) => "business_name",
    }
}

/// SEC-019, D-3641: the `Set-Cookie` value binding this browser to `signup`: its ID, a fresh
/// 128-bit salt and the `link_token` MAC over both; `None` when no random salt could be drawn.
fn binding_cookie(secret: &IdSecret, signup: Uuid) -> Option<HeaderValue> {
    let salt = LinkSalt::generate().ok()?;
    let mac = link_token(secret, LinkKind::Action, signup, &salt);
    let salt: String = salt.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let value = format!(
        "{SIGNUP_COOKIE}={signup}.{salt}.{}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={SIGNUP_COOKIE_SECS}",
        mac.as_str()
    );
    HeaderValue::from_str(&value).ok()
}

/// SEC-019, D-3641: the sign-up the request's [`SIGNUP_COOKIE`] names, only when its MAC is the
/// one `secret` makes over its ID and salt, compared in constant time; any other shape is `None`.
#[must_use]
pub fn bound_signup(jar: &CookieJar, secret: &IdSecret) -> Option<Uuid> {
    let cookie = jar.get(SIGNUP_COOKIE)?;
    let mut parts = cookie.value().split('.');
    let (id, salt, mac) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let id = Uuid::try_parse(id).ok()?;
    let salt = LinkSalt::from_bytes(&unhex(salt)?).ok()?;
    let want = link_token(secret, LinkKind::Action, id, &salt);
    let (want, got) = (Sha256::digest(want.as_str()), Sha256::digest(mac));
    let differ = want
        .iter()
        .zip(got.iter())
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b));
    (differ == 0).then_some(id)
}

/// Lower-case or upper-case hex to bytes; `None` for an odd length or a non-hex digit.
fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let pairs = (0..text.len()).step_by(2);
    pairs
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// D-2954: the license records, or every read unreadable when none are configured (dev and CI).
struct Records<'a>(Option<&'a LicenseLists>);

impl LicenseRecords for Records<'_> {
    async fn lookup_record(&self, list: &str, number: &str) -> Result<Vec<u8>, AdapterError> {
        match self.0 {
            Some(lists) => lists.lookup_record(list, number).await,
            None => Err(AdapterError::NotSent),
        }
    }
}

/// BIZ-001: the Stripe customer through the client, or not made (dev and CI with no client), so
/// `customer_ready` stays false. The sign-up makes no other Stripe call; the rest refuse.
struct Customers<'a>(Option<&'a StripePayments>);

impl PaymentsAdapter for Customers<'_> {
    async fn create_customer(
        &self,
        idempotency: &IdempotencyKey,
        business_id: &str,
    ) -> Result<String, AdapterError> {
        match self.0 {
            Some(stripe) => stripe.create_customer(idempotency, business_id).await,
            None => Err(AdapterError::NotSent),
        }
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// BIZ-042, D-3676: the inbox check, served after activation behind the business session; a path
/// under `/b/signup` outside D-3641's nine open step paths, so it keeps the Business door.
pub const INBOX_CHECK_ROUTE: &str = "/b/signup/inbox";

/// The inbox check's state: the pool and the shared web security its CSRF check records through.
#[derive(Clone)]
struct Inbox {
    pool: PgPool,
    security: WebSecurity,
}

impl FromRef<Inbox> for WebSecurity {
    fn from_ref(inbox: &Inbox) -> Self {
        inbox.security.clone()
    }
}

/// The mail app the page's own `GET` form sends back (BIZ-042).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AppQuery {
    app: Option<String>,
}

/// The inbox check's posted form: the CSRF token, the mail app and either the send step or an
/// answer (`email_check.html`'s two forms, which post to their own path).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxForm {
    csrf: String,
    app: Option<String>,
    step: Option<String>,
    answer: Option<String>,
}

impl CsrfField for InboxForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// SEC-034, SEC-042, D-3676: `GET` and `POST` [`INBOX_CHECK_ROUTE`] over `pool`, every answer
/// `no-store` and every cross-site `POST` refused with its security event.
fn inbox_routes(pool: PgPool) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), clock);
    let state = Inbox {
        pool,
        security: security.clone(),
    };
    Router::new()
        .route(INBOX_CHECK_ROUTE, get(inbox_page).post(inbox_post))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store_layer))
}

/// BIZ-042, SEC-004, D-3676: `GET` [`INBOX_CHECK_ROUTE`], the signed-in person's own inbox check
/// with the steps for the mail app `?app=` names; it changes nothing. No business person gets
/// [`signed_out`].
async fn inbox_page(
    State(inbox): State<Inbox>,
    jar: CookieJar,
    Query(query): Query<AppQuery>,
) -> Response {
    let by = match acting(&inbox.pool, &jar).await {
        Ok(by) => by,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = inbox.pool.acquire().await else {
        return failure("connection");
    };
    let Ok(check) = inbox_facts(&mut db, by).await else {
        return failure("inbox check");
    };
    let app = MailApp::from_code(query.app.as_deref().unwrap_or_default());
    let csrf = csrf_token(&jar).unwrap_or_default();
    shown(email_check_page(check, app, &csrf).ok())
}

/// BIZ-042, SEC-042, D-462, D-3676: `POST` [`INBOX_CHECK_ROUTE`] after the CSRF check: sends the
/// sample card (`step=send_sample`) or stores the person's answer, in one transaction, then
/// shows the page again (303); anything else is a 422 that changes nothing. There is no sample
/// pick step here (D-462).
async fn inbox_post(
    State(inbox): State<Inbox>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<InboxForm>,
) -> Response {
    let by = match acting(&inbox.pool, &jar).await {
        Ok(by) => by,
        Err(answer) => return *answer,
    };
    let answer = form.answer.as_deref().map(InboxAnswer::from_code);
    let (now, app) = (
        Timestamp::now(),
        MailApp::from_code(form.app.as_deref().unwrap_or_default()),
    );
    let Ok(mut tx) = inbox.pool.begin().await else {
        return failure("connection");
    };
    let done = match (form.step.as_deref(), answer) {
        (Some("send_sample"), None) => send_sample_card(&mut tx, by, now).await,
        (None, Some(Some(answer))) => record_inbox_answer(&mut tx, by, answer, now).await,
        _ => return (StatusCode::UNPROCESSABLE_ENTITY, no_store()).into_response(),
    };
    if done.is_err() || tx.commit().await.is_err() {
        return failure("inbox check");
    }
    let back = format!("{INBOX_CHECK_ROUTE}?app={}", app.code());
    match HeaderValue::from_str(&back) {
        Ok(to) => (StatusCode::SEE_OTHER, no_store(), [(LOCATION, to)]).into_response(),
        Err(_) => failure("inbox location"),
    }
}

/// SEC-004, SEC-006, SEC-024: the signed-in business person as `Acting`, else the answer to send.
async fn acting(pool: &PgPool, jar: &CookieJar) -> Result<Acting, Box<Response>> {
    let Ok(mut db) = pool.acquire().await else {
        return Err(Box::new(failure("connection")));
    };
    match business_principal(&mut db, jar, Timestamp::now()).await {
        Ok(Some(who)) => Ok(Acting {
            business: who.business(),
            member: who.member(),
        }),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failure("business principal"))),
    }
}

/// SEC-004: the Business door's refusal for no business person: 401, a sign-in link, no detail.
fn signed_out() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Sign in</title></head><body><main>\
         <h1>Sign in to your business account</h1><p><a href=\"{SIGNIN_ROUTE}\">Sign in</a> to \
         see this page.</p></main></body></html>"
    );
    (StatusCode::UNAUTHORIZED, no_store(), Html(page)).into_response()
}

/// SEC-034: every sign-up answer is `Cache-Control: no-store`.
fn no_store() -> [(axum::http::HeaderName, HeaderValue); 1] {
    [(CACHE_CONTROL, HeaderValue::from_static("no-store"))]
}

fn failure(what: &'static str) -> Response {
    tracing::warn!(what, "business sign-up step failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
