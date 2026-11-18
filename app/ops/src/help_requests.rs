//! Help requests: one row per request, its message references and the HELP keyword times
//! (OPS-036, CON-034, PRIV-030; T151, D-348, D-3753, D-3881, D-3883, D-4044).
//!
//! This module is the only writer and reader of `ops.help_request`, `ops.help_message` and
//! `ops.help_keyword` (`migrations/0194_help_requests.sql`). A message on an open request of the
//! same asker within [`HelpLimits::join_within`] of its opening joins it with no new notification;
//! any other opens a new request, notified at once unless [`HelpLimits::daily`] requests were
//! already notified that Arizona day. Each request also gets one OPS-032 push item of
//! [`HELP_PUSH_ITEM`], listed from its due time less 24 hours, or at once when the daily limit
//! held its notification back, so it reaches the next 9 a.m. or 3 p.m. push. A message is kept as
//! its `privacy.message_body` inbox ID (`body_ref`), never its words (PRIV-001). The caller keeps
//! the words and sends the notification (`intake`, `messaging`): `ops` sends nothing.

use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::alerts::add_push_item;
use crate::models::OpsError;

/// The OPS-032 push item kind and subject kind of a help request.
pub const HELP_PUSH_ITEM: &str = "ops.help_request";
/// CON-034: a HELP keyword counts for the next message within this long.
pub const HELP_ANSWERED_WITHIN: SignedDuration = SignedDuration::from_hours(24);

/// OPS-036's two limits (settings): the join window and the notifications a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpLimits {
    /// A message within this long of an open request's opening joins it (1 hour).
    pub join_within: SignedDuration,
    /// Past this many notifications in an Arizona day, a request waits for the push (10).
    pub daily: i64,
}

impl Default for HelpLimits {
    fn default() -> Self {
        Self {
            join_within: SignedDuration::from_hours(1),
            daily: 10,
        }
    }
}

/// Who asked: a homeowner's vault contact, a business, or a help email with no match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    /// A homeowner, by vault contact ID.
    Homeowner(Uuid),
    /// A business, by ID.
    Business(Uuid),
    /// A help email or form with no record to tie it to; it never joins another request.
    Email,
}

impl Asker {
    /// The stored code name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Homeowner(_) => "homeowner",
            Self::Business(_) => "business",
            Self::Email => "email",
        }
    }

    const fn id(self) -> Option<Uuid> {
        match self {
            Self::Homeowner(id) | Self::Business(id) => Some(id),
            Self::Email => None,
        }
    }
}

/// What [`open_help_request`] did with one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpOpened {
    /// A new request; `notify` is false when the daily limit held its notification back.
    Opened {
        /// The new request.
        request: Uuid,
        /// Whether the caller sends its one notification now.
        notify: bool,
    },
    /// The message joined this open request; no notification.
    Joined(Uuid),
}

impl HelpOpened {
    /// The request the message belongs to.
    #[must_use]
    pub const fn request(self) -> Uuid {
        match self {
            Self::Opened { request, .. } | Self::Joined(request) => request,
        }
    }
}

/// CON-034: records that `contact` texted HELP at `at` (no request, no notification).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn record_help_keyword(
    db: &mut PgConnection,
    contact: Uuid,
    at: Timestamp,
) -> Result<(), OpsError> {
    sqlx::query!(
        "INSERT INTO ops.help_keyword (contact_id, at) VALUES ($1, $2) \
         ON CONFLICT (contact_id) DO UPDATE SET at = EXCLUDED.at",
        contact,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map(drop)
    .map_err(|_| OpsError::Database)
}

/// CON-034: whether `contact` texted HELP within [`HELP_ANSWERED_WITHIN`] before `at`.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn help_keyword_recent(
    db: &mut PgConnection,
    contact: Uuid,
    at: Timestamp,
) -> Result<bool, OpsError> {
    let since = at - HELP_ANSWERED_WITHIN;
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM ops.help_keyword WHERE contact_id = $1 \
         AND at >= $2 AND at <= $3) AS \"recent!\"",
        contact,
        since.to_sqlx() as _,
        at.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// OPS-036, D-4044, JOB-006: one help message from `asker`, its words kept under `body_ref`, at
