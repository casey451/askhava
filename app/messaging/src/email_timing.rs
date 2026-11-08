//! The per-job notice log and its timing record (BIZ-045, OPS-037; T198, D-332, D-347, D-1992).
//!
//! The module that sends a business notice calls [`record_notice`] once per address with its send
//! time or its quiet-hours hold, and [`mark_sent`] when a held one goes. The page behind the
//! notice's own link calls [`record_first_open`] on its first request (T199's quote form, T214's
//! Confirm page); there is no pixel, click redirect or open tracking. [`record_acted`] stores the
//! quote or Confirm time. [`notice_log`] reads one business's rows for one job, each with its
//! address's latest provider status from `messaging.email_event`. Nothing here holds a homeowner
//! detail or an address: the mailbox is an opaque ID (PRIV-001). Table:
//! `migrations/0146_messaging_notice_log.sql`.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::EmailError;

/// What a logged notice was (BIZ-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// A job card (CARD-020).
    Card,
    /// The daily open-cards email (CARD-022).
    OpenCards,
    /// A pick notice (PICK-037).
    Pick,
    /// A Confirm reminder (PICK-037).
    Reminder,
    /// A card-fix notice (FEE-020).
    CardFix,
    /// A release notice (PICK-022).
    Release,
    /// An account notice (BIZ-040, BIZ-044).
    Account,
}

impl NoticeKind {
    /// Every kind, in the stored order of the table's check.
    pub const ALL: [Self; 7] = [
        Self::Card,
        Self::OpenCards,
        Self::Pick,
        Self::Reminder,
        Self::CardFix,
        Self::Release,
        Self::Account,
    ];

    /// The stored code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::OpenCards => "open_cards",
            Self::Pick => "pick",
            Self::Reminder => "reminder",
            Self::CardFix => "card_fix",
            Self::Release => "release",
            Self::Account => "account",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == code)
    }
}

/// An address's latest status from the email provider (BIZ-045); none until an event arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderStatus {
    /// The mail server accepted it; it may still be in Spam.
    Delivered,
    /// The mail server put it off for now.
    Deferred,
    /// The mail server refused it.
    Bounced,
    /// The person marked it as spam.
    Complaint,
}

impl ProviderStatus {
    /// BIZ-045: the status a Resend event kind names; `None` for any other kind.
    #[must_use]
    pub fn from_event(kind: &str) -> Option<Self> {
        match kind {
            "email.delivered" => Some(Self::Delivered),
            "email.delivery_delayed" => Some(Self::Deferred),
            "email.bounced" => Some(Self::Bounced),
            "email.complained" => Some(Self::Complaint),
            _ => None,
        }
    }

    /// BIZ-045: the line the business's notice log shows for this status.
    #[must_use]
    pub const fn line(self) -> &'static str {
        match self {
            Self::Delivered => "Delivered to your mail server",
            Self::Deferred => "Delayed by your mail server; we'll keep trying",
            Self::Bounced => {
                "Bounced: your mail server refused it. Check this address in your notice settings."
            }
            Self::Complaint => "Marked as spam in your inbox",
        }
    }
}

/// When a notice went, or the quiet-hours hold it waits behind (QH-005, BIZ-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sending {
    /// Sent at this time.
    Sent(Timestamp),
    /// Held by quiet hours until this time.
    Held(Timestamp),
}

/// One notice to one address, to log (BIZ-045). All IDs are opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewNoticeLog {
    /// The business it went to.
    pub business: Uuid,
    /// The job it is about.
    pub job: Uuid,
    /// What it was.
    pub kind: NoticeKind,
    /// The opaque mailbox ID of the address, never the address.
    pub mailbox: Uuid,
    /// The email outbox row, whose provider events give the status.
    pub email: Option<Uuid>,
}

/// One row of a job's notice log (BIZ-045, OPS-037).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeEntry {
    /// The row ID, which [`record_first_open`] takes.
    pub id: Uuid,
    /// What it was.
    pub kind: NoticeKind,
    /// The opaque mailbox ID of the address.
    pub mailbox: Uuid,
    /// The send time, or the hold.
    pub sending: Sending,
    /// The address's latest provider status.
    pub status: Option<ProviderStatus>,
    /// When the person first opened the page behind the notice's own link (may be a scanner).
    pub first_opened: Option<Timestamp>,
    /// When the business quoted or confirmed.
    pub acted: Option<Timestamp>,
}

