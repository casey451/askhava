//! The `intake.picture_text` task: a homeowner's picture text taken off the texting provider
//! outside the webhook's transaction (INT-044, PRIV-014, PRIV-025, SEC-041; T675, D-4360,
//! D-5251, D-5337, D-5459).
//!
//! The inbound route commits her text and, in the same transaction, queues one task per
//! homeowner text with media ([`queue_picture_text`]): its payload holds the inbox row's ID, the
//! Twilio message SID and up to [`MAX_PICTURES`] media SIDs only (no URL, phone, words or image),
//! and marks the text a picture text of her request (`photos::note_picture_text`). The handler
//! ([`run_picture_text_at`]) is generic over a [`PictureSource`], which names only `privacy::api`
//! types; the server's implementation rebuilds each Twilio media URL from its configured account
//! SID and the `{message SID}/{media SID}` tail this file passes ([`media_tail`]), and calls
//! `privacy::api::take_picture_text` (SEC-041's guarded fetch, the on-server checks, the provider
//! delete only after the store committed). The tests implement it over the media and storage
//! stand-ins.
//!
//! A request with no job yet (her first text named no trade) holds the picture: nothing is kept or
//! deleted, and the task queues itself again [`HOLD_EVERY`] (each under its own dedupe key with
//! its due time) until the job opens; once the request closes, or [`HOLD_FOR`] after the picture
//! text, it keeps nothing and deletes the media at the provider ([`PictureSource::discard`]). With
//! a job, each media is taken once: a `photo_media` row per media SID records its outcome, so a
//! rerun fetches nothing twice. A store error or a failed fetch retries the task (nothing is
//! deleted at the provider on a store error); on the last attempt it counts as left off and the
//! media is deleted at the provider, nothing being kept. Then, under the homeowner lock,
//! `photos::picture_step` drives step 9 on every outcome.
//!
//! Every provider delete goes through [`PictureSource::discard`] (T783, D-5081, D-5550, D-5646):
//! when it answers false (after a `MediaState::DeletePending` answer, after the last failed store
//! or on the [`Hold`] discard path) one [`PICTURE_DELETE_KIND`] task is queued, deduped on the
//! media SID, whose handler ([`run_picture_delete`]) errors until the delete goes, so `core`'s
//! queue retries it with backoff and dead-letters it after its last attempt (T366's alert).

use std::future::Future;
use std::sync::Arc;

