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
use askhava_core::api::{NewTask, brand, enqueue};
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
/// The audit row naming the member who made a notice-address or channel change (SEC-024, D-1850).
pub const ACTED_BY_ACTION: &str = "businesses.notice_change_by";
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

/// One opening of a mailbox check (D-1850): the member whose address changes (`None` for the
/// sign-up email), the mailbox it replaces (`None` re-verifies the member's own mailbox, BIZ-040
/// "fix") and the member who asked.
#[derive(Debug, Clone, Copy)]
struct Opening {
    business: Uuid,
    mailbox: Uuid,
    change_for: Option<Uuid>,
    replaces: Option<Uuid>,
    requested_by: Option<Uuid>,
}

/// Opens (or resends) the check of the mailbox and returns the code to email; `None` when it is
/// verified, not returned and no change asks for it. A resend makes the earlier code stop working
/// but keeps the wrong-code count within [`CODE_LIFE`] of the window's first send (D-1850).
async fn open_check(
    db_: &mut PgConnection,
    secret: &SecretString,
    o: Opening,
    now: Timestamp,
) -> Result<Option<String>, EmailCheckError> {
    let window = now
        .checked_sub(CODE_LIFE)
        .map_err(|_| EmailCheckError::Database)?;
    let q = sqlx::query_scalar!(
        "INSERT INTO businesses.mailbox_check AS c (business_id, mailbox_id, change_for, replaces, \
         requested_by, sent_at, window_at) VALUES ($1, $2, $3, $4, $5, $6, $6) \
         ON CONFLICT (business_id, mailbox_id) DO UPDATE SET \
         tries = CASE WHEN c.open AND c.window_at > $7 THEN c.tries ELSE 0 END, \
         window_at = CASE WHEN c.open AND c.window_at > $7 THEN c.window_at ELSE $6 END, \
         sent_at = $6, open = true, change_for = $3, replaces = $4, requested_by = $5 \
         WHERE c.verified_at IS NULL OR c.returned_at IS NOT NULL OR $4::uuid IS NOT NULL \
         RETURNING id AS \"id!\"",
        o.business,
        o.mailbox,
        o.change_for,
        o.replaces,
        o.requested_by,
        now.to_sqlx() as _,
        window.to_sqlx() as _
    );
    Ok(db(q.fetch_optional(db_).await)?.map(|id| code_of(secret, id, now)))
}

/// BIZ-040: opens the sign-up email's check and returns the code to email to it; activation
/// waits until [`confirm`] takes it. `None` when already verified and not returned.
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
    let o = Opening {
        business,
        mailbox,
        change_for: None,
        replaces: None,
        requested_by: None,
    };
    open_check(conn, secret, o, now).await
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
        "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, sent_at, \
         window_at, verified_at) VALUES ($1, $2, $3, false, $4, $4, $4) \
         ON CONFLICT (business_id, mailbox_id) DO NOTHING",
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

/// SEC-024 (D-1850): writes the event's row on the member whose address changes (field
/// `mailbox_id`, no value) and one [`ACTED_BY_ACTION`] row on the member who acted, so every
/// notice change names both. `(by, member)` are team member IDs of `business`.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn log_notice_change(
    conn: &mut PgConnection,
    business: Uuid,
    (by, member): (Uuid, Uuid),
    (action, field): (&'static str, &'static str),
) -> Result<(), EmailCheckError> {
    let rows = [
        (action, member, Change::Field(field)),
        (ACTED_BY_ACTION, by, Change::Action),
    ];
    for (action, subject_id, change) in rows {
        let entry = AuditEntry {
            actor: Actor::Business(business),
            action,
            subject_kind: "businesses.team_member",
            subject_id,
            change,
        };
        audit(&mut *conn, &entry)
            .await
            .map_err(|_| EmailCheckError::Database)?;
    }
    Ok(())
}

async fn log(
    conn: &mut PgConnection,
    business: Uuid,
    who: (Uuid, Uuid),
    action: &'static str,
) -> Result<(), EmailCheckError> {
    log_notice_change(conn, business, who, (action, "mailbox_id")).await
}

/// SEC-024, SEC-038: whether `by` may change `member`'s notice address now: the owner, or the
/// member for their own address, with a Verify code no older than [`STEP_UP_LIFE`] and not from
/// the future.
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
    if fresh.is_none_or(|at| at > now || now.duration_since(at) > STEP_UP_LIFE) {
        return Err(EmailCheckError::NoFreshCode);
    }
    Ok(())
}

/// Closes every other open change for `member`, so only the newest request's code works (D-1850).
async fn cancel_older(
    tx: &mut PgConnection,
    (business, member, mailbox): (Uuid, Uuid, Uuid),
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "UPDATE businesses.mailbox_check SET open = false \
         WHERE business_id = $1 AND change_for = $2 AND open AND mailbox_id <> $3",
        business,
        member,
        mailbox
    );
    db(q.execute(tx).await).map(|_| ())
}

