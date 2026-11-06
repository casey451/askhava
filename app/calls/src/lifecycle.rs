//! When a masked line closes, who may use a staff phone on it, the LINK request after a blocked
//! call and the Ask Hava number's greeting (CALL-009 to CALL-012, CALL-018, CALL-020; D-078,
//! D-191, D-227, D-260, D-288, D-311, D-1694 to D-1697).
//!
//! [`close_line`] closes a job's open line when the job is cancelled, the business no-shows or a
//! complaint is filed; [`close_expired`] closes every line 30 days ([`LINE_DAYS`]) after it opened
//! (CALL-009). A cancel tells the business by one business notice and counts no no-show strike
//! (CALL-010): calls records no strike for any reason, and only [`CloseReason::Cancelled`] queues
//! a notice. Each close writes one audit row naming its reason, in the same transaction, and a
//! second close finds nothing open and does nothing (JOB-006).
//!
//! [`phone_works`] is the BIZ-033 gate: a phone a member other than the owner linked works only
//! once the owner approved it; [`gate`] turns such a caller into [`Party::Unlinked`], so it hears
//! the PIN prompt like any unlinked phone (CALL-012). The approval itself is recorded by T148's
//! YES and Approve paths (D-492); the caller passes it in. [`request_link`] queues, after a
//! blocked call from an unlinked phone, one "Link this phone" notice to the owner and every
//! "Confirm and billing" member, by the business's chosen channels, naming only the last 4 digits
//! of the blocked number and the line's PIN for "Reply LINK <PIN>" (CALL-011, D-078).
//!
//! The pool never looks at how the homeowner chose to be reached, so an email-preferred homeowner
//! gets a line like anyone (CALL-018). [`greeting_twiml`] is all a call to the Ask Hava number
//! hears: the greeting in English then Spanish, then a hang-up, with no voicemail, no recording
//! and no forward (CALL-020).

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::CallsError;
use crate::routing::{Party, job_pin};

/// CALL-009: a line closes this many days after it opened.
pub const LINE_DAYS: i64 = 30;
/// The `core.task` kind through which a business notice is asked of `messaging` (D-491).
pub const BUSINESS_NOTICE_KIND: &str = "messaging.business_notice";
/// CALL-010: the template telling the business a cancel closed the line, with no strike.
pub const CANCELLED_TEMPLATE: &str = "line_closed_cancelled";
/// CALL-011, D-078: the "Link this phone" and "Reply LINK <PIN>" notice.
pub const LINK_TEMPLATE: &str = "link_this_phone";
/// CALL-020, D-227: the English greeting of the Ask Hava number.
pub const GREETING_EN: &str = "This is Ask Hava LLC. We work by text. To start a request, text \
     this number. For help, text HELP. If this is an emergency, hang up and call 911.";
/// CALL-020, D-1697: the Spanish greeting, said after the English one.
pub const GREETING_ES: &str = "Habla Ask Hava LLC. Trabajamos por mensaje de texto. Para hacer \
     una solicitud, mande un texto a este número. Para ayuda, mande AYUDA. Si es una emergencia, \
     cuelgue y llame al 911.";
const LINE_KIND: &str = "calls.number_pool";

/// Why a line closes (CALL-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// The job was cancelled after connection (CALL-010).
    Cancelled,
    /// The business did not show.
    NoShow,
    /// A complaint was filed on the job.
    Complaint,
    /// [`LINE_DAYS`] passed since the line opened.
    Expired,
}

impl CloseReason {
    /// The audit action naming this close.
    #[must_use]
    pub fn action(self) -> &'static str {
        match self {
            Self::Cancelled => "calls.line_closed_cancelled",
            Self::NoShow => "calls.line_closed_no_show",
            Self::Complaint => "calls.line_closed_complaint",
            Self::Expired => "calls.line_closed_expired",
        }
    }

    /// CALL-010: only a cancel tells the business (a no-show and a complaint have their own
    /// notices elsewhere; an expiry needs none).
    #[must_use]
    pub fn tells_business(self) -> bool {
        self == Self::Cancelled
    }
}

/// CALL-009: whether a line opened at `assigned_at` has reached its [`LINE_DAYS`] at `now`.
#[must_use]
pub fn expired(assigned_at: Timestamp, now: Timestamp) -> bool {
    now.duration_since(assigned_at) >= SignedDuration::from_hours(LINE_DAYS * 24)
}

/// Writes the close's audit row.
async fn audit_close(
    tx: &mut PgConnection,
    line: Uuid,
    reason: CloseReason,
) -> Result<(), CallsError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: reason.action(),
        subject_kind: LINE_KIND,
        subject_id: line,
        change: Change::Action,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| CallsError::Ops)
}

