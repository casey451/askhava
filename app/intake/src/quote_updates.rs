//! The sender of the quote-count update text: the `quotes.count_update` handler (TXT-017, QH-007,
//! TXT-042, TXT-025, QH-001, QH-004, CON-021, CON-028, SEC-037; D-012, D-158, D-1190, D-4052,
//! D-4280, D-4419).
//!
//! `quotes` queues one `quotes::api::COUNT_UPDATE_KIND` event (the job ID only) for each quote
//! newly shown on a Quotes ready job. [`quote_update_step`] finds the job's open request, her
//! vault contact and language (`job_texts::job_homeowner`), skips a contact with no consent record
//! (CON-021) and a number `messaging::api::send_window` calls opted out (CON-028) or not textable,
//! then, in one transaction, takes the job's row lock (`jobs::api::lock_for_quote_change`), so two
//! events for one job never run side by side, and skips a job no longer Quotes ready: closed,
//! cancelled or picked (QH-007; no update follows a pick, TXT-042).
//!
//! Under the lock it reads the previous quote text (`messaging::api::last_quote_text`, sent or
//! still queued) and the count it told: a `first_quote` text told 1, a `quote_update` text the
//! latest `intake.intake_answer` row of her request with question ID `quote_told.<job simple>`
//! (codes only, one row per job and count, written with the text). `quotes::api::notice_for` then
//! answers: `Held { until }` queues the same event again at `until`, keyed on the job and that
//! instant (`core.task.dedupe_key` is unique for all time, so `quotes`' own key is never reused);
//! `Update { quoted }` renders `quotes::api::update_text` with her `/q/` link (a token issued in
//! the transaction, SEC-037) and queues one outbox row keyed on template, contact, job and count
//! (the count as the second subject ID, `Uuid::from_u128`); `Nothing` and `First` send nothing (the
//! first-quote text is `jobs.homeowner_text`'s). The count is read at send time, so several events
//! released at one instant send one text with the latest count and the rest find nothing new.
//!
//! T708 (D-4419, D-4571): an `Update` in quiet hours is queued again for the window's opening, so
//! events held overnight become one task sending one text with the latest count (TXT-025, D-158);
//! an emergency job's goes at any hour (QH-004, [`update`]); a `First` with more than one shown
//! quote, the first-quote text still held, is retried once ([`first_held`]).

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_jobs::api::{current, job_number, lock_for_quote_change};
use askhava_messaging::api::{
    Exemption, Language, NewText, Relation, SendWindow, enqueue_exempt, last_quote_text,
    send_window,
};
use askhava_quotes::api::{
    COUNT_UPDATE_KIND, Counts, Notice, Reader, Sent, notice_for, sent_count, shown_quotes,
    update_text,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::job_texts::{JobHomeowner, JobTextKeys, JobTextSkip, job_homeowner, quote_link};
use crate::models::{self, IntakeError};
use crate::service::{Queued, may_text};

/// The template of the update text (TXT-017).
const UPDATE_TEMPLATE: &str = "quote_update";
const BAD_PAYLOAD: &str = "bad quotes count-update payload";
const FAILED: &str = "the quotes count-update step failed";
/// D-4419: a `First` with more than one shown quote is tried again this many minutes after the
/// window's next opening.
const RETRY_MINUTES: i64 = 15;

/// What [`quote_update_step`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteUpdateStep {
    /// The update text was queued.
    Queued(Queued),
    /// The event was queued again for this instant: the 3-hour gap has not passed (TXT-017), quiet
    /// hours hold her texts (QH-001, TXT-025) or the first-quote text is still held (D-4419).
    Held(Timestamp),
    /// Nothing new to tell, or the first quote's text is due (TXT-016's own path).
    Nothing,
    /// Nothing was sent, for this reason (`AlreadyQueued` is never given).
    Skipped(JobTextSkip),
}

