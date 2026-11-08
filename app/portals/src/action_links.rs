//! Secure email action links for every former text keyword (T148; BIZ-038, SEC-025, SEC-026,
//! BIZ-028, BIZ-032, BIZ-033, CALL-011, A11Y-023, D-309, D-333).
//!
//! An action link is single-purpose, signed, bound to one person and one object, used once and
//! expiring (SEC-025). Its token is 256 OS-random bits; only the SHA256 of
//! `action:<kind>:<object>:<token>` is stored, in `portals.login_link` beside sign-in links (D-1955),
//! so a token for one kind or object never opens another, a token is never a sign-in link, and
//! removing a member voids their action links with their sign-in links (`run_member_removed`,
//! SEC-024). The row's `created_at` is set [`LINK_LIFE`] back so action links never count toward
//! the sign-in link limit (as D-1776's markers). Opening a link (`GET`) only reads
//! ([`open_action_link`]); only the page's button (`POST`) uses it ([`use_action_link`]), which
//! re-reads the member's roles in the caller's transaction. Quote and Hold act through the link
//! alone, Confirm through the link and a Verify code, every other action through a signed-in
//! session ([`UseBy`]). The token never reaches a log: the minted link is a [`SecretString`].

use askhava_businesses::api::{Roles, members};
use askhava_core::api::brand;
use askhava_picks::api::{HoldAsk, hold_allowed};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::business_auth::{BusinessPrincipal, LINK_LIFE};
use crate::models;
use crate::session::PortalsError;

/// The route every action link opens; `GET` shows the page, `POST` acts (not served yet, D-1955).
pub const ACTION_ROUTE: &str = "/b/a/{kind}/{object}/{token}";

/// BIZ-038, D-333: the line every email with action links carries.
pub const OPEN_IN_BROWSER_LINE: &str =
    "If a link asks you to sign in, open it in your phone's browser (tap ⋯, then Open in browser).";

/// One business action a text keyword or reply could do (BIZ-038).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// The quote form for one job (PICK-040).
    Quote,
    /// The PICK-021 review step for one pick.
    Confirm,
    /// "Hold for 2 more hours" on one pick (PICK-043).
    Hold,
    /// "Away until" (BIZ-034).
    Away,
    /// "Pause job cards" (BIZ-028).
    Pause,
    /// "Resume" job cards (BIZ-028).
    Resume,
    /// "Busy for <n> days" (BIZ-028).
    Busy,
    /// "Link this phone" after a blocked call (CALL-011).
    LinkPhone,
    /// "Approve" a phone a member linked (BIZ-033).
    Approve,
    /// "Ask about a charge" (BIZ-032).
    AskCharge,
}

/// What an action link needs besides the link itself (BIZ-038, SEC-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseBy {
    /// The link alone acts: Quote (until the card's window ends) and Hold (until Confirm's).
    LinkAlone,
    /// The link plus a Verify code to that person's own phone or sign-in email: Confirm.
    LinkAndCode,
    /// A signed-in business session: every pause, busy setting, link, approval and question.
    Session,
}

impl ActionKind {
    /// Every action, in the BIZ-038 order.
    pub const ALL: [Self; 10] = [
        Self::Quote,
        Self::Confirm,
        Self::Hold,
        Self::Away,
        Self::Pause,
        Self::Resume,
        Self::Busy,
        Self::LinkPhone,
        Self::Approve,
        Self::AskCharge,
    ];

    /// The path segment naming the action in [`ACTION_ROUTE`].
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Quote => "quote",
            Self::Confirm => "confirm",
            Self::Hold => "hold",
            Self::Away => "away",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Busy => "busy",
            Self::LinkPhone => "link-phone",
            Self::Approve => "approve",
            Self::AskCharge => "ask-charge",
        }
    }

    /// The action a path segment names, or `None`.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.slug() == slug)
    }

    /// BIZ-038, SEC-004, SEC-025: what besides the link the action needs.
    #[must_use]
    pub const fn use_by(self) -> UseBy {
        match self {
            Self::Quote | Self::Hold => UseBy::LinkAlone,
            Self::Confirm => UseBy::LinkAndCode,
            _ => UseBy::Session,
        }
    }

    /// BIZ-023, BIZ-028, BIZ-033, CALL-011, PICK-033, PICK-043: whether a person (the owner or a
    /// member with `roles`) may get and use this action. Quote needs "Cards and quotes"; Approve
    /// is the owner's alone; every other action needs "Confirm and billing", so a "Cards and
    /// quotes" member gets no Hold, Confirm or Pause link.
    #[must_use]
    pub fn allowed(self, owner: bool, roles: Roles) -> bool {
        match self {
            Self::Quote => owner || roles.cards_and_quotes(),
            Self::Approve => owner,
            _ => owner || roles.confirm_and_billing(),
        }
    }

    /// A11Y-023: link text naming the action and the job ("Confirm job 1042"), never "click here".
    #[must_use]
    pub fn link_text(self, job_number: &str) -> String {
        match self {
            Self::Quote => format!("Quote job {job_number}"),
            Self::Confirm => format!("Confirm job {job_number}"),
            Self::Hold => format!("Hold job {job_number} for 2 more business hours"),
            Self::Away => "Set the time you're back".to_owned(),
            Self::Pause => "Pause job cards".to_owned(),
            Self::Resume => "Resume job cards".to_owned(),
            Self::Busy => "Set Busy for 1 to 60 days".to_owned(),
            Self::LinkPhone => "Link this phone".to_owned(),
            Self::Approve => "Approve this phone".to_owned(),
            Self::AskCharge => format!("Ask about the charge for job {job_number}"),
        }
    }
}

