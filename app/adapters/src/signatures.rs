//! Inbound webhook signature checks (SEC-016, SEC-017, SEC-039, SEC-049, SEC-051; D-820).
//!
//! Every Twilio, Stripe, Resend (Svix) and Lob delivery passes [`verify_webhook`] on its raw body
//! before any module sees it; only it builds a [`VerifiedWebhook`], so a module that takes one
//! cannot be handed an unchecked body. Each MAC is compared in constant time
//! (`Mac::verify_slice`); Stripe, Svix and Lob timestamps must lie within
//! [`WEBHOOK_TOLERANCE_SECS`] of now, either way. A refused delivery goes to [`refuse`], which
//! writes the one log line (vendor and reason codes, no values) and gives [`REJECTED_STATUS`], the
//! 403 the route answers. Stripe source addresses are checked against the committed copy of
//! Stripe's `ips_webhooks.json` compiled in from `data/stripe_webhook_ips.json`, never fetched.
//! Dedupe is the module's: it records [`VerifiedWebhook::event_key`] (or, for Twilio, its own
//! pair) through `askhava_core::api::Dedupe` in its `webhook_event` table (SEC-017).

use std::net::IpAddr;
use std::sync::LazyLock;

use askhava_core::api::EventKey;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha1::Sha1;
use sha2::Sha256;
use thiserror::Error;

use crate::config::AdapterMode;

/// How far a signed timestamp may be from now, in seconds: 300 old is accepted, 301 refused
/// (SEC-016, SEC-039, SEC-051).
pub const WEBHOOK_TOLERANCE_SECS: i64 = 300;

/// The status a refused delivery gets (SEC-016).
pub const REJECTED_STATUS: u16 = 403;

/// The committed copy of Stripe's published webhook addresses (SEC-049, D-492).
const STRIPE_IPS_JSON: &str = include_str!("../data/stripe_webhook_ips.json");

static STRIPE_IPS: LazyLock<Vec<IpAddr>> = LazyLock::new(|| {
    serde_json::from_str::<IpList>(STRIPE_IPS_JSON)
        .map_or_else(|_| Vec::new(), |list| list.webhooks)
});

/// The shape of Stripe's `ips_webhooks.json`.
#[derive(Deserialize)]
struct IpList {
    #[serde(rename = "WEBHOOKS")]
    webhooks: Vec<IpAddr>,
}

/// The vendor a webhook claims to come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookVendor {
    /// Twilio texting, status and voice callbacks (`X-Twilio-Signature`).
    Twilio,
    /// Stripe events (`Stripe-Signature`).
    Stripe,
    /// Resend email events, Svix-signed.
    Resend,
    /// Lob letter tracking events (`Lob-Signature`).
    Lob,
}

impl WebhookVendor {
    /// The vendor's name as logged.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Twilio => "twilio",
            Self::Stripe => "stripe",
            Self::Resend => "resend",
            Self::Lob => "lob",
        }
    }
}

/// Why a delivery was refused. No variant carries a value (PRIV-016).
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum SignatureError {
    /// A signature or timestamp header is absent or malformed.
    #[error("webhook signature header missing or malformed")]
    Missing,
    /// No signature matches the body under the secret.
    #[error("webhook signature does not match")]
    Forged,
    /// The signed timestamp is more than 300 seconds from now.
    #[error("webhook timestamp outside tolerance")]
    Stale,
    /// The source address is not allowed for this mode (SEC-049).
    #[error("webhook source address not allowed")]
    Source,
    /// A signed Stripe, Svix or Lob delivery names no usable event ID.
    #[error("webhook event ID missing or malformed")]
    NoEventId,
}

impl SignatureError {
    /// The reason as logged.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Forged => "forged",
            Self::Stale => "stale",
            Self::Source => "source",
            Self::NoEventId => "no_event_id",
        }
    }
}

/// A delivery whose signature was checked. Private fields and no constructor outside this file,
/// so holding one proves the check ran (SEC-039). `Debug` shows the vendor and key, never the
/// body, which may hold contact values (PRIV-016).
pub struct VerifiedWebhook {
    vendor: WebhookVendor,
    body: Vec<u8>,
    event_key: Option<EventKey>,
    params: Vec<(String, String)>,
}

impl std::fmt::Debug for VerifiedWebhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedWebhook")
            .field("vendor", &self.vendor)
            .field("event_key", &self.event_key)
            .finish_non_exhaustive()
    }
}

impl VerifiedWebhook {
    /// The vendor whose secret signed it.
    #[must_use]
    pub fn vendor(&self) -> WebhookVendor {
        self.vendor
    }

    /// The raw body as signed.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The event ID to dedupe on (SEC-017): Stripe's and Lob's body `id`, Svix's `svix-id`;
    /// `None` for Twilio, whose module keys on (MessageSid, MessageStatus) or (CallSid, step).
    #[must_use]
    pub fn event_key(&self) -> Option<&EventKey> {
        self.event_key.as_ref()
    }

