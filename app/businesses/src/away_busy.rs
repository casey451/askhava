//! A business's "Away until <time>" end (T543; BIZ-034, BIZ-038; D-285, D-309, D-3890).
//!
//! [`set_away`] stores the end of the business's away time in `businesses.business.away_until`
//! for the owner or a current "Confirm and billing" member, with one audit row naming the field,
//! in one transaction. The caller bounds the end first with `picks::api::away_until` (after now,
//! at most the setting's business hours ahead; `businesses` sits below `picks`, so it cannot call
//! it). [`away_until`] reads the end while it is still ahead; `picks::handlers::notice_sent` starts
//! a non-emergency pick's Confirm clock there. `busy_until` (same migration, 0206) is T463's.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{BusinessesError, db};
use crate::team::Acting;

/// The audit row's subject kind (the business record).
const SUBJECT: &str = "businesses.business";

/// BIZ-034, D-285: stores `until` as `by`'s business's away end at `now`, with one audit row, when
/// `by` is a current owner or "Confirm and billing" member. `until` must already be bounded by
/// `picks::api::away_until`; an end at or before `now` is refused here too.
///
/// # Errors
/// [`BusinessesError::Refused`] for an end not after `now`, a removed member, a member without
/// "Confirm and billing" or one of another business (nothing stored); `Database` or `Audit`.
pub async fn set_away(
    conn: &mut PgConnection,
    by: Acting,
    until: Timestamp,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    if until <= now {
        return Err(BusinessesError::Refused);
    }
    let mut tx = db(conn.begin().await)?;
    let stored = sqlx::query!(
        "UPDATE businesses.business b SET away_until = $3 \
         WHERE b.id = $1 AND EXISTS (SELECT 1 FROM businesses.team_member m \
         WHERE m.id = $2 AND m.business_id = b.id AND m.removed_at IS NULL \
         AND (m.is_owner OR m.confirm_and_billing))",
        by.business,
        by.member,
        until.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await;
    if db(stored)?.rows_affected() != 1 {
        return Err(BusinessesError::Refused);
    }
    let entry = AuditEntry {
        actor: Actor::Business(by.business),
        action: "businesses.field_changed",
        subject_kind: SUBJECT,
        subject_id: by.business,
        change: Change::Field("away_until"),
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| BusinessesError::Audit)?;
    db(tx.commit().await)
}

/// BIZ-034: business `business`'s away end while it is after `now`; none when unset, past or no
/// such business.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn away_until(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<Option<Timestamp>, BusinessesError> {
    let until = sqlx::query_scalar!(
        "SELECT away_until FROM businesses.business WHERE id = $1 AND away_until > $2",
        business,
        now.to_sqlx() as _,
    )
    .fetch_optional(conn)
    .await;
    Ok(db(until)?.flatten().map(|t| t.to_jiff()))
}
