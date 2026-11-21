//! "That number is old" resumes when the record shows a new contact (T690, D-4361; BIZ-026,
//! BIZ-002, QH-001, LIC-008; D-178, D-254, D-1699, D-1712, D-2954).
//!
//! [`run_record_resume`] is the daily [`RESUME_KIND`] run chain, due [`RESUME_LAG`] after the ROC
//! list's run hour. Its payload carries the last ROC list it acted on and the last sign-up it
//! queued (IDs only). When the latest recorded ROC list is newer, it queues one
//! [`RESUME_ONE_KIND`] task per sign-up with `waiting_on_record_since` set, at most the batch
//! ([`RESUME_BATCH_KEY`], default [`RESUME_BATCH`]) per list, in business ID order from the one
//! after the last queued round to the start, so every waiting sign-up is reached in turn and those
//! not reached on one list come first on the next; the dedupe key `<kind>.<business>.<list date>`
//! gives one lookup per list. No column is added and no SQL here names `core` (TD-SEC-10; D-5217):
//! the start-up seed asks `core::api::due_or_running` whether a run is live and keys by due time.
//!
//! [`run_resume_one`] holds itself to 8 a.m. Arizona before any lookup outside 8 a.m. to 8 p.m.
//! Arizona; then it makes one `licensing::api::read_own_record` through
//! [`crate::signup_record::signup_record`] (a failed, misread or missing record does nothing and
//! waits for the next list) and, in one transaction, calls
//! [`crate::signup::resume_on_new_record`] and [`crate::identity::send_code_seen`] with the record
//! held in memory, so the code goes only to the new record contact (D-1712). A text step whose
//! record phone's area-code time is still outside 8 a.m. to 8 p.m. rolls back and re-queues under
//! `<kind>.held.<business>.<list date>.<until>` (QH-001); an email or letter step is not held.
//! Nothing from the record is stored or logged (PRIV-001, D-1699).

use askhava_adapters::api::{LicenseRecords, VerifyAdapter};
use askhava_core::api::{Claimed, HandlerError, NewTask, SettingsError, due_or_running, enqueue};
use askhava_licensing::api::{DailyListSettings, next_slot, roc_lists};
use askhava_messaging::api::{AreaCodes, SendWindow, window};
use jiff::tz::{TimeZone, offset};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::identity::{RecordContacts, Step, next_step, send_code_seen};
use crate::models::{self, BusinessesError, db};
use crate::signup::{SignupError, resume_on_new_record, waiting_on_record};
use crate::signup_record::{SignupRecord, signup_record};

/// The `core.task` kind of the daily resume run (BIZ-026).
pub const RESUME_KIND: &str = "businesses.record_resume";
/// The `core.task` kind of one sign-up's record lookup (BIZ-026); payload `{"business": <id>}`.
pub const RESUME_ONE_KIND: &str = "businesses.record_resume_one";
/// The setting for the sign-ups looked up per ROC list (BIZ-026).
pub const RESUME_BATCH_KEY: &str = "ASKHAVA_BIZ_RESUME_BATCH";
/// The default of [`RESUME_BATCH_KEY`].
pub const RESUME_BATCH: u32 = 25;
/// How long after the ROC list's run hour the resume run is due.
pub const RESUME_LAG: SignedDuration = SignedDuration::from_mins(30);
const DAY: SignedDuration = SignedDuration::from_hours(24);
const FAILED: HandlerError = HandlerError("the record resume database call failed");
const NOT_SENT: HandlerError = HandlerError("the record resume code was not sent");
const BAD_PAYLOAD: HandlerError = HandlerError("the record resume payload is not a business");

/// BIZ-026: reads [`RESUME_BATCH_KEY`]; absent or empty keeps [`RESUME_BATCH`].
///
/// # Errors
/// [`SettingsError::Invalid`] naming the key when the value is not a whole number from 1 to 1000.
pub fn resume_batch<I>(pairs: I) -> Result<u32, SettingsError>
where
    I: IntoIterator<Item = (String, String)>,
{
    let value = pairs.into_iter().find(|(k, _)| k == RESUME_BATCH_KEY);
    match value.map(|(_, v)| v.trim().to_owned()) {
        Some(v) if !v.is_empty() => v
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=1000).contains(n))
            .ok_or(SettingsError::Invalid(RESUME_BATCH_KEY)),
        _ => Ok(RESUME_BATCH),
    }
}

/// Queues the run due [`RESUME_LAG`] after the next daily list run hour, carrying `payload`, once
/// per day (dedupe key by the slot). Returns whether it was queued now.
async fn queue_run(
    conn: &mut PgConnection,
    now: Timestamp,
    lists: DailyListSettings,
    payload: serde_json::Value,
) -> Result<bool, BusinessesError> {
    let slot = next_slot(now, lists.run_hour, DAY).ok_or(BusinessesError::Database)?;
    let due = slot.checked_add(RESUME_LAG);
    let due = due.map_err(|_| BusinessesError::Database)?;
    let mut task = NewTask::new(RESUME_KIND, payload, due);
    task.dedupe_key = Some(format!("{RESUME_KIND}.{}", slot.as_second()));
    let queued = enqueue(conn, &task).await;
    Ok(queued.map_err(|_| BusinessesError::Database)?.is_some())
}

