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
//! checked by [`bound_signup`]. T460 to T462 add the later steps (BIZ-002, BIZ-019, BIZ-040,
//! BIZ-042, BIZ-058).

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, LicenseLists, LicenseRecords, PaymentIntent,
    PaymentsAdapter, StripePayments,
};
use askhava_businesses::api::{
    ConsentChannel, Flag, NameRefusal, SignUpForm, SignupError, SignupRecord, SignupSettings,
    record_match, save_business_name, screen_names, signup_record, stored_category_list,
    stored_enabled, waitlisted,
};
use askhava_core::api::{Environment, IdSecret, LinkKind, LinkSalt, Settings, link_token};
use askhava_intake::api::Admission;
use askhava_licensing::api::{
    Agency, ClassMap, DailyCheck, ImportError, PestAttestation, check_license,
    record_agency_license, record_pest_attestation,
};
use askhava_ops::api::Actor;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    ChannelPost, CookieJar, Entered, PageLanguage, Parts, SIGNUP_FORM_ROUTE, SIGNUP_ROUTE, Screen,
    SecurityEvent, SecurityEventKind, SignupConsent, SignupRefusal, admit_security_event,
    admit_sign_up, csrf_matches, csrf_token, ensure_pre_session, fee_numbers, is_cross_site,
    record_security_event, refusal_problem, sealed_signup_page, security_event_client,
    submit_signup,
};
use askhava_privacy::api::{BusinessContactError, Vault, mailbox_for};
use axum::Router;
use axum::extract::{FromRequest, Request, State};
use axum::http::header::{CACHE_CONTROL, HOST, HeaderValue, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use super::business_contacts::ContactDirectory;

/// TXT-035: business texting is off, so the form offers email notices alone and the E-SIGN box
/// names email only (BIZ-036, CON-019).
const TEXTING_ON: bool = false;

/// SEC-019, D-3641: the cookie binding a browser to its sign-up: `<sign-up ID>.<salt>.<MAC>`.
pub const SIGNUP_COOKIE: &str = "__Host-askhava-signup";

/// The binding cookie's life: 30 days, so the wait for an identity letter fits (D-3641).
const SIGNUP_COOKIE_SECS: i64 = 30 * 24 * 60 * 60;

/// LIC-008: how long a business waits before trying again after an unreadable record.
const TRY_AGAIN_SECS: &str = "30";

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
}

/// What the routes read: the pool, the sealing key, the fee box's settings and the adapters.
struct Form {
    pool: PgPool,
    secret: IdSecret,
    confirm: ConfirmSettings,
    start: StartAdapters,
}

/// The served sign-up routes (T302: the form page; T459: the form `POST`).
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
        let state = Arc::new(Form {
            pool,
            secret,
            confirm,
            start,
        });
        let routes = Router::new()
            .route(SIGNUP_ROUTE, get(form_page))
            .route(SIGNUP_FORM_ROUTE, post(start_post))
            .with_state(state);
        Self(routes)
    }

    /// The part `main` passes to `build_router`: `ASKHAVA_ID_SECRET`, the Confirm window, the
    /// vault, the Stripe client and the license records from `pairs` (D-2954, D-3055).
    ///
    /// # Errors
    ///
    /// A missing or weak `ASKHAVA_ID_SECRET`; a bad setting; in test or live a missing vault,
    /// Stripe or `ASKHAVA_LICENSE_BASE_URL` setting. Never a value.
    pub fn from_settings<I>(pool: PgPool, pairs: I) -> io::Result<Self>
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
        let start = start_adapters(raw, &pairs)?;
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
    })
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
    let headers = request.headers().clone();
    let parsed = axum::Form::<Vec<(String, String)>>::from_request(request, &()).await;
    let Ok(axum::Form(fields)) = parsed else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let posted = Posted(&fields);
    let jar = CookieJar::from_headers(&headers);
    let host = headers.get(HOST).and_then(|h| h.to_str().ok());
    let csrf = posted.get("csrf");
    if is_cross_site(&headers, host) || !csrf_matches(&jar, csrf) {
        return refuse(&form.pool, ip, &headers, now).await;
    }
    form.submit(&posted, csrf, now).await
}

/// SEC-048: the 403 for a cross-site or wrong-CSRF post, with its security event (request ID and
/// route, no actor: there is no session) while the per-minute cap admits it.
async fn refuse(pool: &PgPool, ip: IpAddr, headers: &HeaderMap, now: Timestamp) -> Response {
    let kind = SecurityEventKind::CsrfRejected;
    if admit_security_event(kind, ip, SIGNUP_FORM_ROUTE, now) {
        let request_id = headers
            .get("x-request-id")
            .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
        let event = SecurityEvent {
            kind,
            route: SIGNUP_FORM_ROUTE,
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

    /// SEC-019, D-3641: after the sign-up is stored: the binding cookie, the names and licenses,
    /// then the contact screen, all `no-store` (SEC-034).
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
            SignupRecord::Unreadable | SignupRecord::Unbound => {
                let wait = [(RETRY_AFTER, HeaderValue::from_static(TRY_AGAIN_SECS))];
                return (StatusCode::SERVICE_UNAVAILABLE, no_store(), wait).into_response();
            }
        };
        html.map_or_else(
            || failure("sign-up template"),
            |h| (no_store(), Html(h)).into_response(),
        )
    }
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

/// SEC-034: every sign-up answer is `Cache-Control: no-store`.
fn no_store() -> [(axum::http::HeaderName, HeaderValue); 1] {
    [(CACHE_CONTROL, HeaderValue::from_static("no-store"))]
}

fn failure(what: &'static str) -> Response {
    tracing::warn!(what, "business sign-up step failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
