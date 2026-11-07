//! Complaints about a business and the owner's phone-link email (BIZ-011, BIZ-012, BIZ-029,
//! BIZ-033; D-016, D-200, D-222, D-258, D-280, D-311).
//!
//! [`record_complaint`] stores a complaint with its kind and a safety mark, never the homeowner's
//! words or contact (PRIV-001); a safety word raises [`SAFETY_ALERT`] at once (OPS-005), the
//! [`BOX_CLOSED_KIND`] task is due when the 5-day reply box closes, and the third complaint within
//! 90 days pauses the business through `service::pause_for_rule` (BIZ-012). The homeowner's reply
//! is [`ReplyFacts::text`] (BIZ-011). The business reads its
//! complaints with [`complaints`] (kind and dates, no homeowner detail) and answers once with
//! [`reply`] before the box closes (BIZ-029); [`run_box_closed`] then lists the complaint for
//! Casey's digest as one [`DIGEST_ALERT`] row and [`for_casey`] shows it with the reply.
//! [`phone_link_notice`] queues one [`PHONE_LINKED_KIND`] owner email when a member other than the
//! owner links a phone (BIZ-033); the Approve gate and the text are T148's (D-493).

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::BusinessesError;
use crate::service::pause_for_rule;
use crate::status::PauseReason;
use crate::team::Acting;

/// BIZ-029: the reply box stays open this long after the complaint.
pub const REPLY_BOX: SignedDuration = SignedDuration::from_hours(5 * 24);
/// BIZ-029: the longest reply a business may write, in characters.
pub const REPLY_MAX: usize = 1000;
/// BIZ-012, D-016: this many complaints within [`PAUSE_WINDOW_DAYS`] pause the business.
pub const PAUSE_COUNT: i64 = 3;
/// BIZ-012: the pause window in days; a complaint exactly this many days old still counts.
pub const PAUSE_WINDOW_DAYS: i32 = 90;
/// The `core.task` kind due when a complaint's reply box closes; `server` registers
/// [`run_box_closed`] for it.
pub const BOX_CLOSED_KIND: &str = "businesses.complaint_box_closed";
/// OPS-005, BIZ-012: the `ops.alert` kind listing a closed complaint in the weekly digest.
pub const DIGEST_ALERT: &str = "businesses.complaint";
/// OPS-005, D-222: the `ops.alert` kind of a complaint holding a safety word (the OPS-032 push).
pub const SAFETY_ALERT: &str = "businesses.complaint_safety";
/// BIZ-033: the `core.task` kind that emails the owner after another member links a phone.
pub const PHONE_LINKED_KIND: &str = "businesses.phone_linked";

const COMPLAINT: &str = "businesses.complaint";
const PHONE: &str = "businesses.staff_phone";

/// OPS-005, D-222: the safety words, English and Spanish, matched as whole words in any case.
const SAFETY_WORDS: [&str; 24] = [
    "unsafe",
    "threat",
    "threats",
    "threatened",
    "stole",
    "stolen",
    "hurt",
    "injured",
    "injury",
    "police",
    "fire",
    "inseguro",
    "insegura",
    "peligroso",
    "amenaza",
    "amenazó",
    "robó",
    "robaron",
    "herido",
    "herida",
    "lastimado",
    "lastimada",
    "policía",
    "incendio",
];

/// Why a complaint call failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum ComplaintError {
    /// A database call failed.
    #[error("complaint database call failed")]
    Database,
    /// The audit row or the alert could not be written (JOB-005).
    #[error("complaint audit row failed")]
    Audit,
    /// The kind is not a code name of at most 32 lower-case letters and underscores.
    #[error("complaint kind is not a code name")]
    BadKind,
    /// The reply is empty or longer than [`REPLY_MAX`].
    #[error("complaint reply is empty or too long")]
    BadReply,
    /// The reply box is closed, already used, or the complaint is not this business's.
    #[error("complaint reply box is closed")]
    Closed,
    /// The reply box is still open, so the complaint is not listed yet.
    #[error("complaint reply box is still open")]
    BoxOpen,
    /// No complaint has the given ID.
    #[error("complaint not found")]
    Unknown,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, ComplaintError> {
    r.map_err(|_| ComplaintError::Database)
}

/// OPS-005, D-222: whether `words` hold a safety word as a whole word, in any case.
#[must_use]
pub fn has_safety_word(words: &str) -> bool {
    words
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| SAFETY_WORDS.contains(&w))
}

/// BIZ-011: what the homeowner's reply names: the business, its own phone and the job.
#[derive(Debug, Clone, Copy)]
pub struct ReplyFacts<'a> {
    /// The business's public name.
    pub business: &'a str,
    /// The business's own phone, as shown to homeowners.
    pub number: &'a str,
    /// The job number.
    pub job: &'a str,
    /// The job's trade, as the card names it.
    pub trade: &'a str,
}

