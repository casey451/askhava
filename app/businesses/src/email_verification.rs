//! Verified notice addresses (BIZ-040, BIZ-055, SEC-024, SEC-038; T196, D-1846, D-1847).
//!
//! Every business mailbox gets nothing until it is verified: the sign-up email by a 6-digit code
//! ([`start_signup_check`], [`confirm`]; activation waits for it in `identity::settle`), a team
//! member's address by their sign-in link ([`verified_by_link`]), and a changed notice address by
//! a code sent to the new address ([`request_change`], [`confirm`]). Only the owner changes a
//! notice address (a member only their own), only within [`STEP_UP_LIFE`] of passing a Verify code
//! ([`record_step_up`]); the change is audit-logged, and on confirmation the member's mailbox moves
//! and one [`CHANGED_KIND`] task tells the old address. A hard bounce, a complaint, or two soft
//! bounces [`SOFT_SPAN_DAYS`] apart mark it returned ([`mark_returned`]): [`may_get_notices`] is then
//! false, one [`RETURNED_KIND`] task emails the owner's other verified addresses and adds the digest
//! line, and the portal shows [`returned_line`]. No address value is stored here (PRIV-001), and
//! no call is ever placed (BIZ-039). Every email goes from `messaging::api::FROM` (SEC-025).

use askhava_adapters::api::IdempotencyKey;
use askhava_core::api::{NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use subtle::ConstantTimeEq;

use crate::models::BusinessesError;
use crate::team::Acting;

/// How long a mailbox code works after it was sent (BIZ-040).
pub const CODE_LIFE: SignedDuration = SignedDuration::from_mins(30);
/// Wrong mailbox codes allowed per send (BIZ-040, SEC-048).
pub const CHECK_TRIES: i32 = 5;
/// How recent a Verify code must be for a notice-address change (SEC-038, D-492).
pub const STEP_UP_LIFE: SignedDuration = SignedDuration::from_mins(5);
/// Two soft bounces at least this many days apart count as a hard bounce (BIZ-055).
pub const SOFT_SPAN_DAYS: i32 = 14;
/// The task that emails the old address after a notice-address change (SEC-024).
pub const CHANGED_KIND: &str = "businesses.notice_address_changed";
/// The task that tells the owner's other verified addresses and the digest (BIZ-040).
pub const RETURNED_KIND: &str = "businesses.notice_address_returned";
const CODE_ACTION: &str = "businesses.mailbox_code";

/// Why a notice-address call failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum EmailCheckError {
    /// A database call failed.
    #[error("email check database call failed")]
    Database,
    /// Only the owner (or a member for their own address) may change it (SEC-024).
    #[error("notice address change not allowed")]
    NotAllowed,
    /// No Verify code in the last 5 minutes (SEC-038).
    #[error("notice address change needs a fresh code")]
    NoFreshCode,
    /// No such member of the business.
    #[error("unknown team member")]
    UnknownMember,
}

/// What the email provider reported for an address (BIZ-040, BIZ-055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returned {
    /// A permanent failure.
    Hard,
    /// A spam complaint.
    Complaint,
    /// A temporary failure.
    Soft,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, EmailCheckError> {
    r.map_err(|_| EmailCheckError::Database)
}

/// The 6-digit code of one mailbox check send, keyed from its row and send time, never stored.
fn code_of(secret: &SecretString, id: Uuid, sent: Timestamp) -> String {
    let parts = [id.to_string(), sent.as_second().to_string()];
    let key = IdempotencyKey::derive(secret, CODE_ACTION, &[&parts[0], &parts[1]]);
    let n = u64::from_str_radix(&key.as_str()[..12], 16).unwrap_or_default();
    format!("{:06}", n % 1_000_000)
}

/// Opens (or resends) the check of `mailbox` and returns the code to email; `None` when it is
/// already verified. A resend makes the earlier code stop working.
async fn open_check(
    db_: &mut PgConnection,
    secret: &SecretString,
    (business, member, mailbox, replaces): (Uuid, Option<Uuid>, Uuid, Option<Uuid>),
    now: Timestamp,
) -> Result<Option<String>, EmailCheckError> {
    let q = sqlx::query_scalar!(
        "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, replaces, sent_at) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (business_id, mailbox_id) DO UPDATE \
         SET sent_at = $5, tries = 0, member_id = $2, replaces = $4 \
         WHERE businesses.mailbox_check.verified_at IS NULL RETURNING id AS \"id!\"",
        business,
        member,
        mailbox,
        replaces,
        now.to_sqlx() as _
    );
    Ok(db(q.fetch_optional(db_).await)?.map(|id| code_of(secret, id, now)))
}

/// BIZ-040: opens the sign-up email's check and returns the code to email to it; activation
/// waits until [`confirm`] takes it. `None` when already verified.
///
/// # Errors
/// [`EmailCheckError::Database`], also for an unknown sign-up.
pub async fn start_signup_check(
    conn: &mut PgConnection,
    secret: &SecretString,
    business: Uuid,
    now: Timestamp,
) -> Result<Option<String>, EmailCheckError> {
    let q = sqlx::query_scalar!(
        "SELECT mailbox_id FROM businesses.signup WHERE business_id = $1",
        business
    );
    let mailbox = db(q.fetch_one(&mut *conn).await)?;
    open_check(conn, secret, (business, None, mailbox, None), now).await
}

