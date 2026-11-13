//! The mailbox and staff phone IDs a business still uses, for the daily contact sweep (BIZ-037,
//! BIZ-040, BIZ-051, SEC-024, PRIV-002; D-3262, T415).
//!
//! `privacy` seals each sign-in address under an opaque mailbox ID and each texting number under
//! its `businesses.staff_phone` line ID, but only this crate knows which IDs a business still
//! names. [`contact_ids_in_use`] lists them for `server`'s sweep, which hands the list to
//! `privacy::api::forget_unused`. It returns IDs only, never an address or a number (PRIV-016).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::BusinessesError;

/// The contact IDs a business still names: the mailbox IDs and the staff phone line IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactIds {
    /// Mailbox IDs in use, or out of use only since the cut-off.
    pub mailboxes: Vec<Uuid>,
    /// Line IDs of every linked phone of a business that has not left.
    pub lines: Vec<Uuid>,
}

/// BIZ-037, BIZ-040, BIZ-051, SEC-024, D-3262: the mailbox IDs and phone lines still in use, and
/// the mailboxes that stopped being used at or after `since`.
///
/// In use, for a business not `removed` (one that left, BIZ-051): every current member's mailbox,
/// every backup address not removed of a current member, every open `mailbox_check` row for the
/// sign-up email or a current member, and every `staff_phone` line. A removed member's leftover
/// backup and check rows count only through the member's `removed_at`. Stopped at or after
/// `since`: a removed member's mailbox and leftovers (by `removed_at`), a removed backup (its own
/// `removed_at`), a check row's mailbox and the address it replaces (by `verified_at`, else
/// `sent_at`; the row has no closed time), and every mailbox of a business that left (by its
/// status change time). Each ID is listed once, in no set order.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn contact_ids_in_use(
    conn: &mut PgConnection,
    since: Timestamp,
) -> Result<ContactIds, BusinessesError> {
    let mailboxes = mailboxes_in_use(conn, since).await?;
    let lines = sqlx::query_scalar!(
        "SELECT p.id FROM businesses.staff_phone p \
         JOIN businesses.business b ON b.id = p.business_id WHERE b.status <> 'removed'"
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| BusinessesError::Database)?;
    Ok(ContactIds { mailboxes, lines })
}

/// The mailbox half of [`contact_ids_in_use`], one statement so its three sources read one
/// snapshot.
async fn mailboxes_in_use(
    conn: &mut PgConnection,
    since: Timestamp,
) -> Result<Vec<Uuid>, BusinessesError> {
    sqlx::query_scalar!(
        "SELECT m.mailbox_id AS \"id!\" FROM businesses.team_member m \
         JOIN businesses.business b ON b.id = m.business_id \
         WHERE CASE WHEN b.status = 'removed' THEN b.status_changed_at >= $1::timestamptz \
         ELSE m.removed_at IS NULL OR m.removed_at >= $1::timestamptz END \
         UNION \
         SELECT a.mailbox_id FROM businesses.backup_address a \
         JOIN businesses.team_member m ON m.id = a.member_id \
         JOIN businesses.business b ON b.id = a.business_id \
         WHERE CASE WHEN b.status = 'removed' THEN b.status_changed_at >= $1::timestamptz \
         ELSE (a.removed_at IS NULL AND m.removed_at IS NULL) OR a.removed_at >= $1::timestamptz \
         OR m.removed_at >= $1::timestamptz END \
         UNION \
         SELECT v.id FROM businesses.mailbox_check c \
         JOIN businesses.business b ON b.id = c.business_id \
         LEFT JOIN businesses.team_member m ON m.id = COALESCE(c.change_for, c.member_id) \
         CROSS JOIN LATERAL (VALUES (c.mailbox_id), (c.replaces)) AS v (id) \
         WHERE v.id IS NOT NULL AND CASE WHEN b.status = 'removed' \
         THEN b.status_changed_at >= $1::timestamptz \
         ELSE (c.open AND m.removed_at IS NULL) OR m.removed_at >= $1::timestamptz \
         OR COALESCE(c.verified_at, c.sent_at) >= $1::timestamptz END",
        since.to_sqlx() as _
    )
    .fetch_all(conn)
    .await
    .map_err(|_| BusinessesError::Database)
}