impl ReplyFacts<'_> {
    /// BIZ-011, TXT-007, D-1720: the homeowner's reply: the apology, the not-a-contractor line, the
    /// business's own phone, the job and the ROC link through askhava.com; Spanish when `spanish`
    /// (a draft for native review, TXT-003). At most 3 GSM-7 segments with usual values.
    #[must_use]
    pub fn text(&self, spanish: bool) -> String {
        let Self {
            business,
            number,
            job,
            trade,
        } = *self;
        if spanish {
            format!(
                "Ask Hava: Lamentamos el problema con {business} en la solicitud {job} ({trade}). Ask Hava no es contratista y no realiza, cotiza ni supervisa trabajos; {business} es un negocio independiente. Puede llamarlos al {number}. El dueño de Ask Hava lee cada queja. Si el trabajo necesitaba licencia de contratista, también puede quejarse ante el estado: askhava.com/help/roc-complaint"
            )
        } else {
            format!(
                "Ask Hava: Sorry about the trouble with {business} on request {job} ({trade}). Ask Hava is not a contractor and does not perform, bid on or supervise work; {business} is an independent business. You can call them at {number}. The owner of Ask Hava reads every complaint. If the job needed a state contractor license, you can also file with the state here: askhava.com/help/roc-complaint"
            )
        }
    }
}

/// One complaint as it arrives: the business, the job, its kind as a code name and the homeowner's
/// words, read only for the safety check and never stored.
#[derive(Debug, Clone, Copy)]
pub struct NewComplaint<'a> {
    /// The business complained about.
    pub business: Uuid,
    /// The job it is about.
    pub job: Uuid,
    /// The kind, such as `no_show` (lower-case letters and underscores, at most 32).
    pub kind: &'a str,
    /// The homeowner's words.
    pub words: &'a str,
}

/// What [`record_complaint`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    /// The complaint's ID.
    pub id: Uuid,
    /// Whether a safety word raised [`SAFETY_ALERT`].
    pub safety: bool,
    /// Whether this complaint paused the business (BIZ-012).
    pub paused: bool,
}

fn good_kind(kind: &str) -> bool {
    let first = kind.bytes().next().is_some_and(|b| b.is_ascii_lowercase());
    let rest = kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    first && rest && kind.len() <= 32
}

/// BIZ-011, BIZ-012, OPS-005, D-016, D-222, D-280: stores the complaint with its audit row,
/// raises [`SAFETY_ALERT`] at once for a safety word, queues [`BOX_CLOSED_KIND`] for when the
/// reply box closes and, when it is the third or later within 90 days, pauses the business for
/// complaints (a business not active or already paused by a rule stays as it is). One transaction.
///
/// # Errors
/// [`ComplaintError::BadKind`] before anything is written, `Database` or `Audit`.
pub async fn record_complaint(
    conn: &mut PgConnection,
    c: &NewComplaint<'_>,
    now: Timestamp,
) -> Result<Recorded, ComplaintError> {
    if !good_kind(c.kind) {
        return Err(ComplaintError::BadKind);
    }
    let safety = has_safety_word(c.words);
    let mut tx = db(conn.begin().await)?;
    let id = db(sqlx::query_scalar!(
        "INSERT INTO businesses.complaint (business_id, job_id, kind, safety, received_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        c.business,
        c.job,
        c.kind,
        safety,
        now.to_sqlx() as _
    )
    .fetch_one(&mut *tx)
    .await)?;
    if safety {
        alert(&mut tx, SAFETY_ALERT, id).await?;
    }
    let mut task = NewTask::new(BOX_CLOSED_KIND, json!({"complaint": id}), now + REPLY_BOX);
    task.dedupe_key = Some(format!("{BOX_CLOSED_KIND}.{id}"));
    db(enqueue(&mut tx, &task)
        .await
        .map_err(|_| sqlx::Error::PoolClosed))?;
    log(
        &mut tx,
        c.business,
        "businesses.complaint_recorded",
        (COMPLAINT, id),
    )
    .await?;
    let paused = pause_on_count(&mut tx, c.business, now).await?;
    db(tx.commit().await)?;
    Ok(Recorded { id, safety, paused })
}

