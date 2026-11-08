//! Each person's backup notice address (BIZ-043, SEC-024, SEC-038; T149, D-329, D-492).
//!
//! A person (the owner or a team member) may add one backup address. Only that person or the
//! owner may add, change or remove it, and only within [`STEP_UP_LIFE`] of passing a Verify code;
//! a refusal, a request and every change are audit-logged on the person with the acting member.
//! The backup gets nothing until the 6-digit code emailed to it comes back ([`confirm_backup`]);
//! then [`notice_addresses`] adds it after its person's main address for every purpose that person
//! gets but the account notices, so the release reaches it only for the owner or a "Confirm and
//! billing" member (`Roles::fits`). A change or removal emails the person's main address
//! ([`BACKUP_CHANGED_KIND`], [`backup_changed_words`]). A removed member's backup gets nothing more. No address value
//! is stored here (PRIV-001); the code is keyed from the row and send time, never stored.

use askhava_adapters::api::IdempotencyKey;
use askhava_core::api::{NewTask, brand, enqueue};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};
use subtle::ConstantTimeEq;

use crate::email_verification::{
    CHECK_TRIES, CODE_LIFE, EmailCheckError, STEP_UP_LIFE, log_notice_change,
};
use crate::team::{Acting, NoticePurpose, TeamError, members};

/// The task whose sender emails [`backup_changed_words`] to a person's main address (SEC-024).
pub const BACKUP_CHANGED_KIND: &str = "businesses.backup_changed";

/// SEC-024, D-1987: the subject and body telling the main address its backup changed, in English
/// or the Spanish draft.
#[must_use]
pub fn backup_changed_words(spanish: bool) -> [String; 2] {
    if spanish {
        let b = brand().es().short_name();
        return [
            format!("{b}: Cambió su dirección de respaldo"),
            "Alguien agregó, cambió o quitó la dirección de respaldo para nuestros correos. Si no \
             fue usted, responda a este correo. Su respuesta nos llega directamente."
                .to_owned(),
        ];
    }
    [
        format!("{}: Your backup address changed", brand().short_name()),
        "Someone added, changed or removed the backup address for your emails from us. If it \
         wasn't you, reply to this email. Your reply comes straight to us."
            .to_owned(),
    ]
}
const CODE_ACTION: &str = "businesses.backup_code";
const FIELD: &str = "backup_mailbox_id";

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, EmailCheckError> {
    r.map_err(|_| EmailCheckError::Database)
}

fn code_of(secret: &SecretString, id: Uuid, sent: Timestamp) -> String {
    let parts = [id.to_string(), sent.as_second().to_string()];
    let key = IdempotencyKey::derive(secret, CODE_ACTION, &[&parts[0], &parts[1]]);
    let n = u64::from_str_radix(&key.as_str()[..12], 16).unwrap_or_default();
    format!("{:06}", n % 1_000_000)
}

/// BIZ-043, SEC-024, SEC-038: `by` may change `member`'s backup only as the owner or as that
/// member, with a Verify code no older than [`STEP_UP_LIFE`] and not from the future.
async fn allowed(
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
    let at = me.step_up_at.map(|t| t.to_jiff());
    if at.is_none_or(|at| at > now || now.duration_since(at) > STEP_UP_LIFE) {
        return Err(EmailCheckError::NoFreshCode);
    }
    Ok(())
}

/// Runs the SEC-038 check and logs a refusal on `member` in its own committed transaction.
async fn checked(
    conn: &mut PgConnection,
    by: Acting,
    member: Uuid,
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let mut tx = db(conn.begin().await)?;
    let r = allowed(&mut tx, by, member, now).await;
    if r.is_err() {
        let who = (by.member, member);
        let action = ("businesses.backup_change_refused", FIELD);
        log_notice_change(&mut tx, by.business, who, action).await?;
    }
    db(tx.commit().await)?;
    r
}

