//! Portal sessions, their `__Host-` cookies and the CSRF tokens bound to them (SEC-009, SEC-042,
//! SEC-046, D-099, D-480, D-943).
//!
//! A session cookie is `__Host-askhava-session`: `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`
//! and no `Domain`, holding 256 random bits; only the token's SHA256 is stored, in
//! `portals.session`. [`start_session`] is the only way in: it deletes the session the browser
//! held before and drops the pre-session cookie, so no cookie issued before a login survives it.
//! A form without a session (web chat, the quote page, link pages) is bound to its own
//! `__Host-askhava-pre` cookie instead. A CSRF token is the SHA256 of a label and the cookie's
//! token, so it needs no key, no table and no lookup, and only a page that could read the
//! `HttpOnly` cookie could make one. A [`Session`] turns into a realm proof for vault reads only
//! in its own realm (SEC-046).
//!
//! Expired rows are not kept: the daily [`SWEEP_KIND`] task ([`run_session_sweep_at`]) deletes
//! sessions that expired more than a day earlier, [`SWEEP_BATCH`] rows per statement, and queues
//! the next day's run first (SEC-004, D-2207).

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use askhava_privacy::api::{HomeownerSessionProof, OwnerSessionProof, SessionMint};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use subtle::ConstantTimeEq;
use thiserror::Error;

/// The signed-in session cookie (SEC-009, D-099).
pub const SESSION_COOKIE: &str = "__Host-askhava-session";
/// The pre-session cookie a form without a session is bound to (SEC-042).
pub const PRE_SESSION_COOKIE: &str = "__Host-askhava-pre";
/// The daily task that deletes sessions expired more than a day earlier (SEC-004, D-2207).
pub const SWEEP_KIND: &str = "portals.session_sweep";
/// The most sessions one sweep statement deletes (D-2207).
pub const SWEEP_BATCH: i64 = 5_000;
const DAY: SignedDuration = SignedDuration::from_hours(24);

/// Why a session step failed. No variant carries a token, an ID or a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PortalsError {
    /// The operating system's random source failed, so no token could be made.
    #[error("the random source failed")]
    NoRandom,
    /// A database call failed.
    #[error("a portals database call failed")]
    Database,
    /// Raising the security alert failed in `ops`.
    #[error("raising the security alert failed")]
    Alert,
    /// A session lifetime that is not positive or runs past the last representable time.
    #[error("the session lifetime is out of range")]
    BadLifetime,
}

/// The realm a session signs in to; each has its own sign-in and its own reads (SEC-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Realm {
    /// A homeowner, on their own record; the actor is their vault contact ID.
    Homeowner,
    /// A business person; never a vault plaintext read.
    Business,
    /// Casey's owner portal.
    Owner,
}

impl Realm {
    /// The code name stored in `portals.session.realm`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Homeowner => "homeowner",
            Self::Business => "business",
            Self::Owner => "owner",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        [Self::Homeowner, Self::Business, Self::Owner]
            .into_iter()
            .find(|realm| realm.as_str() == code)
    }
}

/// A live, signed-in session, found from its cookie by [`current_session`].
#[derive(Debug)]
pub struct Session {
    id: Uuid,
    realm: Realm,
    actor_id: Uuid,
}

impl Session {
    /// The `portals.session` row ID, the actor reference security events may log (SEC-048).
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The realm this session signed in to.
    #[must_use]
    pub fn realm(&self) -> Realm {
        self.realm
    }

    /// The proof for a homeowner's read of their own record; `None` in any other realm (SEC-046).
    #[must_use]
    pub fn homeowner_proof(&self, mint: &SessionMint) -> Option<HomeownerSessionProof> {
        (self.realm == Realm::Homeowner).then(|| mint.homeowner(self.actor_id))
    }

    /// The proof for an owner-realm read; `None` in any other realm (SEC-046).
    #[must_use]
    pub fn owner_proof(&self, mint: &SessionMint) -> Option<OwnerSessionProof> {
        (self.realm == Realm::Owner).then(|| mint.owner(self.actor_id))
    }
}

fn new_token() -> Result<String, PortalsError> {
    let mut bytes = [0u8; 32];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| PortalsError::NoRandom)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    parts
        .iter()
        .fold(Sha256::new(), |hash, part| hash.chain_update(part))
        .finalize()
        .into()
}

