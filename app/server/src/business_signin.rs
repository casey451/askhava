//! The served business sign-in pages (SEC-004, SEC-014, SEC-025, SEC-026, SEC-048; T280,
//! D-1724, D-1728, D-1844, D-2206, D-3055, D-3562).
//!
//! [`ServedSignIn`] is `portals`' `SignIn` state with the two `.route` lines ([`SIGNIN_ROUTE`],
//! [`SIGNIN_LINK_ROUTE`]), built only through [`ServedSignIn::new`], which takes the
//! new-device [`DeviceCodes`], so the served link never signs a new device in on its own
//! (D-1777). [`ServedSignIn::from_settings`] builds it in `main`: `ASKHAVA_ID_SECRET` (the Verify
//! idempotency key and the notice-link key) and the Twilio Verify settings are required, so the
//! server refuses to start without them in the test and live projects; in dev and CI, with no
//! Twilio settings, the codes go through [`NoVerify`], which sends and checks nothing, so a new
//! device can never sign in there (fail closed, D-3216). `build_router` merges it through
//! `RouterParts::with_sign_in`.
//!
//! [`SignInMailer`] is the composition's `SignInMail`: an address is looked up only through the
//! vault's blind index (`privacy::api::mailboxes_of`) and `businesses::api::businesses_of_mailbox`;
//! the link's address is opened with `read_business_mailbox` under `BusinessPurpose::SignInLink`
//! on the bare connection (D-3262) and the link goes through `messaging`'s [`SignInLinkSender`],
//! never logged and never stored (SEC-026). With no vault or no sender (no Resend client is built
//! yet, D-3186) it finds no one, so nobody learns whether an address is on file.
//! [`MemberCodeAddress`] sends a member's code to that member's own first texting line
//! (`businesses::api::sign_in_lines`, `read_staff_phone`), else to that member's own address
//! (`read_business_mailbox`), both under `SignInCode`, and to no one for a removed member.
//!
//! The limits come first (D-1951, D-2206): a layer notes the client address (SEC-040) and
//! whether a `POST` carries a code; the link request then calls `admit_sign_in_link` per
//! business found (per IP alone when none is) and the code try `admit_code_try` for the member's
//! business, each before any email or Verify call. A refusal finds no one (the same "check your
//! email" screen) or fails the code step as any failure does, and calls nothing.

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, CodeChannel, IdempotencyKey, ResendEmail, TwilioConfig, VerifyAdapter,
    twilio_clients,
};
use askhava_businesses::api::{businesses_of_mailbox, sign_in_lines};
use askhava_core::api::{Clock, Environment, IdSecret, Settings, SystemClock};
use askhava_intake::api::Admission;
use askhava_messaging::api::SignInLinkSender;
use askhava_portals::api::{
    CodeAddress, CodeFuture, CodeTo, DeviceCodes, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE,
    SignIn, SignInAddress, SignInMail, admit_code_try, admit_sign_in_link, security_event_client,
    signin_open, signin_page, signin_post, signin_request,
};
use askhava_privacy::api::{
    BusinessPurpose, Vault, mailboxes_of, read_business_mailbox, read_staff_phone,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The largest sign-in form body read (an address or a code and the CSRF token).
const FORM_LIMIT: usize = 16 * 1024;

tokio::task_local! {
    /// The request's client address and whether it is a code try, for the limits (D-2206).
    static CLIENT: Client;
}

#[derive(Clone, Copy)]
struct Client {
    ip: IpAddr,
    code_try: bool,
}

/// The composition's mail side, as the routes' state names it.
type Mail<E> = SignInMailer<E>;

/// The served sign-in routes, always with the new-device code (SEC-004, D-1777, D-1844).
pub struct ServedSignIn(Router);

impl ServedSignIn {
    /// The two sign-in routes over `pool`, emailing through `mail`, asking every new device for
    /// a code through `codes`, and checking notice links with `notice` (D-1761).
    #[must_use]
    pub fn new<E>(pool: PgPool, mail: SignInMailer<E>, codes: DeviceCodes, notice: IdSecret) -> Self
    where
        E: askhava_adapters::api::EmailAdapter + Send + Sync + 'static,
    {
        let clock = Arc::clone(&mail.clock);
        let state = SignIn::new(pool, clock, Arc::new(mail))
            .with_device_codes(codes)
            .with_notice_links(notice);
        let routes: Router<SignIn<Mail<E>>> = Router::new()
            .route(SIGNIN_ROUTE, get(signin_page).post(signin_request))
            .route(SIGNIN_LINK_ROUTE, get(signin_open).post(signin_post))
            .layer(from_fn(note_client));
        Self(state.into_router(routes))
    }

    /// The part `main` passes to `build_router`: `ASKHAVA_ID_SECRET`, the Verify client and the
    /// vault from `pairs` (D-3055, D-3216).
    ///
    /// # Errors
    ///
    /// A missing or weak `ASKHAVA_ID_SECRET`; missing or bad Twilio settings in the test or live
    /// project; a bad or half-set vault key pair, or none in test or live. Never a value.
    pub fn from_settings<I>(settings: &Settings, pool: PgPool, pairs: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let pairs: Vec<(String, String)> = pairs.into_iter().collect();
        let secret = pairs
            .iter()
            .find(|(k, v)| k == "ASKHAVA_ID_SECRET" && !v.trim().is_empty())
            .map(|(_, v)| SecretString::from(v.trim().to_owned()))
            .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
        let notice = IdSecret::new(1, secret.clone()).map_err(io::Error::other)?;
        let vault = crate::vault_from(settings, pairs.clone())?.map(Arc::new);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let to = MemberCodeAddress::new(pool.clone(), Arc::clone(&clock), vault.clone());
        let codes = match TwilioConfig::from_settings(settings, pairs) {
            Ok(config) => {
                let (_, _, verify) = twilio_clients(&config).map_err(io::Error::other)?;
                DeviceCodes::new(verify, to, secret)
            }
            Err(error) => match settings.environment() {
                Environment::Dev | Environment::Ci => DeviceCodes::new(NoVerify, to, secret),
                Environment::Test | Environment::Live => return Err(io::Error::other(error)),
            },
        };
        let mail = SignInMailer::<ResendEmail>::new((pool.clone(), clock), vault, None);
        Ok(Self::new(pool, mail, codes, notice))
    }

    /// The routes, state and layers applied, ready to merge.
    pub fn into_router(self) -> Router {
        self.0
    }
}

/// Notes the client address (SEC-040) and whether a `POST` carries a non-empty `code` field.
async fn note_client(request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, FORM_LIMIT).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let code_try = parts.method == Method::POST && has_code(&bytes);
    let request = Request::from_parts(parts, Body::from(bytes));
    CLIENT
        .scope(Client { ip, code_try }, next.run(request))
        .await
}

