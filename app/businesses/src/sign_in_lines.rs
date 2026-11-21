//! Where one team member's sign-in code may go, as opaque IDs (SEC-004; D-3562, T280).
//!
//! `portals`' `CodeAddress::code_to` gets only a team-member row ID; the composition needs that
//! member's business, sign-in mailbox and own texting lines to open the right number or address
//! under `privacy`'s `BusinessPurpose::SignInCode`. [`sign_in_lines`] answers with IDs only, never
//! an address or a number (PRIV-016): a removed member has none, and a line is listed only when
//! that member linked it, it is not the business's main phone (a shared line, never "that
//! person's own phone") and a text box is ticked for its current number by a current member (as
//! `consent::may_text` reads it, CON-019), so a code never goes to another person's phone or to a
//! number nobody ticked: after `team::set_number` raises the line's `number_version` the line is
//! left out, and the code goes to the sign-in address, until the new number's box is ticked
//! (D-5081 finding F39, T777). The owner's approval of the line is not asked for here (D-5117).

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::BusinessesError;

/// A current member's sign-in IDs: the business, the sign-in mailbox and that member's own
/// texting lines (`businesses.staff_phone` IDs), oldest link first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInLines {
    /// The member's business.
    pub business: Uuid,
    /// The opaque ID of the member's sign-in address (PRIV-001).
    pub mailbox: Uuid,
    /// The member's own lines that may get texts, in `linked_at`, `id` order.
    pub texting_lines: Vec<Uuid>,
}

/// SEC-004, CON-019, D-3562: `member`'s sign-in IDs, `None` unless `member` is a current member
/// (`removed_at` null). The lines are the `staff_phone` rows `member` linked in the same business,
/// leaving out the main phone, with a text box ticked by a current member for the line's current
/// `number_version` (D-4039, D-5117); the owner's approval is not a condition.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn sign_in_lines(
    conn: &mut PgConnection,
    member: Uuid,
) -> Result<Option<SignInLines>, BusinessesError> {
    let row = sqlx::query!(
        "SELECT business_id, mailbox_id FROM businesses.team_member \
         WHERE id = $1 AND removed_at IS NULL",
        member
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| BusinessesError::Database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let texting_lines = sqlx::query_scalar!(
        "SELECT p.id FROM businesses.staff_phone p \
         WHERE p.linked_by = $1 AND p.business_id = $2 AND NOT p.is_main \
         AND EXISTS (SELECT 1 FROM businesses.esign_consent c \
         JOIN businesses.team_member m ON m.id = c.member_id AND m.business_id = c.business_id \
         WHERE c.business_id = p.business_id AND c.line_id = p.id AND c.channel = 'text' \
         AND c.number_version = p.number_version AND m.removed_at IS NULL) \
         ORDER BY p.linked_at, p.id",
        member,
        row.business_id
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| BusinessesError::Database)?;
    Ok(Some(SignInLines {
        business: row.business_id,
        mailbox: row.mailbox_id,
        texting_lines,
    }))
}