/// BIZ-012, D-016: pauses `business` for complaints when it has [`PAUSE_COUNT`] or more within
/// [`PAUSE_WINDOW_DAYS`] up to `now`; true only when this call paused it.
async fn pause_on_count(
    tx: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<bool, ComplaintError> {
    let count = db(sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.complaint WHERE business_id = $1 \
         AND received_at <= $2 AND received_at >= $2 - make_interval(days => $3)",
        business,
        now.to_sqlx() as _,
        PAUSE_WINDOW_DAYS
    )
    .fetch_one(&mut *tx)
    .await)?;
    if count < PAUSE_COUNT {
        return Ok(false);
    }
    match pause_for_rule(tx, business, PauseReason::Complaints, Actor::System, now).await {
        Ok(_) => Ok(true),
        Err(BusinessesError::Refused) => Ok(false),
        Err(BusinessesError::Audit) => Err(ComplaintError::Audit),
        Err(_) => Err(ComplaintError::Database),
    }
}

async fn alert(tx: &mut PgConnection, kind: &'static str, id: Uuid) -> Result<(), ComplaintError> {
    let new = NewAlert {
        kind,
        severity: Severity::Normal,
        subject: Some((COMPLAINT, id)),
    };
    raise_alert(tx, &new)
        .await
        .map(|_| ())
        .map_err(|_| ComplaintError::Audit)
}

async fn log(
    tx: &mut PgConnection,
    business: Uuid,
    action: &'static str,
    (subject_kind, subject_id): (&'static str, Uuid),
) -> Result<(), ComplaintError> {
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action,
        subject_kind,
        subject_id,
        change: Change::Action,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| ComplaintError::Audit)
}

/// BIZ-029: one complaint as its business sees it: kind and dates, no homeowner detail, no job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Complaint {
    /// The complaint's ID.
    pub id: Uuid,
    /// Its kind as a code name.
    pub kind: String,
    /// When it came in.
    pub received_at: Timestamp,
    /// When the reply box closes (5 days after it came in).
    pub reply_until: Timestamp,
    /// The business's reply, if it wrote one.
    pub reply: Option<String>,
}

/// BIZ-029: the complaints about `business`, newest first.
///
/// # Errors
/// [`ComplaintError::Database`].
pub async fn complaints(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<Complaint>, ComplaintError> {
    let rows = db(sqlx::query!(
        "SELECT id, kind, received_at, reply FROM businesses.complaint \
         WHERE business_id = $1 ORDER BY received_at DESC, id",
        business
    )
    .fetch_all(conn)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| Complaint {
            id: r.id,
            kind: r.kind,
            received_at: r.received_at.to_jiff(),
            reply_until: r.received_at.to_jiff() + REPLY_BOX,
            reply: r.reply,
        })
        .collect())
}

/// BIZ-029, D-200: stores `business`'s one reply to `complaint`, accepted only before the box
/// closes 5 days after the complaint (at exactly 5 days it is refused), with its audit row.
///
/// # Errors
/// [`ComplaintError::BadReply`] for an empty or too long reply, `Closed`, `Database` or `Audit`.
pub async fn reply(
    conn: &mut PgConnection,
    (business, complaint): (Uuid, Uuid),
    words: &str,
    now: Timestamp,
) -> Result<(), ComplaintError> {
    let words = words.trim();
    if words.is_empty() || words.chars().count() > REPLY_MAX {
        return Err(ComplaintError::BadReply);
    }
    let mut tx = db(conn.begin().await)?;
    let done = db(sqlx::query!(
        "UPDATE businesses.complaint SET reply = $3, replied_at = $4 \
         WHERE id = $1 AND business_id = $2 AND reply IS NULL \
         AND $4 >= received_at AND $4 < received_at + interval '120 hours'",
        complaint,
        business,
        words,
        now.to_sqlx() as _
    )
    .execute(&mut *tx)
    .await)?;
    if done.rows_affected() == 0 {
        return Err(ComplaintError::Closed);
    }
    log(
        &mut tx,
        business,
        "businesses.complaint_replied",
        (COMPLAINT, complaint),
    )
    .await?;
    db(tx.commit().await)
}

/// BIZ-012, BIZ-029, OPS-005: a complaint as Casey reads it, with the business's reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseyView {
    /// The business complained about.
    pub business: Uuid,
    /// The job it is about.
    pub job: Uuid,
    /// Its kind as a code name.
    pub kind: String,
    /// Whether the homeowner's words held a safety word.
    pub safety: bool,
    /// When it came in.
    pub received_at: Timestamp,
    /// The business's reply, if any.
    pub reply: Option<String>,
    /// When it was listed for the digest, once the box closed.
    pub listed_at: Option<Timestamp>,
}

