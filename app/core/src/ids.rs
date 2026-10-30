//! Derived IDs and link tokens (CARD-017, PICK-028, SEC-037; `docs/architecture.md` section 5).
//!
//! A card, quote or pick ID is HMAC-SHA256 under the versioned `ID_SECRET` over a kind label and
//! the record IDs it identifies, cut to 128 bits: it cannot be guessed without the secret, and a
//! replay recreates it exactly. A link token that reaches a person (quote page, action and photo
//! links) adds a per-record 128-bit random salt stored with the record, so one link can be
//! re-issued (new salt) or revoked (row state), and the secret plus the record ID alone never
//! reproduce it. Only a token's SHA256 hash is stored; the token itself is never logged.

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use rand::TryRng;
use rand::rngs::SysRng;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use thiserror::Error;

/// Why an ID secret, salt or token was refused. No variant carries the value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    /// The secret is shorter than [`IdSecret::MIN_LEN`] bytes or its version is 0 (SEC-037).
    #[error("the ID secret is too short or has no version")]
    WeakSecret,
    /// A stored salt is not 16 bytes.
    #[error("a link salt must be 16 bytes")]
    BadSalt,
    /// A presented token is not 64 lower-case hex digits.
    #[error("a link token has the wrong shape")]
    BadToken,
    /// The operating system's random source failed, so no salt could be made.
    #[error("the random source failed")]
    NoRandom,
}

/// The versioned server secret every derived ID and link token is keyed by (SEC-037). Its `Debug`
/// shows the version only.
pub struct IdSecret {
    version: u16,
    key: SecretString,
}

impl IdSecret {
    /// The shortest secret accepted, in bytes (256 bits of key).
    pub const MIN_LEN: usize = 32;

    /// Wraps `key` as version `version` of `ID_SECRET` (SEC-037).
    ///
    /// # Errors
    ///
    /// [`IdError::WeakSecret`] for a key under [`Self::MIN_LEN`] bytes or version 0.
    pub fn new(version: u16, key: SecretString) -> Result<Self, IdError> {
        if version == 0 || key.expose_secret().len() < Self::MIN_LEN {
            return Err(IdError::WeakSecret);
        }
        Ok(Self { version, key })
    }

    /// The secret's version, stored with every token so a rotation knows which key made it.
    #[must_use]
    pub fn version(&self) -> u16 {
        self.version
    }

    /// HMAC-SHA256 over the version, `label` and each part, every field length-prefixed so no
    /// two inputs run together.
    fn mac(&self, label: &str, parts: &[&[u8]]) -> [u8; 32] {
        let mut mac =
            <Hmac<Sha256> as KeyInit>::new_from_slice(self.key.expose_secret().as_bytes())
                .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(&self.version.to_be_bytes());
        for part in std::iter::once(label.as_bytes()).chain(parts.iter().copied()) {
            mac.update(&(part.len() as u64).to_be_bytes());
            mac.update(part);
        }
        mac.finalize().into_bytes().into()
    }
}

impl fmt::Debug for IdSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IdSecret(v{}, [redacted])", self.version)
    }
}

/// What a derived ID names; the kind is part of the HMAC input, so a card and a quote on the same
/// job and business never share an ID (CARD-017, PICK-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    /// A job card shown to one business, from (job, business) (CARD-017).
    Card,
    /// A business's quote on a job, from (job, business) (PICK-028).
    Quote,
    /// A homeowner's pick, from (job, quote) (PICK-028).
    Pick,
}

impl IdKind {
    fn label(self) -> &'static str {
        match self {
            Self::Card => "id.card",
            Self::Quote => "id.quote",
            Self::Pick => "id.pick",
        }
    }
}