/// SEC-024, SEC-038, BIZ-040: asks to move `member`'s notice address to `mailbox` and returns the
/// code to email to the new address; the old one keeps every notice until [`confirm`], and any
/// older pending change for the member stops working. `mailbox` equal to the member's own
/// re-verifies it (a returned address is fixed this way); `None` when it is verified and not
/// returned, which logs nothing. A refusal is audit-logged as `businesses.notice_change_refused`,
/// a request as `businesses.notice_change_requested`, each with the acting member (D-1850).
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
    let mut tx = db(conn.begin().await)?;
    let who = (by.member, member);
    if let Err(e) = may_change(&mut tx, by, member, now).await {
        log(
            &mut tx,
            by.business,
            who,
            "businesses.notice_change_refused",
        )
        .await?;
        db(tx.commit().await)?;
        return Err(e);
    }
    let q = sqlx::query_scalar!(
        "SELECT mailbox_id FROM businesses.team_member \
         WHERE id = $1 AND business_id = $2 AND removed_at IS NULL FOR UPDATE",
        member,
        by.business
    );
    let old = db(q.fetch_optional(&mut *tx).await)?.ok_or(EmailCheckError::UnknownMember)?;
    cancel_older(&mut tx, (by.business, member, mailbox)).await?;
    let o = Opening {
        business: by.business,
        mailbox,
        change_for: Some(member),
        replaces: (old != mailbox).then_some(old),
        requested_by: Some(by.member),
    };
    let code = open_check(&mut tx, secret, o, now).await?;
    if code.is_some() {
        log(
            &mut tx,
            by.business,
            who,
            "businesses.notice_change_requested",
        )
        .await?;
    }
    db(tx.commit().await)?;
    Ok(code)
}

/// BIZ-040, SEC-024: takes the code emailed to `mailbox`. Right, within [`CODE_LIFE`] and
/// [`CHECK_TRIES`], for an open check: the mailbox is verified and any returned mark cleared; for
/// a change the member's address moves (only while the member is current and still has the old
/// mailbox, else the check closes unverified and this returns false), the change is audit-logged
/// and one [`CHANGED_KIND`] task tells the old address, in one transaction; for the sign-up email
/// the sign-up settles (activation). Returns whether the code was taken.
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
    let q = sqlx::query_as!(
        Pending,
        "SELECT id, member_id, change_for, replaces, requested_by, sent_at, tries \
         FROM businesses.mailbox_check \
         WHERE business_id = $1 AND mailbox_id = $2 AND open FOR UPDATE",
        business,
        mailbox
    );
    let Some(row) = db(q.fetch_optional(&mut *tx).await)? else {
        return Ok(false);
    };
    let sent = row.sent_at.to_jiff();
    let live = row.tries < CHECK_TRIES && now.duration_since(sent) <= CODE_LIFE;
    let want = code_of(secret, row.id, sent);
    if !(live && bool::from(want.as_bytes().ct_eq(code.as_bytes()))) {
        let q = sqlx::query!(
            "UPDATE businesses.mailbox_check SET tries = tries + 1 WHERE id = $1",
            row.id
        );
        db(q.execute(&mut *tx).await)?;
        db(tx.commit().await)?;
        return Ok(false);
    }
    let taken = match row.change_for {
        Some(member) => moved(&mut tx, (business, member, mailbox), &row, now).await?,
        None => true,
    };
    close(&mut tx, row.id, taken, now).await?;
    db(tx.commit().await)?;
    if taken && row.member_id.is_none() && row.change_for.is_none() {
        crate::identity::settle(conn, business, now)
            .await
            .map_err(|_| EmailCheckError::Database)?;
    }
    Ok(taken)
}

/// The open check `confirm` reads, locked.
struct Pending {
    id: Uuid,
    member_id: Option<Uuid>,
    change_for: Option<Uuid>,
    replaces: Option<Uuid>,
    requested_by: Option<Uuid>,
    sent_at: jiff_sqlx::Timestamp,
    tries: i32,
}

/// Closes a check; `verified` sets it verified and clears any returned mark (BIZ-040 "fix").
async fn close(
    tx: &mut PgConnection,
    id: Uuid,
    verified: bool,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query!(
        "UPDATE businesses.mailbox_check SET open = false, \
         verified_at = CASE WHEN $2 THEN $3::timestamptz ELSE verified_at END, \
         returned_at = CASE WHEN $2 THEN NULL ELSE returned_at END, \
         soft_at = CASE WHEN $2 THEN NULL ELSE soft_at END WHERE id = $1",
        id,
        verified,
        now.to_sqlx() as _
    );
    db(q.execute(tx).await).map(|_| ())
}

/// SEC-024: moves the member's address (or, for a re-verify, checks it is still theirs) only while
/// the member is current; then queues the old address's notice and writes the audit rows. Returns
/// false, writing nothing, when no member row matched.
async fn moved(
    tx: &mut PgConnection,
    (business, member, new): (Uuid, Uuid, Uuid),
    row: &Pending,
    now: Timestamp,
) -> Result<bool, EmailCheckError> {
    let old = row.replaces.unwrap_or(new);
    let q = sqlx::query!(
        "UPDATE businesses.team_member SET mailbox_id = $4 \
         WHERE id = $2 AND business_id = $1 AND mailbox_id = $3 AND removed_at IS NULL",
        business,
        member,
        old,
        new
    );
    if db(q.execute(&mut *tx).await)?.rows_affected() == 0 {
        return Ok(false);
    }
    let who = (row.requested_by.unwrap_or(member), member);
    if row.replaces.is_none() {
        log(tx, business, who, "businesses.notice_address_reverified").await?;
        return Ok(true);
    }
    let payload = json!({"business": business, "member": member, "old": old});
    let mut task = NewTask::new(CHANGED_KIND, payload, now);
    task.dedupe_key = Some(format!("{CHANGED_KIND}.{old}.{new}.{}", now.as_second()));
    enqueue(&mut *tx, &task)
        .await
        .map_err(|_| EmailCheckError::Database)?;
    log(tx, business, who, "businesses.notice_address_changed").await?;
    Ok(true)
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
        "{} emails to {address} are being returned or marked as spam, so you may miss job \
         requests. Add or fix an email address.",
        brand().display_name()
    )
}
