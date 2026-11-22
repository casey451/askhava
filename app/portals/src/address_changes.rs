//! The notice and backup address changes behind SEC-038's step-up (BIZ-040, BIZ-043, SEC-024;
//! T542, D-3890, D-3892, D-5081).
//!
//! [`request_address_change`] passes `require_step_up` first (under
//! `SensitiveChange::NoticeAddress` or `BackupAddress`), then mints the typed address's mailbox
//! on the server with `privacy::api::mailbox_for` (never a posted ID, D-3262) and asks
//! `businesses` for the change, which returns the code the composition emails to the new address
//! only. The old notice address keeps every notice until [`confirm_address_change`] takes that
//! code; `businesses` then queues the email telling the old address (for a backup, the person's
//! main address). The code form carries the typed address again and its mailbox is minted again
//! (the same one, D-3892); a backup's code needs only the member. [`remove_backup_address`] needs
//! the same step-up. `server::address_changes` serves the page and its forms.

use askama::Template;
use askhava_businesses::api::{
    Acting, EmailCheckError, confirm_backup, confirm_mailbox, members, remove_backup,
    request_backup, request_notice_change,
};
use askhava_privacy::api::{BusinessContactError, Vault, mailbox_for};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::business_auth::BusinessPrincipal;
use crate::business_portal::{SensitiveChange, StepUpError, require_step_up};

/// The page's path (BIZ-040, BIZ-043).
pub const ADDRESS_CHANGE_ROUTE: &str = "/b/addresses";
/// Where a new notice or backup address is posted (SEC-038); the member travels in the form body.
pub const ADDRESS_REQUEST_ROUTE: &str = "/b/addresses/request";
/// Where the emailed code is posted (BIZ-040).
pub const ADDRESS_CONFIRM_ROUTE: &str = "/b/addresses/confirm";
/// Where a backup address's removal is posted (SEC-038, BIZ-043).
pub const BACKUP_REMOVE_ROUTE: &str = "/b/addresses/remove-backup";

/// Which address a form changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    /// The person's main notice address (BIZ-040).
    Notice,
    /// The person's one backup notice address (BIZ-043).
    Backup,
}

impl AddressKind {
    /// The form value `notice` or `backup` (R1), else `None`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "notice" => Some(Self::Notice),
            "backup" => Some(Self::Backup),
            _ => None,
        }
    }

    /// The form value.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Notice => "notice",
            Self::Backup => "backup",
        }
    }

    fn change(self) -> SensitiveChange {
        match self {
            Self::Notice => SensitiveChange::NoticeAddress,
            Self::Backup => SensitiveChange::BackupAddress,
        }
    }
}

/// Why an address change did not go ahead. No variant carries a value (R5, R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AddressChangeError {
    /// The member is not one this person may change: the one not-found answer (SEC-019).
    #[error("not found")]
    NotFound,
    /// No Verify code in the last 5 minutes: the step-up page (SEC-038).
    #[error("this change needs a fresh code")]
    NeedsCode,
    /// The typed text is not an email address.
    #[error("not an email address")]
    BadAddress,
    /// Another current team member signs in with the address (BIZ-040, D-5081).
    #[error("address taken")]
    Taken,
    /// The address already is the person's verified notice address; nothing was sent.
    #[error("address unchanged")]
    Unchanged,
    /// A database or vault call failed.
    #[error("address change failed")]
    Database,
}

impl From<StepUpError> for AddressChangeError {
    fn from(error: StepUpError) -> Self {
        match error {
            StepUpError::NeedsCode => Self::NeedsCode,
            StepUpError::Database => Self::Database,
        }
    }
}

impl From<EmailCheckError> for AddressChangeError {
    fn from(error: EmailCheckError) -> Self {
        match error {
            EmailCheckError::NotAllowed | EmailCheckError::UnknownMember => Self::NotFound,
            EmailCheckError::NoFreshCode => Self::NeedsCode,
            EmailCheckError::MailboxTaken => Self::Taken,
            EmailCheckError::Database => Self::Database,
        }
    }
}

/// The code to email to the new address only, and that address's mailbox; no `Debug` (R6).
pub struct AddressCode {
    /// The new address's mailbox, minted on the server.
    pub mailbox: Uuid,
    /// The 6-digit code.
    pub code: SecretString,
}