/// SEC-024: queues the [`BACKUP_CHANGED_KIND`] email to `member`'s main address.
async fn tell_main(
    tx: &mut PgConnection,
    (business, member, row): (Uuid, Uuid, Uuid),
    now: Timestamp,
) -> Result<(), EmailCheckError> {
    let q = sqlx::query_scalar!(
        "SELECT mailbox_id FROM businesses.team_member WHERE id = $1 AND business_id = $2",
        member,
        business
    );
    let main = db(q.fetch_one(&mut *tx).await)?;
    let payload = json!({"business": business, "member": member, "to": [main]});
    let mut task = NewTask::new(BACKUP_CHANGED_KIND, payload, now);
    task.dedupe_key = Some(format!("{BACKUP_CHANGED_KIND}.{row}.{}", now.as_second()));
    enqueue(&mut *tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| EmailCheckError::Database)
}

/// BIZ-043: whether `mailbox` is the main address of a current member of `business` (the person's
/// own or a teammate's); a backup there would list one mailbox twice for a notice.
async fn is_main_address(
    tx: &mut PgConnection,
    business: Uuid,
    mailbox: Uuid,
) -> Result<bool, EmailCheckError> {
    let q = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM businesses.team_member WHERE business_id = $1 \
         AND mailbox_id = $2 AND removed_at IS NULL) AS \"taken!\"",
        business,
        mailbox
    );
    db(q.fetch_one(&mut *tx).await)
}

/// BIZ-043, SEC-038: asks to make `mailbox` the backup of `member` and returns the code to email
/// to it; any older unverified request closes, and a verified backup keeps working until the new
/// one is confirmed. Refused unless `by` is the owner or `member` with a fresh Verify code, and
/// refused as [`EmailCheckError::NotAllowed`] when `mailbox` is the main address of any current
/// member of the business, the person's own included.
///
/// # Errors
/// [`EmailCheckError::NotAllowed`], [`EmailCheckError::NoFreshCode`],
/// [`EmailCheckError::UnknownMember`] or `Database`.
pub async fn request_backup(
    conn: &mut PgConnection,
    secret: &SecretString,
    by: Acting,
    (member, mailbox): (Uuid, Uuid),
    now: Timestamp,
) -> Result<String, EmailCheckError> {
    checked(conn, by, member, now).await?;
    let mut tx = db(conn.begin().await)?;
    if is_main_address(&mut tx, by.business, mailbox).await? {
        return Err(EmailCheckError::NotAllowed);
    }
    let q = sqlx::query!(
        "UPDATE businesses.backup_address SET removed_at = $3 \
         WHERE business_id = $1 AND member_id = $2 AND verified_at IS NULL AND removed_at IS NULL",
        by.business,
        member,
        now.to_sqlx() as _
    );
    db(q.execute(&mut *tx).await)?;
    let q = sqlx::query_scalar!(
        "INSERT INTO businesses.backup_address \
         (business_id, member_id, mailbox_id, requested_by, sent_at) \
         SELECT $1, id, $3, $4, $5 FROM businesses.team_member \
         WHERE id = $2 AND business_id = $1 AND removed_at IS NULL RETURNING id",
        by.business,
        member,
        mailbox,
        by.member,
        now.to_sqlx() as _
    );
    let id = db(q.fetch_optional(&mut *tx).await)?.ok_or(EmailCheckError::UnknownMember)?;
    let action = ("businesses.backup_change_requested", FIELD);
    log_notice_change(&mut tx, by.business, (by.member, member), action).await?;
    db(tx.commit().await)?;
    Ok(code_of(secret, id, now))
}

