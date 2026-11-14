//! The read-only view of a sign-up's identity progress for the served sign-up steps (T485,
//! D-3779; BIZ-002, BIZ-016, BIZ-040).
//!
//! [`signup_progress`] answers [`SignupProgress`]: the stored identity step, whether the identity
//! code was completed and whether the sign-up's own mailbox (`businesses.signup.mailbox_id`, the
//! `mailbox_check` row with no member) is verified. `server` calls it before each identity and
//! email step so it calls `check_code` only once a step exists (D-3641). It writes nothing, takes
//! no lock, uses no code try and calls no Verify; it carries no contact, mailbox ID, record value,
//! flag or count (PRIV-016).

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::identity::Step;
use crate::models::{self, BusinessesError};

/// The three facts the served sign-up steps check (BIZ-002, BIZ-016, BIZ-040). Its `Debug` shows
/// only these three facts (PRIV-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignupProgress {
    step: Option<Step>,
    code_done: bool,
    email_verified: bool,
}

impl SignupProgress {
    /// BIZ-002: the identity step a code or letter last went out on; `None` while none was sent.
    #[must_use]
    pub const fn step(&self) -> Option<Step> {
        self.step
    }

    /// BIZ-016: whether the identity code was completed.
    #[must_use]
    pub const fn code_done(&self) -> bool {
        self.code_done
    }

    /// BIZ-040: whether the sign-up's own email has a verified mailbox check.
    #[must_use]
    pub const fn email_verified(&self) -> bool {
        self.email_verified
    }
}

/// BIZ-002, BIZ-016, BIZ-040: read a sign-up's identity progress. One plain `SELECT`, no lock.
///
/// # Errors
/// [`BusinessesError::UnknownBusiness`] with no sign-up row; [`BusinessesError::Database`] on a
/// failed read or a stored step no [`Step`] names.
pub async fn signup_progress(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<SignupProgress, BusinessesError> {
    let q = sqlx::query!(
        "SELECT s.identity_step, s.code_done, EXISTS (SELECT 1 FROM businesses.mailbox_check c \
         WHERE c.business_id = s.business_id AND c.mailbox_id = s.mailbox_id \
         AND c.member_id IS NULL AND c.verified_at IS NOT NULL) AS \"email_verified!\" \
         FROM businesses.signup s WHERE s.business_id = $1",
        business
    );
    let row = models::db(q.fetch_optional(conn).await)?.ok_or(BusinessesError::UnknownBusiness)?;
    Ok(SignupProgress {
        step: row.identity_step.as_deref().map(step_of).transpose()?,
        code_done: row.code_done,
        email_verified: row.email_verified,
    })
}

/// The [`Step`] whose [`Step::code`] is the stored code; an unknown code is a database fault.
fn step_of(code: &str) -> Result<Step, BusinessesError> {
    [Step::Text, Step::Email, Step::Letter]
        .into_iter()
        .find(|s| s.code() == code)
        .ok_or(BusinessesError::Database)
}
