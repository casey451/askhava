//! Shared pieces of every adapter: the derived idempotency key, the adapter
//! error, the Twilio-shaped HTTP client that the texting, voice and Verify
//! adapters share, the JSON or form client of the other vendors, and the guard
//! for fetching a URL a person or vendor supplied (SEC-018, SEC-032, SEC-041).
//!
//! There is one client per vendor and no fake code path: stand-in, sandbox and
//! live differ only by the base URLs and credentials (D-453).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use sha2::Sha256;
use thiserror::Error;

use crate::config::TwilioConfig;
use crate::sms::TwilioSms;
use crate::verify::TwilioVerify;
use crate::voice::TwilioVoice;

/// The key every outside action carries (SEC-018): HMAC-SHA256 under the
/// caller's `ID_SECRET` over the action name and the IDs the action is about,
/// so a retry, or a replay after a restore, reproduces it, and it holds no
/// personal data (`docs/brains/rust-production.md` R13). Built only by
/// [`IdempotencyKey::derive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Derives the key for `action` (for example `sms.send`) on the record IDs
    /// in `ids` (SEC-018). Pass record IDs, never phone numbers or text.
    ///
    /// # Panics
    ///
    /// Never: HMAC accepts a key of any length.
    #[must_use]
    pub fn derive(secret: &SecretString, action: &str, ids: &[&str]) -> Self {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.expose_secret().as_bytes())
            .expect("HMAC accepts a key of any length");
        for part in std::iter::once(action).chain(ids.iter().copied()) {
            mac.update(&(part.len() as u64).to_be_bytes());
            mac.update(part.as_bytes());
        }
        let digest = mac.finalize().into_bytes();
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// The key as sent in the `Idempotency-Key` header: 64 lower-case hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why an adapter call failed. No variant carries a URL, a phone number or a
/// vendor message, only codes (R5, PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    /// The request was never sent: the HTTP client could not be built or the
    /// connection could not be made, so a retry cannot duplicate the action.
    #[error("vendor request was not sent")]
    NotSent,
    /// The request may have reached the vendor but no complete answer came
    /// back (timeout, dropped connection, unreadable body); the action may have
    /// happened, so the caller checks its sent log before any resend (SEC-018).
    #[error("vendor request outcome is unknown")]
    OutcomeUnknown,
    /// The vendor answered with an error status and, when it sent one, its error code.
    #[error("vendor refused the request with status {status}")]
    Vendor {
        /// The HTTP status.
        status: u16,
        /// The vendor's numeric error code (Twilio's `code`), if any.
        code: Option<i64>,
    },
    /// The vendor's success body did not have the expected shape.
    #[error("vendor response did not parse")]
    Decode,
    /// A vendor ID passed in is not of the vendor's form, so it never reaches a URL path.
    #[error("vendor ID has the wrong form")]
    InvalidId,
    /// A base URL or object name was refused before any request (AI-009, SEC-032).
    #[error("adapter setting refused")]
    BadConfig,
    /// A write-once object of that name already holds different bytes, so
    /// the write was refused rather than ignored (SEC-018, D-728).
    #[error("stored object already holds other bytes")]
    ObjectExists,
}

/// Builds the texting, voice and Verify clients from one checked config; the
/// mode in it decided the base URLs, so this is the only place a client is
/// chosen and there is no separate fake (D-453, SEC-032).
///
/// # Errors
///
/// [`AdapterError::NotSent`] when the HTTP client cannot be built.
pub fn twilio_clients(
    config: &TwilioConfig,
) -> Result<(TwilioSms, TwilioVoice, TwilioVerify), AdapterError> {
    let http = TwilioHttp::new(config)?;
    Ok((
        TwilioSms { http: http.clone() },
        TwilioVoice { http: http.clone() },
        TwilioVerify { http },
    ))
}

