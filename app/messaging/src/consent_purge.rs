//! The `messaging` half of the CON-022 five-year purge (CON-022, CON-036; D-2208, D-3155, D-3753).
//!
//! CON-022 keeps a number's consent and opt-out records and its message logs 5 years after the last
//! message to that number. A number's last message is the later of its newest
//! `messaging.inbox_contact` row (each inbound text's vault contact, written by `on_inbound_text`)
//! and its newest `messaging.outbox` row (sent, else queued). "Older than 5 years" is always
//! strictly older than `now() - 1827 days` on the database clock, never a time the caller passes, so
//! a leap day never cuts it short. [`due_numbers`] lists the numbers past it; [`purge_optout_events`]
//! deletes their old opt-out events through `messaging.purge_expired_optout_events` (migration
//! 0195: `SECURITY DEFINER`, since the table is append-only for the app role; it keeps an event a
//! suppression row references and the number's newest one); [`delete_message_log`] deletes their
//! outbox and inbox rows. The caller (the daily `intake.consent_sweep`, T563) runs, per number in
//! one transaction, the opt-out purge and its `intake` purge before the message-log delete (the
//! guards read those rows) and writes the `ops` audit rows from the IDs returned. Only opaque IDs
//! and blind-index digests pass through here (PRIV-002).

use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::OutboxError;

/// The message-log rows [`delete_message_log`] removed, IDs only, for the audit rows (CON-022).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedLog {
    /// The `messaging.outbox` rows (with their retry, portal-notice and delivery-event rows).
    pub outbox: Vec<Uuid>,
    /// The `messaging.inbox` IDs whose `messaging.inbox_contact` rows went (with the inbox rows).
    pub inbox: Vec<Uuid>,
}

/// Up to `limit` vault contacts whose last message (inbound or outbound) is strictly older than
/// 1,827 days on the database clock, oldest ID first (CON-022, D-3155).
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn due_numbers(db: &mut PgConnection, limit: i64) -> Result<Vec<Uuid>, OutboxError> {
    sqlx::query_scalar!(
        r#"SELECT m.contact AS "contact!" FROM (
             SELECT c.contact_id AS contact, c.received_at AS at FROM messaging.inbox_contact c
             UNION ALL
             SELECT o.recipient_id, coalesce(o.sent_at, o.created_at) FROM messaging.outbox o
           ) AS m
           GROUP BY m.contact
           HAVING max(m.at) < now() - interval '1827 days'
           ORDER BY m.contact
           LIMIT $1"#,
        limit
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// Deletes the opt-out events of `contact`'s number (found by `digests`, its homeowner-phone
/// blind-index digests under every key version, from `privacy::api`) that are past CON-022's
/// 5 years, only when its last message is too, and returns their IDs. An event a
/// `messaging.suppression` row references and the number's newest event are always kept, so the
/// opt-out stays honored (CON-022, CON-035, CON-036, D-3753).
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn purge_optout_events(
    db: &mut PgConnection,
    contact: Uuid,
    digests: &[(u16, [u8; 32])],
) -> Result<Vec<Uuid>, OutboxError> {
    let hashes: Vec<Vec<u8>> = digests.iter().map(|(_, d)| d.to_vec()).collect();
    sqlx::query_scalar!(
        r#"SELECT messaging.purge_expired_optout_events($1, $2) AS "id!""#,
        contact,
        &hashes
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// Deletes `contact`'s message log once its last message is past CON-022's 5 years: its outbox
/// rows (with their retry, portal-notice and delivery-event rows) and its inbound texts' inbox and
/// `inbox_contact` rows, each itself strictly older than 1,827 days on the database clock. A HELP
/// is its inbox row and its reply's outbox row, so it goes here too (D-3155). Nothing goes while
/// the number has a newer message. Runs in a savepoint of the caller's transaction.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn delete_message_log(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<DeletedLog, OutboxError> {
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    if !log_due(&mut tx, contact).await? {
        return Ok(DeletedLog::default());
    }
    let outbox = delete_outbox(&mut tx, contact).await?;
    let inbox = sqlx::query_scalar!(
        "WITH gone AS ( \
           DELETE FROM messaging.inbox_contact c \
           WHERE c.contact_id = $1 AND c.received_at < now() - interval '1827 days' \
           RETURNING c.inbox_id \
         ), inbox AS ( \
           DELETE FROM messaging.inbox i USING gone g WHERE i.id = g.inbox_id \
         ) \
         SELECT g.inbox_id FROM gone g ORDER BY g.inbox_id",
        contact
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| OutboxError::Database)?;
    tx.commit().await.map_err(|_| OutboxError::Database)?;
    Ok(DeletedLog { outbox, inbox })
}

/// Whether `contact`'s last message is strictly older than 1,827 days (the guard
/// `messaging.last_message_at` applies in the definer functions; CON-022).
async fn log_due(db: &mut PgConnection, contact: Uuid) -> Result<bool, OutboxError> {
    let due = sqlx::query_scalar!(
        r#"SELECT max(m.at) < now() - interval '1827 days' AS "due" FROM (
             SELECT c.received_at AS at FROM messaging.inbox_contact c WHERE c.contact_id = $1
             UNION ALL
             SELECT coalesce(o.sent_at, o.created_at) FROM messaging.outbox o
             WHERE o.recipient_id = $1
           ) AS m"#,
        contact
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(due.unwrap_or(false))
}

/// Deletes `contact`'s outbox rows older than 1,827 days, their rows that reference them first,
/// and returns the outbox IDs (CON-022).
async fn delete_outbox(db: &mut PgConnection, contact: Uuid) -> Result<Vec<Uuid>, OutboxError> {
    let ids = sqlx::query_scalar!(
        "SELECT o.id FROM messaging.outbox o WHERE o.recipient_id = $1 \
         AND coalesce(o.sent_at, o.created_at) < now() - interval '1827 days' ORDER BY o.id",
        contact
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    sqlx::query!(
        "WITH retry AS (DELETE FROM messaging.text_retry t WHERE t.outbox_id = ANY ($1)), \
         notice AS (DELETE FROM messaging.portal_notice n WHERE n.outbox_id = ANY ($1)) \
         DELETE FROM messaging.delivery_event e USING messaging.outbox o \
         WHERE o.id = ANY ($1) AND e.provider_sid = o.provider_sid",
        &ids
    )
    .execute(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    sqlx::query!("DELETE FROM messaging.outbox o WHERE o.id = ANY ($1)", &ids)
        .execute(db)
        .await
        .map_err(|_| OutboxError::Database)?;
    Ok(ids)
}
