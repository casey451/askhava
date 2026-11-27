//! The phones one team member linked, as the text-consent page shows them (CON-019, BIZ-036;
//! T522, D-3881).
//!
//! [`member_text_lines`] lists each `businesses.staff_phone` row of a business whose `linked_by`
//! is the member: its line ID, the number's last 4 digits (`number_end`, NULL for a phone whose
//! number `businesses` never saw), its current number version (raised by `team::change_phone_at`,
//! D-2707) and whether `consent::may_text` holds for it now. The page draws its boxes from this
//! list and its `POST` records only for a line in it, so a member never ticks the box for another
//! member's or another business's phone. Only IDs and the last 4 digits leave here (PRIV-002).

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::consent::{ConsentError, may_text};

/// One phone a member linked, with its consent state for the number it has now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberTextLine {
    /// The `businesses.staff_phone` ID.
    pub line: Uuid,
    /// The number's last 4 digits, if `businesses` saw the number (D-078).
    pub number_end: Option<String>,
    /// The phone's current number version; a text box is recorded for this version only.
    pub number_version: i32,
    /// Whether a current member ticked the text box for this number version (`may_text`).
    pub may_text: bool,
}

/// CON-019, BIZ-036, D-3881: the phones `member` of `business` linked, oldest link first, each
/// with its current number version and whether it may get texts now.
///
/// # Errors
/// [`ConsentError::Database`].
pub async fn member_text_lines(
    db: &mut PgConnection,
    business: Uuid,
    member: Uuid,
) -> Result<Vec<MemberTextLine>, ConsentError> {
    let rows = sqlx::query!(
        "SELECT id, number_end, number_version FROM businesses.staff_phone \
         WHERE business_id = $1 AND linked_by = $2 ORDER BY linked_at, id",
        business,
        member
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| ConsentError::Database)?;
    let mut lines = Vec::with_capacity(rows.len());
    for row in rows {
        lines.push(MemberTextLine {
            line: row.id,
            number_end: row.number_end,
            number_version: row.number_version,
            may_text: may_text(db, business, row.id).await?,
        });
    }
    Ok(lines)
}
