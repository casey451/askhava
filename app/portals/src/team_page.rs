//! The business team page (BIZ-023, FEE-041, SEC-038, SEC-019; T544, D-3890, D-3892, D-5567).
//!
//! [`team_page`] renders `templates/business_team.html` (on `business_layout.html`): the
//! business's current members, owner first, each with their roles and their sign-in address
//! (read from the vault by the `server` composition and handed in; `None` shows a numbered fixed
//! line), and the monthly-limit form. Only the owner gets a role-change form and a removal button
//! per member other than the owner, since `guarded_changes`' role change and removal are the
//! owner's alone (D-5567); a "Confirm and billing" member sees the list and the monthly-limit form
//! only. Every form carries the CSRF token and, for a member's change, the member's ID in the body,
//! and posts to T565's routes, which send the person to the step-up page first. [`may_see_team`] is
//! the page's door: the owner or a "Confirm and billing" member; anyone else gets the one
//! not-found answer. This module reads nothing from the database or the vault; `server`'s
//! `business_step_up` serves `GET` [`TEAM_ROUTE`] behind `business_principal`.

use askama::Template;
use askhava_businesses::api::TeamMember;

use crate::business_auth::{BusinessPrincipal, Need};
use crate::guarded_changes::{FEE_LIMIT_ROUTE, TEAM_REMOVE_ROUTE, TEAM_ROLE_ROUTE};

/// The team page (BIZ-023); `/b/billing` links to it (D-3892).
pub const TEAM_ROUTE: &str = "/b/team";

/// One current member as the page shows them: the member and their sign-in address, `None` when
/// the composition has no vault or the address can't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownMember {
    /// The member (`businesses::api::members`).
    pub member: TeamMember,
    /// The member's sign-in address, when it could be read.
    pub address: Option<String>,
}

/// One member's entry in the template.
struct Row {
    id: String,
    who: String,
    owner: bool,
    you: bool,
    cards: bool,
    billing: bool,
}

#[derive(Template)]
#[template(path = "business_team.html")]
struct TeamView<'a> {
    rows: Vec<Row>,
    owner_view: bool,
    csrf: &'a str,
    role_route: &'a str,
    remove_route: &'a str,
    limit_route: &'a str,
}

/// SEC-019, BIZ-023, FEE-041: whether `principal` may open the team page: the owner or a
/// "Confirm and billing" member of their own business.
#[must_use]
pub fn may_see_team(principal: &BusinessPrincipal) -> bool {
    principal.may(principal.business(), Need::ConfirmAndBilling)
}

/// The member at `index` (from 1) as the page names them.
fn row(index: usize, shown: &ShownMember, viewer: &BusinessPrincipal) -> Row {
    let m = &shown.member;
    let who = match (&shown.address, m.owner) {
        (Some(address), _) => address.clone(),
        (None, true) => "The owner".to_owned(),
        (None, false) => format!("Team member {index}"),
    };
    Row {
        id: m.id.to_string(),
        who,
        owner: m.owner,
        you: m.id == viewer.member(),
        cards: m.roles.cards_and_quotes(),
        billing: m.roles.confirm_and_billing(),
    }
}

/// BIZ-023, FEE-041, SEC-038, D-5567: the team page for `viewer` listing `members`, with the
/// role-change and removal forms for the owner only, the monthly-limit form, and `csrf` in each.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn team_page(
    members: &[ShownMember],
    viewer: &BusinessPrincipal,
    csrf: &str,
) -> Result<String, askama::Error> {
    TeamView {
        rows: (members.iter().enumerate())
            .map(|(i, m)| row(i + 1, m, viewer))
            .collect(),
        owner_view: viewer.is_owner(),
        csrf,
        role_route: TEAM_ROLE_ROUTE,
        remove_route: TEAM_REMOVE_ROUTE,
        limit_route: FEE_LIMIT_ROUTE,
    }
    .render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use askhava_businesses::api::Roles;
    use sqlx::types::Uuid;

    fn person(member: u128, owner: bool, roles: (bool, bool)) -> BusinessPrincipal {
        BusinessPrincipal {
            member: Uuid::from_u128(member),
            business: Uuid::from_u128(1),
            owner,
            roles,
            session: Uuid::from_u128(9),
        }
    }

    fn team() -> Vec<ShownMember> {
        let member = |id: u128, owner: bool, roles: Roles, address: Option<&str>| ShownMember {
            member: TeamMember {
                id: Uuid::from_u128(id),
                business: Uuid::from_u128(1),
                mailbox: Uuid::from_u128(id + 100),
                owner,
                roles,
            },
            address: address.map(str::to_owned),
        };
        let cards = Roles::new(true, false).expect("a role");
        vec![
            member(2, true, Roles::BOTH, Some("owner@example.com")),
            member(3, false, cards, None),
        ]
    }

    #[test]
    fn the_owner_gets_role_and_removal_forms_for_members_only() {
        let html = team_page(&team(), &person(2, true, (true, true)), "tok").expect("page");
        assert!(html.contains("owner@example.com") && html.contains("Team member 2"));
        assert!(html.contains(TEAM_ROLE_ROUTE) && html.contains(TEAM_REMOVE_ROUTE));
        assert_eq!(
            html.matches("Save roles").count(),
            1,
            "none for the owner's own row"
        );
        assert!(html.contains(&format!("value=\"{}\"", Uuid::from_u128(3))));
        assert!(!html.contains(&format!("value=\"{}\"", Uuid::from_u128(2))));
        assert!(html.contains(FEE_LIMIT_ROUTE) && html.contains("data-get-help"));
    }

    #[test]
    fn a_billing_member_sees_the_list_and_the_limit_form_only() {
        let viewer = person(3, false, (false, true));
        let html = team_page(&team(), &viewer, "tok").expect("page");
        assert!(html.contains("Team member 2") && html.contains(FEE_LIMIT_ROUTE));
        assert!(!html.contains(TEAM_ROLE_ROUTE) && !html.contains(TEAM_REMOVE_ROUTE));
        assert!(may_see_team(&viewer) && may_see_team(&person(2, true, (true, true))));
        assert!(!may_see_team(&person(4, false, (true, false))));
    }
}
