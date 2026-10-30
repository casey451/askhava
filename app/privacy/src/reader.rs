//! The inbound reader: the only code that sees raw homeowner text (AI-004, INT-012, PRIV-026; D-093, D-801).
//!
//! `messaging` hands each deduplicated inbound text, with the keyword lists it reads from
//! `catalog`, to [`read_inbound`]: it matches the emergency stems and the sole-word opt-out, HELP
//! and START words on the raw text, remembers any name the person states (INT-012, AI-020), and
//! returns the flags plus [`ScrubbedText`]; the raw text itself never leaves. Inbound email arrives
//! only through the email provider's Svix-signed webhook into [`read_inbound_email`], which checks
//! the signature and its age, reads SPF, DKIM and DMARC, limits a sender failing any of them to an
//! opt-out, and gives the sender's per-purpose email blind index (PRIV-026). Storing the raw body
//! in `privacy.message_body` waits for that table's migration, which no task owns yet (D-801).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::Sha256;
use thiserror::Error;

use crate::models::IndexPurpose;
use crate::scrub::{Known, NameMemory, ScrubbedText, normalize, scrub};
use crate::vault::Vault;

/// How old a signed delivery may be, in seconds, before it is refused as a replay (PRIV-026).
pub const WEBHOOK_TOLERANCE_SECS: i64 = 300;

/// The keyword lists `messaging` passes in from `catalog` (D-093).
#[derive(Debug, Clone, Copy)]
pub struct InboundWords<'a> {
    /// Emergency stems, matched at the start of a word (INT-002).
    pub emergency: &'a [&'a str],
    /// Sole-word opt-out words, English and Spanish, "cancel" included (CON-008).
    pub opt_out: &'a [&'a str],
    /// Sole-word HELP words (CON-018).
    pub help: &'a [&'a str],
    /// Sole-word START words.
    pub start: &'a [&'a str],
}

/// What the raw text matched (D-093).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    /// An emergency stem appeared (INT-002).
    pub emergency: bool,
    /// The whole message is one opt-out word (CON-008).
    pub opt_out: bool,
    /// The whole message is one HELP word (CON-018).
    pub help: bool,
    /// The whole message is one START word.
    pub start: bool,
}

/// The reader's result: flags and scrubbed text, never the raw text (AI-004).
#[derive(Debug)]
pub struct Inbound {
    /// The keyword matches.
    pub flags: Flags,
    /// The text after the scrubber (AI-002, AI-003, AI-020).
    pub text: ScrubbedText,
}

/// Reads one inbound homeowner message (D-093, INT-012, AI-002, AI-003, AI-020): flags from the
/// raw text, then the scrub, remembering in `memory` any name stated for later turns.
pub fn read_inbound(
    raw: &str,
    words: &InboundWords<'_>,
    known: &Known<'_>,
    memory: &mut NameMemory,
) -> Inbound {
    Inbound {
        flags: flags(raw, words),
        text: scrub(raw, known, memory),
    }
}

fn flags(raw: &str, words: &InboundWords<'_>) -> Flags {
    let norm = normalize(raw).to_lowercase();
    let sole = norm.trim_matches(|c: char| !c.is_alphanumeric());
    let is_sole = |list: &[&str]| list.iter().any(|w| w.to_lowercase() == sole);
    let emergency = words.emergency.iter().any(|stem| {
        let stem = stem.to_lowercase();
        norm.match_indices(stem.as_str()).any(|(at, _)| {
            !norm[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        })
    });
    Flags {
        emergency,
        opt_out: is_sole(words.opt_out),
        help: is_sole(words.help),
        start: is_sole(words.start),
    }
}

/// One webhook delivery as received, before any check: the `svix-id`, `svix-timestamp` and
/// `svix-signature` headers and the body bytes (PRIV-026).
#[derive(Debug, Clone, Copy)]
pub struct SignedDelivery<'a> {
    /// `svix-id`.
    pub id: &'a str,
    /// `svix-timestamp`, seconds since the epoch.
    pub timestamp: &'a str,
    /// `svix-signature`, space-separated `v1,<base64>` entries.
    pub signature: &'a str,
    /// The raw body.
    pub body: &'a [u8],
}

/// What [`read_inbound_email`] needs besides the delivery; no `Debug`, as it holds the vault.
pub struct EmailContext<'a> {
    /// The webhook signing secret, `whsec_<base64>`.
    pub secret: &'a SecretString,
    /// The vault, for the email blind index (PRIV-024).
    pub vault: &'a Vault,
    /// The keyword lists.
    pub words: InboundWords<'a>,
    /// The scrubber's knowledge of the sender.
    pub known: Known<'a>,
    /// The time of receipt.
    pub now: Timestamp,
}

/// Why an inbound email was refused. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InboundError {
    /// The signature is missing or does not match the body.
    #[error("the inbound email signature does not match")]
    Signature,
    /// The delivery's timestamp is unreadable or more than five minutes from now (replay).
    #[error("the inbound email delivery is stale")]
    Stale,
    /// The body is not an `email.received` event of the expected shape.
    #[error("the inbound email payload is malformed")]
    Payload,
}

/// What the sender may do, from SPF, DKIM and DMARC (PRIV-026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// All three passed: the message may act on a job or change data.
    Authenticated,
    /// Any one failed: the message may only opt out, never act or change data.
    OptOutOnly,
}

