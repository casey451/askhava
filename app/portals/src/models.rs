//! The business sign-in queries on `portals.login_link`, `portals.business_session` and
//! `portals.session` (SEC-004, SEC-024, SEC-026; T084, D-1724).
//!
//! A link token is 256 OS-random bits as hex; only its SHA256 is stored. [`use_link`] marks a link
//! used in one statement that also checks it is unused and unexpired, so of two presses of one
//! link only one signs in. Every query names only the `portals` schema.
//!
//! T086 added the owner queries on `portals.owner`, `owner_factor`, `owner_recovery_code`,
//! `owner_session` and `owner_challenge` (SEC-005, SEC-007, SEC-043, SEC-044, D-1917): a TOTP
//! step, a recovery code and a passkey challenge are each used by one guarded statement.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::session::PortalsError;

/// A new 256-bit link token as 64 hex characters (SEC-026: at least 128 random bits).
pub(crate) fn new_link_token() -> Result<String, PortalsError> {
    let mut bytes = [0u8; 32];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| PortalsError::NoRandom)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, PortalsError> {
    result.map_err(|_| PortalsError::Database)
}

/// Stores the link `token` for `member` of `business`, made at `now`, ending at `expires`.
pub(crate) async fn insert_link(
    conn: &mut PgConnection,
    token: &str,
    (member, business): (Uuid, Uuid),
    (now, expires): (Timestamp, Timestamp),
) -> Result<(), PortalsError> {
    db(sqlx::query!(
        "INSERT INTO portals.login_link \
         (token_sha256, member_id, business_id, created_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5)",
        digest(token),
        member,
        business,
        now.to_sqlx(),
        expires.to_sqlx(),
    )
    .execute(conn)
    .await)
    .map(|_| ())
}

/// How many links `member` was sent since `since`.
pub(crate) async fn links_since(
    conn: &mut PgConnection,
    member: Uuid,
    since: Timestamp,
) -> Result<i64, PortalsError> {
    let count = db(sqlx::query_scalar!(
        "SELECT count(*) FROM portals.login_link WHERE member_id = $1 AND created_at > $2",
        member,
        since.to_sqlx(),
    )
    .fetch_one(conn)
    .await)?;
    Ok(count.unwrap_or(0))
}

/// Whether `token` names a link that is unused and unexpired at `now`; reads only (SEC-026).
pub(crate) async fn link_open(
    conn: &mut PgConnection,
    token: &str,
    now: Timestamp,
) -> Result<bool, PortalsError> {
    let row = db(sqlx::query_scalar!(
        "SELECT id FROM portals.login_link \
         WHERE token_sha256 = $1 AND used_at IS NULL AND expires_at > $2",
        digest(token),
        now.to_sqlx(),
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.is_some())
}

/// Uses `token` at `now`: the link's `(member, business)` when it was unused and unexpired, and
/// it is used from then on; `None` otherwise, changing nothing (SEC-004, single use).
pub(crate) async fn use_link(
    conn: &mut PgConnection,
    token: &str,
    now: Timestamp,
) -> Result<Option<(Uuid, Uuid)>, PortalsError> {
    let row = db(sqlx::query!(
        "UPDATE portals.login_link SET used_at = $2 \
         WHERE token_sha256 = $1 AND used_at IS NULL AND expires_at > $2 \
         RETURNING member_id, business_id",
        digest(token),
        now.to_sqlx(),
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.map(|r| (r.member_id, r.business_id)))
}

/// Names `business` as the business of session `session`.
pub(crate) async fn insert_business_session(
    conn: &mut PgConnection,
    session: Uuid,
    business: Uuid,
) -> Result<(), PortalsError> {
    db(sqlx::query!(
        "INSERT INTO portals.business_session (session_id, business_id) VALUES ($1, $2)",
        session,
        business,
    )
    .execute(conn)
    .await)
    .map(|_| ())
}

/// The `(member, business)` of business session `session`, or `None`.
pub(crate) async fn session_member(
    conn: &mut PgConnection,
    session: Uuid,
) -> Result<Option<(Uuid, Uuid)>, PortalsError> {
    let row = db(sqlx::query!(
        "SELECT s.actor_id, b.business_id FROM portals.session s \
         JOIN portals.business_session b ON b.session_id = s.id \
         WHERE s.id = $1 AND s.realm = 'business'",
        session,
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.map(|r| (r.actor_id, r.business_id)))
}

/// Ends every business session of `member` and uses up its unused links at `now` (SEC-004,
/// SEC-024). Returns the sessions ended.
pub(crate) async fn end_member(
    conn: &mut PgConnection,
    member: Uuid,
    now: Timestamp,
) -> Result<u64, PortalsError> {
    db(sqlx::query!(
        "UPDATE portals.login_link SET used_at = $2 WHERE member_id = $1 AND used_at IS NULL",
        member,
        now.to_sqlx(),
    )
    .execute(&mut *conn)
    .await)?;
    let ended = db(sqlx::query!(
        "DELETE FROM portals.session WHERE realm = 'business' AND actor_id = $1",
        member,
    )
    .execute(conn)
    .await)?;
    Ok(ended.rows_affected())
}

/// One owner session as [`crate::owner_auth::owner_principal`] reads it (SEC-043).
pub(crate) struct OwnerSessionRow {
    /// The `portals.session` row.
    pub(crate) session: Uuid,
    /// The owner record.
    pub(crate) owner: Uuid,
    /// The last request.
    pub(crate) last_seen_at: Timestamp,
    /// The last second factor.
    pub(crate) factor_at: Timestamp,
}

/// Inserts the one owner with password hash `hash`; `None` when an owner exists (SEC-044).
pub(crate) async fn insert_owner(
    conn: &mut PgConnection,
    hash: &str,
    now: Timestamp,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO portals.owner (password_hash, created_at) VALUES ($1, $2) \
         ON CONFLICT (singleton) DO NOTHING RETURNING id",
        hash,
        now.to_sqlx(),
    )
    .fetch_optional(conn)
    .await
}

/// The owner's ID and argon2id hash, or `None` before the bootstrap.
pub(crate) async fn owner_password(
    conn: &mut PgConnection,
) -> Result<Option<(Uuid, String)>, sqlx::Error> {
    let row = sqlx::query!("SELECT id, password_hash FROM portals.owner")
        .fetch_optional(conn)
        .await?;
    Ok(row.map(|r| (r.id, r.password_hash)))
}

/// Replaces the owner's password hash (SEC-043).
pub(crate) async fn set_owner_password(
    conn: &mut PgConnection,
    owner: Uuid,
    hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE portals.owner SET password_hash = $2 WHERE id = $1",
        owner,
        hash
    )
    .execute(conn)
    .await
    .map(|_| ())
}

/// Registers a TOTP authenticator with its 20-byte `secret`; returns the factor ID.
pub(crate) async fn insert_totp(
    conn: &mut PgConnection,
    owner: Uuid,
    secret: &[u8],
    now: Timestamp,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO portals.owner_factor (owner_id, kind, totp_secret, created_at) \
         VALUES ($1, 'totp', $2, $3) RETURNING id",
        owner,
        secret,
        now.to_sqlx(),
    )
    .fetch_one(conn)
    .await
}