/// BIZ-045, OPS-037: logs `notice` with its send time or hold; returns the row ID.
///
/// # Errors
/// [`EmailError::Database`].
pub async fn record_notice(
    db: &mut PgConnection,
    notice: &NewNoticeLog,
    sending: Sending,
) -> Result<Uuid, EmailError> {
    let (sent, held) = match sending {
        Sending::Sent(at) => (Some(at.to_sqlx()), None),
        Sending::Held(until) => (None, Some(until.to_sqlx())),
    };
    sqlx::query_scalar!(
        "INSERT INTO messaging.notice_log (business_id, job_id, kind, mailbox_id, email_id, \
         sent_at, held_until) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        notice.business,
        notice.job,
        notice.kind.code(),
        notice.mailbox,
        notice.email,
        sent as _,
        held as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// OPS-037: a held notice went at `at`; false when it had a send time already.
///
/// # Errors
/// [`EmailError::Database`].
pub async fn mark_sent(db: &mut PgConnection, id: Uuid, at: Timestamp) -> Result<bool, EmailError> {
    let done = sqlx::query!(
        "UPDATE messaging.notice_log SET sent_at = $2 WHERE id = $1 AND sent_at IS NULL",
        id,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| EmailError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// OPS-037: the first request at `at` of the page behind notice `id`'s own link; a later one
/// changes nothing (false).
///
/// # Errors
/// [`EmailError::Database`].
pub async fn record_first_open(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<bool, EmailError> {
    let done = sqlx::query!(
        "UPDATE messaging.notice_log SET first_opened_at = $2 \
         WHERE id = $1 AND first_opened_at IS NULL",
        id,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| EmailError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// OPS-037: `business` quoted (cards) or confirmed (picks) `job` at `at`; stamps each `kinds` row
/// not stamped yet and returns how many.
///
/// # Errors
/// [`EmailError::Database`].
pub async fn record_acted(
    db: &mut PgConnection,
    (business, job): (Uuid, Uuid),
    kinds: &[NoticeKind],
    at: Timestamp,
) -> Result<u64, EmailError> {
    let codes: Vec<String> = kinds.iter().map(|k| k.code().to_owned()).collect();
    let done = sqlx::query!(
        "UPDATE messaging.notice_log SET acted_at = $4 WHERE business_id = $1 AND job_id = $2 \
         AND kind = ANY($3) AND acted_at IS NULL",
        business,
        job,
        &codes,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| EmailError::Database)?;
    Ok(done.rows_affected())
}

/// BIZ-045: `business`'s notice log for `job`, oldest first; another business's rows never.
///
/// # Errors
/// [`EmailError::Database`], also for a stored row the table's checks should have refused.
pub async fn notice_log(
    db: &mut PgConnection,
    business: Uuid,
    job: Uuid,
) -> Result<Vec<NoticeEntry>, EmailError> {
    let rows = sqlx::query!(
        "SELECT n.id, n.kind, n.mailbox_id, n.sent_at, n.held_until, n.first_opened_at, \
         n.acted_at, (SELECT e.kind FROM messaging.email_outbox o JOIN messaging.email_event e \
         ON e.provider_id = o.provider_id WHERE o.id = n.email_id \
         ORDER BY e.event_at DESC, e.id DESC LIMIT 1) AS \"event?\" \
         FROM messaging.notice_log n WHERE n.business_id = $1 AND n.job_id = $2 \
         ORDER BY COALESCE(n.sent_at, n.held_until), n.id",
        business,
        job,
    )
    .fetch_all(db)
    .await
    .map_err(|_| EmailError::Database)?;
    rows.into_iter()
        .map(|r| {
            let sending = match (r.sent_at, r.held_until) {
                (Some(at), _) => Sending::Sent(at.to_jiff()),
                (None, Some(until)) => Sending::Held(until.to_jiff()),
                (None, None) => return Err(EmailError::Database),
            };
            Ok(NoticeEntry {
                id: r.id,
                kind: NoticeKind::parse(&r.kind).ok_or(EmailError::Database)?,
                mailbox: r.mailbox_id,
                sending,
                status: r.event.as_deref().and_then(ProviderStatus::from_event),
                first_opened: r.first_opened_at.map(|t| t.to_jiff()),
                acted: r.acted_at.map(|t| t.to_jiff()),
            })
        })
        .collect()
}