/// The SPF, DKIM and DMARC results recorded with the message (PRIV-026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailAuth {
    /// SPF passed.
    pub spf: bool,
    /// DKIM passed.
    pub dkim: bool,
    /// DMARC passed.
    pub dmarc: bool,
}

/// The sender's email blind index, one digest per index key version for each purpose, derived
/// apart so a business match and a homeowner match never share a digest (PRIV-024, PRIV-026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderIndex {
    /// Digests to match against businesses' notice emails.
    pub business: Vec<(u16, [u8; 32])>,
    /// Digests to match against homeowner contacts.
    pub homeowner: Vec<(u16, [u8; 32])>,
}

/// One verified inbound email: no raw body, sender address or display name (PRIV-026).
#[derive(Debug)]
pub struct InboundEmail {
    /// Keyword matches on the body.
    pub flags: Flags,
    /// The body after the scrubber.
    pub text: ScrubbedText,
    /// SPF, DKIM and DMARC.
    pub auth: EmailAuth,
    /// What the sender may do.
    pub standing: Standing,
    /// The sender's blind index.
    pub sender: SenderIndex,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    data: Received,
}

#[derive(Deserialize)]
struct Received {
    from: String,
    text: String,
    auth: Results,
}

#[derive(Deserialize)]
struct Results {
    spf: String,
    dkim: String,
    dmarc: String,
}

/// Reads one inbound email from the signed webhook (PRIV-026, D-480, D-801): refuses a bad or
/// stale signature, then returns flags, scrubbed body, SPF/DKIM/DMARC, standing and sender index.
///
/// # Errors
///
/// [`InboundError::Signature`], [`InboundError::Stale`] or [`InboundError::Payload`].
pub fn read_inbound_email(
    delivery: &SignedDelivery<'_>,
    ctx: &EmailContext<'_>,
    memory: &mut NameMemory,
) -> Result<InboundEmail, InboundError> {
    verify(delivery, ctx.secret, ctx.now)?;
    let envelope: Envelope =
        serde_json::from_slice(delivery.body).map_err(|_| InboundError::Payload)?;
    if envelope.kind != "email.received" {
        return Err(InboundError::Payload);
    }
    let Received { from, text, auth } = envelope.data;
    let pass = |r: &str| r.eq_ignore_ascii_case("pass");
    let auth = EmailAuth {
        spf: pass(&auth.spf),
        dkim: pass(&auth.dkim),
        dmarc: pass(&auth.dmarc),
    };
    let standing = match auth {
        EmailAuth {
            spf: true,
            dkim: true,
            dmarc: true,
        } => Standing::Authenticated,
        _ => Standing::OptOutOnly,
    };
    let inbound = read_inbound(&text, &ctx.words, &ctx.known, memory);
    Ok(InboundEmail {
        flags: inbound.flags,
        text: inbound.text,
        auth,
        standing,
        sender: sender_index(ctx.vault, &from),
    })
}

/// Svix: HMAC-SHA256 under the decoded `whsec_` key over `<id>.<timestamp>.<body>`, compared in
/// constant time with every `v1,` entry, and the timestamp within [`WEBHOOK_TOLERANCE_SECS`].
fn verify(
    d: &SignedDelivery<'_>,
    secret: &SecretString,
    now: Timestamp,
) -> Result<(), InboundError> {
    let sent: i64 = d.timestamp.parse().map_err(|_| InboundError::Stale)?;
    if (now.as_second() - sent).abs() > WEBHOOK_TOLERANCE_SECS {
        return Err(InboundError::Stale);
    }
    let key = secret.expose_secret().strip_prefix("whsec_");
    let key = key
        .and_then(|k| STANDARD.decode(k).ok())
        .ok_or(InboundError::Signature)?;
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(&key).map_err(|_| InboundError::Signature)?;
    mac.update(format!("{}.{}.", d.id, d.timestamp).as_bytes());
    mac.update(d.body);
    let matched = d
        .signature
        .split_whitespace()
        .filter_map(|s| s.strip_prefix("v1,"))
        .filter_map(|s| STANDARD.decode(s).ok())
        .any(|sig| mac.clone().verify_slice(&sig).is_ok());
    matched.then_some(()).ok_or(InboundError::Signature)
}

/// The sender address, lower-cased, from `Name <a@b>` or `a@b`, hashed under the email index key
/// with a purpose prefix for each match target (PRIV-024, PRIV-026).
fn sender_index(vault: &Vault, from: &str) -> SenderIndex {
    let address = from
        .rsplit_once('<')
        .map_or(from, |(_, rest)| rest.trim_end_matches('>'))
        .trim()
        .to_lowercase();
    let index =
        |purpose: &str| vault.blind_index(IndexPurpose::Email, &format!("{purpose}|{address}"));
    SenderIndex {
        business: index("business"),
        homeowner: index("homeowner"),
    }
}

/// The email blind index of `address` for matching a business (`business`) or a homeowner, as
/// [`read_inbound_email`] derives it, for the code that stores those addresses (PRIV-026).
#[must_use]
pub fn email_index(vault: &Vault, address: &str, business: bool) -> Vec<(u16, [u8; 32])> {
    let index = sender_index(vault, address);
    if business {
        index.business
    } else {
        index.homeowner
    }
}