/// BIZ-029, OPS-005: complaint `id` with its reply, for the owner portal and the digest.
///
/// # Errors
/// [`ComplaintError::Unknown`] or `Database`.
pub async fn for_casey(conn: &mut PgConnection, id: Uuid) -> Result<CaseyView, ComplaintError> {
    let r = db(sqlx::query!(
        "SELECT business_id, job_id, kind, safety, received_at, reply, listed_at \
         FROM businesses.complaint WHERE id = $1",
        id
    )
    .fetch_optional(conn)
    .await)?
    .ok_or(ComplaintError::Unknown)?;
    Ok(CaseyView {
        business: r.business_id,
        job: r.job_id,
        kind: r.kind,
        safety: r.safety,
        received_at: r.received_at.to_jiff(),
        reply: r.reply,
        listed_at: r.listed_at.map(|t| t.to_jiff()),
    })
}

/// BIZ-012, OPS-005, D-222: once complaint `id`'s reply box has closed at `now`, marks it listed
/// and raises one [`DIGEST_ALERT`] row for the weekly digest, in one transaction; false when it
/// was listed before, so a repeat is safe (JOB-006).
///
/// # Errors
/// [`ComplaintError::BoxOpen`] before the box closes, `Unknown`, `Database` or `Audit`.
pub async fn list_closed(
    conn: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<bool, ComplaintError> {
    let mut tx = db(conn.begin().await)?;
    let row = db(sqlx::query!(
        "SELECT listed_at IS NOT NULL AS \"listed!\", \
         $2 >= received_at + interval '120 hours' AS \"closed!\" \
         FROM businesses.complaint WHERE id = $1 FOR UPDATE",
        id,
        now.to_sqlx() as _
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(ComplaintError::Unknown)?;
    if row.listed {
        return Ok(false);
    }
    if !row.closed {
        return Err(ComplaintError::BoxOpen);
    }
    let q = sqlx::query!(
        "UPDATE businesses.complaint SET listed_at = $2 WHERE id = $1",
        id,
        now.to_sqlx() as _
    );
    db(q.execute(&mut *tx).await)?;
    alert(&mut tx, DIGEST_ALERT, id).await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// The [`BOX_CLOSED_KIND`] handler: [`list_closed`] at the wall clock; a box still open fails the
/// attempt so it is retried, never dropped (JOB-006).
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload, an open box or a database failure.
pub async fn run_box_closed(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let id = task.payload["complaint"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok());
    let id = id.ok_or(HandlerError("complaint task payload is not a complaint ID"))?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| HandlerError("complaint database call failed"))?;
    match list_closed(&mut conn, id, Timestamp::now()).await {
        Ok(_) | Err(ComplaintError::Unknown) => Ok(()),
        Err(ComplaintError::BoxOpen) => Err(HandlerError("complaint reply box is still open")),
        Err(_) => Err(HandlerError("complaint listing failed")),
    }
}

/// BIZ-033, D-258, D-311: after `by` linked phone `line`, queues one [`PHONE_LINKED_KIND`] email
/// to the owner, with its audit row, when `by` is not the owner; true when queued. Call it in the
/// link's transaction (`api::link_phone` does). The Approve gate and the owner text are T148's.
///
/// # Errors
/// [`ComplaintError::Unknown`] for a member not of the business, `Database` or `Audit`.
pub async fn phone_link_notice(
    tx: &mut PgConnection,
    by: Acting,
    line: Uuid,
    now: Timestamp,
) -> Result<bool, ComplaintError> {
    let owner = db(sqlx::query_scalar!(
        "SELECT is_owner FROM businesses.team_member WHERE id = $1 AND business_id = $2",
        by.member,
        by.business
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(ComplaintError::Unknown)?;
    if owner {
        return Ok(false);
    }
    let payload = json!({"business": by.business, "line": line, "member": by.member});
    let mut task = NewTask::new(PHONE_LINKED_KIND, payload, now);
    task.dedupe_key = Some(format!("{PHONE_LINKED_KIND}.{line}"));
    db(enqueue(&mut *tx, &task)
        .await
        .map_err(|_| sqlx::Error::PoolClosed))?;
    log(
        tx,
        by.business,
        "businesses.phone_link_notice",
        (PHONE, line),
    )
    .await?;
    Ok(true)
}

/// BIZ-033, BIZ-018: the owner email's line after a member linked the phone ending `last4`, in
/// Spanish when `spanish`.
#[must_use]
pub fn phone_linked_line(last4: &str, spanish: bool) -> String {
    if spanish {
        format!(
            "Ask Hava: Un miembro de su equipo vinculó el número que termina en {last4} a su cuenta. Si no lo reconoce, entre en askhava.com/b y quítelo."
        )
    } else {
        format!(
            "Ask Hava: Someone on your team linked the phone number ending in {last4} to your account. If you don't recognize it, sign in at askhava.com/b and remove it."
        )
    }
}
