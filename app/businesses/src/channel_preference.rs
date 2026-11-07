//! The business's notice channel (email, text or both) and the recipients of one notice with each
//! person's channels (BIZ-019, BIZ-036, BIZ-037, TXT-035, TXT-037, D-307, D-308, D-491, D-492).
//!
//! Email is the default (no row in `businesses.notice_channel`) and the only choice while the
//! business-texting switch is off; [`set_notice_channel`] refuses text or both then. The enqueuing
//! module (`quotes`, `picks`, `businesses`) calls [`notice_recipients`] and puts the opaque IDs it
//! returns in its `messaging.business_notice` payload; `messaging` never calls this crate. A phone
//! listed here is only a candidate: the sender checks [`crate::api::may_text`] for it at send
//! time (D-1715, D-1760).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::team::{NoticePurpose, TeamError, members};

/// How a business wants its job cards, picks and reminders (BIZ-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeChannel {
    /// Email only: the default and the only choice while business texting is off (TXT-035).
    Email,
    /// Text only, to each recipient's linked phone with text consent.
    Text,
    /// Email and text.
    Both,
}

impl NoticeChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Text => "text",
            Self::Both => "both",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "text" => Self::Text,
            "both" => Self::Both,
            _ => Self::Email,
        }
    }
}

/// Why a channel choice was not stored.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChannelError {
    /// Text or both while the business-texting switch is off (BIZ-036, TXT-035).
    #[error("text is offered only while business texting is on")]
    TextingOff,
    /// A statement failed.
    #[error("database error")]
    Database,
}

impl From<TeamError> for ChannelError {
    fn from(_: TeamError) -> Self {
        Self::Database
    }
}

/// One person a notice goes to: their sign-in email's opaque ID, and their linked phones' line
/// IDs when the business chose text or both (BIZ-037). `email` is false when the business chose
/// text only and the person has a phone; the sender still falls back to the email (BIZ-019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeRecipient {
    /// The opaque ID of the person's sign-in email (D-492).
    pub mailbox: Uuid,
    /// Whether the notice goes to that email as well as any text.
    pub email: bool,
    /// The line IDs of the person's linked phones that may get the text (CON-019 is checked later).
    pub phones: Vec<Uuid>,
}

/// BIZ-036, TXT-035, D-307: stores `channel` for `business`; text and both only while
/// `business_texting_on` (the switch, which needs the filed written answer and the business number).
///
/// # Errors
/// [`ChannelError::TextingOff`] or [`ChannelError::Database`].
pub async fn set_notice_channel(
    conn: &mut PgConnection,
    business: Uuid,
    (channel, business_texting_on): (NoticeChannel, bool),
    now: Timestamp,
) -> Result<(), ChannelError> {
    if channel != NoticeChannel::Email && !business_texting_on {
        return Err(ChannelError::TextingOff);
    }
    sqlx::query!(
        "INSERT INTO businesses.notice_channel (business_id, channel, changed_at) \
         VALUES ($1, $2, $3) ON CONFLICT (business_id) \
         DO UPDATE SET channel = EXCLUDED.channel, changed_at = EXCLUDED.changed_at",
        business,
        channel.as_str(),
        now.to_sqlx() as _
    )
    .execute(conn)
    .await
    .map(|_| ())
    .map_err(|_| ChannelError::Database)
}

/// BIZ-036: the business's stored channel, email when it never chose.
///
/// # Errors
/// [`ChannelError::Database`].
pub async fn notice_channel(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<NoticeChannel, ChannelError> {
    let raw = sqlx::query_scalar!(
        "SELECT channel FROM businesses.notice_channel WHERE business_id = $1",
        business
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| ChannelError::Database)?;
    Ok(raw
        .as_deref()
        .map_or(NoticeChannel::Email, NoticeChannel::parse))
}

/// BIZ-019, BIZ-037, TXT-037, D-492: the people a notice of `purpose` goes to (the owner and every
/// member whose roles fit it, owner first), each with their channels. While business texting is
/// off (`business_texting_on` false) nobody gets a phone, whatever the stored choice.
///
/// # Errors
/// [`ChannelError::Database`].
pub async fn notice_recipients(
    conn: &mut PgConnection,
    business: Uuid,
    purpose: NoticePurpose,
    business_texting_on: bool,
) -> Result<Vec<NoticeRecipient>, ChannelError> {
    let channel = notice_channel(conn, business).await?;
    let texts = business_texting_on && channel != NoticeChannel::Email;
    let phones = if texts {
        sqlx::query!(
            "SELECT id, linked_by FROM businesses.staff_phone WHERE business_id = $1 \
             ORDER BY linked_at, id",
            business
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(|_| ChannelError::Database)?
    } else {
        Vec::new()
    };
    let people = members(conn, business).await?;
    Ok(people
        .into_iter()
        .filter(|m| m.owner || m.roles.fits(purpose))
        .map(|m| {
            let mine: Vec<Uuid> = phones
                .iter()
                .filter(|p| p.linked_by == m.id)
                .map(|p| p.id)
                .collect();
            NoticeRecipient {
                mailbox: m.mailbox,
                email: channel != NoticeChannel::Text || mine.is_empty(),
                phones: mine,
            }
        })
        .collect())
}
