//! Leaving Ask Hava and the optional reason (BIZ-051, BIZ-054; D-446, D-463, D-491; T228).
//!
//! [`leave`] moves the business to `removed` (final, BIZ-009) with its audit row, inside the
//! caller's transaction; `portals::business_left` runs it with the quote withdrawals, the pick
//! ends, the card removal and the session ends. The ledger and audit rows are never touched.
//!
//! BIZ-054: leaving, or a pause longer than [`PAUSE_ASK_AFTER_DAYS`] ([`pause_ask_due`]), asks
//! once, optionally, why, with the [`LeaveReason::ALL`] choices, "Busy for a while" first. The
//! answer, or none, is stored once per business ([`record_reason`],
//! `migrations/0161_businesses_leaving.sql`); a business that already answered is not asked again
//! ([`reason_asked`]). Nothing here or elsewhere reads the answer for routing or fees; the weekly
//! digest line (OPS-030) is T204's.

use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, BusinessesError, db};
use crate::rules::Event;
use crate::service::apply;
use crate::status::AccountStatus;

/// BIZ-054: a pause longer than this many days asks the reason (the spec's setting, default 14).
pub const PAUSE_ASK_AFTER_DAYS: i64 = 14;

/// The longest "Something else" answer kept, in characters (the table's check).
pub const OTHER_MAX: usize = 500;

/// One BIZ-054 choice, in the order the question shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    /// "Busy for a while": the page points to BUSY (BIZ-028).
    Busy,
    /// "Too few jobs".
    FewJobs,
    /// "Jobs weren't a fit".
    NotAFit,
    /// "The fee".
    Fee,
    /// "Too many businesses quoting".
    TooMany,
    /// "Something else", with a box.
    Other,
}

impl LeaveReason {
    /// BIZ-054: every choice, "Busy for a while" first.
    pub const ALL: [Self; 6] = [
        Self::Busy,
        Self::FewJobs,
        Self::NotAFit,
        Self::Fee,
        Self::TooMany,
        Self::Other,
    ];

    /// The stored code, also the form value.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::FewJobs => "few_jobs",
            Self::NotAFit => "not_a_fit",
            Self::Fee => "fee",
            Self::TooMany => "too_many",
            Self::Other => "other",
        }
    }

    /// BIZ-054: the choice's words as the question shows them.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Busy => "Busy for a while",
            Self::FewJobs => "Too few jobs",
            Self::NotAFit => "Jobs weren't a fit",
            Self::Fee => "The fee",
            Self::TooMany => "Too many businesses quoting",
            Self::Other => "Something else",
        }
    }
}

/// What asked the question (BIZ-054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskedBy {
    /// The business left (BIZ-051).
    Left,
    /// The business stayed paused past [`PAUSE_ASK_AFTER_DAYS`].
    Paused,
}

/// A checked BIZ-054 answer: a choice, and the box's words only for "Something else". Made only
/// by [`ReasonAnswer::parse`]; `Debug` leaves the words out.
#[derive(Clone, PartialEq, Eq)]
pub struct ReasonAnswer {
    choice: LeaveReason,
    other: Option<String>,
}

impl std::fmt::Debug for ReasonAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReasonAnswer")
            .field("choice", &self.choice)
            .finish_non_exhaustive()
    }
}

impl ReasonAnswer {
    /// BIZ-054: reads the posted choice code and box. No choice (or an unknown one) is `None`:
    /// the question is optional. The box counts only beside "Something else", trimmed, cut to
    /// [`OTHER_MAX`] characters; an empty box keeps the choice alone.
    #[must_use]
    pub fn parse(choice: &str, other: &str) -> Option<Self> {
        let choice = LeaveReason::ALL.into_iter().find(|r| r.code() == choice)?;
        let words: String = other.trim().chars().take(OTHER_MAX).collect();
        let words = words.trim_end().to_owned();
        let other = (choice == LeaveReason::Other && !words.is_empty()).then_some(words);
        Some(Self { choice, other })
    }

    /// The choice.
    #[must_use]
    pub const fn choice(&self) -> LeaveReason {
        self.choice
    }
}

/// BIZ-054: whether the business was already asked (and answered or skipped) the reason.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn reason_asked(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<bool, BusinessesError> {
    let row = db(sqlx::query_scalar!(
        "SELECT true FROM businesses.leave_reason WHERE business_id = $1",
        business
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.is_some())
}

/// BIZ-054, D-463: stores the answer (or `None`, skipped) once per business; `false` when the
/// business was already asked, which keeps the first answer. Never changes a status, a pause, a
/// card match or a fee.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn record_reason(
    conn: &mut PgConnection,
    business: Uuid,
    (asked_by, answer): (AskedBy, Option<&ReasonAnswer>),
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let by = match asked_by {
        AskedBy::Left => "left",
        AskedBy::Paused => "paused",
    };
    let row = db(sqlx::query_scalar!(
        "INSERT INTO businesses.leave_reason (business_id, asked_by, choice, other_words, \
         answered_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (business_id) DO NOTHING \
         RETURNING true",
        business,
        by,
        answer.map(|a| a.choice.code()),
        answer.and_then(|a| a.other.as_deref()),
        now.to_sqlx() as _,
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.is_some())
}

/// BIZ-054: the pause question is due when the business is paused, has been for more than
/// `after_days` days (strictly), and was never asked.
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] or [`BusinessesError::Database`].
pub async fn pause_ask_due(
    conn: &mut PgConnection,
    business: Uuid,
    after_days: i64,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let row = models::read(conn, business).await?;
    let paused_for = now.duration_since(row.status_changed_at);
    let long = paused_for > SignedDuration::from_hours(after_days.saturating_mul(24));
    if row.status != AccountStatus::Paused || !long {
        return Ok(false);
    }
    Ok(!reason_asked(conn, business).await?)
}

/// BIZ-051, BIZ-009: the business leaves: `removed` (final) with its status audit row, the
/// business as actor, in the caller's transaction. Every status but `removed` may leave.
///
/// # Errors
///
/// [`BusinessesError::Refused`] when already removed; `MovedOn`, `UnknownBusiness`, `Database`
/// or `Audit`.
pub async fn leave(
    tx: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<AccountStatus, BusinessesError> {
    apply(tx, business, Event::Remove, Actor::Business(business), now).await
}
