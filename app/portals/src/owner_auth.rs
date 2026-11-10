//! Casey's owner sign-in (T086; SEC-005 to SEC-007, SEC-019, SEC-043, SEC-044; D-098, D-480,
//! D-1917 to D-1921).
//!
//! The owner signs in with an argon2id password plus a second factor: a passkey (WebAuthn,
//! checked by webauthn-rs; the browser half is `static/passkey.ts`), a TOTP authenticator code, or,
//! once at least two factors are registered, one offline recovery code (SEC-007). There is no
//! email path of any kind. A sign-in starts a 12-hour owner session and its `owner_session` row;
//! [`owner_principal`] ends it after 30 minutes without a request (SEC-043). The listed sensitive
//! actions ([`OwnerAction`]) need a second factor within the last 5 minutes
//! ([`require_owner_step_up`]); pause, refund and release ([`OwnerPower`]) need an owner principal
//! ([`owner_power`]), so no business or homeowner session ever holds them (SEC-006). The first
//! owner exists only through [`bootstrap_owner`], which the `server bootstrap-owner` command runs
//! as the migration role; the app role cannot insert an owner, so no route can (SEC-044).
//! The queries live in `models.rs`. No route is served yet (D-1921). Homeowner sign-in (SEC-008)
//! is not here: it is phone plus texted code, never this password.

use askama::Template;
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use jiff::{SignedDuration, Timestamp};
use rand::TryRng;
use rand::rngs::SysRng;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use thiserror::Error;
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Url, Webauthn, WebauthnBuilder,
};

use crate::models as m;
use crate::security::{SecurityEvent, SecurityEventKind, record_security_event};
use crate::session::{Realm, SESSION_COOKIE, start_session};

/// The longest an owner session lives, however busy (SEC-005, D-014).
pub const OWNER_SESSION_LIFE: SignedDuration = SignedDuration::from_hours(12);
/// An owner session with no request for longer than this is gone (SEC-043).
pub const OWNER_IDLE_LIMIT: SignedDuration = SignedDuration::from_mins(30);
/// How recent a second factor must be for an [`OwnerAction`] (SEC-043).
pub const OWNER_STEP_UP_WINDOW: SignedDuration = SignedDuration::from_mins(5);
/// The shortest owner password, in characters (SEC-043).
pub const MIN_OWNER_PASSWORD_CHARS: usize = 15;
/// How many offline recovery codes the bootstrap makes (SEC-007, OPS-042).
pub const RECOVERY_CODE_COUNT: usize = 10;
/// Registered factors needed before a recovery code signs in (SEC-007, D-098).
pub const MIN_OWNER_FACTORS: i64 = 2;
/// The owner sign-in page's path (logged as the route of a failed sign-in).
pub const OWNER_SIGNIN_ROUTE: &str = "/owner/sign-in";
/// The `POST` that issues a passkey sign-in challenge: [`passkey_sign_in_options`] (D-1919).
pub const OWNER_PASSKEY_ROUTE: &str = "/owner/sign-in/passkey";
/// The most passkey sign-in challenges live at once; another is refused until one ends (D-1919).
pub const MAX_LIVE_SIGN_IN_CHALLENGES: i64 = 20;
/// The host variable holding the one-time bootstrap token (SEC-044).
pub const BOOTSTRAP_TOKEN_VAR: &str = "ASKHAVA_OWNER_BOOTSTRAP_TOKEN";
const MIN_BOOTSTRAP_TOKEN_CHARS: usize = 32;
const CHALLENGE_LIFE: SignedDuration = SignedDuration::from_mins(5);
const BREACHED: &str = include_str!("../data/breached_passwords.txt");

/// Why a new owner password is refused (SEC-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordProblem {
    /// Fewer than [`MIN_OWNER_PASSWORD_CHARS`] characters.
    TooShort,
    /// On the offline breached-password list (`data/breached_passwords.txt`).
    Breached,
}

