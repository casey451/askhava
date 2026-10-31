//! Team members, their roles and the staff phones each linked (BIZ-023, CALL-012, D-191, D-491),
//! and the notice-address lookup the enqueuing modules call before a business notice (D-492).
//!
//! Every change runs in one transaction with its audit row (JOB-005, R11): removing a member also
//! unlinks every phone it linked, takes each out of `messaging`'s sender directory and enqueues one
//! [`MEMBER_REMOVED_KIND`] event that `portals` handles by ending the member's sessions. A phone
//! added or changed reaches the directory through [`SenderLink`], which the composition implements
//! with `messaging::api::sender_add` and the vault, since `businesses` may not name `privacy`
//! (`kit/layers.json`, D-963); a removal calls `messaging::api::sender_remove` itself. No number or
//! email is stored or logged here, only opaque IDs (PRIV-001, PRIV-028).

use std::future::Future;

use askhava_core::api::{NewTask, enqueue};
use askhava_messaging::api::sender_remove;
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

/// The `core.task` kind enqueued once per removed member; `portals` ends that member's sessions
/// (TD-BIZ-06). Payload: `{"business": <id>, "member": <id>}`.
pub const MEMBER_REMOVED_KIND: &str = "businesses.member_removed";
const MEMBER: &str = "businesses.team_member";
const PHONE: &str = "businesses.staff_phone";

/// Why a team or phone call failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TeamError {
    /// A database call failed.
    #[error("team database call failed")]
    Database,
    /// The audit row could not be written (JOB-005).
    #[error("team audit row failed")]
    Audit,
    /// No business has the given ID.
    #[error("business not found")]
    UnknownBusiness,
    /// No current member has the given ID.
    #[error("team member not found")]
    UnknownMember,
    /// No linked phone has the given line ID.
    #[error("phone not found")]
    UnknownPhone,
    /// The owner always holds both roles and is never removed (BIZ-023).
    #[error("the owner keeps both roles")]
    OwnerKeepsBoth,
    /// The business already has an owner.
    #[error("the business already has an owner")]
    OwnerExists,
    /// A current member already signs in with that email.
    #[error("that email already belongs to a member")]
    MailboxTaken,
    /// Only the owner or a "Confirm and billing" member links a phone, and only the owner the main
    /// phone (CALL-012).
    #[error("this member may not link that phone")]
    NotAllowed,
    /// The business already has a main phone; change it instead.
    #[error("the business already has a main phone")]
    MainExists,
    /// The number is not a valid US phone number.
    #[error("not a valid phone number")]
    BadPhone,
    /// The number is already linked to another phone record.
    #[error("that number is already linked")]
    PhoneTaken,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, TeamError> {
    r.map_err(|_| TeamError::Database)
}

fn insert_error(e: &sqlx::Error) -> TeamError {
    match e.as_database_error() {
        Some(d) if d.constraint() == Some("team_member_one_owner_idx") => TeamError::OwnerExists,
        Some(d) if d.constraint() == Some("team_member_mailbox_idx") => TeamError::MailboxTaken,
        Some(d) if d.constraint() == Some("staff_phone_one_main_idx") => TeamError::MainExists,
        Some(d) if d.is_foreign_key_violation() => TeamError::UnknownBusiness,
        _ => TeamError::Database,
    }
}

/// A member's roles (BIZ-023): "Cards and quotes", "Confirm and billing" or both, never neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    cards_and_quotes: bool,
    confirm_and_billing: bool,
}

impl Roles {
    /// Both roles, which the owner always holds (BIZ-023).
    pub const BOTH: Self = Self {
        cards_and_quotes: true,
        confirm_and_billing: true,
    };

    /// The roles ticked, or `None` when neither is (BIZ-023: "and/or").
    #[must_use]
    pub fn new(cards_and_quotes: bool, confirm_and_billing: bool) -> Option<Self> {
        (cards_and_quotes || confirm_and_billing).then_some(Self {
            cards_and_quotes,
            confirm_and_billing,
        })
    }

    /// Holds "Cards and quotes": gets job cards and sends quotes.
    #[must_use]
    pub fn cards_and_quotes(self) -> bool {
        self.cards_and_quotes
    }