/// BIZ-026, JOB-006: seeds the run chain with no list acted on yet; a no-op while any run of the
/// chain is due or running (`core::api::due_or_running`), so a start-up never takes the key the
/// live chain's next run needs, even with an overdue run not yet claimed. Otherwise it queues the
/// run whose due time ([`RESUME_LAG`] after the slot) is next, so a seed between the list hour and
/// its run takes today's key, not tomorrow's. A chain lost to a dead letter is seeded again.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn schedule_record_resume(
    conn: &mut PgConnection,
    now: Timestamp,
    lists: DailyListSettings,
) -> Result<(), BusinessesError> {
    let prefix = format!("{RESUME_KIND}.");
    let live = due_or_running(conn, RESUME_KIND, &prefix).await;
    if live.map_err(|_| BusinessesError::Database)? {
        return Ok(());
    }
    let since = now.checked_sub(RESUME_LAG);
    let since = since.map_err(|_| BusinessesError::Database)?;
    queue_run(conn, since, lists, json!({})).await.map(|_| ())
}

/// The [`RESUME_KIND`] handler (BIZ-026, JOB-006): when the latest ROC list is not the one its
/// payload says it acted on, [`queue_resumes`]; otherwise it only queues the next run with the
/// same payload.
///
/// # Errors
/// A fixed [`HandlerError`]; a retry queues nothing twice.
pub async fn run_record_resume(
    pool: &PgPool,
    task: &Claimed,
    now: Timestamp,
    (lists, batch): (DailyListSettings, u32),
) -> Result<(), HandlerError> {
    let acted = task.payload.get("list").and_then(|v| v.as_str());
    let after = task.payload.get("after").and_then(|v| v.as_str());
    let after = after.and_then(|v| v.parse::<Uuid>().ok());
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    let latest = latest_list(&mut conn).await.map_err(|_| FAILED)?;
    let done = match latest {
        Some(list) if acted != Some(list.as_str()) => {
            let state = (list, after);
            let queued = queue_resumes(&mut conn, (now, lists, batch), state).await;
            queued.map(|_| ())
        }
        _ => {
            let same = task.payload.clone();
            queue_run(&mut conn, now, lists, same).await.map(|_| ())
        }
    };
    done.map_err(|_| FAILED)
}

/// The latest recorded ROC list's date, as `YYYY-MM-DD`.
async fn latest_list(conn: &mut PgConnection) -> Result<Option<String>, BusinessesError> {
    let lists = roc_lists(conn).await;
    let lists = lists.map_err(|_| BusinessesError::Database)?;
    Ok(lists.last().map(|l| l.list_date.to_string()))
}

/// BIZ-026: for ROC list `list`, queues one [`RESUME_ONE_KIND`] task per waiting sign-up, at most
/// `batch`, in business ID order from the one after `after` round to the start, and the next run
/// carrying the list and the last one queued, all in one transaction. When that next run was
/// already queued (an earlier attempt committed) it queues nothing. Returns how many it queued.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn queue_resumes(
    conn: &mut PgConnection,
    (now, lists, batch): (Timestamp, DailyListSettings, u32),
    (list, after): (String, Option<Uuid>),
) -> Result<usize, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT business_id FROM businesses.signup WHERE waiting_on_record_since IS NOT NULL \
         ORDER BY (business_id <= $1) IS TRUE, business_id LIMIT $2",
        after,
        i64::from(batch)
    );
    let ids = db(q.fetch_all(&mut *conn).await)?;
    let mut tx = db(conn.begin().await)?;
    let last = ids.last().copied().or(after);
    let next = json!({ "list": list, "after": last });
    if !queue_run(&mut tx, now, lists, next).await? {
        return Ok(0);
    }
    for id in &ids {
        let mut task = NewTask::new(RESUME_ONE_KIND, json!({ "business": id }), now);
        task.dedupe_key = Some(format!("{RESUME_ONE_KIND}.{id}.{list}"));
        let queued = enqueue(&mut tx, &task).await;
        queued.map_err(|_| BusinessesError::Database)?;
    }
    db(tx.commit().await)?;
    Ok(ids.len())
}

/// The parts one lookup needs: the license-record source, Verify and the ID secret.
pub type ResumeParts<'a, S, V> = (&'a S, &'a V, &'a SecretString);