/// Why an owner step failed. No variant carries a password, code, token or ID (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OwnerAuthError {
    /// The password or the second factor was wrong; logged as `auth_failed`.
    #[error("the owner sign-in was refused")]
    Refused,
    /// A new password breaks SEC-043's rules.
    #[error("the new owner password is refused")]
    WeakPassword(PasswordProblem),
    /// The bootstrap token variable is missing or too short (SEC-044).
    #[error("no usable bootstrap token")]
    NoBootstrapToken,
    /// An owner already exists, so the bootstrap does nothing (SEC-044).
    #[error("the owner already exists")]
    AlreadyBootstrapped,
    /// No second factor in the last 5 minutes; logged as `authz_denied` (SEC-043).
    #[error("a fresh second factor is needed")]
    NeedsFreshFactor,
    /// No live owner session; logged as `authz_denied` (SEC-006).
    #[error("not signed in as the owner")]
    NotOwner,
    /// The relying-party settings or a passkey ceremony step was unusable.
    #[error("the passkey step failed")]
    Passkey,
    /// The operating system's random source failed.
    #[error("the random source failed")]
    NoRandom,
    /// Hashing the password failed.
    #[error("hashing the password failed")]
    Hash,
    /// A database call failed.
    #[error("an owner database call failed")]
    Database,
}

/// A sensitive owner action that needs a second factor within [`OWNER_STEP_UP_WINDOW`] (SEC-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerAction {
    /// The AI switch.
    AiSwitch,
    /// The fee mode.
    FeeMode,
    /// A refund.
    Refund,
    /// A settings change (password, factors included).
    Settings,
    /// The Stripe-answer record.
    StripeAnswer,
    /// A data export.
    DataExport,
    /// The safety-call switch.
    SafetyCallSwitch,
}

impl OwnerAction {
    /// Every action SEC-043 lists.
    pub const ALL: [Self; 7] = [
        Self::AiSwitch,
        Self::FeeMode,
        Self::Refund,
        Self::Settings,
        Self::StripeAnswer,
        Self::DataExport,
        Self::SafetyCallSwitch,
    ];
}

/// A power only the owner portal holds (SEC-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerPower {
    /// A pause switch.
    Pause,
    /// A refund (also an [`OwnerAction`]: needs a fresh factor).
    Refund,
    /// Completing or re-sending a release of a confirmed, paid or $0-decided pick.
    Release,
}

