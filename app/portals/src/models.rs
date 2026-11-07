//! The business sign-in queries on `portals.login_link`, `portals.business_session` and
//! `portals.session` (SEC-004, SEC-024, SEC-026; T084, D-1724).
//!
//! A link token is 256 OS-random bits as hex; only its SHA256 is stored. [`use_link`] marks a link
//! used in one statement that also checks it is unused and unexpired, so of two presses of one
//! link only one signs in. Every query names only the `portals` schema.

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