    /// Twilio's decoded form fields; empty for the other vendors.
    #[must_use]
    pub fn params(&self) -> &[(String, String)] {
        &self.params
    }
}

/// Logs one refused delivery (vendor and reason only) and gives the status to answer (SEC-016).
#[must_use]
pub fn refuse(vendor: WebhookVendor, error: SignatureError) -> u16 {
    tracing::warn!(
        vendor = vendor.name(),
        reason = error.code(),
        "webhook refused"
    );
    REJECTED_STATUS
}

/// One delivery as received: the URL it was posted to (Twilio signs it), the raw body exactly as
/// it arrived, and its headers. No `Debug`: the body may hold contact values (PRIV-016).
#[derive(Clone, Copy)]
pub struct Delivery<'a> {
    /// The full public URL the vendor posted to, with any query string.
    pub url: &'a str,
    /// The raw body, never re-serialised.
    pub body: &'a [u8],
    /// The request headers as (name, value) pairs; names match case-insensitively.
    pub headers: &'a [(&'a str, &'a str)],
}

impl<'a> Delivery<'a> {
    fn header(&self, name: &str) -> Option<&'a str> {
        let found = self
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name));
        found.map(|&(_, value)| value)
    }
}

/// Checks one delivery under `vendor`'s scheme with its webhook `secret`, `now` in unix seconds,
/// before any module sees it (SEC-016, SEC-039, SEC-051): Twilio base64 HMAC-SHA1 over the URL and
/// the sorted form fields (`X-Twilio-Signature`); Stripe hex HMAC-SHA256 over `<t>.<body>`
/// against any `v1` of `Stripe-Signature` (several during a secret roll); Resend's Svix base64
/// HMAC-SHA256 under the decoded `whsec_` key over `<svix-id>.<svix-timestamp>.<body>`; Lob hex
/// HMAC-SHA256 over `<Lob-Signature-Timestamp>.<body>`. Stripe, Svix and Lob timestamps must be
/// within [`WEBHOOK_TOLERANCE_SECS`] of `now`, checked after the MAC.
///
/// # Errors
///
/// [`SignatureError::Missing`], [`SignatureError::Forged`], [`SignatureError::Stale`] or
/// [`SignatureError::NoEventId`].
pub fn verify_webhook(
    vendor: WebhookVendor,
    secret: &SecretString,
    delivery: Delivery<'_>,
    now: i64,
) -> Result<VerifiedWebhook, SignatureError> {
    let key = secret.expose_secret();
    let body = delivery.body;
    let event = match vendor {
        WebhookVendor::Twilio => return twilio(key, &delivery),
        WebhookVendor::Stripe => stripe(key, &delivery, now)?,
        WebhookVendor::Resend => svix(key, &delivery, now)?,
        WebhookVendor::Lob => {
            let header = delivery.header("lob-signature");
            let stamp = delivery.header("lob-signature-timestamp");
            let (Some(header), Some(stamp)) = (header, stamp) else {
                return Err(SignatureError::Missing);
            };
            let sigs: Vec<Vec<u8>> = hex_bytes(header).into_iter().collect();
            check_mac(key.as_bytes(), &[stamp.as_bytes(), b".", body], &sigs)?;
            fresh(stamp, now)?;
            json_id(body)?
        }
    };
    Ok(VerifiedWebhook {
        vendor,
        body: body.to_vec(),
        event_key: Some(event),
        params: Vec::new(),
    })
}