/// BIZ-043, BIZ-040: takes the code emailed to `member`'s newest backup request. Right, within
/// [`CODE_LIFE`] and [`CHECK_TRIES`], while the member is current: the backup is verified, an
/// earlier verified one is removed, the change is audit-logged and the main address told, in one
/// transaction. Returns whether the code was taken.
///
/// # Errors
/// [`EmailCheckError::Database`].
pub async fn confirm_backup(
    conn: &mut PgConnection,
    secret: &SecretString,
    (business, member): (Uuid, Uuid),
    code: &str,
    now: Timestamp,
) -> Result<bool, EmailCheckError> {
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query!(
        "SELECT b.id, b.requested_by, b.sent_at, b.tries FROM businesses.backup_address b \
         JOIN businesses.team_member m ON m.id = b.member_id AND m.removed_at IS NULL \
         WHERE b.business_id = $1 AND b.member_id = $2 AND b.verified_at IS NULL \
         AND b.removed_at IS NULL FOR UPDATE OF b",
        business,
        member
    );
    let Some(row) = db(q.fetch_optional(&mut *tx).await)? else {
        return Ok(false);
    };
    let sent = row.sent_at.to_jiff();
    let live = row.tries < CHECK_TRIES && now.duration_since(sent) <= CODE_LIFE;
    let want = code_of(secret, row.id, sent);
    if !(live && bool::from(want.as_bytes().ct_eq(code.as_bytes()))) {
        let q = sqlx::query!(
            "UPDATE businesses.backup_address SET tries = tries + 1 WHERE id = $1",
            row.id
        );
        db(q.execute(&mut *tx).await)?;
        db(tx.commit().await)?;
        return Ok(false);
    }
    let q = sqlx::query!(
        "UPDATE businesses.backup_address SET removed_at = $3 WHERE business_id = $1 \
         AND member_id = $2 AND verified_at IS NOT NULL AND removed_at IS NULL",
        business,
        member,
        now.to_sqlx() as _
    );
    db(q.execute(&mut *tx).await)?;
    let q = sqlx::query!(
        "UPDATE businesses.backup_address SET verified_at = $2 WHERE id = $1",
        row.id,
        now.to_sqlx() as _
    );
    db(q.execute(&mut *tx).await)?;
    let action = ("businesses.backup_changed", FIELD);
    log_notice_change(&mut tx, business, (row.requested_by, member), action).await?;
    tell_main(&mut tx, (business, member, row.id), now).await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// BIZ-043, SEC-024, SEC-038: removes `member`'s backup (verified or pending); the main address is
/// told when a verified one went. Returns whether a verified backup was removed.
///
/// # Errors
/// As [`request_backup`].
pub async fn remove_backup(
    conn: &mut PgConnection,
    by: Acting,
    member: Uuid,
    now: Timestamp,
) -> Result<bool, EmailCheckError> {
    checked(conn, by, member, now).await?;
    let mut tx = db(conn.begin().await)?;
    let q = sqlx::query_scalar!(
        "UPDATE businesses.backup_address SET removed_at = $3 WHERE business_id = $1 \
         AND member_id = $2 AND removed_at IS NULL RETURNING verified_at IS NOT NULL AS \"v!\"",
        by.business,
        member,
        now.to_sqlx() as _
    );
    let removed = db(q.fetch_all(&mut *tx).await)?.into_iter().any(|v| v);
    if removed {
        let action = ("businesses.backup_removed", FIELD);
        log_notice_change(&mut tx, by.business, (by.member, member), action).await?;
        tell_main(&mut tx, (by.business, member, member), now).await?;
    }
    db(tx.commit().await)?;
    Ok(removed)
}

/// The opaque email IDs a notice of `purpose` goes to (D-492): the owner plus every current
/// member whose roles fit it, the owner first, each followed by that person's verified backup
/// (BIZ-043) except for account notices. The release (`PicksAndRelease`) therefore reaches a
/// backup only for the owner or a "Confirm and billing" member (D-329). `quotes`, `picks` and
/// `businesses` call it before enqueuing a notice.
///
/// # Errors
///
/// [`TeamError::Database`].
pub async fn notice_addresses(
    conn: &mut PgConnection,
    business: Uuid,
    purpose: NoticePurpose,
) -> Result<Vec<Uuid>, TeamError> {
    let main = crate::team::notice_addresses(conn, business, purpose).await?;
    let people = members(conn, business).await?;
    let ids: Vec<Uuid> = people.iter().map(|m| m.id).collect();
    let q = sqlx::query!(
        "SELECT member_id, mailbox_id FROM businesses.backup_address WHERE business_id = $1 \
         AND member_id = ANY($2) AND verified_at IS NOT NULL AND removed_at IS NULL",
        business,
        &ids
    );
    let backups = q.fetch_all(conn).await.map_err(|_| TeamError::Database)?;
    let with_backup = purpose != NoticePurpose::Account;
    let backup_of = |mailbox: Uuid| {
        let member = people.iter().find(|m| m.mailbox == mailbox).map(|m| m.id);
        let mine = backups.iter().filter(move |b| Some(b.member_id) == member);
        mine.filter(move |_| with_backup).map(|b| b.mailbox_id)
    };
    Ok(main
        .iter()
        .flat_map(|&m| std::iter::once(m).chain(backup_of(m)))
        .collect())
}
