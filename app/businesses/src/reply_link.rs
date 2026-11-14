//! The staff phone a "LINK <PIN>" reply from the main phone adds (CALL-011, CALL-012, BIZ-033;
//! D-311, D-3079, T391).
//!
//! `calls::api::link_reply` calls [`is_main_phone`] to check the reply came from the business's
//! main phone (only the owner links that one, so the reply speaks for the owner, CALL-012), then
//! [`link_by_reply`] in its transaction: one `businesses.staff_phone` row, not the main phone,
//! linked and approved (`approved_at = now`, D-3081) by the owner's member row so it works at once
//! with no Approve step (BIZ-033), and one `businesses.phone_linked` audit row naming the
//! business and the line ID only. The number is never here: the caller files the blocked phone's
//! digests in `messaging`'s sender directory under the returned line ID (PRIV-002).

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{BusinessesError, db};

/// Whether phone `line` is the main phone of `business` (CALL-012); a line of another business
/// is not.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn is_main_phone(
    conn: &mut PgConnection,
    business: Uuid,
    line: Uuid,
) -> Result<bool, BusinessesError> {
    let main = db(sqlx::query_scalar!(
        "SELECT is_main FROM businesses.staff_phone WHERE id = $1 AND business_id = $2",
        line,
        business
    )
    .fetch_optional(conn)
    .await)?;
    Ok(main == Some(true))
}

/// Adds one staff phone (not the main one) to `business`, linked and approved by its owner at
/// `now` (its number set then by the owner; no `number_end`, since the number is never here), and
/// its `businesses.phone_linked` audit row, on the caller's transaction (CALL-011, CALL-012,
/// BIZ-033, D-3079). Returns the new line ID for the sender directory.
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] when the business has no owner, `Database` or `Audit`.
pub async fn link_by_reply(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<Uuid, BusinessesError> {
    let line = db(sqlx::query_scalar!(
        "INSERT INTO businesses.staff_phone (business_id, linked_by, is_main, linked_at, \
         approved_at, number_set_at, number_set_by) \
         SELECT business_id, id, false, $2, $2, $2, id FROM businesses.team_member \
         WHERE business_id = $1 AND is_owner RETURNING id",
        business,
        now.to_sqlx() as _
    )
    .fetch_optional(&mut *conn)
    .await)?
    .ok_or(BusinessesError::UnknownBusiness)?;
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action: "businesses.phone_linked",
        subject_kind: "businesses.staff_phone",
        subject_id: line,
        change: Change::Action,
    };
    audit(conn, &entry)
        .await
        .map_err(|_| BusinessesError::Audit)?;
    Ok(line)
}