use askhava_core::api::{Claimed, DEFAULT_MAX_ATTEMPTS, HandlerError, NewTask, enqueue};
use askhava_messaging::api::spanish_switch;
use askhava_privacy::api::{
    DEFAULT_MIN_SHORT_SIDE, MediaState, PhotoError, PhotoTaken, PictureText,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::{self, IntakeError};
use crate::photos::{PhotoJob, PhotoStep, after_photo, picture_request, picture_step, reasked};
use crate::rules::Inbound;
use crate::service::{TextIn, may_text};

/// The task kind the inbound route queues and `server` registers (T675).
pub const PICTURE_TEXT_KIND: &str = "intake.picture_text";
/// The most media of one picture text taken (INT-044: at most 3 photos).
pub const MAX_PICTURES: usize = 3;
/// How often a held picture text looks for its request's job again.
pub const HOLD_EVERY: SignedDuration = SignedDuration::from_mins(15);
/// How long a picture text is held for a job, from the text (INT-019's no-reply close).
pub const HOLD_FOR: SignedDuration = SignedDuration::from_hours(72);
/// The prefix of the row recording one media's outcome: `photo_media.<media SID in lowercase>`,
/// its choice the outcome ([`code_of`]).
const PHOTO_MEDIA: &str = "photo_media.";
const BAD_PAYLOAD: &str = "bad intake picture text payload";
const FAILED: &str = "the intake picture text task failed";
const NOT_DELETED: &str = "the picture text media is not deleted at the provider yet";
/// The task kind of one media's delete-only retry at the provider (T783, D-5550).
pub const PICTURE_DELETE_KIND: &str = "intake.picture_delete";

/// A picture text's media, as the handler reaches it (D-5337). `url` is always the tail
/// `{message SID}/{media SID}` ([`media_tail`]); only the implementation builds the provider URL.
pub trait PictureSource {
    /// SEC-041, INT-044, PRIV-014: fetches the media and takes it as a photo of `job` (the re-ask
    /// size `min_side`, at `at`), then deletes it at the provider once the store committed.
    fn take(
        &self,
        db: &mut PgConnection,
        job: Uuid,
        url: &str,
        at: (u32, Timestamp),
    ) -> impl Future<Output = Result<PictureText, PhotoError>> + Send;
    /// Deletes the media at the provider, keeping nothing; whether the delete went.
    fn discard(&self, url: &str) -> impl Future<Output = bool> + Send;
}

/// What the handler needs: the media source and the `ID_SECRET` for outbox keys and photo links.
pub struct PictureKeys<P> {
    /// The picture texts' media.
    pub source: P,
    /// The `ID_SECRET` (SEC-018).
    pub secret: SecretString,
}

/// One task's payload, read back.
struct Picture {
    inbox: Uuid,
    message: String,
    media: Vec<String>,
}

/// The tail `{message SID}/{media SID}` passed to [`PictureSource`] (D-5459).
#[must_use]
pub fn media_tail(message: &str, media: &str) -> String {
    format!("{message}/{media}")
}

/// The payload of a picture text's task: the inbox row's ID and the Twilio SIDs only.
#[must_use]
pub fn picture_payload(inbox: Uuid, message: &str, media: &[&str]) -> Value {
    let media: Vec<&str> = media.iter().take(MAX_PICTURES).copied().collect();
    json!({ "inbox": inbox, "message": message, "media": media })
}

/// INT-044 (T675): queues the one `intake.picture_text` task of picture text `inbox` (deduped per
/// inbox row), due at `at`, in the caller's transaction; true when it was new.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn queue_picture_text(
    db: &mut PgConnection,
    inbox: Uuid,
    (message, media): (&str, &[&str]),
    at: Timestamp,
) -> Result<bool, IntakeError> {
    let mut task = NewTask::new(
        PICTURE_TEXT_KIND,
        picture_payload(inbox, message, media),
        at,
    );
    task.dedupe_key = Some(format!("{PICTURE_TEXT_KIND}:{inbox}"));
    enqueue(db, &task)
        .await
        .map(|id| id.is_some())
        .map_err(|_| IntakeError::Database)
}

/// The payload's fields; `None` for a bad payload or no media.
fn parse(payload: &Value) -> Option<Picture> {
    let inbox = payload["inbox"].as_str()?.parse::<Uuid>().ok()?;
    let message = payload["message"].as_str()?.to_owned();
    let media: Vec<String> = payload["media"]
        .as_array()?
        .iter()
        .filter_map(|m| m.as_str().map(str::to_owned))
        .take(MAX_PICTURES)
        .collect();
    (!media.is_empty()).then_some(Picture {
        inbox,
        message,
        media,
    })
}

/// What a picture text's request allows now.
enum Hold {
    /// No open request, or held past [`HOLD_FOR`]: keep nothing, delete at the provider.
    Discard,
    /// Her open request has no job yet: look again later.
    Wait,
    /// Take the media for the request and its job.
    Take(Uuid, Uuid),
}

/// The [`Hold`] of picture text `inbox` at `now`.
async fn hold(db: &mut PgConnection, inbox: Uuid, now: Timestamp) -> Result<Hold, IntakeError> {
    let Some((conversation, sent)) = picture_request(db, inbox).await? else {
        return Ok(Hold::Discard);
    };
    let Some(row) = models::conversation(db, conversation).await? else {
        return Ok(Hold::Discard);
    };
    Ok(match row.job_id {
        _ if row.closed_at.is_some() => Hold::Discard,
        Some(job) => Hold::Take(conversation, job),
        None if now >= sent + HOLD_FOR => Hold::Discard,
        None => Hold::Wait,
    })
}