/// `at`, answered by `due`: joins an open request opened within the join window, or opens one,
/// notified now within the daily limit, with its OPS-032 push item. A `body_ref` already kept (a
/// rerun) returns its request as joined, with nothing written; the whole check-then-insert runs
/// under one transaction lock, so two requests at once cannot beat either limit (D-4278).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn open_help_request(
    db: &mut PgConnection,
    asker: Asker,
    (body_ref, at, due): (Uuid, Timestamp, Timestamp),
    limits: HelpLimits,
) -> Result<HelpOpened, OpsError> {
    if let Some(request) = already_kept(db, body_ref).await? {
        return Ok(HelpOpened::Joined(request));
    }
    if let Some(request) = joinable(db, asker, at, limits).await? {
        add_message(db, request, body_ref, (false, at)).await?;
        return Ok(HelpOpened::Joined(request));
    }
    let notify = notified_today(db, at).await? < limits.daily;
    let request = sqlx::query_scalar!(
        "INSERT INTO ops.help_request (asker, asker_id, opened_at, due_at, notified_at) \
         VALUES ($1, $2, $3, $4, CASE WHEN $5 THEN $3::timestamptz END) RETURNING id",
        asker.code(),
        asker.id(),
        at.to_sqlx() as _,
        due.to_sqlx() as _,
        notify,
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| OpsError::Database)?;
    add_message(db, request, body_ref, (false, at)).await?;
    let show_from = if notify {
        due - SignedDuration::from_hours(24)
    } else {
        at
    };
    add_push_item(db, HELP_PUSH_ITEM, (HELP_PUSH_ITEM, request), show_from).await?;
    Ok(HelpOpened::Opened { request, notify })
}

/// Takes the help-request transaction lock (held to commit), then finds the request that already
/// holds the asker's message `body_ref`, open or closed (JOB-006, D-4278).
async fn already_kept(db: &mut PgConnection, body_ref: Uuid) -> Result<Option<Uuid>, OpsError> {
    sqlx::query!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended('ops.help_request', 151))) AS lock"#
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| OpsError::Database)?;
    sqlx::query_scalar!(
        "SELECT request_id FROM ops.help_message WHERE body_ref = $1 AND NOT from_owner LIMIT 1",
        body_ref,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// The asker's newest open request opened within the join window before `at`.
async fn joinable(
    db: &mut PgConnection,
    asker: Asker,
    at: Timestamp,
    limits: HelpLimits,
) -> Result<Option<Uuid>, OpsError> {
    let Some(id) = asker.id() else {
        return Ok(None);
    };
    let since = at - limits.join_within;
    sqlx::query_scalar!(
        "SELECT id FROM ops.help_request WHERE asker_id = $1 AND asker = $2 \
         AND closed_at IS NULL AND opened_at >= $3 AND opened_at <= $4 \
         ORDER BY opened_at DESC LIMIT 1",
        id,
        asker.code(),
        since.to_sqlx() as _,
        at.to_sqlx() as _,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)
}

/// The requests notified in `at`'s Arizona day, up to `at`.
async fn notified_today(db: &mut PgConnection, at: Timestamp) -> Result<i64, OpsError> {
    sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM ops.help_request WHERE notified_at <= $1 \
         AND (notified_at AT TIME ZONE 'America/Phoenix')::date \
           = ($1::timestamptz AT TIME ZONE 'America/Phoenix')::date",
        at.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| OpsError::Database)
}

