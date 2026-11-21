//! The step-up-guarded team and billing changes (SEC-038, SEC-019, BIZ-023, FEE-041; T565,
//! D-3144, D-3890, D-4045).
//!
//! [`change_team_role`] and [`remove_team_member`] are the owner's alone (BIZ-023) and pass
//! `require_step_up` under `SensitiveChange::TeamRole` before `businesses::api::set_roles` or
//! `remove_member`, which keep their own checks; [`change_monthly_limit`] passes through
//! `change_fee_limit`, which checks the role and the step-up itself. The role is checked before
//! the step-up, so a member without it never reaches the step-up page; a refusal of either kind
//! changes nothing. `server::business_guarded_changes` serves them and turns
//! [`GuardedChangeError::NeedsCode`] into a redirect to the step-up page.

use askhava_businesses::api::{Acting, Roles, TeamError, remove_member, set_roles};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::business_auth::BusinessPrincipal;
use crate::business_portal::{
    BillingChangeError, SensitiveChange, StepUpError, change_fee_limit, require_step_up,
};

/// Where the role change form posts (SEC-038); the member travels in the form body.
pub const TEAM_ROLE_ROUTE: &str = "/b/team/role";
/// Where the removal form posts (SEC-038); the member travels in the form body.
pub const TEAM_REMOVE_ROUTE: &str = "/b/team/remove";
/// Where the monthly-limit form posts (SEC-038, FEE-041).
pub const FEE_LIMIT_ROUTE: &str = "/b/billing/limit";

/// Why a guarded change did not go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GuardedChangeError {
    /// The person may not make the change, or the member is not one of the business's current
    /// members: the one not-found answer (SEC-019).
    #[error("not found")]
    NotFound,
    /// No Verify code in the last 5 minutes; the person is sent to the step-up page (SEC-038).
    #[error("this change needs a fresh code")]
    NeedsCode,
    /// The change breaks a rule: the owner keeps both roles and is never removed (BIZ-023).
    #[error("the owner keeps both roles")]
    OwnerKeepsBoth,
    /// A database call failed.
    #[error("guarded change failed")]
    Database,
}

impl From<StepUpError> for GuardedChangeError {
    fn from(error: StepUpError) -> Self {
        match error {
            StepUpError::NeedsCode => Self::NeedsCode,
            StepUpError::Database => Self::Database,
        }
    }
}

impl From<TeamError> for GuardedChangeError {
    fn from(error: TeamError) -> Self {
        match error {
            TeamError::NotAllowed | TeamError::UnknownMember | TeamError::UnknownBusiness => {
                Self::NotFound
            }
            TeamError::OwnerKeepsBoth => Self::OwnerKeepsBoth,
            _ => Self::Database,
        }
    }
}

/// SEC-038, BIZ-023: the owner check, then the step-up for a team change on `route`.
async fn team_gate(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    route: &str,
    now: Timestamp,
) -> Result<Acting, GuardedChangeError> {
    if !principal.is_owner() {
        return Err(GuardedChangeError::NotFound);
    }
    require_step_up(db, principal, (SensitiveChange::TeamRole, route), now).await?;
    Ok(Acting {
        business: principal.business,
        member: principal.member,
    })
}

/// SEC-038, BIZ-023, D-480: gives `member` of the owner's business `roles`, only for the owner
/// with a Verify code from the last 5 minutes; the same roles again change nothing.
///
/// # Errors
/// [`GuardedChangeError`].
pub async fn change_team_role(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (member, roles): (Uuid, Roles),
    now: Timestamp,
) -> Result<(), GuardedChangeError> {
    let by = team_gate(db, principal, TEAM_ROLE_ROUTE, now).await?;
    set_roles(db, by, member, roles).await?;
    Ok(())
}

/// SEC-038, BIZ-023, D-480: removes `member` from the owner's business, only for the owner with
/// a Verify code from the last 5 minutes; a member already removed is not found.
///
/// # Errors
/// [`GuardedChangeError`].
pub async fn remove_team_member(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    member: Uuid,
    now: Timestamp,
) -> Result<(), GuardedChangeError> {
    let by = team_gate(db, principal, TEAM_REMOVE_ROUTE, now).await?;
    remove_member(db, by, member, now).await?;
    Ok(())
}

/// SEC-038, FEE-041, D-480: sets (or with `None` removes) the monthly fee limit, only for the
/// owner or a "Confirm and billing" member with a Verify code from the last 5 minutes.
///
/// # Errors
/// [`GuardedChangeError`].
pub async fn change_monthly_limit(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    limit_cents: Option<u32>,
    now: Timestamp,
) -> Result<(), GuardedChangeError> {
    match change_fee_limit(db, principal, limit_cents, now).await {
        Ok(()) => Ok(()),
        Err(BillingChangeError::Role) => Err(GuardedChangeError::NotFound),
        Err(BillingChangeError::StepUp(step_up)) => Err(step_up.into()),
        Err(BillingChangeError::Payments) => Err(GuardedChangeError::Database),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_errors_map_to_one_not_found() {
        for error in [
            TeamError::NotAllowed,
            TeamError::UnknownMember,
            TeamError::UnknownBusiness,
        ] {
            assert_eq!(
                GuardedChangeError::from(error),
                GuardedChangeError::NotFound
            );
        }
        assert_eq!(
            GuardedChangeError::from(TeamError::OwnerKeepsBoth),
            GuardedChangeError::OwnerKeepsBoth
        );
        assert_eq!(
            GuardedChangeError::from(TeamError::Audit),
            GuardedChangeError::Database
        );
        assert_eq!(
            GuardedChangeError::from(StepUpError::NeedsCode),
            GuardedChangeError::NeedsCode
        );
    }
}
