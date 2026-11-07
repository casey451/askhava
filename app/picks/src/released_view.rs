//! The business portal's released view: a released pick's homeowner first name, address and
//! (when she chose email contact) email, shown again to the signed-in business, with no stored
//! copy (PICK-022 to PICK-024, PRIV-002, PRIV-003, PRIV-005, BIZ-037, BIZ-043, SEC-046; D-1739).
//!
//! The business portal asks `picks`, never `privacy` (`docs/architecture.md` section 8).
//! [`released_view`] takes the [`BusinessSessionProof`] the business session extractor makes and
//! the pick's ID, and checks, server side and before any vault read: the pick has a
//! `picks.release` row; its status is paid or free ([`crate::status::shows_release`]); the
//! proof's business is the pick's business; and the person is, now, its owner or a "Confirm and
//! billing" member, read through `businesses::api::members` at each view, so a member removed or
//! demoted after the release is refused (PICK-024 and BIZ-043 send the release only to them). The
//! pure [`may_view`] is the rule. Every refusal is one [`ViewAnswer::NotFound`] that reads nothing
//! in the vault and says nothing about the pick. Then it opens the contact the release row is
//! bound to (never one looked up again) through `privacy::api::read_released`: name and address,
//! and the email only when the release row says she chose email contact (PICK-018); never the
//! phone (PICK-023). Viewing is not an introduction: it writes no release row, notice, charge, fee
//! or masked line, and may repeat. After the PRIV-007 purge or "Delete my data" the answer is
//! [`ViewAnswer::Removed`] and nothing is opened.

use askhava_businesses::api::members;
use askhava_privacy::api::{BusinessSessionProof, Field, Vault, VaultError, read_released};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::{self, PicksError, ReleaseBinding};
use crate::release::{EMAIL_CHOSEN_FIELDS, RELEASE_FIELDS, ReleaseView};
use crate::status::release_shown_codes;

/// The signed-in person's seat in the pick's business, read at each view (BIZ-023, BIZ-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// Not a current member of the pick's business (removed, or never on its team).
    Absent,
    /// The business's owner, who holds every role.
    Owner,
    /// A current member; `confirm_and_billing` is whether they hold that role.
    Member {
        /// The "Confirm and billing" role (BIZ-043).
        confirm_and_billing: bool,
    },
}

/// What [`may_view`] decides on; nothing here is read from the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewFacts {
    /// The proof's business is the pick's business (SEC-019).
    pub same_business: bool,
    /// The pick has its `picks.release` row (PICK-022).
    pub released: bool,
    /// The pick's status is paid or free now ([`crate::status::shows_release`], PRIV-005).
    pub shown_status: bool,
    /// The person's seat in the pick's business now.
    pub seat: Seat,
}

/// The released-view rule (PICK-022, PICK-024, PRIV-005, BIZ-037, BIZ-043, D-1739): released,
/// paid or free, the same business, and the owner or a "Confirm and billing" member; anything
/// else is the one not-found answer.
#[must_use]
pub const fn may_view(facts: ViewFacts) -> bool {
    let seated = match facts.seat {
        Seat::Owner => true,
        Seat::Member {
            confirm_and_billing,
        } => confirm_and_billing,
        Seat::Absent => false,
    };
    facts.same_business && facts.released && facts.shown_status && seated
}

/// What [`released_view`] answers.
#[derive(Debug)]
pub enum ViewAnswer {
    /// The details, read now from the vault and stored nowhere (PRIV-002).
    Shown(ReleaseView),
    /// The purge or "Delete my data" cleared them; nothing was opened (PRIV-007, PRIV-011).
    Removed,
    /// Every refusal: no such released pick for this person; nothing was read (D-1739).
    NotFound,
}

/// Shows released pick `pick` again to the business person `proof` names (PICK-022 to PICK-024,
/// PRIV-003, PRIV-005, BIZ-037, BIZ-043, SEC-046, D-1739); see the module docs. The vault is read
/// on a bare pooled connection, outside any transaction (D-753), and nothing is written but the
/// vault's own audited read rows.
///
/// # Errors
///
/// [`PicksError::Database`] for a failed statement, a failed team read or a failed vault read
/// (which `privacy` has already audited or alerted).
pub async fn released_view(
    pool: &PgPool,
    vault: &Vault,
    proof: &BusinessSessionProof,
    pick: Uuid,
) -> Result<ViewAnswer, PicksError> {
    let mut conn = pool.acquire().await.map_err(|_| PicksError::Database)?;
    let who = (proof.person(), proof.business());
    match viewable(&mut conn, who, pick).await? {
        Some(bound) => open(&mut conn, vault, proof, (pick, &bound)).await,
        None => Ok(ViewAnswer::NotFound),
    }
}

/// The release binding of `pick` when [`may_view`] lets `person` of `business` see it, else
/// `None`; reads `picks` and `businesses::api` only, never the vault (PICK-024, BIZ-037, BIZ-043,
/// D-1739).
pub(crate) async fn viewable(
    conn: &mut PgConnection,
    (person, business): (Uuid, Uuid),
    pick: Uuid,
) -> Result<Option<ReleaseBinding>, PicksError> {
    let codes = release_shown_codes();
    let Some(bound) = models::release_binding(conn, pick, &codes).await? else {
        return Ok(None);
    };
    let same_business = bound.business == business;
    let seat = if same_business {
        seat_of(conn, bound.business, person).await?
    } else {
        Seat::Absent
    };
    let facts = ViewFacts {
        same_business,
        released: true,
        shown_status: bound.shown_status,
        seat,
    };
    Ok(may_view(facts).then_some(bound))
}

/// `person`'s seat in `business` now, through `businesses::api` (BIZ-023, BIZ-037).
async fn seat_of(
    conn: &mut PgConnection,
    business: Uuid,
    person: Uuid,
) -> Result<Seat, PicksError> {
    let team = members(conn, business)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(team
        .iter()
        .find(|m| m.id == person)
        .map_or(Seat::Absent, |m| {
            if m.owner {
                Seat::Owner
            } else {
                Seat::Member {
                    confirm_and_billing: m.roles.confirm_and_billing(),
                }
            }
        }))
}

/// Opens the bound contact's name and address, and the email only when she chose email contact
/// (PICK-018, PICK-023); a cleared contact is [`ViewAnswer::Removed`].
async fn open(
    conn: &mut PgConnection,
    vault: &Vault,
    proof: &BusinessSessionProof,
    (pick, bound): (Uuid, &ReleaseBinding),
) -> Result<ViewAnswer, PicksError> {
    let fields: &[Field] = if bound.email_chosen {
        &EMAIL_CHOSEN_FIELDS
    } else {
        &RELEASE_FIELDS
    };
    match read_released(conn, vault, proof, (pick, bound.contact), fields).await {
        Ok(Some(details)) => Ok(ViewAnswer::Shown(ReleaseView::from_details(details))),
        Ok(None) | Err(VaultError::NotFound) => Ok(ViewAnswer::Removed),
        Err(_) => Err(PicksError::Database),
    }
}