/// Registers a passkey (webauthn-rs's credential JSON); returns the factor ID.
pub(crate) async fn insert_passkey(
    conn: &mut PgConnection,
    owner: Uuid,
    passkey: serde_json::Value,
    now: Timestamp,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO portals.owner_factor (owner_id, kind, passkey, created_at) \
         VALUES ($1, 'passkey', $2, $3) RETURNING id",
        owner,
        passkey,
        now.to_sqlx(),
    )
    .fetch_one(conn)
    .await
}

/// How many second factors the owner has registered (SEC-007).
pub(crate) async fn factor_count(conn: &mut PgConnection, owner: Uuid) -> Result<i64, sqlx::Error> {
    let count = sqlx::query_scalar!(
        "SELECT count(*) FROM portals.owner_factor WHERE owner_id = $1",
        owner
    )
    .fetch_one(conn)
    .await?;
    Ok(count.unwrap_or(0))
}

/// The owner's TOTP factors and their secrets.
pub(crate) async fn totp_factors(
    conn: &mut PgConnection,
    owner: Uuid,
) -> Result<Vec<(Uuid, Vec<u8>)>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT id, totp_secret AS \"secret!\" FROM portals.owner_factor \
         WHERE owner_id = $1 AND kind = 'totp' ORDER BY created_at, id",
        owner
    )
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.secret)).collect())
}

/// Accepts TOTP step `step` for `factor` only if it is later than the last one accepted, so a
/// code is never accepted twice (RFC 6238 section 5.2).
pub(crate) async fn use_totp_step(
    conn: &mut PgConnection,
    factor: Uuid,
    step: i64,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query!(
        "UPDATE portals.owner_factor SET totp_last_step = $2 \
         WHERE id = $1 AND (totp_last_step IS NULL OR totp_last_step < $2)",
        factor,
        step
    )
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// The owner's passkeys as webauthn-rs JSON.
pub(crate) async fn passkeys(
    conn: &mut PgConnection,
    owner: Uuid,
) -> Result<Vec<(Uuid, serde_json::Value)>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT id, passkey AS \"passkey!\" FROM portals.owner_factor \
         WHERE owner_id = $1 AND kind = 'passkey' ORDER BY created_at, id",
        owner
    )
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|r| (r.id, r.passkey)).collect())
}