/// Twilio's check; the module keys a Twilio event on its own pair, so no event key here.
fn twilio(token: &str, d: &Delivery<'_>) -> Result<VerifiedWebhook, SignatureError> {
    let sig = d.header("x-twilio-signature");
    let sig = sig.and_then(|s| STANDARD.decode(s.trim()).ok());
    let sig = sig.ok_or(SignatureError::Missing)?;
    let mut params = form_pairs(d.body).ok_or(SignatureError::Missing)?;
    params.sort();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(token.as_bytes())
        .map_err(|_| SignatureError::Forged)?;
    mac.update(d.url.as_bytes());
    for (name, value) in &params {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    mac.verify_slice(&sig).map_err(|_| SignatureError::Forged)?;
    Ok(VerifiedWebhook {
        vendor: WebhookVendor::Twilio,
        body: d.body.to_vec(),
        event_key: None,
        params,
    })
}

/// Stripe's check; the event key is the body's `id`.
fn stripe(secret: &str, d: &Delivery<'_>, now: i64) -> Result<EventKey, SignatureError> {
    let header = d
        .header("stripe-signature")
        .ok_or(SignatureError::Missing)?;
    let fields = header.split(',').filter_map(|f| f.trim().split_once('='));
    let (mut stamp, mut sigs) = (None, Vec::new());
    for (name, value) in fields {
        match name {
            "t" => stamp = Some(value),
            "v1" => sigs.extend(hex_bytes(value)),
            _ => {}
        }
    }
    let stamp = stamp.ok_or(SignatureError::Missing)?;
    check_mac(secret.as_bytes(), &[stamp.as_bytes(), b".", d.body], &sigs)?;
    fresh(stamp, now)?;
    json_id(d.body)
}

/// Svix's check (Resend); the event key is `svix-id`, the message ID deduped on (SEC-051).
fn svix(secret: &str, d: &Delivery<'_>, now: i64) -> Result<EventKey, SignatureError> {
    let id = d.header("svix-id");
    let stamp = d.header("svix-timestamp");
    let header = d.header("svix-signature");
    let (Some(id), Some(stamp), Some(header)) = (id, stamp, header) else {
        return Err(SignatureError::Missing);
    };
    let key = secret.strip_prefix("whsec_");
    let key = key.and_then(|k| STANDARD.decode(k).ok());
    let key = key.ok_or(SignatureError::Forged)?;
    let sigs: Vec<Vec<u8>> = header
        .split_whitespace()
        .filter_map(|s| s.strip_prefix("v1,"))
        .filter_map(|s| STANDARD.decode(s).ok())
        .collect();
    let message = [id.as_bytes(), b".", stamp.as_bytes(), b".", d.body];
    check_mac(&key, &message, &sigs)?;
    fresh(stamp, now)?;
    EventKey::new(id).map_err(|_| SignatureError::NoEventId)
}

/// Whether a Stripe webhook may come from `client`, the address as SEC-040 takes it
/// (`askhava_core::api::ClientIpPolicy::client_ip`): in sandbox and live only an address on the
/// committed copy of Stripe's list, in stand-in mode only `stand_in_host` (SEC-049, D-492). Only
/// Stripe publishes its webhook addresses; the other vendors are checked by signature alone.
///
/// # Errors
///
/// [`SignatureError::Source`].
pub fn check_webhook_source(
    mode: AdapterMode,
    client: IpAddr,
    stand_in_host: IpAddr,
) -> Result<(), SignatureError> {
    let client = client.to_canonical();
    let allowed = match mode {
        AdapterMode::StandIn => client == stand_in_host.to_canonical(),
        AdapterMode::Sandbox | AdapterMode::Live => STRIPE_IPS.contains(&client),
    };
    allowed.then_some(()).ok_or(SignatureError::Source)
}

/// The committed Stripe webhook addresses, parsed once from the compiled-in copy; an unreadable
/// copy gives an empty list, so every Stripe address is refused (fails closed, SEC-049).
#[must_use]
pub fn webhook_source_list() -> &'static [IpAddr] {
    &STRIPE_IPS
}

/// HMAC-SHA256 under `key` over the concatenated `parts`, matched in constant time against any of
/// `sigs`.
fn check_mac(key: &[u8], parts: &[&[u8]], sigs: &[Vec<u8>]) -> Result<(), SignatureError> {
    if sigs.is_empty() {
        return Err(SignatureError::Missing);
    }
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| SignatureError::Forged)?;
    for part in parts {
        mac.update(part);
    }
    let matched = sigs.iter().any(|sig| mac.clone().verify_slice(sig).is_ok());
    matched.then_some(()).ok_or(SignatureError::Forged)
}

/// The signed unix-seconds `stamp` is within [`WEBHOOK_TOLERANCE_SECS`] of `now`.
fn fresh(stamp: &str, now: i64) -> Result<(), SignatureError> {
    let sent: i64 = stamp.trim().parse().map_err(|_| SignatureError::Missing)?;
    let gap = now.checked_sub(sent).map(i64::unsigned_abs);
    match gap {
        Some(gap) if gap <= WEBHOOK_TOLERANCE_SECS.unsigned_abs() => Ok(()),
        _ => Err(SignatureError::Stale),
    }
}

/// The top-level `id` of a JSON event body.
fn json_id(body: &[u8]) -> Result<EventKey, SignatureError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| SignatureError::NoEventId)?;
    let id = value.get("id").and_then(serde_json::Value::as_str);
    id.and_then(|id| EventKey::new(id).ok())
        .ok_or(SignatureError::NoEventId)
}

/// Lower- or upper-case hex to bytes.
fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    let text = text.trim().as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.chunks(2)
        .map(|pair| Some(hex_digit(pair[0])? << 4 | hex_digit(pair[1])?))
        .collect()
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// An `application/x-www-form-urlencoded` body as decoded pairs; `None` when it is not UTF-8 or
/// holds a bad escape.
fn form_pairs(body: &[u8]) -> Option<Vec<(String, String)>> {
    let text = std::str::from_utf8(body).ok()?;
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some((form_decode(name)?, form_decode(value)?))
        })
        .collect()
}

fn form_decode(text: &str) -> Option<String> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(b' '),
            b'%' => out.push(hex_digit(bytes.next()?)? << 4 | hex_digit(bytes.next()?)?),
            other => out.push(other),
        }
    }
    String::from_utf8(out).ok()
}