/// Sorts a send failure: a build or connect error means nothing left this
/// process, anything later means the vendor may have acted (SEC-018).
fn send_error(error: &reqwest::Error) -> AdapterError {
    if error.is_builder() || error.is_connect() {
        AdapterError::NotSent
    } else {
        AdapterError::OutcomeUnknown
    }
}

/// Whether `id` is a Twilio SID with the given two-letter prefix and 32 hex or
/// alphanumeric characters, so it is safe in a URL path.
pub(crate) fn is_sid(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|tail| tail.len() == 32 && tail.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Whether `id` is a vendor object ID of the form `<prefix><tail>` (Stripe's
/// `pi_…`, Lob's `ltr_…`): a non-empty tail of ASCII letters, digits and `_`,
/// at most 255 characters in all, so no dot, slash or space can reach a URL
/// path or a form (SEC-032).
pub(crate) fn is_vendor_id(id: &str, prefix: &str) -> bool {
    id.len() <= 255
        && id.strip_prefix(prefix).is_some_and(|tail| {
            !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

#[derive(serde::Deserialize)]
struct TwilioErrorBody {
    code: Option<i64>,
}

/// One HTTP client for the Twilio APIs, shared by the texting, voice and
/// Verify adapters; cheap to clone.
#[derive(Debug, Clone)]
pub(crate) struct TwilioHttp {
    client: reqwest::Client,
    pub(crate) api_base: Arc<str>,
    pub(crate) verify_base: Arc<str>,
    pub(crate) account_sid: Arc<str>,
    pub(crate) verify_service: Arc<str>,
    auth_token: Arc<SecretString>,
}

impl TwilioHttp {
    pub(crate) fn new(config: &TwilioConfig) -> Result<Self, AdapterError> {
        Ok(Self {
            client: vendor_client()?,
            api_base: config.api_base().into(),
            verify_base: config.verify_base().into(),
            account_sid: config.account_sid().into(),
            verify_service: config.verify_service().into(),
            auth_token: Arc::new(config.auth_token().clone()),
        })
    }

    /// The Messages and Calls root: `<api_base>/2010-04-01/Accounts/<sid>`.
    pub(crate) fn account_url(&self) -> String {
        format!("{}/2010-04-01/Accounts/{}", self.api_base, self.account_sid)
    }

    /// POSTs a form with Basic auth; `key` goes in `Idempotency-Key` and, for
    /// texts, in `X-Askhava-Message-Key` too, the derived key the sent log is
    /// queried by after a restore (OPS-017; SEC-018).
    pub(crate) async fn post_form<T: DeserializeOwned>(
        &self,
        url: &str,
        form: &[(&str, &str)],
        key: Option<&IdempotencyKey>,
    ) -> Result<T, AdapterError> {
        let mut request = self
            .client
            .post(url)
            .basic_auth(&*self.account_sid, Some(self.auth_token.expose_secret()))
            .form(form);
        if let Some(key) = key {
            request = request
                .header("Idempotency-Key", key.as_str())
                .header("X-Askhava-Message-Key", key.as_str());
        }
        let (status, bytes) = exchange(request).await?;
        if !status.is_success() {
            let code = serde_json::from_slice::<TwilioErrorBody>(&bytes)
                .ok()
                .and_then(|body| body.code);
            return Err(AdapterError::Vendor {
                status: status.as_u16(),
                code,
            });
        }
        serde_json::from_slice(&bytes).map_err(|_| AdapterError::Decode)
    }
}

/// The vendor HTTP client: a 15-second timeout, no redirects and no proxy
/// from the environment, so a credential never goes to another host (SEC-032).
fn vendor_client() -> Result<reqwest::Client, AdapterError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| AdapterError::NotSent)
}

/// Sends `request` and reads the whole answer (SEC-018 error sorting).
async fn exchange(
    request: reqwest::RequestBuilder,
) -> Result<(reqwest::StatusCode, Vec<u8>), AdapterError> {
    let response = request.send().await.map_err(|error| send_error(&error))?;
    let status = response.status();
    let bytes = response.bytes().await;
    Ok((
        status,
        bytes.map_err(|_| AdapterError::OutcomeUnknown)?.to_vec(),
    ))
}

/// How a non-Twilio vendor client proves who it is; the secret never reaches
/// `Debug` (R6).
#[derive(Debug, Clone)]
pub(crate) enum Auth {
    /// No credentials (public license lists).
    None,
    /// `Authorization: Bearer` (Stripe, Resend).
    Bearer(Arc<SecretString>),
    /// Basic auth with the key as user and no password (Lob).
    BasicUser(Arc<SecretString>),
    /// A named header (Claude's `x-api-key`).
    Header(&'static str, Arc<SecretString>),
}

/// One HTTP client for a JSON or form vendor API under one base URL; path
/// segments are percent-encoded one by one, so a caller's ID or address can
/// never change the path (SEC-032).
#[derive(Debug, Clone)]
pub(crate) struct VendorHttp {
    client: reqwest::Client,
    base: reqwest::Url,
    auth: Auth,
}

impl VendorHttp {
    pub(crate) fn new(base: &str, auth: Auth) -> Result<Self, AdapterError> {
        let base = reqwest::Url::parse(base).map_err(|_| AdapterError::BadConfig)?;
        if !matches!(base.scheme(), "http" | "https") || base.cannot_be_a_base() {
            return Err(AdapterError::BadConfig);
        }
        let client = vendor_client()?;
        Ok(Self { client, base, auth })
    }

    /// A request to the base URL plus `segments`, authenticated, carrying
    /// `key` in `Idempotency-Key` when the call acts outside (SEC-018).
    pub(crate) fn request(
        &self,
        method: reqwest::Method,
        segments: &[&str],
        key: Option<&IdempotencyKey>,
    ) -> reqwest::RequestBuilder {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        let mut request = self.client.request(method, url);
        request = match &self.auth {
            Auth::None => request,
            Auth::Bearer(secret) => request.bearer_auth(secret.expose_secret()),
            Auth::BasicUser(secret) => request.basic_auth(secret.expose_secret(), None::<&str>),
            Auth::Header(name, secret) => request.header(*name, secret.expose_secret()),
        };
        match key {
            Some(key) => request.header("Idempotency-Key", key.as_str()),
            None => request,
        }
    }

    /// Sends and returns the raw success body; an error status keeps only the
    /// status (PRIV-016).
    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Vec<u8>, AdapterError> {
        let (status, bytes) = exchange(request).await?;
        if !status.is_success() {
            let status = status.as_u16();
            return Err(AdapterError::Vendor { status, code: None });
        }
        Ok(bytes)
    }

    /// Sends and parses the success body as `T`.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, AdapterError> {
        let bytes = self.send(request).await?;
        serde_json::from_slice(&bytes).map_err(|_| AdapterError::Decode)
    }
}

/// Whether `name` is safe as one file or object name: ASCII letters, digits,
/// `_`, `-` and `.`, not starting with a dot, so it cannot climb out of a directory.
pub(crate) fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

/// The hosts Twilio serves MMS media from: the API, which redirects, and its
/// media CDN (SEC-041). A listed host also allows its subdomains.
pub const TWILIO_MEDIA_HOSTS: [&str; 2] = ["api.twilio.com", "twiliocdn.com"];
/// The most redirects one guarded fetch follows, each re-checked (SEC-041).
pub const MAX_REDIRECTS: usize = 3;

/// The rules for one server-side fetch of a URL a person supplied (a business
/// review link) or a vendor supplied (MMS media): the allowed hosts, a size cap
/// and a timeout (SEC-041). Only `https` on port 443 is fetched, and never a
/// URL carrying a user name or password.
#[derive(Debug, Clone)]
pub struct FetchPolicy {
    hosts: Vec<String>,
    max_bytes: usize,
    timeout: Duration,
    loopback_for_tests: bool,
}

impl FetchPolicy {
    /// A policy allowing exactly `hosts` (and their subdomains) (SEC-041).
    /// An entry that is empty, starts or ends with a dot, or has no dot is
    /// dropped, so a bad entry can never match every host (it fails closed).
    /// `timeout` bounds the whole fetch, every hop and redirect included.
    #[must_use]
    pub fn new(hosts: &[&str], max_bytes: usize, timeout: Duration) -> Self {
        let hosts = hosts
            .iter()
            .filter(|host| host.contains('.') && !host.starts_with('.') && !host.ends_with('.'))
            .map(|host| host.to_ascii_lowercase())
            .collect();
        let loopback_for_tests = false;
        Self {
            hosts,
            max_bytes,
            timeout,
            loopback_for_tests,
        }
    }

    /// MMS media: Twilio's hosts only, 10 MB, 20 seconds (SEC-041).
    #[must_use]
    pub fn twilio_media() -> Self {
        Self::new(
            &TWILIO_MEDIA_HOSTS,
            10 * 1024 * 1024,
            Duration::from_secs(20),
        )
    }

    /// Tests only: also `http`, any port and loopback addresses, so a local
    /// server can play the remote host; private, link-local and metadata stay refused.
    #[cfg(test)]
    pub(crate) fn with_loopback_for_tests(mut self) -> Self {
        self.loopback_for_tests = true;
        self
    }

    /// The lower-case host of `url` when its scheme, port, userinfo and host
    /// pass the policy (SEC-041).
    fn host_allowed(&self, url: &reqwest::Url) -> Option<String> {
        let scheme_ok = (url.scheme() == "https" && url.port_or_known_default() == Some(443))
            || (self.loopback_for_tests && matches!(url.scheme(), "http" | "https"));
        let no_userinfo = url.username().is_empty() && url.password().is_none();
        let host = url
            .host_str()?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let host = host.to_ascii_lowercase();
        let listed = self.hosts.iter().any(|allowed| {
            host == *allowed
                || host
                    .strip_suffix(allowed.as_str())
                    .is_some_and(|sub| sub.ends_with('.'))
        });
        (scheme_ok && no_userinfo && listed).then_some(host)
    }

    fn address_allowed(&self, ip: IpAddr) -> bool {
        is_public_address(ip) || (self.loopback_for_tests && ip.is_loopback())
    }
}

/// Why a guarded fetch stopped. No variant carries the URL (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FetchError {
    /// The URL did not parse, or a redirect had no usable `Location`.
    #[error("URL is not usable")]
    BadUrl,
    /// The scheme, port or host is not allowed, or the URL carries a user
    /// name or password (SEC-041).
    #[error("host is not allowed")]
    HostNotAllowed,
    /// The host resolved to a private, loopback, link-local or metadata address (SEC-041).
    #[error("address is not public")]
    AddressRefused,
    /// More than [`MAX_REDIRECTS`] redirects.
    #[error("too many redirects")]
    TooManyRedirects,
    /// The body passed the size cap; reading stopped there (SEC-041).
    #[error("body is over the size cap")]
    TooLarge,
    /// The host did not answer in time (SEC-041).
    #[error("fetch timed out")]
    TimedOut,
    /// The host answered with this non-success status.
    #[error("fetch answered status {0}")]
    Status(u16),
    /// Resolution or the connection failed.
    #[error("fetch failed")]
    Failed,
}

/// Fetches `url` under `policy` (SEC-041): the whole fetch, every lookup,
/// hop and redirect included, must finish within the policy's timeout,
/// every hop's host must be allowed, every address it
/// resolves to must be public, the connection goes only to
/// those checked addresses (no second lookup), at most [`MAX_REDIRECTS`]
/// redirects are followed and each is checked the same way, and the body is
/// read only up to the cap.
///
/// # Errors
///
/// A [`FetchError`] naming the rule that stopped it.
pub async fn fetch_untrusted(policy: &FetchPolicy, url: &str) -> Result<Vec<u8>, FetchError> {
    let url = reqwest::Url::parse(url).map_err(|_| FetchError::BadUrl)?;
    tokio::time::timeout(policy.timeout, follow_hops(policy, url))
        .await
        .map_err(|_| FetchError::TimedOut)?
}

/// The redirect loop of [`fetch_untrusted`], run under its overall timeout.
async fn follow_hops(policy: &FetchPolicy, mut url: reqwest::Url) -> Result<Vec<u8>, FetchError> {
    for _ in 0..=MAX_REDIRECTS {
        let response = fetch_hop(policy, &url).await?;
        if !response.status().is_redirection() {
            return read_capped(policy, response).await;
        }
        let location = response.headers().get(reqwest::header::LOCATION);
        let location = location
            .and_then(|value| value.to_str().ok())
            .ok_or(FetchError::BadUrl)?;
        url = url.join(location).map_err(|_| FetchError::BadUrl)?;
    }
    Err(FetchError::TooManyRedirects)
}

async fn fetch_hop(
    policy: &FetchPolicy,
    url: &reqwest::Url,
) -> Result<reqwest::Response, FetchError> {
    let host = policy.host_allowed(url).ok_or(FetchError::HostNotAllowed)?;
    let port = url.port_or_known_default().ok_or(FetchError::BadUrl)?;
    let lookup = tokio::net::lookup_host((host.as_str(), port));
    let found = tokio::time::timeout(policy.timeout, lookup).await;
    let found = found.map_err(|_| FetchError::TimedOut)?;
    let addrs: Vec<SocketAddr> = found.map_err(|_| FetchError::Failed)?.collect();
    if addrs.is_empty() || !addrs.iter().all(|addr| policy.address_allowed(addr.ip())) {
        return Err(FetchError::AddressRefused);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(policy.timeout)
        .resolve_to_addrs(&host, &addrs)
        .build()
        .map_err(|_| FetchError::Failed)?;
    client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| fetch_error(&error))
}

async fn read_capped(
    policy: &FetchPolicy,
    mut response: reqwest::Response,
) -> Result<Vec<u8>, FetchError> {
    if !response.status().is_success() {
        return Err(FetchError::Status(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > policy.max_bytes as u64)
    {
        return Err(FetchError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| fetch_error(&error))?
    {
        if body.len() + chunk.len() > policy.max_bytes {
            return Err(FetchError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn fetch_error(error: &reqwest::Error) -> FetchError {
    if error.is_timeout() {
        FetchError::TimedOut
    } else {
        FetchError::Failed
    }
}

/// Whether `ip` is a public unicast address: not private, loopback,
/// link-local (169.254.0.0/16 holds the cloud metadata address), shared,
/// IETF protocol (192.0.0.0/24), benchmark, documentation, multicast, reserved
/// or unspecified, including those carried inside an IPv4-mapped or NAT64 IPv6
/// address, and not an IPv6 tunnel or translation prefix that can carry an
/// IPv4 address (6to4 2002::/16, Teredo 2001::/32, local NAT64 64:ff9b:1::/48).
/// Any other IPv6 address passes only inside global unicast 2000::/3, minus
/// documentation 2001:db8::/32, so site-local, discard, unique-local and every
/// range assigned later are refused by default (SEC-041, D-727).
#[must_use]
pub fn is_public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match (v6.to_ipv4(), v6.segments()) {
            (Some(v4), _) => is_public_v4(v4),
            (None, [0x64, 0xff9b, 0, 0, 0, 0, high, low]) => {
                is_public_v4(Ipv4Addr::from((u32::from(high) << 16) | u32::from(low)))
            }
            (None, _) => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19)))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let [first, second, ..] = ip.segments();
    first & 0xe000 == 0x2000
        && first != 0x2002
        && !(first == 0x2001 && (second == 0 || second == 0x0db8))
}