async fn add_message(
    db: &mut PgConnection,
    request: Uuid,
    body_ref: Uuid,
    (from_owner, at): (bool, Timestamp),
) -> Result<(), OpsError> {
    sqlx::query!(
        "INSERT INTO ops.help_message (request_id, body_ref, from_owner, at) \
         VALUES ($1, $2, $3, $4)",
        request,
        body_ref,
        from_owner,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map(drop)
    .map_err(|_| OpsError::Database)
}

/// OPS-036, D-3883: the owner's reply on `request`, its words kept under `body_ref` by the caller.
///
/// # Errors
///
/// [`OpsError::Database`] (an unknown request included: the reference is a foreign key).
pub async fn add_owner_reply(
    db: &mut PgConnection,
    request: Uuid,
    body_ref: Uuid,
    at: Timestamp,
) -> Result<(), OpsError> {
    add_message(db, request, body_ref, (true, at)).await
}

/// OPS-036, D-3883: closes `request` at `at` (its open push item marked done) or, with `open`,
/// reopens it (listed again from `at`); false when it was already in that state or is unknown.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn set_help_closed(
    db: &mut PgConnection,
    request: Uuid,
    (closed, at): (bool, Timestamp),
) -> Result<bool, OpsError> {
    let moved = sqlx::query_scalar!(
        "UPDATE ops.help_request SET closed_at = CASE WHEN $2 THEN $3::timestamptz END \
         WHERE id = $1 AND (closed_at IS NULL) = $2 RETURNING id",
        request,
        closed,
        at.to_sqlx() as _,
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| OpsError::Database)?;
    if moved.is_none() {
        return Ok(false);
    }
    if closed {
        sqlx::query!(
            "UPDATE ops.push_item SET done_at = $3 WHERE kind = $1 AND subject_kind = $1 \
             AND subject_id = $2 AND done_at IS NULL",
            HELP_PUSH_ITEM,
            request,
            at.to_sqlx() as _,
        )
        .execute(db)
        .await
        .map_err(|_| OpsError::Database)?;
    } else {
        add_push_item(db, HELP_PUSH_ITEM, (HELP_PUSH_ITEM, request), at).await?;
    }
    Ok(true)
}

/// One help request with its messages, oldest first (D-3883).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRequest {
    /// The request.
    pub id: Uuid,
    /// The asker's code name (`homeowner`, `business` or `email`).
    pub asker: String,
    /// The vault contact or business; `None` for a help email.
    pub asker_id: Option<Uuid>,
    /// When it opened.
    pub opened_at: Timestamp,
    /// The reply-by time.
    pub due_at: Timestamp,
    /// When its one notification went; `None` while the daily limit holds it.
    pub notified_at: Option<Timestamp>,
    /// When it closed; `None` while open.
    pub closed_at: Option<Timestamp>,
    /// Each message: its `privacy.message_body` inbox ID, whether the owner wrote it, and when.
    pub messages: Vec<(Uuid, bool, Timestamp)>,
}

/// D-491, D-3883: the open requests and their due times, soonest due first (T524's digest, T530).
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn open_help_requests(db: &mut PgConnection) -> Result<Vec<(Uuid, Timestamp)>, OpsError> {
    let rows = sqlx::query!(
        "SELECT id, due_at FROM ops.help_request WHERE closed_at IS NULL ORDER BY due_at, id"
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.id, r.due_at.to_jiff()))
        .collect())
}

/// D-3883: one request with its message references; `None` for an unknown ID.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn help_request(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<HelpRequest>, OpsError> {
    let Some(r) = sqlx::query!(
        "SELECT id, asker, asker_id, opened_at, due_at, notified_at, closed_at \
         FROM ops.help_request WHERE id = $1",
        id
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| OpsError::Database)?
    else {
        return Ok(None);
    };
    let messages = sqlx::query!(
        "SELECT body_ref, from_owner, at FROM ops.help_message WHERE request_id = $1 \
         ORDER BY at, id",
        id
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(Some(HelpRequest {
        id: r.id,
        asker: r.asker,
        asker_id: r.asker_id,
        opened_at: r.opened_at.to_jiff(),
        due_at: r.due_at.to_jiff(),
        notified_at: r.notified_at.map(|t| t.to_jiff()),
        closed_at: r.closed_at.map(|t| t.to_jiff()),
        messages: messages
            .into_iter()
            .map(|m| (m.body_ref, m.from_owner, m.at.to_jiff()))
            .collect(),
    }))
}