/// BIZ-040: whether the sign-up email has an unverified check, which holds activation.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn signup_email_waits(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<bool, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.mailbox_check c JOIN businesses.signup s \
         ON s.business_id = c.business_id AND s.mailbox_id = c.mailbox_id \
         WHERE c.business_id = $1 AND c.member_id IS NULL AND c.verified_at IS NULL) AS \"w!\"",
        business
    );
    q.fetch_one(conn)
        .await
        .map_err(|_| BusinessesError::Database)
}

/// BIZ-040: a team member's own address is verified by their used sign-in link.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn verified_by_link(
    conn: &mut PgConnection,
    by: Acting,
    mailbox: Uuid,
    at: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, sent_at, \
         verified_at) VALUES ($1, $2, $3, $4, $4) ON CONFLICT (business_id, mailbox_id) DO NOTHING",
        by.business,
        by.member,
        mailbox,
        at.to_sqlx() as _
    );
    db(q.execute(conn).await).map(|_| ())
}

/// SEC-038: records that `by` passed a Verify code to their verified phone or email at `at`.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn record_step_up(
    conn: &mut PgConnection,
    by: Acting,
    at: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "UPDATE businesses.team_member SET step_up_at = $3 WHERE id = $2 AND business_id = $1",
        by.business,
        by.member,
        at.to_sqlx() as _
    );
    db(q.execute(conn).await).map(|_| ())
}

async fn log(
    conn: &mut PgConnection,
    by: Acting,
    action: &'static str,
) -> Result<(), EmailCheckError> {
    let entry = AuditEntry {
        actor: Actor::Business(by.business),
        action,
        subject_kind: "businesses.team_member",
        subject_id: by.member,
        change: Change::Field("mailbox_id"),
    };
    audit(conn, &entry)
        .await
        .map(|_| ())
        .map_err(|_| EmailCheckError::Database)
}

/// SEC-024, SEC-038: whether `by` may change `member`'s notice address now: the owner, or the
/// member for their own address, with a Verify code no older than [`STEP_UP_LIFE`].
async fn may_change(
    conn: &mut PgConnection,
    by: Acting,
    member: Uuid,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "SELECT is_owner, step_up_at FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL",
        by.member,
        by.business
    );
    let me = db(q.fetch_optional(&mut *conn).await)?.ok_or(EmailCheckError::NotAllowed)?;
    if !me.is_owner && by.member != member {
        return Err(EmailCheckError::NotAllowed);
    }
    let fresh = me.step_up_at.map(|t| t.to_jiff());
    if fresh.is_none_or(|at| now.duration_since(at) > STEP_UP_LIFE) {
        return Err(EmailCheckError::NoFreshCode);
    }
    Ok(())
}

/// SEC-024, SEC-038, BIZ-040: asks to move `member`'s notice address to `mailbox` and returns the
/// code to email to the new address; the old one keeps every notice until [`confirm`]. A refusal
/// is audit-logged as `businesses.notice_change_refused`, a request as
/// `businesses.notice_change_requested`.
///
/// # Errors
/// [`EmailCheckError::NotAllowed`], [`EmailCheckError::NoFreshCode`],
/// [`EmailCheckError::UnknownMember`] or `Database`.
pub async fn request_change(
    conn: &mut PgConnection,
    secret: &SecretString,
    by: Acting,
    (member, mailbox): (Uuid, Uuid),
    now: Timestamp,
) -> Result<Option<String>, EmailCheckError> {
    if let Err(e) = may_change(conn, by, member, now).await {
        log(conn, by, "businesses.notice_change_refused").await?;
        return Err(e);
    }
    let q = sqlx::query_scalar!(
        "SELECT mailbox_id FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL",
        member,
        by.business
    );
    let old = db(q.fetch_optional(&mut *conn).await)?.ok_or(EmailCheckError::UnknownMember)?;
    let ids = (by.business, Some(member), mailbox, Some(old));
    let code = open_check(conn, secret, ids, now).await?;
    log(conn, by, "businesses.notice_change_requested").await?;
    Ok(code)
}