    /// Holds "Confirm and billing": confirms picks, pays and links phones (CALL-012).
    #[must_use]
    pub fn confirm_and_billing(self) -> bool {
        self.confirm_and_billing
    }

    /// Whether these roles receive notices of `purpose` (D-492, D-964); account notices go to the
    /// owner alone, whatever a member's roles.
    #[must_use]
    pub fn fits(self, purpose: NoticePurpose) -> bool {
        match purpose {
            NoticePurpose::Cards => self.cards_and_quotes,
            NoticePurpose::PicksAndRelease | NoticePurpose::Money => self.confirm_and_billing,
            NoticePurpose::Account => false,
        }
    }
}

/// What a business notice is about, choosing its recipients (D-492).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticePurpose {
    /// Job cards and quote notices (`quotes`).
    Cards,
    /// Picks and the release (`picks`).
    PicksAndRelease,
    /// Account notices such as a member added or removed (`businesses`).
    Account,
    /// Charges, declines and refunds (`picks`' money notices).
    Money,
}

/// One current member of a business account (BIZ-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamMember {
    /// Row ID.
    pub id: Uuid,
    /// The business account.
    pub business: Uuid,
    /// The opaque ID of the member's sign-in email (PRIV-001).
    pub mailbox: Uuid,
    /// The account owner.
    pub owner: bool,
    /// The member's roles; [`Roles::BOTH`] for the owner.
    pub roles: Roles,
}

/// How a phone added or changed reaches `messaging`'s sender directory (D-491, D-963): the
/// composition implements it with `messaging::api::sender_add` and the vault, mapping
/// `BadPhone` and `SenderTaken` to [`TeamError::BadPhone`] and [`TeamError::PhoneTaken`]. It runs
/// on the caller's transaction, so the directory changes with the phone row or not at all.
pub trait SenderLink {
    /// Links `line` of `business` to `number`, replacing the line's earlier number.
    fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> impl Future<Output = Result<(), TeamError>> + Send;
}

async fn log(
    tx: &mut PgConnection,
    business: Uuid,
    action: &'static str,
    (kind, id): (&'static str, Uuid),
    change: Change,
) -> Result<(), TeamError> {
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action,
        subject_kind: kind,
        subject_id: id,
        change,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| TeamError::Audit)
}

/// Locks and reads a current member (`FOR UPDATE`), so a removal and a change apply in turn.
async fn locked_member(tx: &mut PgConnection, id: Uuid) -> Result<TeamMember, TeamError> {
    let row = db(sqlx::query!(
        "SELECT id, business_id, mailbox_id, is_owner, cards_and_quotes, confirm_and_billing \
         FROM businesses.team_member WHERE id = $1 AND removed_at IS NULL FOR UPDATE",
        id
    )
    .fetch_optional(tx)
    .await)?
    .ok_or(TeamError::UnknownMember)?;
    Ok(TeamMember {
        id: row.id,
        business: row.business_id,
        mailbox: row.mailbox_id,
        owner: row.is_owner,
        roles: Roles::new(row.cards_and_quotes, row.confirm_and_billing)
            .ok_or(TeamError::Database)?,
    })
}

async fn add(
    conn: &mut PgConnection,
    (business, mailbox): (Uuid, Uuid),
    (owner, roles): (bool, Roles),
    now: Timestamp,
) -> Result<Uuid, TeamError> {
    let mut tx = db(conn.begin().await)?;
    let id = sqlx::query_scalar!(
        "INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes, \
         confirm_and_billing, added_at) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        business,
        mailbox,
        owner,
        roles.cards_and_quotes,
        roles.confirm_and_billing,
        now.to_sqlx() as _
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| insert_error(&e))?;
    log(
        &mut tx,
        business,
        "businesses.member_added",
        (MEMBER, id),
        Change::Action,
    )
    .await?;
    db(tx.commit().await)?;
    Ok(id)
}

/// Records the account owner, holding both roles, signing in with email `mailbox` (BIZ-023).
///
/// # Errors
///
/// [`TeamError::OwnerExists`], `MailboxTaken`, `UnknownBusiness`, `Database` or `Audit`.
pub async fn add_owner(
    conn: &mut PgConnection,
    business: Uuid,
    mailbox: Uuid,
    now: Timestamp,
) -> Result<Uuid, TeamError> {
    add(conn, (business, mailbox), (true, Roles::BOTH), now).await
}