/// The owner's second factor for one sign-in or step-up (SEC-005, SEC-007, D-098).
#[derive(Debug, Clone, Copy)]
pub enum SecondFactor<'a> {
    /// A 6-digit code from a registered authenticator app.
    Totp(&'a str),
    /// The browser's WebAuthn answer (JSON) to the challenge row `challenge`.
    Passkey {
        /// The `owner_challenge` row the options came from.
        challenge: Uuid,
        /// `passkey.ts`'s JSON of the `PublicKeyCredential`.
        credential: &'a str,
    },
    /// One offline recovery code; works only with [`MIN_OWNER_FACTORS`] registered.
    RecoveryCode(&'a str),
}

/// A live, non-idle owner session (SEC-005, SEC-043).
#[derive(Debug)]
pub struct OwnerPrincipal {
    owner: Uuid,
    session: Uuid,
    factor_at: Timestamp,
}

impl OwnerPrincipal {
    /// The owner record's ID (the audit actor).
    #[must_use]
    pub fn owner(&self) -> Uuid {
        self.owner
    }

    /// The session row's ID (the security events' actor reference).
    #[must_use]
    pub fn session(&self) -> Uuid {
        self.session
    }
}

/// What the bootstrap hands Casey once: the first authenticator's secret and the recovery codes.
/// Both are secrets: never logged, stored or shown again (SEC-007, SEC-044).
#[derive(Debug)]
pub struct OwnerEnrolment {
    /// The TOTP secret in base32, for an authenticator app.
    pub totp_secret: SecretString,
    /// The offline one-time recovery codes, to keep on paper (OPS-042).
    pub recovery_codes: Vec<SecretString>,
}

/// The passkey relying party: Ask Hava's domain and origin (D-098).
#[derive(Debug)]
pub struct OwnerAuth {
    webauthn: Webauthn,
}

impl OwnerAuth {
    /// The relying party for `rp_id` (such as `askhava.com`) served at `origin`, named with the
    /// brand file's display name (WORD-001, D-1932, T294).
    ///
    /// # Errors
    ///
    /// [`OwnerAuthError::Passkey`] for an origin that is not a URL on `rp_id`.
    pub fn new(rp_id: &str, origin: &str) -> Result<Self, OwnerAuthError> {
        let url = Url::parse(origin).map_err(|_| OwnerAuthError::Passkey)?;
        let builder = WebauthnBuilder::new(rp_id, &url).map_err(|_| OwnerAuthError::Passkey)?;
        let webauthn = builder
            .rp_name(askhava_core::api::brand().display_name())
            .build();
        Ok(Self {
            webauthn: webauthn.map_err(|_| OwnerAuthError::Passkey)?,
        })
    }
}

/// Why `password` may not be the owner password, or `None` when it may: at least 15 characters
/// and not on the offline breached list, compared without case (SEC-043, D-491).
#[must_use]
pub fn password_problem(password: &str) -> Option<PasswordProblem> {
    if password.chars().count() < MIN_OWNER_PASSWORD_CHARS {
        return Some(PasswordProblem::TooShort);
    }
    let breached = BREACHED
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .any(|line| line.to_lowercase() == password.to_lowercase());
    breached.then_some(PasswordProblem::Breached)
}

/// Whether an owner session last used at `last_seen` has ended by `now`: more than 30 minutes
/// idle, so 29:59 and 30:00 are live and 30:01 is gone (SEC-043).
#[must_use]
pub fn idle_ended(last_seen: Timestamp, now: Timestamp) -> bool {
    now.duration_since(last_seen) > OWNER_IDLE_LIMIT
}

/// Whether a second factor at `factor_at` is fresh at `now`: not in the future and at most 5
/// minutes old, 5:00 included (SEC-043).
#[must_use]
pub fn factor_fresh(factor_at: Timestamp, now: Timestamp) -> bool {
    let age = now.duration_since(factor_at);
    !age.is_negative() && age <= OWNER_STEP_UP_WINDOW
}

/// When an owner session signed in at `created` and last used at `now` ends: 30 minutes idle
/// (plus one microsecond, the database's resolution, so `expires_at > now` keeps 30:00 live as
/// [`idle_ended`] does), never past 12 hours. Stored as `portals.session.expires_at`, so
/// `current_session` and `Session::owner_proof` refuse an idle owner session too (D-1917).
fn owner_session_end(created: Timestamp, now: Timestamp) -> Timestamp {
    let idle_end = now + OWNER_IDLE_LIMIT + SignedDuration::from_micros(1);
    idle_end.min(created + OWNER_SESSION_LIFE)
}

fn random<const N: usize>() -> Result<[u8; N], OwnerAuthError> {
    let mut bytes = [0u8; N];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| OwnerAuthError::NoRandom)?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(text: &str) -> Vec<u8> {
    Sha256::digest(text.as_bytes()).to_vec()
}

fn db<T, E>(result: Result<T, E>) -> Result<T, OwnerAuthError> {
    result.map_err(|_| OwnerAuthError::Database)
}

/// The argon2id PHC string of `password` with a fresh 16-byte salt and the crate's default
/// (OWASP) parameters (SEC-005).
fn hash_password(password: &str) -> Result<String, OwnerAuthError> {
    use argon2::{Argon2, PasswordHasher};
    let salt = random::<16>()?;
    let hash = Argon2::default().hash_password_with_salt(password.as_bytes(), &salt);
    hash.map(|h| h.to_string())
        .map_err(|_| OwnerAuthError::Hash)
}

fn password_matches(password: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

/// A recovery code as stored: dashes and spaces dropped, lower case.
fn normal_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn totp(secret: Vec<u8>) -> Option<totp_rs::Totp> {
    totp_rs::Builder::new().with_secret(secret).build().ok()
}

async fn record(conn: &mut PgConnection, entry: AuditEntry) -> Result<(), OwnerAuthError> {
    db(audit(conn, &entry).await).map(|_| ())
}

fn owner_entry(owner: Uuid, action: &'static str, subject: (&'static str, Uuid)) -> AuditEntry {
    AuditEntry {
        actor: Actor::Owner(owner),
        action,
        subject_kind: subject.0,
        subject_id: subject.1,
        change: Change::Action,
    }
}

/// Creates the one owner from the bootstrap command (SEC-044, SEC-007, D-1918): needs a token of
/// at least 32 characters from [`BOOTSTRAP_TOKEN_VAR`], a password [`password_problem`] passes and
/// no owner yet. Stores the argon2id hash, one TOTP authenticator and [`RECOVERY_CODE_COUNT`]
/// recovery codes (hashed) with a system audit row, in one transaction, and returns the secrets
/// once. Run as the migration role: the app role cannot insert an owner.
///
/// # Errors
///
/// [`OwnerAuthError::NoBootstrapToken`], [`OwnerAuthError::WeakPassword`],
/// [`OwnerAuthError::AlreadyBootstrapped`] (a second run), or a random, hash or database failure;
/// nothing is stored then.
pub async fn bootstrap_owner(
    conn: &mut PgConnection,
    token: Option<&str>,
    password: &str,
    now: Timestamp,
) -> Result<OwnerEnrolment, OwnerAuthError> {
    if token.is_none_or(|t| t.trim().chars().count() < MIN_BOOTSTRAP_TOKEN_CHARS) {
        return Err(OwnerAuthError::NoBootstrapToken);
    }
    if let Some(problem) = password_problem(password) {
        return Err(OwnerAuthError::WeakPassword(problem));
    }
    let hash = hash_password(password)?;
    let mut tx = db(conn.begin().await)?;
    let owner = db(m::insert_owner(&mut tx, &hash, now).await)?
        .ok_or(OwnerAuthError::AlreadyBootstrapped)?;
    let secret = random::<20>()?;
    db(m::insert_totp(&mut tx, owner, &secret, now).await)?;
    let mut recovery_codes = Vec::with_capacity(RECOVERY_CODE_COUNT);
    for _ in 0..RECOVERY_CODE_COUNT {
        let code = hex(&random::<10>()?);
        db(m::insert_recovery_code(&mut tx, owner, &sha256(&code)).await)?;
        recovery_codes.push(SecretString::from(code));
    }
    let mut entry = owner_entry(
        owner,
        "portals.owner_bootstrapped",
        ("portals.owner", owner),
    );
    entry.actor = Actor::System;
    record(&mut tx, entry).await?;
    db(tx.commit().await)?;
    let totp_secret = SecretString::from(totp_rs::Secret::from(secret).to_base32());
    Ok(OwnerEnrolment {
        totp_secret,
        recovery_codes,
    })
}

async fn totp_ok(
    conn: &mut PgConnection,
    owner: Uuid,
    code: &str,
    now: Timestamp,
) -> Result<bool, OwnerAuthError> {
    let secs = u64::try_from(now.as_second()).unwrap_or(0);
    for (factor, secret) in db(m::totp_factors(conn, owner).await)? {
        let step = totp(secret).and_then(|t| t.check(code.trim(), secs));
        if let Some(step) = step.and_then(|s| i64::try_from(s).ok()) {
            return db(m::use_totp_step(conn, factor, step).await);
        }
    }
    Ok(false)
}

async fn passkey_ok(
    conn: &mut PgConnection,
    owner: Uuid,
    (auth, challenge, credential): (&OwnerAuth, Uuid, &str),
    now: Timestamp,
) -> Result<bool, OwnerAuthError> {
    let Some(state) = db(m::take_challenge(conn, challenge, "sign_in", now).await)? else {
        return Ok(false);
    };
    let state: Option<PasskeyAuthentication> = serde_json::from_value(state).ok();
    let answer: Option<PublicKeyCredential> = serde_json::from_str(credential).ok();
    let (Some(state), Some(answer)) = (state, answer) else {
        return Ok(false);
    };
    let Ok(result) = auth.webauthn.finish_passkey_authentication(&answer, &state) else {
        return Ok(false);
    };
    for (factor, json) in db(m::passkeys(conn, owner).await)? {
        let Ok(mut key) = serde_json::from_value::<Passkey>(json) else {
            continue;
        };
        if key.update_credential(&result).is_some() {
            let json = db(serde_json::to_value(&key))?;
            db(m::update_passkey(conn, factor, json).await)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `factor` is good for `owner` at `now`; a good TOTP step, passkey challenge or
/// recovery code is used up (SEC-005, SEC-007, D-098). A recovery code counts only while at
/// least [`MIN_OWNER_FACTORS`] factors are registered.
async fn factor_ok(
    conn: &mut PgConnection,
    owner: Uuid,
    (auth, factor): (&OwnerAuth, SecondFactor<'_>),
    now: Timestamp,
) -> Result<bool, OwnerAuthError> {
    match factor {
        SecondFactor::Totp(code) => totp_ok(conn, owner, code, now).await,
        SecondFactor::Passkey {
            challenge,
            credential,
        } => passkey_ok(conn, owner, (auth, challenge, credential), now).await,
        SecondFactor::RecoveryCode(code) => {
            if db(m::factor_count(conn, owner).await)? < MIN_OWNER_FACTORS {
                return Ok(false);
            }
            let digest = sha256(&normal_code(code));
            db(m::use_recovery_code(conn, owner, &digest, now).await)
        }
    }
}

async fn refuse(
    conn: &mut PgConnection,
    (kind, route, actor): (SecurityEventKind, &str, Option<Uuid>),
    error: OwnerAuthError,
    now: Timestamp,
) -> Result<OwnerAuthError, OwnerAuthError> {
    let event = SecurityEvent {
        kind,
        route,
        request_id: None,
        actor_ref: actor,
    };
    db(record_security_event(conn, &event, now).await)?;
    Ok(error)
}

/// Signs the owner in with `password` and `factor` (SEC-005, SEC-007): the argon2id hash must
/// match and the factor be good, then a 12-hour owner session starts with its idle and step-up
/// clock at `now` and a `portals.owner_signed_in` audit row, in one transaction. Either check
/// failing is one `auth_failed` event and [`OwnerAuthError::Refused`], whichever failed.
///
/// # Errors
///
/// [`OwnerAuthError::Refused`], or a random or database failure; nothing is signed in then.
pub async fn owner_sign_in(
    conn: &mut PgConnection,
    jar: CookieJar,
    (password, factor): (&str, SecondFactor<'_>),
    auth: &OwnerAuth,
    now: Timestamp,
) -> Result<CookieJar, OwnerAuthError> {
    let owner = db(m::owner_password(conn).await)?
        .filter(|(_, hash)| password_matches(password, hash))
        .map(|(owner, _)| owner);
    let good = match owner {
        Some(owner) => factor_ok(conn, owner, (auth, factor), now).await?,
        None => false,
    };
    let (Some(owner), true) = (owner, good) else {
        let why = (SecurityEventKind::AuthFailed, OWNER_SIGNIN_ROUTE, None);
        return Err(refuse(conn, why, OwnerAuthError::Refused, now).await?);
    };
    let mut tx = db(conn.begin().await)?;
    let started = start_session(&mut tx, jar, (Realm::Owner, owner), OWNER_SESSION_LIFE, now);
    let jar = started.await.map_err(|e| match e {
        crate::session::PortalsError::NoRandom => OwnerAuthError::NoRandom,
        _ => OwnerAuthError::Database,
    })?;
    let token = jar.get(SESSION_COOKIE).map(|c| c.value().to_owned());
    let token = token.ok_or(OwnerAuthError::Database)?;
    // A recovery code is not a fresh factor (SEC-043, D-1917): its session starts stale.
    let factor_at = match factor {
        SecondFactor::RecoveryCode(_) => Timestamp::UNIX_EPOCH,
        _ => now,
    };
    let times = (now, factor_at);
    let session = db(m::insert_owner_session(&mut tx, &sha256(&token), times).await)?;
    let ends = (now, owner_session_end(now, now));
    db(m::touch_owner_session(&mut tx, session, ends).await)?;
    let entry = owner_entry(
        owner,
        "portals.owner_signed_in",
        ("portals.session", session),
    );
    record(&mut tx, entry).await?;
    db(tx.commit().await)?;
    Ok(jar)
}

/// The signed-in owner, or `None` for no cookie, a non-owner or expired session, or one idle for
/// more than 30 minutes, which is deleted; a live one's idle clock restarts at `now` and its
/// session row's end moves to `owner_session_end` (SEC-005, SEC-043). A business or homeowner session is never an owner principal (SEC-006, SEC-019).
///
/// # Errors
///
/// [`OwnerAuthError::Database`].
pub async fn owner_principal(
    conn: &mut PgConnection,
    jar: &CookieJar,
    now: Timestamp,
) -> Result<Option<OwnerPrincipal>, OwnerAuthError> {
    let Some(cookie) = jar.get(SESSION_COOKIE) else {
        return Ok(None);
    };
    let digest = sha256(cookie.value());
    let Some(row) = db(m::owner_session(conn, &digest, now).await)? else {
        return Ok(None);
    };
    if idle_ended(row.last_seen_at, now) {
        db(m::end_session(conn, row.session).await)?;
        return Ok(None);
    }
    let ends = (now, owner_session_end(row.created_at, now));
    db(m::touch_owner_session(conn, row.session, ends).await)?;
    Ok(Some(OwnerPrincipal {
        owner: row.owner,
        session: row.session,
        factor_at: row.factor_at,
    }))
}

/// Passes only when the principal's last second factor is [`factor_fresh`] at `now`; otherwise one
/// `authz_denied` event on `route` with the session as actor (SEC-043, SEC-048).
///
/// # Errors
///
/// [`OwnerAuthError::NeedsFreshFactor`] or [`OwnerAuthError::Database`].
pub async fn require_owner_step_up(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (action, route): (OwnerAction, &str),
    now: Timestamp,
) -> Result<(), OwnerAuthError> {
    if factor_fresh(principal.factor_at, now) {
        return Ok(());
    }
    tracing::warn!(action = ?action, "owner action needs a fresh second factor");
    let why = (
        SecurityEventKind::AuthzDenied,
        route,
        Some(principal.session),
    );
    Err(refuse(conn, why, OwnerAuthError::NeedsFreshFactor, now).await?)
}

/// Records a fresh second factor for the signed-in owner (the step-up, SEC-043): a good TOTP code
/// or passkey answer restarts the 5-minute window at `now`; a bad one, or any recovery code (not
/// a fresh passkey or TOTP, and left unused), is an `auth_failed` event (D-1917).
///
/// # Errors
///
/// [`OwnerAuthError::Refused`] or [`OwnerAuthError::Database`].
pub async fn owner_step_up(
    conn: &mut PgConnection,
    principal: &mut OwnerPrincipal,
    (auth, factor, route): (&OwnerAuth, SecondFactor<'_>, &str),
    now: Timestamp,
) -> Result<(), OwnerAuthError> {
    let recovery = matches!(factor, SecondFactor::RecoveryCode(_));
    if recovery || !factor_ok(conn, principal.owner, (auth, factor), now).await? {
        let why = (
            SecurityEventKind::AuthFailed,
            route,
            Some(principal.session),
        );
        return Err(refuse(conn, why, OwnerAuthError::Refused, now).await?);
    }
    db(m::set_factor_at(conn, principal.session, now).await)?;
    principal.factor_at = now;
    Ok(())
}

/// The gate for pause, refund and release (SEC-006, SEC-019): only a live owner principal passes,
/// and a refund also needs [`require_owner_step_up`]; anything else is one `authz_denied` event.
///
/// # Errors
///
/// [`OwnerAuthError::NotOwner`], [`OwnerAuthError::NeedsFreshFactor`] or a database failure.
pub async fn owner_power(
    conn: &mut PgConnection,
    jar: &CookieJar,
    (power, route): (OwnerPower, &str),
    now: Timestamp,
) -> Result<OwnerPrincipal, OwnerAuthError> {
    let Some(principal) = owner_principal(conn, jar, now).await? else {
        let why = (SecurityEventKind::AuthzDenied, route, None);
        return Err(refuse(conn, why, OwnerAuthError::NotOwner, now).await?);
    };
    if power == OwnerPower::Refund {
        let action = (OwnerAction::Refund, route);
        require_owner_step_up(conn, &principal, action, now).await?;
    }
    Ok(principal)
}

/// Logs out: deletes this browser's session on the server, so its old cookie is refused from
/// then on, and clears the cookie (SEC-043).
///
/// # Errors
///
/// [`OwnerAuthError::Database`].
pub async fn owner_sign_out(
    conn: &mut PgConnection,
    jar: CookieJar,
) -> Result<CookieJar, OwnerAuthError> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        db(m::end_owner_session_by_token(conn, &sha256(cookie.value())).await)?;
    }
    let gone = format!("{SESSION_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0");
    let gone = Cookie::parse(gone).map_err(|_| OwnerAuthError::Database)?;
    Ok(jar.add(gone))
}

/// "Sign out everywhere": ends every owner session, this one included, with an audit row
/// (SEC-043). Returns how many ended.
///
/// # Errors
///
/// [`OwnerAuthError::Database`].
pub async fn sign_out_everywhere(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
) -> Result<u64, OwnerAuthError> {
    let mut tx = db(conn.begin().await)?;
    let ended = db(m::end_owner_sessions(&mut tx).await)?;
    let subject = ("portals.owner", principal.owner);
    record(
        &mut tx,
        owner_entry(principal.owner, "portals.owner_signed_out_all", subject),
    )
    .await?;
    db(tx.commit().await)?;
    Ok(ended)
}

/// Changes the owner password (SEC-043): needs a fresh second factor, then [`password_problem`];
/// stores the new argon2id hash and ends every other owner session (this one stays) with an audit
/// row, in one transaction.
///
/// # Errors
///
/// [`OwnerAuthError::NeedsFreshFactor`], [`OwnerAuthError::WeakPassword`], or a hash or
/// database failure.
pub async fn change_owner_password(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (new_password, route): (&str, &str),
    now: Timestamp,
) -> Result<(), OwnerAuthError> {
    require_owner_step_up(conn, principal, (OwnerAction::Settings, route), now).await?;
    if let Some(problem) = password_problem(new_password) {
        return Err(OwnerAuthError::WeakPassword(problem));
    }
    let hash = hash_password(new_password)?;
    let mut tx = db(conn.begin().await)?;
    db(m::set_owner_password(&mut tx, principal.owner, &hash).await)?;
    db(m::end_other_owner_sessions(&mut tx, principal.session).await)?;
    let entry = owner_entry(
        principal.owner,
        "portals.owner_password_changed",
        ("portals.owner", principal.owner),
    );
    record(&mut tx, entry).await?;
    db(tx.commit().await)
}

/// Registers another authenticator app (D-098, SEC-007): needs a fresh second factor; returns its
/// base32 secret once, with an audit row.
///
/// # Errors
///
/// [`OwnerAuthError::NeedsFreshFactor`], or a random or database failure.
pub async fn add_owner_totp(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    route: &str,
    now: Timestamp,
) -> Result<SecretString, OwnerAuthError> {
    require_owner_step_up(conn, principal, (OwnerAction::Settings, route), now).await?;
    let secret = random::<20>()?;
    let mut tx = db(conn.begin().await)?;
    let factor = db(m::insert_totp(&mut tx, principal.owner, &secret, now).await)?;
    let entry = owner_entry(
        principal.owner,
        "portals.owner_factor_added",
        ("portals.owner_factor", factor),
    );
    record(&mut tx, entry).await?;
    db(tx.commit().await)?;
    Ok(SecretString::from(
        totp_rs::Secret::from(secret).to_base32(),
    ))
}

async fn owner_passkeys(conn: &mut PgConnection) -> Result<Vec<Passkey>, OwnerAuthError> {
    let Some((owner, _)) = db(m::owner_password(conn).await)? else {
        return Ok(Vec::new());
    };
    Ok(db(m::passkeys(conn, owner).await)?
        .into_iter()
        .filter_map(|(_, json)| serde_json::from_value(json).ok())
        .collect())
}

/// Whether the owner has a passkey, so the sign-in page shows "Use a passkey" (D-098). It only
/// reads: the page's `GET` stores nothing (D-1919).
///
/// # Errors
///
/// [`OwnerAuthError::Database`].
pub async fn passkey_registered(conn: &mut PgConnection) -> Result<bool, OwnerAuthError> {
    Ok(!owner_passkeys(conn).await?.is_empty())
}

/// The passkey sign-in options for `passkey.ts` and their challenge row, or `None` when no
/// passkey is registered (D-098). Only the `POST` on [`OWNER_PASSKEY_ROUTE`] calls it, never the
/// page's `GET` (D-1919). It first deletes every expired challenge, and refuses once
/// [`MAX_LIVE_SIGN_IN_CHALLENGES`] sign-in challenges are live, so the table stays small. The row
/// is used once and lasts 5 minutes.
///
/// # Errors
///
/// [`OwnerAuthError::Passkey`] (also when the cap is reached) or [`OwnerAuthError::Database`].
pub async fn passkey_sign_in_options(
    conn: &mut PgConnection,
    auth: &OwnerAuth,
    now: Timestamp,
) -> Result<Option<(Uuid, String)>, OwnerAuthError> {
    db(m::sweep_challenges(conn, now).await)?;
    let keys = owner_passkeys(conn).await?;
    if keys.is_empty() {
        return Ok(None);
    }
    let started = auth.webauthn.start_passkey_authentication(&keys);
    let (options, state) = started.map_err(|_| OwnerAuthError::Passkey)?;
    let state = serde_json::to_value(&state).map_err(|_| OwnerAuthError::Passkey)?;
    let row = (state, now + CHALLENGE_LIFE, MAX_LIVE_SIGN_IN_CHALLENGES);
    let id = db(m::insert_sign_in_challenge(conn, row, now).await)?;
    let id = id.ok_or(OwnerAuthError::Passkey)?;
    let options = serde_json::to_string(&options).map_err(|_| OwnerAuthError::Passkey)?;
    Ok(Some((id, options)))
}

/// Starts registering a passkey (D-098): needs a fresh second factor; returns the challenge row
/// and the creation options for `passkey.ts`, excluding the passkeys already registered. The
/// passkey's display name is "<display name> owner" from the brand file (WORD-001, D-1932, T294).
///
/// # Errors
///
/// [`OwnerAuthError::NeedsFreshFactor`], [`OwnerAuthError::Passkey`] or a database failure.
pub async fn passkey_registration_options(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (auth, route): (&OwnerAuth, &str),
    now: Timestamp,
) -> Result<(Uuid, String), OwnerAuthError> {
    require_owner_step_up(conn, principal, (OwnerAction::Settings, route), now).await?;
    let known: Vec<_> = db(m::passkeys(conn, principal.owner).await)?
        .into_iter()
        .filter_map(|(_, json)| serde_json::from_value::<Passkey>(json).ok())
        .map(|key| key.cred_id().clone())
        .collect();
    let shown = format!("{} owner", askhava_core::api::brand().display_name());
    let started =
        auth.webauthn
            .start_passkey_registration(principal.owner, "owner", &shown, Some(known));
    let (options, state) = started.map_err(|_| OwnerAuthError::Passkey)?;
    let state = serde_json::to_value(&state).map_err(|_| OwnerAuthError::Passkey)?;
    let id = db(m::insert_challenge(conn, "register", state, now + CHALLENGE_LIFE).await)?;
    let options = serde_json::to_string(&options).map_err(|_| OwnerAuthError::Passkey)?;
    Ok((id, options))
}

/// Finishes registering a passkey from the browser's answer (D-098): needs a fresh second factor
/// and an unused, unexpired registration challenge; stores the credential with an audit row.
///
/// # Errors
///
/// [`OwnerAuthError::NeedsFreshFactor`], [`OwnerAuthError::Passkey`] for a bad or replayed
/// answer, or a database failure.
pub async fn finish_passkey_registration(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (auth, challenge, credential, route): (&OwnerAuth, Uuid, &str, &str),
    now: Timestamp,
) -> Result<(), OwnerAuthError> {
    require_owner_step_up(conn, principal, (OwnerAction::Settings, route), now).await?;
    let state = db(m::take_challenge(conn, challenge, "register", now).await)?;
    let state: Option<PasskeyRegistration> = state.and_then(|s| serde_json::from_value(s).ok());
    let answer: Option<RegisterPublicKeyCredential> = serde_json::from_str(credential).ok();
    let (Some(state), Some(answer)) = (state, answer) else {
        return Err(OwnerAuthError::Passkey);
    };
    let key = auth.webauthn.finish_passkey_registration(&answer, &state);
    let key = key.map_err(|_| OwnerAuthError::Passkey)?;
    let json = serde_json::to_value(&key).map_err(|_| OwnerAuthError::Passkey)?;
    let mut tx = db(conn.begin().await)?;
    let factor = db(m::insert_passkey(&mut tx, principal.owner, json, now).await)?;
    let entry = owner_entry(
        principal.owner,
        "portals.owner_factor_added",
        ("portals.owner_factor", factor),
    );
    record(&mut tx, entry).await?;
    db(tx.commit().await)
}

#[derive(Template)]
#[template(path = "owner_signin.html")]
struct SignInView<'a> {
    csrf: &'a str,
    passkey: bool,
    passkey_route: &'a str,
    failed: bool,
}

/// The owner sign-in page (SEC-005, A11Y-013): password, then a code from an authenticator app
/// or a recovery code, or "Use a passkey" when `passkey` ([`passkey_registered`]); that button's
/// script asks [`OWNER_PASSKEY_ROUTE`] for the options by `POST`. `failed` shows the error
/// summary. No email field and no email path (SEC-007).
///
/// # Errors
///
/// The template's render error.
pub fn owner_signin_page(csrf: &str, passkey: bool, failed: bool) -> Result<String, askama::Error> {
    SignInView {
        csrf,
        passkey,
        passkey_route: OWNER_PASSKEY_ROUTE,
        failed,
    }
    .render()
}