/// The `quotes.count_update` handler `server` registers: [`quote_update_step`] on the task's job
/// at `now`, the worker clock's time, a payload's `"retry": true` marking D-4419's one retry.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed step.
pub async fn run_quote_update_at(
    pool: PgPool,
    keys: Arc<JobTextKeys>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let job = task.payload["job"]
        .as_str()
        .and_then(|j| Uuid::parse_str(j).ok())
        .ok_or(HandlerError(BAD_PAYLOAD))?;
    let retry = task.payload["retry"].as_bool().unwrap_or(false);
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    quote_update_step(&mut db, &keys, (job, retry), now)
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// TXT-017, TXT-025, QH-001, QH-004, QH-007, TXT-042, CON-021, CON-028: one count-update event for
/// `job` at `now` (`retry` marks D-4419's one retry of a `First`), the checks and the answer in the
/// module's order. Call it outside a transaction (the vault read). Only a queued text or a held
/// event commits; anything else rolls its transaction back.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Outbox`], [`IntakeError::Template`] or
/// [`IntakeError::Database`].
pub async fn quote_update_step(
    db: &mut PgConnection,
    keys: &JobTextKeys,
    (job, retry): (Uuid, bool),
    now: Timestamp,
) -> Result<QuoteUpdateStep, IntakeError> {
    let Some(her) = job_homeowner(db, job).await? else {
        return Ok(QuoteUpdateStep::Skipped(JobTextSkip::NoRequest));
    };
    if !may_text(db, her.contact).await? {
        return Ok(QuoteUpdateStep::Skipped(JobTextSkip::NoConsent));
    }
    let window = send_window(db, &keys.vault, her.contact, now)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    let opens = match window {
        SendWindow::OptedOut => return Ok(QuoteUpdateStep::Skipped(JobTextSkip::OptedOut)),
        SendWindow::NotTextable => return Ok(QuoteUpdateStep::Skipped(JobTextSkip::NotTextable)),
        SendWindow::Now => None,
        SendWindow::Hold { until } => Some(until),
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let status = lock_for_quote_change(&mut tx, job)
        .await
        .map_err(|_| IntakeError::Job)?;
    if !status.gets_count_updates() {
        return Ok(QuoteUpdateStep::Skipped(JobTextSkip::MovedOn));
    }
    let previous = previous(&mut tx, &her, job).await?;
    let notice = notice_for(&mut tx, job, previous, now)
        .await
        .map_err(|_| IntakeError::Job)?;
    let step = match notice {
        Notice::Held { until } => hold(&mut tx, job, until, false).await?,
        Notice::Update { quoted } => {
            update(&mut tx, keys, (&her, job), quoted, (now, opens)).await?
        }
        Notice::First if !retry => first_held(&mut tx, job, opens.unwrap_or(now)).await?,
        Notice::Nothing | Notice::First => QuoteUpdateStep::Nothing,
    };
    if matches!(step, QuoteUpdateStep::Queued(_) | QuoteUpdateStep::Held(_)) {
        tx.commit().await.map_err(|_| IntakeError::Database)?;
    }
    Ok(step)
}

/// The question ID of the counts told on `job`: `quote_told.<job simple>` (never `job.`, which
/// the job-answer readers match).
fn told_question(job: Uuid) -> String {
    format!("quote_told.{}", job.simple())
}

/// TXT-017, D-1190: the previous quote text to her about `job` and the count it told: 1 for the
/// first-quote text, else the latest told count on her request (at least 1).
async fn previous(
    tx: &mut PgConnection,
    her: &JobHomeowner,
    job: Uuid,
) -> Result<Option<Sent>, IntakeError> {
    let Some(last) = last_quote_text(&mut *tx, her.contact, job)
        .await
        .map_err(|_| IntakeError::Outbox)?
    else {
        return Ok(None);
    };
    let conversation = her.conversation;
    let quoted = if last.template_id == UPDATE_TEMPLATE {
        models::latest_answer(tx, conversation, &told_question(job))
            .await?
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1)
    } else {
        1
    };
    Ok(Some(Sent {
        at: last.at,
        quoted,
    }))
}

/// QH-001, QH-004, TXT-025, D-4419: an `Update` of an emergency job goes now with the exemption;
/// any other goes now inside the window, else the event is held to the window's opening `opens`.
async fn update(
    tx: &mut PgConnection,
    keys: &JobTextKeys,
    (her, job): (&JobHomeowner, Uuid),
    quoted: u32,
    (now, opens): (Timestamp, Option<Timestamp>),
) -> Result<QuoteUpdateStep, IntakeError> {
    let emergency = current(&mut *tx, job)
        .await
        .map_err(|_| IntakeError::Job)?
        .emergency;
    match (emergency, opens) {
        (true, _) => {
            let exempt = Some(Exemption::EmergencyJob);
            send(tx, keys, (her, job), quoted, (now, exempt)).await
        }
        (false, Some(until)) => hold(tx, job, until, false).await,
        (false, None) => send(tx, keys, (her, job), quoted, (now, None)).await,
    }
}

/// D-4419, D-1190: `First` with more than one shown quote means the first-quote text is still held
/// (no outbox row yet), so the event is queued once more, marked as the retry, [`RETRY_MINUTES`]
/// after `opens` (the window's next opening, or now inside it); one shown quote sends nothing.
async fn first_held(
    tx: &mut PgConnection,
    job: Uuid,
    opens: Timestamp,
) -> Result<QuoteUpdateStep, IntakeError> {
    let shown = shown_quotes(&mut *tx, job)
        .await
        .map_err(|_| IntakeError::Job)?;
    if shown.len() < 2 {
        return Ok(QuoteUpdateStep::Nothing);
    }
    hold(
        tx,
        job,
        opens + SignedDuration::from_mins(RETRY_MINUTES),
        true,
    )
    .await
}

/// TXT-017, TXT-025: queues the same event again for `until` (`retry` marks D-4419's one retry),
/// keyed on the job and that instant (`core.task.dedupe_key` is unique for all time), so every
/// event held to one instant becomes one task.
async fn hold(
    tx: &mut PgConnection,
    job: Uuid,
    until: Timestamp,
    retry: bool,
) -> Result<QuoteUpdateStep, IntakeError> {
    let mut payload = json!({ "job": job });
    if retry {
        payload["retry"] = true.into();
    }
    let mut task = NewTask::new(COUNT_UPDATE_KIND, payload, until);
    let run = until.as_microsecond();
    task.dedupe_key = Some(format!("{COUNT_UPDATE_KIND}.{job}.{run}"));
    enqueue(tx, &task)
        .await
        .map_err(|_| IntakeError::Database)?;
    Ok(QuoteUpdateStep::Held(until))
}

/// TXT-017, TXT-032, SEC-037: renders "You now have <k> quotes" in her language with her `/q/`
/// link and queues it, keyed on template, contact, job and count, with the told row for `quoted`;
/// `exempt` lets an emergency job's go at any hour (QH-004). A row already queued gives
/// [`QuoteUpdateStep::Nothing`] (its transaction rolls back).
async fn send(
    tx: &mut PgConnection,
    keys: &JobTextKeys,
    (her, job): (&JobHomeowner, Uuid),
    quoted: u32,
    (now, exempt): (Timestamp, Option<Exemption>),
) -> Result<QuoteUpdateStep, IntakeError> {
    let number = job_number(&mut *tx, job)
        .await
        .map_err(|_| IntakeError::Job)?
        .to_string();
    let sent = sent_count(&mut *tx, job)
        .await
        .map_err(|_| IntakeError::Job)?;
    let link = quote_link(&mut *tx, keys, job, now).await?;
    let reader = Reader {
        wanted: her.language,
        spanish_on: her.language == Language::Es,
    };
    let counts = Counts {
        job: &number,
        quoted,
        sent,
    };
    let body = update_text(reader, counts, &link).map_err(|_| IntakeError::Template)?;
    let subject = [job, Uuid::from_u128(u128::from(quoted))];
    let new = NewText {
        template_id: UPDATE_TEMPLATE,
        subject: &subject,
        recipient: her.contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let queued = match exempt {
        Some(e) => enqueue_exempt(&mut *tx, &keys.outbox, &new, e).await,
        None => askhava_messaging::api::enqueue(&mut *tx, &keys.outbox, &new).await,
    };
    let (outbox_id, created) = queued.map_err(|_| IntakeError::Outbox)?;
    if !created {
        return Ok(QuoteUpdateStep::Nothing);
    }
    let told = quoted.to_string();
    models::insert_plain(tx, her.conversation, (&told_question(job), &told), now).await?;
    Ok(QuoteUpdateStep::Queued(Queued {
        outbox_id,
        template: UPDATE_TEMPLATE,
    }))
}