/// INT-044, PRIV-014, PRIV-025, SEC-041 (T675, D-4360): the `intake.picture_text` handler at
/// `now`, as the module comment says.
///
/// # Errors
///
/// A fixed [`HandlerError`] on a bad payload, a database error, a store error or a failed fetch
/// before the last attempt, so the task retries.
pub async fn run_picture_text_at<P: PictureSource + Send + Sync>(
    pool: PgPool,
    keys: Arc<PictureKeys<P>>,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let picture = parse(&task.payload).ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    let held = hold(&mut db, picture.inbox, now).await;
    match held.map_err(|_| HandlerError(FAILED))? {
        Hold::Discard => {
            for media in &picture.media {
                let tail = media_tail(&picture.message, media);
                discard_or_queue(&mut db, &keys.source, &tail, now).await?;
            }
            Ok(())
        }
        Hold::Wait => {
            let due = now + HOLD_EVERY;
            let mut next = NewTask::new(PICTURE_TEXT_KIND, task.payload.clone(), due);
            next.dedupe_key = Some(format!("{PICTURE_TEXT_KIND}:{}:{due}", picture.inbox));
            let queued = enqueue(&mut db, &next).await;
            queued.map(|_| ()).map_err(|_| HandlerError(FAILED))
        }
        Hold::Take(conversation, job) => {
            let last = task.attempts >= DEFAULT_MAX_ATTEMPTS;
            let ids = (conversation, job);
            let steps = take_all(&mut db, &keys, (&picture, ids), (last, now)).await?;
            drive(&mut db, &keys.secret, (&picture, ids), &steps, now)
                .await
                .map_err(|_| HandlerError(FAILED))
        }
    }
}

/// Each media taken once, outside any held transaction, as its [`PhotoStep`].
async fn take_all<P: PictureSource>(
    db: &mut PgConnection,
    keys: &PictureKeys<P>,
    (picture, (conversation, job)): (&Picture, (Uuid, Uuid)),
    (last, now): (bool, Timestamp),
) -> Result<Vec<PhotoStep>, HandlerError> {
    let fail = |_| HandlerError(FAILED);
    let again = reasked(db, conversation).await.map_err(fail)?;
    let done = outcomes(db, conversation).await.map_err(fail)?;
    let mut steps = Vec::new();
    for media in &picture.media {
        let question = format!("{PHOTO_MEDIA}{}", media.to_lowercase());
        let known = done
            .iter()
            .find(|(q, _)| *q == question)
            .map(|(_, c)| c.as_str());
        let taken = match known {
            Some(code) => taken_of(code),
            None => {
                let tail = media_tail(&picture.message, media);
                let one = take_one(db, &keys.source, (job, &tail), now).await;
                match one {
                    None if last => {
                        discard_or_queue(db, &keys.source, &tail, now).await?;
                        PhotoTaken::Refused
                    }
                    Some(taken) => {
                        let code = code_of(taken);
                        models::insert_plain(db, conversation, (&question, &code), now)
                            .await
                            .map_err(fail)?;
                        taken
                    }
                    None => return Err(HandlerError(FAILED)),
                }
            }
        };
        steps.push(after_photo(taken, again));
    }
    Ok(steps)
}

/// One media taken; `None` for a store error (nothing deleted at the provider) or a fetch that
/// reached or tried Twilio and failed, both retried. A stored photo whose provider delete failed
/// gets one more delete and, if that fails too, its delete-only task ([`discard_or_queue`]).
async fn take_one<P: PictureSource>(
    db: &mut PgConnection,
    source: &P,
    (job, tail): (Uuid, &str),
    now: Timestamp,
) -> Option<PhotoTaken> {
    let taken = source
        .take(db, job, tail, (DEFAULT_MIN_SHORT_SIDE, now))
        .await;
    match taken.ok()? {
        PictureText {
            taken: PhotoTaken::Refused,
            media: MediaState::DeletePending,
        } => None,
        PictureText { taken, media } => {
            if media == MediaState::DeletePending {
                discard_or_queue(db, source, tail, now).await.ok()?;
            }
            Some(taken)
        }
    }
}

/// INT-044, PRIV-014 (T783, D-5550, D-5646): deletes media `tail` at the provider through
/// [`PictureSource::discard`]; when it answers false, queues its [`PICTURE_DELETE_KIND`] task, due
/// at `now` and deduped on the media SID.
async fn discard_or_queue<P: PictureSource>(
    db: &mut PgConnection,
    source: &P,
    tail: &str,
    now: Timestamp,
) -> Result<(), HandlerError> {
    if source.discard(tail).await {
        return Ok(());
    }
    let media = tail.rsplit('/').next().unwrap_or(tail);
    let mut task = NewTask::new(PICTURE_DELETE_KIND, json!({ "media": tail }), now);
    task.dedupe_key = Some(format!("{PICTURE_DELETE_KIND}:{media}"));
    let queued = enqueue(db, &task).await;
    queued.map(|_| ()).map_err(|_| HandlerError(FAILED))
}

