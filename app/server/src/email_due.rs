//! The queued-email drain's business addresses (BIZ-019, BIZ-043, BIZ-051, SEC-024, PRIV-003,
//! PRIV-004, PRIV-016; T470, D-3750, D-4196, D-4670 to D-4673).
//!
//! [`NoticeAddresses`] is the `messaging::api::BusinessAddresses` the composition hands to
//! `messaging.email_due`. A row queued to `messaging::api::OWNER_INBOX` (the OPS-041 coverage
//! notice) goes to the owner inbox setting, the address `run_coverage_notice` sends to. Any other
//! business row goes only while one business its subject IDs name (or, for a receipt, the business
//! of the pick it names) has not left (BIZ-051: leaving
//! stops all notices) and its mailbox is still one of that business's: a current member's main
//! address or a member's verified, not-removed backup (BIZ-043; a removed member gets nothing,
//! SEC-024); a row naming no such business is `Gone`, and the drain drops it unsent, since it holds
//! that person's sign-in link. The address is then read with `privacy::api::read_business_mailbox`
//! under `BusinessPurpose::Notice` on the handler's bare connection (one audited read). No address
//! is stored or logged here.

use std::sync::Arc;

use askhava_businesses::api::{
    AccountStatus, Business, BusinessesError, NoticePurpose, business, members, notice_addresses,
};
use askhava_core::api::HandlerError;
use askhava_messaging::api::{BusinessAddresses, DueAddress, DueBusiness, OWNER_INBOX};
use askhava_picks::api::pick_business;
use askhava_privacy::api::{BusinessContactError, BusinessPurpose, Vault, read_business_mailbox};
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const FAILED: HandlerError = HandlerError("the notice address could not be read");
/// The notice purposes a backup may receive (account notices go to main addresses only).
const BACKUP_PURPOSES: [NoticePurpose; 3] = [
    NoticePurpose::Cards,
    NoticePurpose::PicksAndRelease,
    NoticePurpose::Money,
];

/// The drain's business address reads over the one vault and the owner inbox. No `Debug`.
pub struct NoticeAddresses {
    vault: Arc<Vault>,
    owner_inbox: Option<SecretString>,
}

impl NoticeAddresses {
    /// The resolver reading mailboxes in `vault`; `owner_inbox` is the OPS-041 address setting.
    #[must_use]
    pub fn new(vault: Arc<Vault>, owner_inbox: Option<SecretString>) -> Self {
        Self { vault, owner_inbox }
    }
}

impl BusinessAddresses for NoticeAddresses {
    /// BIZ-051, SEC-024, BIZ-043, D-4196: the owner inbox for its rows, else the mailbox's
    /// address while it is one of a named business's that has not left, else `Gone`.
    async fn address(
        &self,
        db: &mut PgConnection,
        row: DueBusiness<'_>,
    ) -> Result<DueAddress, HandlerError> {
        if row.recipient == OWNER_INBOX {
            return self
                .owner_inbox
                .clone()
                .map(DueAddress::To)
                .ok_or(HandlerError("no owner inbox address is set"));
        }
        if !current(db, row.recipient, row.subjects).await? {
            return Ok(DueAddress::Gone);
        }
        let purpose = BusinessPurpose::Notice;
        match read_business_mailbox(db, &self.vault, purpose, row.recipient).await {
            Ok(address) => Ok(DueAddress::To(address)),
            Err(BusinessContactError::NotFound) => Ok(DueAddress::Gone),
            Err(_) => Err(FAILED),
        }
    }
}

/// BIZ-051, SEC-024, BIZ-043: whether `mailbox` is a current member's main address, or the
/// verified backup of a current member, of a business the row names that has not left.
async fn current(
    db: &mut PgConnection,
    mailbox: Uuid,
    subjects: &[Uuid],
) -> Result<bool, HandlerError> {
    for named in named_businesses(db, subjects).await? {
        if named.status == AccountStatus::Removed {
            continue;
        }
        let team = members(db, named.id).await.map_err(|_| FAILED)?;
        if team.iter().any(|m| m.mailbox == mailbox) {
            return Ok(true);
        }
        for purpose in BACKUP_PURPOSES {
            let to = notice_addresses(db, named.id, purpose)
                .await
                .map_err(|_| FAILED)?;
            if to.contains(&mailbox) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The businesses a row names: each subject ID that is a business (a business notice names its
/// own first) or, when none is, the business of each subject ID that is a pick (a receipt names
/// only its pick, FEE-031).
async fn named_businesses(
    db: &mut PgConnection,
    subjects: &[Uuid],
) -> Result<Vec<Business>, HandlerError> {
    let mut named = Vec::new();
    for &id in subjects {
        match business(db, id).await {
            Ok(found) => named.push(found),
            Err(BusinessesError::UnknownBusiness) => {}
            Err(_) => return Err(FAILED),
        }
    }
    if named.is_empty() {
        for &id in subjects {
            let Ok(owner) = pick_business(db, id).await else {
                continue;
            };
            named.push(business(db, owner).await.map_err(|_| FAILED)?);
        }
    }
    Ok(named)
}