/// D-3262: the typed address's mailbox, minted here, never taken from the request.
async fn minted(
    db: &mut PgConnection,
    vault: &Vault,
    address: &SecretString,
) -> Result<Uuid, AddressChangeError> {
    match mailbox_for(db, vault, address.expose_secret()).await {
        Ok(mailbox) => Ok(mailbox),
        Err(BusinessContactError::BadAddress) => Err(AddressChangeError::BadAddress),
        Err(_) => Err(AddressChangeError::Database),
    }
}

/// SEC-038, BIZ-040, BIZ-043, SEC-024: asks to make `address` `member`'s notice or backup address
/// after a step-up in the last 5 minutes; returns the code to email to `address` alone. A backup
/// that is any current member's sign-in address is [`AddressChangeError::Taken`].
///
/// # Errors
/// [`AddressChangeError`].
pub async fn request_address_change(
    db: &mut PgConnection,
    (vault, secret): (&Vault, &SecretString),
    principal: &BusinessPrincipal,
    (kind, member, address): (AddressKind, Uuid, &SecretString),
    now: Timestamp,
) -> Result<AddressCode, AddressChangeError> {
    let route = (kind.change(), ADDRESS_REQUEST_ROUTE);
    require_step_up(db, principal, route, now).await?;
    may_act(principal, member)?;
    let mailbox = minted(db, vault, address).await?;
    let by = acting(principal);
    let code = match kind {
        AddressKind::Notice => request_notice_change(db, secret, by, (member, mailbox), now)
            .await?
            .ok_or(AddressChangeError::Unchanged)?,
        AddressKind::Backup => match request_backup(db, secret, by, (member, mailbox), now).await {
            Err(EmailCheckError::NotAllowed) => return Err(AddressChangeError::Taken),
            done => done?,
        },
    };
    let code = SecretString::from(code);
    Ok(AddressCode { mailbox, code })
}

/// BIZ-040, BIZ-043: takes the code emailed for `member`'s change to `address` (a notice change's
/// mailbox minted again, D-3892; a backup's newest request); returns whether it was taken. A
/// wrong or old code confirms nothing.
///
/// # Errors
/// [`AddressChangeError::Taken`], `BadAddress` or `Database`.
pub async fn confirm_address_change(
    db: &mut PgConnection,
    (vault, secret): (&Vault, &SecretString),
    principal: &BusinessPrincipal,
    (kind, member, address): (AddressKind, Uuid, &SecretString),
    (code, now): (&str, Timestamp),
) -> Result<bool, AddressChangeError> {
    may_act(principal, member)?;
    let business = principal.business;
    let taken = match kind {
        AddressKind::Notice => {
            let mailbox = minted(db, vault, address).await?;
            confirm_mailbox(db, secret, (business, mailbox), code, now).await
        }
        AddressKind::Backup => confirm_backup(db, secret, (business, member), code, now).await,
    };
    match taken {
        Err(EmailCheckError::NotAllowed) => Err(AddressChangeError::Taken),
        done => Ok(done?),
    }
}

/// SEC-038, BIZ-043: removes `member`'s backup address after a step-up in the last 5 minutes;
/// the main address is told when a verified one went. A member who is not a current member of
/// the person's business is not found (SEC-019).
///
/// # Errors
/// [`AddressChangeError`].
pub async fn remove_backup_address(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    member: Uuid,
    now: Timestamp,
) -> Result<bool, AddressChangeError> {
    let route = (SensitiveChange::BackupAddress, BACKUP_REMOVE_ROUTE);
    require_step_up(db, principal, route, now).await?;
    may_act(principal, member)?;
    let team = members(db, principal.business).await;
    let team = team.map_err(|_| AddressChangeError::Database)?;
    if !team.iter().any(|m| m.id == member) {
        return Err(AddressChangeError::NotFound);
    }
    Ok(remove_backup(db, acting(principal), member, now).await?)
}

/// SEC-024: the owner acts for any member, a member only for themself; else not found.
fn may_act(principal: &BusinessPrincipal, member: Uuid) -> Result<(), AddressChangeError> {
    if principal.owner || principal.member == member {
        Ok(())
    } else {
        Err(AddressChangeError::NotFound)
    }
}

