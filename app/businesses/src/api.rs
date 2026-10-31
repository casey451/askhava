//! The public face of `askhava-businesses`: everything other crates may call (D-481).
//!
//! The business account and its status moves (BIZ-009, BIZ-003, BIZ-016), the towns and the
//! job-type checklist within the license (BIZ-004, BIZ-010), the overflow switch, the Spanish
//! tick, the business-reported insurance answer and the after-hours tick (BIZ-005 to BIZ-008), and
//! the night count for INT-033 (D-491); team members with their roles, the main and staff phones
//! kept in `messaging`'s sender directory, and `notice_addresses` (BIZ-023, CALL-012, D-491, D-492).

use sqlx::PgConnection;
use sqlx::types::Uuid;

pub use crate::models::{Business, BusinessesError, Insurance};
pub use crate::rules::JobTypeChoice;
pub use crate::rules::{Checks, Event};
pub use crate::service::{Ticks, apply, create, night_count, set_job_types, set_ticks, set_towns};
pub use crate::status::{AccountStatus, PauseReason, Refused};
pub use crate::team::{
    MEMBER_REMOVED_KIND, NoticePurpose, Roles, SenderLink, TeamError, TeamMember, add_member,
    add_owner, change_phone, link_phone, members, notice_addresses, remove_member, set_roles,
    unlink_phone,
};

/// The pure status and checklist rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{checklist, towns};
    pub use crate::status::next;
}

/// One business's stored record (BIZ-005 to BIZ-009).
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] or [`BusinessesError::Database`].
pub async fn business(db: &mut PgConnection, id: Uuid) -> Result<Business, BusinessesError> {
    crate::models::read(db, id).await
}

/// The towns a business serves, sorted (BIZ-010).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn towns(db: &mut PgConnection, id: Uuid) -> Result<Vec<String>, BusinessesError> {
    crate::models::towns(db, id).await
}

/// The job types a business ticked, sorted by category then name (BIZ-004).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn job_types(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Vec<JobTypeChoice>, BusinessesError> {
    crate::models::job_types(db, id).await
}
