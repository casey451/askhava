//! A homeowner's opt-out and START records, for her "Download my data" file (T820, D-5399,
//! D-5804; PRIV-031, CON-022, CON-035).
//!
//! [`optout_records`] reads `messaging.optout_event` under her homeowner-phone digests as
//! `privacy::api::phone_digests` gives them (the way `intake::consent_purge` builds them), only
//! from her vault contact's creation (`ops::api::contact_created_at`) on, so a reused number never
//! shows an earlier holder's events; with no creation time known it returns none. Each record is
//! its action, channel and time only: the detection (`detected`) is never returned.

use askhava_ops::api::contact_created_at;
use askhava_privacy::api::phone_digests;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;

/// One opt-out or START event of hers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptOutRecord {
    /// True for a START (texts back on), false for an opt-out.
    pub start: bool,
    /// The stored channel: `text`, `email`, `email_reply`, `help_form`, `portal`, `owner`,
    /// `carrier` or `web_chat`.
    pub channel: String,
    /// When it took effect.
    pub at: Timestamp,
}

/// PRIV-031, CON-022, D-5399: `contact`'s opt-out and START events, oldest first, each at or after
/// her contact's creation; none when she has no phone digest or no creation time is held.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn optout_records(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Vec<OptOutRecord>, OutboxError> {
    let digests = phone_digests(db, contact)
        .await
        .map_err(|_| OutboxError::Database)?;
    let created = contact_created_at(db, contact)
        .await
        .map_err(|_| OutboxError::Database)?;
    let Some(since) = created.filter(|_| !digests.is_empty()) else {
        return Ok(Vec::new());
    };
    let hashes: Vec<Vec<u8>> = digests.iter().map(|(_, d)| d.to_vec()).collect();
    let rows = sqlx::query!(
        "SELECT action, channel, effective_at FROM messaging.optout_event \
         WHERE digest = ANY($1) AND effective_at >= $2 ORDER BY effective_at, id",
        &hashes,
        since.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| OptOutRecord {
            start: r.action == "start",
            channel: r.channel,
            at: r.effective_at.to_jiff(),
        })
        .collect())
}