/// The ID of the `kind` record that `parts` identify, in the order given (CARD-017, PICK-028):
/// the first 128 bits of the HMAC, the same on every call and replay.
#[must_use]
pub fn derive_id(secret: &IdSecret, kind: IdKind, parts: &[Uuid]) -> Uuid {
    let bytes: Vec<&[u8]> = parts.iter().map(|id| id.as_bytes().as_slice()).collect();
    let mac = secret.mac(kind.label(), &bytes);
    let mut id = [0_u8; 16];
    id.copy_from_slice(&mac[..16]);
    Uuid::from_bytes(id)
}

/// Job card ID for `business` on `job` (CARD-017).
#[must_use]
pub fn card_id(secret: &IdSecret, job: Uuid, business: Uuid) -> Uuid {
    derive_id(secret, IdKind::Card, &[job, business])
}

/// Quote ID for `business`'s quote on `job` (PICK-028).
#[must_use]
pub fn quote_id(secret: &IdSecret, job: Uuid, business: Uuid) -> Uuid {
    derive_id(secret, IdKind::Quote, &[job, business])
}

/// Pick ID for the homeowner's pick of `quote` on `job` (PICK-028).
#[must_use]
pub fn pick_id(secret: &IdSecret, job: Uuid, quote: Uuid) -> Uuid {
    derive_id(secret, IdKind::Pick, &[job, quote])
}

/// A link's per-record 128-bit random salt, stored with the record and copied to the
/// off-database stream so a replay reproduces the token (SEC-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkSalt([u8; 16]);

impl LinkSalt {
    /// A fresh salt from the operating system's random source (SEC-037).
    ///
    /// # Errors
    ///
    /// [`IdError::NoRandom`] when the source fails; no link may be issued without it.
    pub fn generate() -> Result<Self, IdError> {
        let mut salt = [0_u8; 16];
        SysRng
            .try_fill_bytes(&mut salt)
            .map_err(|_| IdError::NoRandom)?;
        Ok(Self(salt))
    }

    /// The salt as read back from its `bytea` column.
    ///
    /// # Errors
    ///
    /// [`IdError::BadSalt`] unless `bytes` is exactly 16 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, IdError> {
        bytes.try_into().map(Self).map_err(|_| IdError::BadSalt)
    }

    /// The 16 bytes to store.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// What a link opens; part of the HMAC input, so one record's links of two kinds differ (SEC-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// The homeowner's quote page (PICK-028).
    QuotePage,
    /// A one-tap action link in a text or email.
    Action,
    /// A photo link.
    Photo,
}

impl LinkKind {
    fn label(self) -> &'static str {
        match self {
            Self::QuotePage => "link.quote_page",
            Self::Action => "link.action",
            Self::Photo => "link.photo",
        }
    }
}

/// A link token: 64 lower-case hex digits that reach a person in a URL (SEC-037). `Debug` prints
/// only a marker and there is no `Display`, so it cannot reach a log by accident (SEC-033).
#[derive(Clone, PartialEq, Eq)]
pub struct LinkToken(String);

impl LinkToken {
    /// Parses a token presented in a URL; anything but 64 lower-case hex digits is refused before
    /// any lookup, so the caller answers 404 (SEC-037).
    ///
    /// # Errors
    ///
    /// [`IdError::BadToken`].
    pub fn parse(raw: &str) -> Result<Self, IdError> {
        let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
        if raw.len() == 64 && raw.chars().all(hex) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(IdError::BadToken)
        }
    }

    /// The token for the URL. Never log it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The SHA256 of the token, the only form stored and looked up (SEC-037).
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}

impl fmt::Debug for LinkToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LinkToken([redacted])")
    }
}

/// The `kind` link token for `record` with its stored `salt` (SEC-037, PICK-028): the same on every
/// call and replay with that salt; a new salt gives a new token.
#[must_use]
pub fn link_token(secret: &IdSecret, kind: LinkKind, record: Uuid, salt: &LinkSalt) -> LinkToken {
    let mac = secret.mac(kind.label(), &[record.as_bytes(), salt.as_bytes()]);
    LinkToken(mac.iter().map(|byte| format!("{byte:02x}")).collect())
}