/// Queues one business notice of `template` with `values` about `line`, once per `once` (a
/// dedupe key holding no personal data), to the owner and every "Confirm and billing" member
/// (BIZ-019, D-492).
async fn notify(
    tx: &mut PgConnection,
    (business, line): (Uuid, Uuid),
    (template, values): (&'static str, serde_json::Value),
    once: String,
    now: Timestamp,
) -> Result<(), CallsError> {
    let to = notice_addresses(tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| CallsError::Database)?;
    let payload = json!({
        "template": template, "business": business, "line": line, "to": to, "values": values,
    });
    let mut task = NewTask::new(BUSINESS_NOTICE_KIND, payload, now);
    task.dedupe_key = Some(once);
    enqueue(tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| CallsError::Database)
}

/// Closes the open line of job `job` (its number `number` names it in the notice) for `reason` at
/// `now`, with its audit row and, for a cancel, the business's notice, in one transaction; no
/// strike is recorded (CALL-009, CALL-010, JOB-005). Returns the closed line, or `None` when the
/// job has no open line (already closed, or never connected), so it is safe to call twice.
///
/// # Errors
///
/// [`CallsError::Database`] or [`CallsError::Ops`].
pub async fn close_line(
    db: &mut PgConnection,
    (job, number): (Uuid, i64),
    reason: CloseReason,
    now: Timestamp,
) -> Result<Option<Uuid>, CallsError> {
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    let row = sqlx::query!(
        "UPDATE calls.number_pool SET closed_at = $2 WHERE job_id = $1 AND closed_at IS NULL \
         RETURNING id, business_id",
        job,
        now.to_sqlx() as _
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    audit_close(&mut tx, row.id, reason).await?;
    if reason.tells_business() {
        let notice = (CANCELLED_TEMPLATE, json!({ "job": number }));
        let once = format!("{BUSINESS_NOTICE_KIND}.{CANCELLED_TEMPLATE}.{}", row.id);
        notify(&mut tx, (row.business_id, row.id), notice, once, now).await?;
    }
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(Some(row.id))
}

/// Closes every open line that reached its [`LINE_DAYS`] at `now`, each with its audit row, in one
/// transaction; returns how many closed (CALL-009). Safe to run twice.
///
/// # Errors
///
/// [`CallsError::Database`] or [`CallsError::Ops`].
pub async fn close_expired(db: &mut PgConnection, now: Timestamp) -> Result<usize, CallsError> {
    let cutoff = now
        .checked_sub(SignedDuration::from_hours(LINE_DAYS * 24))
        .map_err(|_| CallsError::Database)?;
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    let lines = sqlx::query_scalar!(
        "UPDATE calls.number_pool SET closed_at = $2 \
         WHERE closed_at IS NULL AND assigned_at <= $1 RETURNING id",
        cutoff.to_sqlx() as _,
        now.to_sqlx() as _
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    for line in &lines {
        audit_close(&mut tx, *line, CloseReason::Expired).await?;
    }
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(lines.len())
}

/// How a linked phone stands (CALL-012, BIZ-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhoneStanding {
    /// The owner linked it (the main phone always is).
    pub linked_by_owner: bool,
    /// The owner approved it, by Approve from the email or YES by text (T148).
    pub owner_approved: bool,
}

/// CALL-012, BIZ-033, D-288, D-311: a phone works once the owner linked or approved it.
#[must_use]
pub fn phone_works(standing: PhoneStanding) -> bool {
    standing.linked_by_owner || standing.owner_approved
}

/// CALL-012: the caller as routing must see it: a business phone that does not work yet is
/// [`Party::Unlinked`], so it hears the PIN prompt and never connects as the business.
#[must_use]
pub fn gate(party: Party, standing: PhoneStanding) -> Party {
    match party {
        Party::Business(_) if !phone_works(standing) => Party::Unlinked,
        other => other,
    }
}

/// The last 4 digits of `phone`, or `None` when it has fewer than 4 (CALL-011, D-078).
#[must_use]
pub fn last4(phone: &SecretString) -> Option<String> {
    let digits: Vec<char> = phone
        .expose_secret()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let start = digits.len().checked_sub(4)?;
    digits.get(start..).map(|d| d.iter().collect())
}

/// Queues the "Link this phone" notice for a blocked call from the unlinked phone `blocked` on
/// line `line` at `now`: to the owner and every "Confirm and billing" member, naming the last 4
/// digits and the line's PIN for "Reply LINK <PIN>", once per line and last 4 digits (CALL-011, D-078, D-311).
/// The full number is never stored or sent. Returns whether a notice was queued: `false` when the
/// line is not open or the number has fewer than 4 digits.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn request_link(
    db: &mut PgConnection,
    line: Uuid,
    blocked: &SecretString,
    now: Timestamp,
) -> Result<bool, CallsError> {
    let Some(last4) = last4(blocked) else {
        return Ok(false);
    };
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    let business = sqlx::query_scalar!(
        "SELECT business_id FROM calls.number_pool WHERE id = $1 AND closed_at IS NULL",
        line
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    let Some(business) = business else {
        return Ok(false);
    };
    let once = format!("{BUSINESS_NOTICE_KIND}.{LINK_TEMPLATE}.{line}.{last4}");
    let notice = (
        LINK_TEMPLATE,
        json!({ "number": last4, "job": job_pin(line).digits() }),
    );
    notify(&mut tx, (business, line), notice, once, now).await?;
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(true)
}

/// CALL-020, D-227: the whole answer to a call to the Ask Hava number: the greeting in English,
/// then Spanish, then a hang-up. No `Record`, `Dial` or voicemail verb, so nothing is recorded and
/// no call is forwarded.
#[must_use]
pub fn greeting_twiml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response>\
         <Say language=\"en-US\">{GREETING_EN}</Say><Say language=\"es-MX\">{GREETING_ES}</Say>\
         <Hangup/></Response>"
    )
}