/// INT-044, PRIV-014 (T783, D-5081, D-5550): the [`PICTURE_DELETE_KIND`] handler: deletes the
/// payload's media tail at the provider through [`PictureSource::discard`].
///
/// # Errors
///
/// A fixed [`HandlerError`] on a bad payload or whenever `discard` answers false, so `core`'s
/// queue retries it with backoff and dead-letters it after its last attempt.
pub async fn run_picture_delete<P: PictureSource + Send + Sync>(
    keys: Arc<PictureKeys<P>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let tail = task.payload["media"]
        .as_str()
        .ok_or(HandlerError(BAD_PAYLOAD))?;
    if keys.source.discard(tail).await {
        Ok(())
    } else {
        Err(HandlerError(NOT_DELETED))
    }
}

/// The `photo_media.<SID>` rows of `conversation`, as (question, outcome).
async fn outcomes(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<(String, String)>, IntakeError> {
    let rows = sqlx::query!(
        r#"SELECT question_id, choice AS "choice!" FROM intake.intake_answer
           WHERE conversation_id = $1 AND starts_with(question_id, $2) AND choice IS NOT NULL"#,
        conversation,
        PHOTO_MEDIA
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.question_id, r.choice))
        .collect())
}

/// A `photo_media` outcome code.
fn code_of(taken: PhotoTaken) -> String {
    match taken {
        PhotoTaken::Stored(id) => format!("stored.{}", id.simple()),
        PhotoTaken::ReAsk => "reask".to_owned(),
        PhotoTaken::Heic => "heic".to_owned(),
        PhotoTaken::Refused | PhotoTaken::TooManyPixels => "off".to_owned(),
    }
}

/// The outcome a `photo_media` code records.
fn taken_of(code: &str) -> PhotoTaken {
    match code {
        "reask" => PhotoTaken::ReAsk,
        "heic" => PhotoTaken::Heic,
        _ => code
            .strip_prefix("stored.")
            .and_then(|id| Uuid::parse_str(id).ok())
            .map_or(PhotoTaken::Refused, PhotoTaken::Stored),
    }
}

/// The trade of the request's `job` (its `job.<trade>.<property>` link row).
async fn job_trade(
    db: &mut PgConnection,
    (conversation, job): (Uuid, Uuid),
) -> Result<Option<String>, IntakeError> {
    sqlx::query_scalar!(
        r#"SELECT split_part(question_id, '.', 2) AS "trade!" FROM intake.intake_answer
           WHERE conversation_id = $1 AND question_id LIKE 'job.%' AND choice = $2
           ORDER BY answered_at, id LIMIT 1"#,
        conversation,
        job.simple().to_string()
    )
    .fetch_optional(db)
    .await
    .map_err(|_| IntakeError::Database)
}

/// INT-044, CON-009: under the homeowner lock, `photos::picture_step` for the open request; a
/// contact `service::may_text` refuses gets nothing.
async fn drive(
    db: &mut PgConnection,
    secret: &SecretString,
    (picture, (conversation, job)): (&Picture, (Uuid, Uuid)),
    steps: &[PhotoStep],
    now: Timestamp,
) -> Result<(), IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let Some((homeowner, true)) = models::conversation_state(&mut tx, conversation).await? else {
        return Ok(());
    };
    models::lock_homeowner(&mut tx, homeowner).await?;
    let to = models::recipient(&mut tx, conversation).await?;
    let trade = job_trade(&mut tx, (conversation, job)).await?;
    let (Some((contact, _)), Some(trade)) = (to, trade) else {
        return Ok(());
    };
    if !may_text(&mut tx, contact).await? {
        return Ok(());
    }
    let spanish = spanish_switch(&mut tx).await;
    let spanish_on = spanish.map_err(|_| IntakeError::Database)?.is_on();
    let msg = TextIn {
        inbox_id: picture.inbox,
        contact,
        inbound: Inbound {
            words: "",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now,
    };
    let about = PhotoJob {
        trade: &trade,
        job,
        spanish_on,
    };
    picture_step(&mut tx, secret, (&msg, conversation), about, steps).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)
}