/// Whether a form body has a `code` field with a value.
fn has_code(body: &[u8]) -> bool {
    body.split(|b| *b == b'&')
        .filter_map(|pair| pair.strip_prefix(b"code="))
        .any(|value| value.iter().any(|b| !matches!(b, b'+' | b' ')))
}

/// Whether `admitted` let the request through; an error refuses (D-1951).
fn passed<E>(admitted: Result<Admission, E>) -> bool {
    matches!(admitted, Ok(Admission::Admitted))
}

/// A Verify stand-in for dev and CI with no Twilio settings: every send and check fails, so no
/// new device signs in (D-3216).
pub struct NoVerify;

impl VerifyAdapter for NoVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn check_code(&self, _: &str, _: &str) -> Result<bool, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// The composition's `SignInMail` (SEC-004, SEC-014, SEC-025, D-1724, D-3262).
pub struct SignInMailer<E> {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    vault: Option<Arc<Vault>>,
    sender: Option<SignInLinkSender<E>>,
}

impl<E> SignInMailer<E> {
    /// Looks addresses up in `vault` and sends links through `sender`; with either missing it
    /// finds no one. The limits are counted over `pool` at `clock`'s time.
    #[must_use]
    pub fn new(
        (pool, clock): (PgPool, Arc<dyn Clock>),
        vault: Option<Arc<Vault>>,
        sender: Option<SignInLinkSender<E>>,
    ) -> Self {
        Self {
            pool,
            clock,
            vault,
            sender,
        }
    }

