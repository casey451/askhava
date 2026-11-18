//! The sender of the quote-count update text: the `quotes.count_update` handler (TXT-017, QH-007,
//! TXT-042, CON-021, CON-028, SEC-037; D-012, D-1190, D-4052, D-4280, D-4419).
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
//! released at one instant send one text with the latest count and the rest find nothing new. A row
//! queued in quiet hours is held by the outbox sender (QH-001, QH-006); deferring the event itself,
//! QH-004's exemption and TXT-025's merge are T708's (D-4419).

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_jobs::api::{job_number, lock_for_quote_change};
use askhava_messaging::api::{
    Language, NewText, Relation, SendWindow, last_quote_text, send_window,
};
use askhava_quotes::api::{
    COUNT_UPDATE_KIND, Counts, Notice, Reader, Sent, notice_for, sent_count, update_text,
};
use jiff::Timestamp;
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

/// What [`quote_update_step`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteUpdateStep {
    /// The update text was queued.
    Queued(Queued),
    /// The 3-hour gap has not passed: the event was queued again for this instant (TXT-017).
    Held(Timestamp),
    /// Nothing new to tell, or the first quote's text is due (TXT-016's own path).
    Nothing,
    /// Nothing was sent, for this reason (`AlreadyQueued` is never given).
    Skipped(JobTextSkip),
}

/// The `quotes.count_update` handler `server` registers: [`quote_update_step`] on the task's job
/// at `now`, the worker clock's time.
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
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    quote_update_step(&mut db, &keys, job, now)
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// TXT-017, QH-007, TXT-042, CON-021, CON-028: one count-update event for `job` at `now`, the
/// checks and the answer in the module's order. Call it outside a transaction (the vault read).
/// Only a queued text or a held event commits; anything else rolls its transaction back.
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Outbox`], [`IntakeError::Template`] or
/// [`IntakeError::Database`].
pub async fn quote_update_step(
    db: &mut PgConnection,
    keys: &JobTextKeys,
    job: Uuid,
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
    match window {
        SendWindow::OptedOut => return Ok(QuoteUpdateStep::Skipped(JobTextSkip::OptedOut)),
        SendWindow::NotTextable => return Ok(QuoteUpdateStep::Skipped(JobTextSkip::NotTextable)),
        SendWindow::Now | SendWindow::Hold { .. } => {}
    }
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
        Notice::Held { until } => hold(&mut tx, job, until).await?,
        Notice::Update { quoted } => send(&mut tx, keys, (&her, job), quoted, now).await?,
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

/// TXT-017: queues the same event again for `until`, keyed on the job and that instant, so every
/// event held to one instant becomes one task.
async fn hold(
    tx: &mut PgConnection,
    job: Uuid,
    until: Timestamp,
) -> Result<QuoteUpdateStep, IntakeError> {
    let mut task = NewTask::new(COUNT_UPDATE_KIND, json!({ "job": job }), until);
    let run = until.as_microsecond();
    task.dedupe_key = Some(format!("{COUNT_UPDATE_KIND}.{job}.{run}"));
    enqueue(tx, &task)
        .await
        .map_err(|_| IntakeError::Database)?;
    Ok(QuoteUpdateStep::Held(until))
}

/// TXT-017, TXT-032, SEC-037: renders "You now have <k> quotes" in her language with her `/q/`
/// link and queues it, keyed on template, contact, job and count, with the told row for `quoted`;
/// a row already queued gives [`QuoteUpdateStep::Nothing`] (its transaction rolls back).
async fn send(
    tx: &mut PgConnection,
    keys: &JobTextKeys,
    (her, job): (&JobHomeowner, Uuid),
    quoted: u32,
    now: Timestamp,
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
    let (outbox_id, created) = askhava_messaging::api::enqueue(&mut *tx, &keys.outbox, &new)
        .await
        .map_err(|_| IntakeError::Outbox)?;
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