/// Adds a team member signing in with email `mailbox`, holding `roles`; audited (BIZ-023).
///
/// # Errors
///
/// [`TeamError::MailboxTaken`], `UnknownBusiness`, `Database` or `Audit`.
pub async fn add_member(
    conn: &mut PgConnection,
    business: Uuid,
    mailbox: Uuid,
    roles: Roles,
    now: Timestamp,
) -> Result<Uuid, TeamError> {
    add(conn, (business, mailbox), (false, roles), now).await
}

/// Changes a member's roles; the owner's stay both (BIZ-023). An unchanged set writes nothing.
///
/// # Errors
///
/// [`TeamError::OwnerKeepsBoth`], `UnknownMember`, `Database` or `Audit`.
pub async fn set_roles(
    conn: &mut PgConnection,
    member: Uuid,
    roles: Roles,
) -> Result<(), TeamError> {
    let mut tx = db(conn.begin().await)?;
    let m = locked_member(&mut tx, member).await?;
    if m.owner && roles != Roles::BOTH {
        return Err(TeamError::OwnerKeepsBoth);
    }
    if m.roles == roles {
        return Ok(());
    }
    db(sqlx::query!(
        "UPDATE businesses.team_member SET cards_and_quotes = $2, confirm_and_billing = $3 \
         WHERE id = $1",
        member,
        roles.cards_and_quotes,
        roles.confirm_and_billing
    )
    .execute(&mut *tx)
    .await)?;
    let change = Change::Field("roles");
    log(
        &mut tx,
        m.business,
        "businesses.member_changed",
        (MEMBER, member),
        change,
    )
    .await?;
    db(tx.commit().await)
}

/// Removes a member (never the owner, BIZ-023): unlinks every phone it linked and takes each out
/// of the sender directory (CALL-012, D-491), and enqueues one [`MEMBER_REMOVED_KIND`] event so
/// `portals` ends its sessions (TD-BIZ-06), all with their audit rows in one transaction.
///
/// # Errors
///
/// [`TeamError::OwnerKeepsBoth`], `UnknownMember`, `Database` or `Audit`.
pub async fn remove_member(
    conn: &mut PgConnection,
    member: Uuid,
    now: Timestamp,
) -> Result<(), TeamError> {
    let mut tx = db(conn.begin().await)?;
    let m = locked_member(&mut tx, member).await?;
    if m.owner {
        return Err(TeamError::OwnerKeepsBoth);
    }
    db(sqlx::query!(
        "UPDATE businesses.team_member SET removed_at = $2 WHERE id = $1",
        member,
        now.to_sqlx() as _
    )
    .execute(&mut *tx)
    .await)?;
    let lines = db(sqlx::query_scalar!(
        "DELETE FROM businesses.staff_phone WHERE linked_by = $1 RETURNING id",
        member
    )
    .fetch_all(&mut *tx)
    .await)?;
    for line in lines {
        unlink_line(&mut tx, m.business, line).await?;
    }
    log(
        &mut tx,
        m.business,
        "businesses.member_removed",
        (MEMBER, member),
        Change::Action,
    )
    .await?;
    let payload = json!({"business": m.business, "member": member});
    let mut task = NewTask::new(MEMBER_REMOVED_KIND, payload, now);
    task.dedupe_key = Some(format!("{MEMBER_REMOVED_KIND}.{member}"));
    enqueue(&mut tx, &task)
        .await
        .map_err(|_| TeamError::Database)?;
    db(tx.commit().await)
}