    /// SEC-014, D-2206: one link request from the noted client, per business in `found`, or per
    /// IP alone when `found` is empty; `false` on any refusal or error.
    async fn admitted(&self, found: &[SignInAddress]) -> bool {
        let Ok(ip) = CLIENT.try_with(|c| c.ip) else {
            return false;
        };
        let now = self.clock.now();
        let mut businesses: Vec<Uuid> = found.iter().map(|a| a.business).collect();
        businesses.sort_unstable();
        businesses.dedup();
        if businesses.is_empty() {
            return passed(admit_sign_in_link(&self.pool, (ip, None), now).await);
        }
        for business in businesses {
            if !passed(admit_sign_in_link(&self.pool, (ip, Some(business)), now).await) {
                return false;
            }
        }
        true
    }
}

impl<E: askhava_adapters::api::EmailAdapter + Send + Sync + 'static> SignInMail
    for SignInMailer<E>
{
    async fn addresses(
        &self,
        db: &mut PgConnection,
        address: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        let mut found = Vec::new();
        let mailboxes = match (&self.vault, &self.sender) {
            (Some(vault), Some(_)) => mailboxes_of(db, vault, address).await,
            _ => Ok(Vec::new()),
        };
        for mailbox in mailboxes.map_err(|_| PortalsError::Database)? {
            let of = businesses_of_mailbox(db, mailbox).await;
            let of = of.map_err(|_| PortalsError::Database)?;
            found.extend(
                of.into_iter()
                    .map(|business| SignInAddress { business, mailbox }),
            );
        }
        Ok(if self.admitted(&found).await {
            found
        } else {
            Vec::new()
        })
    }

    async fn send_link(
        &self,
        db: &mut PgConnection,
        to: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let (Some(vault), Some(sender)) = (&self.vault, &self.sender) else {
            return Ok(());
        };
        let purpose = BusinessPurpose::SignInLink;
        let address = read_business_mailbox(db, vault, purpose, to.mailbox).await;
        let address = address.map_err(|_| PortalsError::Database)?;
        let now = self.clock.now();
        let sent = sender.send(db, (to.mailbox, &address), link, now).await;
        sent.map(|_| ()).map_err(|_| PortalsError::Database)
    }
}

/// The composition's `CodeAddress`: the member's own texting line, else own address (SEC-004,
/// D-3562).
pub struct MemberCodeAddress {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    vault: Option<Arc<Vault>>,
}

impl MemberCodeAddress {
    /// Opens numbers and addresses in `vault` (none: no code goes) and counts code tries over
    /// `pool` at `clock`'s time.
    #[must_use]
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, vault: Option<Arc<Vault>>) -> Self {
        Self { pool, clock, vault }
    }

    /// SEC-004, SEC-020, D-2206, D-3562: where `member`'s code goes, after `admit_code_try` for a
    /// code try; an error (no code) for a removed member, a refusal or no vault.
    async fn find(&self, db: &mut PgConnection, member: Uuid) -> Result<CodeTo, PortalsError> {
        let lines = sign_in_lines(db, member).await;
        let lines = lines.map_err(|_| PortalsError::Database)?;
        let lines = lines.ok_or(PortalsError::Database)?;
        let client = CLIENT
            .try_with(|c| *c)
            .map_err(|_| PortalsError::Database)?;
        let now = self.clock.now();
        let at = (client.ip, Some(lines.business));
        if client.code_try && !passed(admit_code_try(&self.pool, at, now).await) {
            return Err(PortalsError::Database);
        }
        let vault = self.vault.as_deref().ok_or(PortalsError::Database)?;
        let purpose = BusinessPurpose::SignInCode;
        let to = match lines.texting_lines.first() {
            Some(line) => read_staff_phone(db, vault, purpose, *line)
                .await
                .map(CodeTo::Phone),
            None => read_business_mailbox(db, vault, purpose, lines.mailbox)
                .await
                .map(CodeTo::Email),
        };
        to.map_err(|_| PortalsError::Database)
    }
}

impl CodeAddress for MemberCodeAddress {
    fn code_to<'a>(
        &'a self,
        db: &'a mut PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        Box::pin(self.find(db, member))
    }
}
