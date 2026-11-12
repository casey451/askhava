//! The local challenge that binds every homeowner Verify code to its purpose and subject, with
//! its try cap (SEC-055, SEC-008, PRIV-031; T346, D-2348, D-2349, pentest PT-03).
//!
//! One Twilio Verify service serves every purpose and a check names only the number, so Verify
//! alone cannot tell a sign-in code from a delete code. Each send records or reuses a row of
//! `portals.code_challenge` (migration 0176) before Verify is called ([`open_challenge`]); each
//! check first reserves a try against the newest unused, unexpired challenge of its purpose and
//! subject ([`reserve_try`]), or reaches no Verify at all; a right code then uses the challenge,
//! once, in the transaction of the action it confirms ([`use_challenge`]). Each step is one short
//! committed statement on the pool, so nothing is held across a Verify call (PT-04).

use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::business_auth::CODE_TRIES;

/// The Verify code's life, 10 minutes from its first send; one send key per window (R13).
pub(crate) const SEND_WINDOW_SECONDS: i64 = 600;

/// What a code is sent for; stored as the challenge's `purpose` (SEC-055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// Homeowner portal sign-in; the subject is her vault contact (SEC-008).
    HomeownerSignin,
    /// A copy request; the subject is the request (PRIV-031).
    DataCopy,
    /// A delete request; the subject is the request (PRIV-031).
    DataDelete,
}

impl Purpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::HomeownerSignin => "homeowner_signin",
            Self::DataCopy => "data_copy",
            Self::DataDelete => "data_delete",
        }
    }
}

/// A challenge sent at or before this instant has outlived its code at `now`.
fn expired_at_or_before(now: Timestamp) -> Timestamp {
    now.saturating_sub(SignedDuration::from_secs(SEND_WINDOW_SECONDS))
        .unwrap_or(Timestamp::MIN)
}

/// SEC-055: the challenge a send at `now` texts the code for: the newest one of `purpose` and
/// `subject` while it is unused and alive (its tries and sent time kept, since Twilio re-sends
/// the same pending code), else a new one. One committed transaction under an advisory lock on
/// the purpose and subject, so simultaneous sends open one challenge; returns its ID.
///
/// # Errors
/// [`sqlx::Error`] when the database call fails.
pub(crate) async fn open_challenge(
    pool: &PgPool,
    (purpose, subject): (Purpose, Uuid),
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended('portals.code_challenge:' || $1::text || ':' || $2::uuid::text, 0))) AS lock"#,
        purpose.as_str(),
        subject,
    )
    .fetch_one(&mut *tx)
    .await?;
    let live = sqlx::query_scalar!(
        r#"SELECT id FROM portals.code_challenge
           WHERE id = (SELECT max(id) FROM portals.code_challenge WHERE purpose = $1 AND subject_id = $2)
             AND used_at IS NULL AND sent_at > $3"#,
        purpose.as_str(),
        subject,
        expired_at_or_before(now).to_sqlx(),
    )
    .fetch_optional(&mut *tx)
    .await?;
    let id = match live {
        Some(id) => id,
        None => {
            sqlx::query_scalar!(
                r#"INSERT INTO portals.code_challenge (purpose, subject_id, sent_at)
                   VALUES ($1, $2, $3) RETURNING id"#,
                purpose.as_str(),
                subject,
                now.to_sqlx(),
            )
            .fetch_one(&mut *tx)
            .await?
        }
    };
    tx.commit().await?;
    Ok(id)
}

/// SEC-055: reserves one try against the newest challenge of `purpose` and `subject` when it is
/// unused, alive at `now` and under [`CODE_TRIES`], in one atomic committed `UPDATE` made before
/// Verify is called; `None` means the check is refused and Verify is never called. A reserved
/// try never comes back, so a crash or a timeout uses it.
///
/// # Errors
/// [`sqlx::Error`] when the database call fails.
pub(crate) async fn reserve_try(
    pool: &PgPool,
    (purpose, subject): (Purpose, Uuid),
    now: Timestamp,
) -> Result<Option<i64>, sqlx::Error> {
    let cap = i32::try_from(CODE_TRIES).unwrap_or(i32::MAX);
    sqlx::query_scalar!(
        r#"UPDATE portals.code_challenge SET tries = tries + 1
           WHERE id = (SELECT max(id) FROM portals.code_challenge WHERE purpose = $1 AND subject_id = $2)
             AND used_at IS NULL AND sent_at > $3 AND tries < $4
           RETURNING id"#,
        purpose.as_str(),
        subject,
        expired_at_or_before(now).to_sqlx(),
        cap,
    )
    .fetch_optional(pool)
    .await
}

/// SEC-055: uses challenge `id` at `now` on the caller's transaction (the one holding the action
/// the code confirms); `false` when another right check used it first, so the caller rolls back
/// and nothing is done twice.
///
/// # Errors
/// [`sqlx::Error`] when the database call fails.
pub(crate) async fn use_challenge(
    tx: &mut PgConnection,
    id: i64,
    now: Timestamp,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query!(
        "UPDATE portals.code_challenge SET used_at = $2 WHERE id = $1 AND used_at IS NULL",
        id,
        now.to_sqlx(),
    )
    .execute(&mut *tx)
    .await?;
    Ok(done.rows_affected() == 1)
}