/// A `__Host-` cookie: `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain` (SEC-009,
/// D-099); `max_age` makes it outlive the browser session.
fn host_cookie(name: &str, value: &str, max_age: Option<SignedDuration>) -> Cookie<'static> {
    let age = max_age.map_or(String::new(), |age| {
        format!("; Max-Age={}", age.as_secs().max(0))
    });
    Cookie::parse(format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax{age}"
    ))
    .unwrap_or_else(|_| unreachable!("a hex token always makes a valid cookie"))
}

fn removal(name: &'static str) -> Cookie<'static> {
    host_cookie(name, "", Some(SignedDuration::ZERO))
}

/// Signs `actor_id` in to `realm` for `lifetime`: deletes the session the browser held, drops the
/// pre-session cookie and sets a new session cookie with a new token (SEC-009, SEC-042, D-480).
/// Sign-in tasks call it only after their checks pass, with their realm's lifetime (SEC-005,
/// SEC-008).
///
/// # Errors
///
/// [`PortalsError::BadLifetime`], [`PortalsError::NoRandom`] or [`PortalsError::Database`];
/// nothing is signed in then.
pub async fn start_session(
    db: &mut PgConnection,
    jar: CookieJar,
    login: (Realm, Uuid),
    lifetime: SignedDuration,
    now: Timestamp,
) -> Result<CookieJar, PortalsError> {
    if !lifetime.is_positive() {
        return Err(PortalsError::BadLifetime);
    }
    let expires = now
        .checked_add(lifetime)
        .map_err(|_| PortalsError::BadLifetime)?;
    if let Some(old) = jar.get(SESSION_COOKIE) {
        let old_hash = sha256(&[old.value().as_bytes()]);
        sqlx::query!(
            "DELETE FROM portals.session WHERE token_sha256 = $1",
            &old_hash[..]
        )
        .execute(&mut *db)
        .await
        .map_err(|_| PortalsError::Database)?;
    }
    let token = new_token()?;
    sqlx::query!(
        "INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5)",
        &sha256(&[token.as_bytes()])[..],
        login.0.as_str(),
        login.1,
        now.to_sqlx(),
        expires.to_sqlx(),
    )
    .execute(db)
    .await
    .map_err(|_| PortalsError::Database)?;
    Ok(jar.add(removal(PRE_SESSION_COOKIE)).add(host_cookie(
        SESSION_COOKIE,
        &token,
        Some(lifetime),
    )))
}

/// The live session the request's cookie names, or `None` for no cookie, an unknown token or
/// an expired session (SEC-009).
///
/// # Errors
///
/// [`PortalsError::Database`].
pub async fn current_session(
    db: &mut PgConnection,
    jar: &CookieJar,
    now: Timestamp,
) -> Result<Option<Session>, PortalsError> {
    let Some(cookie) = jar.get(SESSION_COOKIE) else {
        return Ok(None);
    };
    let row = sqlx::query!(
        "SELECT id, realm, actor_id FROM portals.session \
         WHERE token_sha256 = $1 AND expires_at > $2",
        &sha256(&[cookie.value().as_bytes()])[..],
        now.to_sqlx(),
    )
    .fetch_optional(db)
    .await
    .map_err(|_| PortalsError::Database)?;
    Ok(row.and_then(|row| {
        Realm::parse(&row.realm).map(|realm| Session {
            id: row.id,
            realm,
            actor_id: row.actor_id,
        })
    }))
}

/// Deletes at most `most` sessions that expired before `before`, in one short statement on its
/// own, and returns how many (SEC-004, D-2207). The `expires_at` index serves it.
///
/// # Errors
///
/// [`PortalsError::Database`].
pub async fn sweep_batch(pool: &PgPool, before: Timestamp, most: i64) -> Result<u64, PortalsError> {
    let done = sqlx::query!(
        "DELETE FROM portals.session WHERE id IN \
         (SELECT id FROM portals.session WHERE expires_at < $1 LIMIT $2)",
        before.to_sqlx(),
        most,
    )
    .execute(pool)
    .await
    .map_err(|_| PortalsError::Database)?;
    Ok(done.rows_affected())
}

/// Queues [`SWEEP_KIND`] for the day after `now`, once per day (dedupe key by day number,
/// SEC-004, D-2207).
///
/// # Errors
///
/// [`TaskError::Db`].
pub async fn schedule_session_sweep(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let day = run_at.as_second() / 86_400;
    let mut task = NewTask::new(SWEEP_KIND, serde_json::json!({}), run_at);
    task.dedupe_key = Some(format!("{SWEEP_KIND}:{day}"));
    enqueue(db, &task).await?;
    Ok(())
}

/// SEC-004, D-2207: the [`SWEEP_KIND`] handler at `now` (the worker's clock): queues the next
/// day's sweep first, so a run that fails for good still leaves it, then deletes every session
/// that expired more than a day before `now`, [`SWEEP_BATCH`] rows per statement, until a batch
/// comes back short. A live session and one expired less than a day ago stay. Safe to run twice.
///
/// # Errors
///
/// A fixed [`HandlerError`] when a database call fails.
pub async fn run_session_sweep_at(
    pool: PgPool,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let failed = HandlerError("session sweep database call failed");
    {
        let mut conn = pool.acquire().await.map_err(|_| failed)?;
        schedule_session_sweep(&mut conn, now)
            .await
            .map_err(|_| failed)?;
    }
    let before = now - DAY;
    let full = u64::try_from(SWEEP_BATCH).unwrap_or(u64::MAX);
    loop {
        let deleted = sweep_batch(&pool, before, SWEEP_BATCH)
            .await
            .map_err(|_| failed)?;
        if deleted < full {
            return Ok(());
        }
    }
}

/// Gives a page without a session its pre-session cookie, keeping one the browser already has,
/// so the page can put [`csrf_token`] in its forms (SEC-042).
///
/// # Errors
///
/// [`PortalsError::NoRandom`].
pub fn ensure_pre_session(jar: CookieJar) -> Result<CookieJar, PortalsError> {
    if jar.get(SESSION_COOKIE).is_some() || jar.get(PRE_SESSION_COOKIE).is_some() {
        return Ok(jar);
    }
    let token = new_token()?;
    Ok(jar.add(host_cookie(PRE_SESSION_COOKIE, &token, None)))
}

/// The CSRF token every form on the page carries in its `csrf` field: bound to the session
/// cookie, or with no session to the pre-session cookie; `None` with neither (SEC-009, SEC-042).
#[must_use]
pub fn csrf_token(jar: &CookieJar) -> Option<String> {
    let (label, cookie) = match jar.get(SESSION_COOKIE) {
        Some(cookie) => ("askhava.csrf.session", cookie),
        None => ("askhava.csrf.pre", jar.get(PRE_SESSION_COOKIE)?),
    };
    let hash = sha256(&[label.as_bytes(), b"\0", cookie.value().as_bytes()]);
    Some(hash.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether `presented` is the CSRF token of this request's own cookie, compared in constant time
/// (SEC-009, SEC-042). A token from another session or another pre-session cookie never matches.
#[must_use]
pub fn csrf_matches(jar: &CookieJar, presented: &str) -> bool {
    csrf_token(jar)
        .is_some_and(|expected| bool::from(expected.as_bytes().ct_eq(presented.as_bytes())))
}

#[cfg(test)]
mod tests {
    //! A session proves only its own realm (TD-SEC-25; SEC-046).

    use askhava_privacy::api::session_mint;
    use sqlx::types::Uuid;

    use super::{Realm, Session};

    /// Homeowner sessions alone get the homeowner proof, owner sessions alone the owner proof,
    /// and business sessions neither (TD-SEC-25).
    #[test]
    fn a_session_proves_only_its_own_realm() {
        let mint = session_mint();
        for realm in [Realm::Homeowner, Realm::Business, Realm::Owner] {
            let session = Session {
                id: Uuid::from_u128(1),
                realm,
                actor_id: Uuid::from_u128(11),
            };
            assert_eq!(
                session.homeowner_proof(&mint).is_some(),
                realm == Realm::Homeowner
            );
            assert_eq!(session.owner_proof(&mint).is_some(), realm == Realm::Owner);
        }
    }
}