fn acting(principal: &BusinessPrincipal) -> Acting {
    Acting {
        business: principal.business,
        member: principal.member,
    }
}

/// What the page shows.
#[derive(Clone, Copy)]
pub enum AddressScreen<'a> {
    /// The two forms.
    Forms,
    /// A form again, its typed value kept, with the problem beside the field (A11Y-030).
    Problem(AddressKind, &'a str, AddressChangeError),
    /// The code form for the typed address; `true` after a wrong or old code.
    Code(AddressKind, &'a str, bool),
    /// The code was taken.
    Confirmed(AddressKind),
    /// The backup address is gone; `true` when a verified one went and the main address was told.
    Removed(bool),
    /// Too many codes (`true`) or code tries (`false`) from here or for this person (SEC-014).
    Limited(bool),
}

#[derive(Template)]
#[template(path = "address_change.html")]
struct AddressView<'a> {
    csrf: &'a str,
    member: Uuid,
    kind: &'a str,
    typed: &'a str,
    problem: &'a str,
    code_form: bool,
    wrong: bool,
    said: &'a str,
    forms: [(&'static str, &'static str, &'static str); 2],
}

impl AddressView<'_> {
    /// Whether the problem sits beside the `code` form's field.
    fn on(&self, code: &str) -> bool {
        code == self.kind && !self.problem.is_empty()
    }

    /// The typed value kept in the `code` form's field (A11Y-030).
    fn kept(&self, code: &str) -> &str {
        if code == self.kind { self.typed } else { "" }
    }
}

/// The two address forms: value, label and hint.
const FORMS: [(&str, &str, &str); 2] = [
    (
        "notice",
        "New email for job notices",
        "Job requests, reminders and word when a customer picks you go here.",
    ),
    (
        "backup",
        "Backup email",
        "A second address that gets the same notices.",
    ),
];

/// The line beside the field for `error` (A11Y-030); it names no other member.
fn problem_line(error: AddressChangeError) -> &'static str {
    match error {
        AddressChangeError::BadAddress => "Type a full email address, like name@example.com.",
        AddressChangeError::Taken => {
            "Someone on your team already signs in with this address. Type a different one."
        }
        AddressChangeError::Unchanged => {
            "We already send your notices to this address. Type a different one."
        }
        _ => "Something went wrong on our side. Try again in a minute.",
    }
}

/// The status line for a finished screen.
fn said_line(screen: AddressScreen<'_>) -> &'static str {
    match screen {
        AddressScreen::Confirmed(AddressKind::Notice) => {
            "Done. Your notices now go to your new address, and we emailed your old one to say so."
        }
        AddressScreen::Confirmed(AddressKind::Backup) => {
            "Done. Your backup address now gets your notices too. We emailed your notice address to let you know."
        }
        AddressScreen::Removed(true) => {
            "We've removed your backup address and emailed your notice address to let you know."
        }
        AddressScreen::Removed(false) => "You don't have a backup address now.",
        AddressScreen::Limited(true) => {
            "You've asked for too many codes. Try again later. Your notices still go to your current address."
        }
        AddressScreen::Limited(false) => {
            "You've tried too many codes. Try again later. Your notices still go to your current address."
        }
        _ => "",
    }
}

/// BIZ-040, BIZ-043, A11Y-030: the page for `screen`, its forms carrying `csrf` and `member`.
///
/// # Errors
/// The template's render error.
pub fn address_change_page(
    screen: AddressScreen<'_>,
    member: Uuid,
    csrf: &str,
) -> Result<String, askama::Error> {
    let (kind, typed, problem, code_form, wrong) = match screen {
        AddressScreen::Problem(kind, typed, why) => (kind, typed, problem_line(why), false, false),
        AddressScreen::Code(kind, typed, wrong) => (kind, typed, "", true, wrong),
        _ => (AddressKind::Notice, "", "", false, false),
    };
    AddressView {
        csrf,
        member,
        kind: kind.code(),
        typed,
        problem,
        code_form,
        wrong,
        said: said_line(screen),
        forms: FORMS,
    }
    .render()
}