/// BIZ-040, SEC-024: takes the code emailed to `mailbox`. Right, within [`CODE_LIFE`] and
/// [`CHECK_TRIES`]: the mailbox is verified; for a change the member's address moves, the change is
/// audit-logged and one [`CHANGED_KIND`] task tells the old address, in one transaction; for the
/// sign-up email the sign-up settles (activation). Returns whether the code was right.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn confirm(
    conn: &mut PgConnection,
    secret: &SecretString,
    (business, mailbox): (Uuid, Uuid),
    code: &str,
    now: Timestamp,
) -> Result<bool, EmailCheckError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query!(
        "SELECT id, member_id, replaces, sent_at, tries FROM businesses.mailbox_check \
         WHERE business_id = $1 AND mailbox_id = $2 AND verified_at IS NULL FOR UPDATE",
        business,
        mailbox
    );
    let Some(row) = db(q.fetch_optional(&mut *tx).await)? else {
        return Ok(false);
    };
    let sent = row.sent_at.to_jiff();
    let live = row.tries < CHECK_TRIES && now.duration_since(sent) <= CODE_LIFE;
    let right = live
        && bool::from(
            code_of(secret, row.id, sent)
                .as_bytes()
                .ct_eq(code.as_bytes()),
        );
    let q = sqlx::query!(
        "UPDATE businesses.mailbox_check SET tries = tries + (NOT $2)::int, \
         verified_at = CASE WHEN $2 THEN $3::timestamptz END WHERE id = $1",
        row.id,
        right,
        now.to_sqlx() as _
    );
    db(q.execute(&mut *tx).await)?;
    if let (true, Some(member), Some(old)) = (right, row.member_id, row.replaces) {
        moved(&mut tx, Acting { business, member }, (old, mailbox), now).await?;
    }
    db(tx.commit().await)?;
    if right && row.member_id.is_none() {
        crate::identity::settle(conn, business, now)
            .await
            .map_err(|_| EmailCheckError::Database)?;
    }
    Ok(right)
}

/// SEC-024: moves the member's address and queues the old address's notice, with its audit row.
async fn moved(
    tx: &mut PgConnection,
    who: Acting,
    (old, new): (Uuid, Uuid),
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "UPDATE businesses.team_member SET mailbox_id = $4 \
         WHERE id = $2 AND business_id = $1 AND mailbox_id = $3",
        who.business,
        who.member,
        old,
        new
    );
    db(q.execute(&mut *tx).await)?;
    let payload = json!({"business": who.business, "member": who.member, "old": old});
    let mut task = NewTask::new(CHANGED_KIND, payload, now);
    task.dedupe_key = Some(format!("{CHANGED_KIND}.{old}.{new}"));
    enqueue(&mut *tx, &task)
        .await
        .map_err(|_| EmailCheckError::Database)?;
    log(tx, who, "businesses.notice_address_changed").await
}

/// BIZ-040: whether `mailbox` may get notices: verified and not returned.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn may_get_notices(
    conn: &mut PgConnection,
    business: Uuid,
    mailbox: Uuid,
) -> Result<bool, EmailCheckError> {
    let q = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.mailbox_check WHERE business_id = $1 \
         AND mailbox_id = $2 AND verified_at IS NOT NULL AND returned_at IS NULL) AS \"ok!\"",
        business,
        mailbox
    );
    db(q.fetch_one(conn).await)
}

/// BIZ-040, BIZ-055: records what the provider reported; a hard bounce, a complaint or a second
/// soft bounce [`SOFT_SPAN_DAYS`] or more after the first marks the address returned and queues
/// one [`RETURNED_KIND`] task. Returns true when the address just turned returned. Texts continue
/// and no call is placed (BIZ-039).
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn mark_returned(
    conn: &mut PgConnection,
    (business, mailbox): (Uuid, Uuid),
    what: Returned,
    at: Timestamp,
) -> Result<bool, EmailCheckError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query_scalar!(
        "UPDATE businesses.mailbox_check SET returned_at = CASE WHEN $3 OR (soft_at IS NOT NULL \
         AND $4 >= soft_at + make_interval(days => $5)) THEN $4 END, \
         soft_at = COALESCE(soft_at, $4) \
         WHERE business_id = $1 AND mailbox_id = $2 AND returned_at IS NULL \
         RETURNING returned_at IS NOT NULL AS \"r!\"",
        business,
        mailbox,
        what != Returned::Soft,
        at.to_sqlx() as _,
        SOFT_SPAN_DAYS
    );
    let turned = db(q.fetch_optional(&mut *tx).await)?.unwrap_or(false);
    if turned {
        let payload = json!({"business": business, "mailbox": mailbox});
        let mut task = NewTask::new(RETURNED_KIND, payload, at);
        task.dedupe_key = Some(format!("{RETURNED_KIND}.{mailbox}.{}", at.as_second()));
        enqueue(&mut tx, &task)
            .await
            .map_err(|_| EmailCheckError::Database)?;
    }
    db(tx.commit().await)?;
    Ok(turned)
}

/// BIZ-040: the business's returned mailboxes, for the portal banner.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn returned_mailboxes(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<Uuid>, EmailCheckError> {
    let q = sqlx::query_scalar!(
        "SELECT mailbox_id FROM businesses.mailbox_check \
         WHERE business_id = $1 AND returned_at IS NOT NULL ORDER BY returned_at",
        business
    );
    db(q.fetch_all(conn).await)
}

/// BIZ-040: the portal banner for a returned address, word for word.
#[must_use]
pub fn returned_line(address: &str) -> String {
    format!(
        "Ask Hava emails to {address} are being returned or marked as spam, so you may miss job \
         requests. Add or fix an email address."
    )
}