/// The one person and one object a link is for (SEC-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionTarget {
    /// The action.
    pub kind: ActionKind,
    /// The object it acts on: the job (Quote), pick (Confirm, Hold), charge (Ask about a charge),
    /// phone link (Link this phone, Approve) or business (Away, Pause, Resume, Busy).
    pub object: Uuid,
    /// The team member the email goes to.
    pub member: Uuid,
    /// That member's business.
    pub business: Uuid,
}

/// The person a usable link was made for (SEC-025). Only [`use_action_link`] makes one, and it is
/// neither `Clone` nor `Copy`, so one used link gives one grant (rust-production R7).
#[derive(Debug, PartialEq, Eq)]
pub struct ActionGrant {
    member: Uuid,
    business: Uuid,
}

impl ActionGrant {
    /// The team member the link was emailed to.
    #[must_use]
    pub const fn member(&self) -> Uuid {
        self.member
    }

    /// Their business.
    #[must_use]
    pub const fn business(&self) -> Uuid {
        self.business
    }

    /// SEC-025: a signed-in person may act through a link only when it was emailed to them.
    #[must_use]
    pub fn same_person(&self, principal: &BusinessPrincipal) -> bool {
        self.member == principal.member() && self.business == principal.business()
    }
}

/// Why no link was made or a link did not work. No variant carries the token.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActionLinkError {
    /// The person is not a current member, or their role doesn't allow the action.
    #[error("the person may not take this action")]
    NotAllowed,
    /// A Hold on an emergency job, an already-held pick or an Away-delayed one (PICK-043), at
    /// minting or at use.
    #[error("this pick can't be held")]
    NoHold,
    /// The link is unknown, used, voided, for another action or object, or past its end.
    #[error("the link has ended")]
    Ended,
    /// No token could be made or stored.
    #[error("the link store failed")]
    Store(#[from] PortalsError),
}

/// PICK-043, D-1956: a Hold needs the pick's current facts and `hold_allowed` to pass with
/// `roles`; every other action passes.
fn hold_ok(kind: ActionKind, hold: Option<HoldAsk>, roles: Roles) -> Result<(), ActionLinkError> {
    if kind != ActionKind::Hold {
        return Ok(());
    }
    let ask = hold.ok_or(ActionLinkError::NoHold)?;
    hold_allowed(HoldAsk { by: roles, ..ask }).map_err(|_| ActionLinkError::NoHold)
}

fn binding(kind: ActionKind, object: Uuid, token: &str) -> String {
    format!("action:{}:{object}:{token}", kind.slug())
}

async fn current_roles(
    db: &mut PgConnection,
    member: Uuid,
    business: Uuid,
) -> Result<Option<(bool, Roles)>, ActionLinkError> {
    let team = members(db, business)
        .await
        .map_err(|_| PortalsError::Database)?;
    Ok(team
        .into_iter()
        .find(|m| m.id == member)
        .map(|m| (m.owner, m.roles)))
}

/// SEC-025, BIZ-038, PICK-043: makes the link for `target`, working until `ends`, as
/// `https://<domain>/b/a/<kind>/<object>/<token>`. A Hold link is made only when `hold` (the pick's
/// facts, with the member's roles) passes `picks::api::hold_allowed`.
///
/// # Errors
///
/// [`ActionLinkError::NotAllowed`] for a removed member or a role without the action,
/// [`ActionLinkError::NoHold`], [`ActionLinkError::Ended`] when `ends` is not after `now`, or
/// [`ActionLinkError::Store`].
pub async fn mint_action_link(
    db: &mut PgConnection,
    target: ActionTarget,
    hold: Option<HoldAsk>,
    (now, ends): (Timestamp, Timestamp),
) -> Result<SecretString, ActionLinkError> {
    let Some((owner, roles)) = current_roles(db, target.member, target.business).await? else {
        return Err(ActionLinkError::NotAllowed);
    };
    if !target.kind.allowed(owner, roles) {
        return Err(ActionLinkError::NotAllowed);
    }
    hold_ok(target.kind, hold, roles)?;
    if ends <= now {
        return Err(ActionLinkError::Ended);
    }
    let made = now
        .checked_sub(LINK_LIFE)
        .map_err(|_| PortalsError::BadLifetime)?;
    let token = models::new_link_token()?;
    let stored = binding(target.kind, target.object, &token);
    let who = (target.member, target.business);
    models::insert_link(db, &stored, who, (made, ends)).await?;
    let (kind, object) = (target.kind.slug(), target.object);
    let domain = brand().domain();
    Ok(SecretString::from(format!(
        "https://{domain}/b/a/{kind}/{object}/{token}"
    )))
}