/// The [`RESUME_ONE_KIND`] handler (BIZ-026, BIZ-002, QH-001, LIC-008): outside 8 a.m. to 8 p.m.
/// Arizona it re-queues itself for 8 a.m. with no lookup; otherwise one lookup and, on a changed
/// record contact, the resume and the next code in one transaction (the module doc).
///
/// # Errors
/// A fixed [`HandlerError`]: the task retries; a resume is kept only with its code sent.
pub async fn run_resume_one<S: LicenseRecords, V: VerifyAdapter>(
    pool: &PgPool,
    task: &Claimed,
    parts: ResumeParts<'_, S, V>,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let business = task.payload.get("business").and_then(|v| v.as_str());
    let business = business.and_then(|v| v.parse::<Uuid>().ok());
    let business = business.ok_or(BAD_PAYLOAD)?;
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    if let Some(until) = arizona_hold(now) {
        return hold(&mut conn, business, until).await;
    }
    if !waiting_on_record(&mut conn, business)
        .await
        .map_err(|_| FAILED)?
    {
        return Ok(());
    }
    let read = signup_record(&mut conn, parts.0, business).await;
    let SignupRecord::Read(found) = read.map_err(|_| FAILED)? else {
        return Ok(());
    };
    let mut tx = conn.begin().await.map_err(|_| FAILED)?;
    let resumed = resume_on_new_record(&mut tx, parts.2, (business, found.seen())).await;
    if !resumed.map_err(|_| FAILED)? {
        return Ok(());
    }
    match text_window(&mut tx, business, found.contacts(), now).await? {
        Gate::Send => {}
        Gate::Wait => return Ok(()),
        Gate::Hold(until) => {
            drop(tx);
            return hold(&mut conn, business, until).await;
        }
    }
    let verify = (parts.1, parts.2);
    match send_code_seen(&mut tx, verify, business, found.seen(), now).await {
        Ok(_) => tx.commit().await.map_err(|_| FAILED),
        Err(SignupError::Verify) => Err(NOT_SENT),
        Err(SignupError::Businesses(BusinessesError::Refused)) => Ok(()),
        Err(_) => Err(FAILED),
    }
}

/// What the resumed step's text may do now (QH-001).
enum Gate {
    /// Not a text step, or inside the window in both zones.
    Send,
    /// A text step outside the area-code window: re-queue for `until`.
    Hold(Timestamp),
    /// A record phone the area-code table cannot place: never texted, keep waiting (QH-003).
    Wait,
}

/// QH-001: the gate of the step the resumed sign-up sends next, read inside the resume's
/// transaction; only a text step is held, to the next instant both zones are open.
async fn text_window(
    tx: &mut PgConnection,
    business: Uuid,
    contacts: &RecordContacts,
    now: Timestamp,
) -> Result<Gate, HandlerError> {
    let stored = models::signup_state(tx, business)
        .await
        .map_err(|_| FAILED)?;
    let steps = [Step::Text, Step::Email, Step::Letter];
    let tried = steps
        .into_iter()
        .find(|s| stored.step.as_deref() == Some(s.code()));
    let phone = contacts.texting_phone.as_ref();
    let Some(phone) = phone.filter(|_| next_step(contacts, tried) == Some(Step::Text)) else {
        return Ok(Gate::Send);
    };
    let table = AreaCodes::embedded().map_err(|_| FAILED)?;
    match window(&table, &e164(phone), now).map_err(|_| FAILED)? {
        SendWindow::Now => Ok(Gate::Send),
        SendWindow::Hold { until } => Ok(Gate::Hold(until)),
        SendWindow::OptedOut | SendWindow::NotTextable => Ok(Gate::Wait),
    }
}

/// A record phone as `+1` and ten digits for the area-code table; anything else is left as it
/// is, which the table refuses.
fn e164(phone: &SecretString) -> String {
    let raw = phone.expose_secret();
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        10 => format!("+1{digits}"),
        11 if digits.starts_with('1') => format!("+{digits}"),
        _ => raw.to_owned(),
    }
}

/// QH-001: the next 8 a.m. Arizona when `now` is outside 8 a.m. to 8 p.m. Arizona, else `None`.
#[must_use]
pub fn arizona_hold(now: Timestamp) -> Option<Timestamp> {
    let zone = TimeZone::fixed(offset(-7));
    let local = now.to_zoned(zone.clone());
    if (8..20).contains(&local.hour()) {
        return None;
    }
    let day = if local.hour() < 8 {
        local.date()
    } else {
        local.date().tomorrow().ok()?
    };
    let at = day.at(8, 0, 0, 0).to_zoned(zone).ok()?;
    Some(at.timestamp())
}

/// QH-001: re-queues the sign-up's lookup for `until` under its own held key (the target time in
/// it, so a second hold is never dropped); the payload carries no contact.
async fn hold(
    conn: &mut PgConnection,
    business: Uuid,
    until: Timestamp,
) -> Result<(), HandlerError> {
    let list = latest_list(conn).await.map_err(|_| FAILED)?;
    let list = list.unwrap_or_default();
    let mut task = NewTask::new(RESUME_ONE_KIND, json!({ "business": business }), until);
    let at = until.as_second();
    task.dedupe_key = Some(format!("{RESUME_ONE_KIND}.held.{business}.{list}.{at}"));
    enqueue(conn, &task).await.map(|_| ()).map_err(|_| FAILED)
}