/// Links a phone for the business of `member`, the main phone when `main` (CALL-012, D-491):
/// only the owner or a "Confirm and billing" member links a phone, only the owner the main one.
/// The number is in the sender directory when this returns; the line ID is returned.
///
/// # Errors
///
/// [`TeamError::NotAllowed`], `MainExists`, `BadPhone`, `PhoneTaken`, `UnknownMember`,
/// `Database` or `Audit`.
pub async fn link_phone(
    conn: &mut PgConnection,
    dir: &impl SenderLink,
    (member, main): (Uuid, bool),
    number: &SecretString,
    now: Timestamp,
) -> Result<Uuid, TeamError> {
    let mut tx = db(conn.begin().await)?;
    let m = locked_member(&mut tx, member).await?;
    if !(m.owner || m.roles.confirm_and_billing) || (main && !m.owner) {
        return Err(TeamError::NotAllowed);
    }
    let line = sqlx::query_scalar!(
        "INSERT INTO businesses.staff_phone (business_id, linked_by, is_main, linked_at) \
         VALUES ($1, $2, $3, $4) RETURNING id",
        m.business,
        member,
        main,
        now.to_sqlx() as _
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| insert_error(&e))?;
    dir.link(&mut tx, (m.business, line), number).await?;
    log(
        &mut tx,
        m.business,
        "businesses.phone_linked",
        (PHONE, line),
        Change::Action,
    )
    .await?;
    db(tx.commit().await)?;
    Ok(line)
}

/// Changes the number of linked phone `line`; the sender directory follows at once (D-491).
///
/// # Errors
///
/// [`TeamError::UnknownPhone`], `BadPhone`, `PhoneTaken`, `Database` or `Audit`.
pub async fn change_phone(
    conn: &mut PgConnection,
    dir: &impl SenderLink,
    line: Uuid,
    number: &SecretString,
) -> Result<(), TeamError> {
    let mut tx = db(conn.begin().await)?;
    let business = db(sqlx::query_scalar!(
        "SELECT business_id FROM businesses.staff_phone WHERE id = $1 FOR UPDATE",
        line
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(TeamError::UnknownPhone)?;
    dir.link(&mut tx, (business, line), number).await?;
    let change = Change::Field("number");
    log(
        &mut tx,
        business,
        "businesses.phone_changed",
        (PHONE, line),
        change,
    )
    .await?;
    db(tx.commit().await)
}

/// Unlinks phone `line` and takes it out of the sender directory at once (CALL-012, D-491).
///
/// # Errors
///
/// [`TeamError::UnknownPhone`], `Database` or `Audit`.
pub async fn unlink_phone(conn: &mut PgConnection, line: Uuid) -> Result<(), TeamError> {
    let mut tx = db(conn.begin().await)?;
    let business = db(sqlx::query_scalar!(
        "DELETE FROM businesses.staff_phone WHERE id = $1 RETURNING business_id",
        line
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(TeamError::UnknownPhone)?;
    unlink_line(&mut tx, business, line).await?;
    db(tx.commit().await)
}

async fn unlink_line(tx: &mut PgConnection, business: Uuid, line: Uuid) -> Result<(), TeamError> {
    sender_remove(tx, line)
        .await
        .map_err(|_| TeamError::Database)?;
    log(
        tx,
        business,
        "businesses.phone_unlinked",
        (PHONE, line),
        Change::Action,
    )
    .await
}

/// The current members of `business`, the owner first (BIZ-023).
///
/// # Errors
///
/// [`TeamError::Database`].
pub async fn members(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<TeamMember>, TeamError> {
    let rows = db(sqlx::query!(
        "SELECT id, mailbox_id, is_owner, cards_and_quotes, confirm_and_billing \
         FROM businesses.team_member WHERE business_id = $1 AND removed_at IS NULL \
         ORDER BY is_owner DESC, added_at, id",
        business
    )
    .fetch_all(conn)
    .await)?;
    rows.into_iter()
        .map(|r| {
            Ok(TeamMember {
                id: r.id,
                business,
                mailbox: r.mailbox_id,
                owner: r.is_owner,
                roles: Roles::new(r.cards_and_quotes, r.confirm_and_billing)
                    .ok_or(TeamError::Database)?,
            })
        })
        .collect()
}

/// The opaque email IDs a notice of `purpose` goes to: the owner plus every current member whose
/// roles fit it, the owner first (D-492). `quotes`, `picks` and `businesses` call it before
/// enqueuing a notice; T149 adds backup addresses and T196 verification.
///
/// # Errors
///
/// [`TeamError::Database`].
pub async fn notice_addresses(
    conn: &mut PgConnection,
    business: Uuid,
    purpose: NoticePurpose,
) -> Result<Vec<Uuid>, TeamError> {
    Ok(members(conn, business)
        .await?
        .into_iter()
        .filter(|m| m.owner || m.roles.fits(purpose))
        .map(|m| m.mailbox)
        .collect())
}