/// SEC-026: whether the `kind` link `token` for `object` is unused and unexpired at `now`. Reads
/// only: opening a link never acts or uses it up.
///
/// # Errors
///
/// [`ActionLinkError::Ended`] for any link that would not work; [`ActionLinkError::Store`].
pub async fn open_action_link(
    db: &mut PgConnection,
    (kind, object): (ActionKind, Uuid),
    token: &str,
    now: Timestamp,
) -> Result<(), ActionLinkError> {
    if models::link_open(db, &binding(kind, object, token), now).await? {
        Ok(())
    } else {
        Err(ActionLinkError::Ended)
    }
}

/// SEC-025, SEC-026, PICK-043: uses the `kind` link `token` for `object` at `now` (the page's
/// button): for a Hold, `hold` (the pick's facts read now, in this transaction) must still pass
/// `hold_allowed` before the link is touched, so a pick held, made an emergency or Away-delayed
/// since the email never gets a second hold and the link stays unused (D-1956); then one guarded
/// update, so of two presses only one gets the grant, then the member's current roles are
/// checked. Run it in the transaction that performs the action and roll back on any error, so a
/// refused or failed action leaves the link unused (a Quote link is used up only by the
/// submitted quote).
///
/// # Errors
///
/// [`ActionLinkError::NoHold`], [`ActionLinkError::Ended`], [`ActionLinkError::NotAllowed`] (the
/// member was removed or lost the role) or [`ActionLinkError::Store`].
pub async fn use_action_link(
    db: &mut PgConnection,
    (kind, object): (ActionKind, Uuid),
    token: &str,
    hold: Option<HoldAsk>,
    now: Timestamp,
) -> Result<ActionGrant, ActionLinkError> {
    hold_ok(kind, hold, Roles::BOTH)?;
    let used = models::use_link(db, &binding(kind, object, token), now).await?;
    let Some((member, business)) = used else {
        return Err(ActionLinkError::Ended);
    };
    match current_roles(db, member, business).await? {
        Some((owner, roles)) if kind.allowed(owner, roles) => Ok(ActionGrant { member, business }),
        _ => Err(ActionLinkError::NotAllowed),
    }
}

/// BIZ-038, SEC-004: whether a signed-in `principal` may take `kind` for `business` in the
/// portal (Approve is the owner's alone).
#[must_use]
pub fn session_may(principal: &BusinessPrincipal, kind: ActionKind, business: Uuid) -> bool {
    let roles = if principal.owner {
        Some(Roles::BOTH)
    } else {
        Roles::new(principal.roles.0, principal.roles.1)
    };
    principal.business == business && roles.is_some_and(|r| kind.allowed(principal.owner, r))
}

/// How an email reply to an Ask Hava email is handled (BIZ-032, BIZ-038, TXT-037): never as a
/// command, only as the automatic reply, a help request or the latest charge's details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailReply {
    /// A question mark or a problem word: the reply also opens a help request (OPS-036).
    pub opens_help: bool,
    /// A charge word or "$": the reply carries the latest charge's details (FEE-031).
    pub charge_details: bool,
}

const HELP_WORDS: [&str; 10] = [
    "charge",
    "refund",
    "wrong",
    "problem",
    "cargo",
    "cobro",
    "reembolso",
    "mal",
    "error",
    "problema",
];
const CHARGE_WORDS: [&str; 7] = [
    "charge",
    "fee",
    "refund",
    "cargo",
    "cobro",
    "tarifa",
    "reembolso",
];

/// BIZ-032, BIZ-038: how the email reply `body` is handled. No word in it, "CONFIRM 1042" or
/// "HOLD" included, ever performs an action.
#[must_use]
pub fn email_reply(body: &str) -> EmailReply {
    let lower = body.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    EmailReply {
        opens_help: lower.contains('?') || has(&HELP_WORDS),
        charge_details: lower.contains('$') || has(&CHARGE_WORDS),
    }
}