/// Stores a passkey's updated counter and flags after a sign-in.
pub(crate) async fn update_passkey(
    conn: &mut PgConnection,
    factor: Uuid,
    passkey: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE portals.owner_factor SET passkey = $2 WHERE id = $1 AND kind = 'passkey'",
        factor,
        passkey
    )
    .execute(conn)
    .await
    .map(|_| ())
}

/// Stores one recovery code's SHA256 (SEC-007).
pub(crate) async fn insert_recovery_code(
    conn: &mut PgConnection,
    owner: Uuid,
    digest: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO portals.owner_recovery_code (owner_id, code_sha256) VALUES ($1, $2)",
        owner,
        digest
    )
    .execute(conn)
    .await
    .map(|_| ())
}

/// Uses the recovery code with SHA256 `digest` at `now`: true once, false after (SEC-007).
pub(crate) async fn use_recovery_code(
    conn: &mut PgConnection,
    owner: Uuid,
    digest: &[u8],
    now: Timestamp,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query!(
        "UPDATE portals.owner_recovery_code SET used_at = $3 \
         WHERE owner_id = $1 AND code_sha256 = $2 AND used_at IS NULL",
        owner,
        digest,
        now.to_sqlx(),
    )
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Adds the owner row to the session whose token hashes to `digest`; returns the session ID.
pub(crate) async fn insert_owner_session(
    conn: &mut PgConnection,
    digest: &[u8],
    now: Timestamp,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO portals.owner_session (session_id, last_seen_at, factor_at) \
         SELECT id, $2, $2 FROM portals.session WHERE token_sha256 = $1 AND realm = 'owner' \
         RETURNING session_id",
        digest,
        now.to_sqlx(),
    )
    .fetch_one(conn)
    .await
}

/// The unexpired owner session whose token hashes to `digest`, or `None`.
pub(crate) async fn owner_session(
    conn: &mut PgConnection,
    digest: &[u8],
    now: Timestamp,
) -> Result<Option<OwnerSessionRow>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT s.id, s.actor_id, o.last_seen_at, o.factor_at FROM portals.session s \
         JOIN portals.owner_session o ON o.session_id = s.id \
         WHERE s.token_sha256 = $1 AND s.realm = 'owner' AND s.expires_at > $2",
        digest,
        now.to_sqlx(),
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|r| OwnerSessionRow {
        session: r.id,
        owner: r.actor_id,
        last_seen_at: r.last_seen_at.to_jiff(),
        factor_at: r.factor_at.to_jiff(),
    }))
}

/// Restarts session `session`'s idle clock at `now`.
pub(crate) async fn touch_owner_session(
    conn: &mut PgConnection,
    session: Uuid,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE portals.owner_session SET last_seen_at = $2 WHERE session_id = $1",
        session,
        now.to_sqlx(),
    )
    .execute(conn)
    .await
    .map(|_| ())
}

/// Records a second factor for session `session` at `now` (the step-up).
pub(crate) async fn set_factor_at(
    conn: &mut PgConnection,
    session: Uuid,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE portals.owner_session SET factor_at = $2 WHERE session_id = $1",
        session,
        now.to_sqlx(),
    )
    .execute(conn)
    .await
    .map(|_| ())
}

/// Deletes session `session` (its owner row goes with it).
pub(crate) async fn end_session(conn: &mut PgConnection, session: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM portals.session WHERE id = $1", session)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Deletes the owner session whose token hashes to `digest` (logout).
pub(crate) async fn end_owner_session_by_token(
    conn: &mut PgConnection,
    digest: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM portals.session WHERE token_sha256 = $1 AND realm = 'owner'",
        digest
    )
    .execute(conn)
    .await
    .map(|_| ())
}

/// Deletes every owner session ("Sign out everywhere"); returns how many.
pub(crate) async fn end_owner_sessions(conn: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!("DELETE FROM portals.session WHERE realm = 'owner'")
        .execute(conn)
        .await?;
    Ok(done.rows_affected())
}

/// Stores a passkey ceremony's state for `purpose`, ending at `expires`; returns its ID.
pub(crate) async fn insert_challenge(
    conn: &mut PgConnection,
    purpose: &str,
    state: serde_json::Value,
    expires: Timestamp,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO portals.owner_challenge (purpose, state, expires_at) \
         VALUES ($1, $2, $3) RETURNING id",
        purpose,
        state,
        expires.to_sqlx(),
    )
    .fetch_one(conn)
    .await
}

/// Uses challenge `id` of `purpose`: its state once while unexpired at `now`, else `None`.
pub(crate) async fn take_challenge(
    conn: &mut PgConnection,
    id: Uuid,
    purpose: &str,
    now: Timestamp,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar!(
        "DELETE FROM portals.owner_challenge \
         WHERE id = $1 AND purpose = $2 AND expires_at > $3 RETURNING state",
        id,
        purpose,
        now.to_sqlx(),
    )
    .fetch_optional(conn)
    .await
}
