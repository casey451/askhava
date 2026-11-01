//! The homeowner's card approval step: she sees the job card before it goes out (INT-041,
//! CARD-007; D-395, D-419, D-434, D-460, D-1274 to D-1277).
//!
//! After matching and the card gate (`quotes::api::build_card`), [`show_card`] texts her the card
//! itself when the whole text fits in [`MAX_CARD_SEGMENTS`], else the link text to her page, and
//! queues one [`CARD_TIMER_KIND`] task that watches the text until it is sent, then waits the
//! wait from there (default [`AUTO_SEND_AFTER`], a setting read into the text), so the wait never
//! starts before she has the card (D-1278). An emergency job skips the step. [`answer_card`]
//! reads her reply: SEND, OK, YES or ENVIAR approves the card at once; EDIT or CAMBIAR asks what
//! to change (the caller then builds the new card and calls [`show_card`] again). With no reply
//! the timer approves the card she was shown. Nothing waits on a reply (D-419).
//!
//! State is `intake_answer` rows under `card.<job>`: `shown.<digest>.<show>`, `edit`,
//! `sent.<digest>`, the digest being 32 hex digits of the SHA256 of the card text, so no homeowner
//! text is stored here (PRIV-001), and `<show>` the row's own time in microseconds: each show is
//! its own text and its own timer, so a timer of an earlier show never approves a later one. Rows
//! are written in time order (`next_time`), so the latest row is the state. Approval enqueues one
//! [`CARDS_APPROVED_KIND`] event per job for the card sender, which sends only a card for which
//! [`approved_card`] holds: the card the businesses get equals, character for character, the card
//! she was shown (TD-INT-23, TD-AI-17).

use askhava_core::api::{
    Claimed, Clock, HandlerError, NewTask, SystemClock, enqueue as enqueue_task,
};
use askhava_jobs::api::job_number;
use askhava_messaging::api::{Language, count_segments, delivery_status, render};
use askhava_ops::api::Actor;
use askhava_quotes::api::JobCard;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::handlers::queue_body as queue;
use crate::models::{self, IntakeError};
use crate::rules::language_from_code;
use crate::service::{Queued, TextIn, audit_action, card_allowed, sha256_hex};
use crate::status::card_text_watch;

/// The card texted in full (INT-041); its `[card]` slot takes the card text.
pub const CARD_SHOWN_TEMPLATE: &str = "card_shown";
/// The short text with the link to the card on her page (INT-041).
pub const CARD_LINK_TEMPLATE: &str = "card_link";
/// The reply to EDIT or CAMBIAR: what should change (INT-041).
pub const CARD_EDIT_TEMPLATE: &str = "card_edit";
/// Where [`CARD_SHOWN_TEMPLATE`] holds the card; the card is outside the template (TXT-004).
pub const CARD_SLOT: &str = "[card]";
/// The `core.task` kind of the no-reply auto-send (INT-041).
pub const CARD_TIMER_KIND: &str = "intake.card_timer";
/// The event the card sender handles once a job's card is approved (INT-041, CARD-007).
pub const CARDS_APPROVED_KIND: &str = "intake.cards_approved";
/// INT-041's default wait before the cards go out as shown (a setting, D-460).
pub const AUTO_SEND_AFTER: SignedDuration = SignedDuration::from_mins(30);
/// The card is texted in full only when the whole text fits in this many segments (INT-041).
pub const MAX_CARD_SEGMENTS: usize = 2;
/// How often a card timer looks again for the card text to be sent (D-1278).
pub const RECHECK_AFTER: SignedDuration = SignedDuration::from_mins(1);
/// The audit action of a SEND, OK, YES or ENVIAR reply (the homeowner approved).
pub const APPROVED_ACTION: &str = "intake.card_approved";
/// The audit action of the no-reply auto-send (the timer approved).
pub const AUTO_SENT_ACTION: &str = "intake.card_auto_sent";
const BAD_PAYLOAD: &str = "bad intake card timer payload";
const FAILED: &str = "the intake card timer failed";

/// A reply to the card text (INT-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardReply {
    /// SEND, OK, YES or ENVIAR: the cards go now.
    Send,
    /// EDIT or CAMBIAR: she wants to change the card.
    Edit,
}

/// Reads a whole message as a card reply, any case, trailing `.` or `!` ignored (INT-041).
#[must_use]
pub fn card_reply(words: &str) -> Option<CardReply> {
    let word = words.trim().trim_end_matches(['.', '!']).to_uppercase();
    match word.as_str() {
        "SEND" | "OK" | "YES" | "ENVIAR" => Some(CardReply::Send),
        "EDIT" | "CAMBIAR" => Some(CardReply::Edit),
        _ => None,
    }
}

/// 32 hex digits of the SHA256 of a card text: what approval stores and checks (CARD-007).
#[must_use]
pub fn card_digest(card: &str) -> String {
    sha256_hex(card)[..32].to_owned()
}

/// INT-041's text for request `number` and card `card`: the full card when the whole text fits
/// [`MAX_CARD_SEGMENTS`], else the link text with `link`; "It goes out in <wait>" reads the
/// setting. Returns the template ID and the body.
///
/// # Errors
///
/// [`IntakeError::Template`].
pub fn preview_text(
    (number, card): (&str, &str),
    link: &str,
    wait: SignedDuration,
    language: Language,
) -> Result<(&'static str, String), IntakeError> {
    let spanish = language == Language::Es;
    let unit = if spanish { "minutos" } else { "minutes" };
    let time = format!("{} {unit}", wait.as_mins());
    let values = [("job", number), ("time", time.as_str()), ("link", link)];
    let fill = |id| render(id, language, spanish, &values).map_err(|_| IntakeError::Template);
    let full = fill(CARD_SHOWN_TEMPLATE)?.body.replacen(CARD_SLOT, card, 1);
    if count_segments(&full).segments <= MAX_CARD_SEGMENTS {
        return Ok((CARD_SHOWN_TEMPLATE, full));
    }
    Ok((CARD_LINK_TEMPLATE, fill(CARD_LINK_TEMPLATE)?.body))
}

/// The card shown to the homeowner of `conversation` for `job` at `at` (INT-041).
#[derive(Debug, Clone, Copy)]
pub struct Preview<'a> {
    /// The request.
    pub conversation: Uuid,
    /// The job whose card it is.
    pub job: Uuid,
    /// The gated card, exactly as businesses will get it (CARD-007, CARD-008).
    pub card: &'a JobCard,
    /// Her page link holding the card, for a card too long to text.
    pub link: &'a str,
    /// The wait before the cards go out as shown (the setting, [`AUTO_SEND_AFTER`]).
    pub wait: SignedDuration,
    /// Now, from the caller's clock.
    pub at: Timestamp,
}

/// What [`show_card`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// An emergency job: no step, the caller sends the cards at once.
    Emergency,
    /// No consent record, so no text (CON-007).
    NotAllowed,
    /// The card was already approved; nothing changes.
    AlreadySent,
    /// The card text or the link text was queued and the timer set.
    Texted(Queued),
}

/// INT-041: shows the card to the homeowner and starts the wait; one transaction under the
/// homeowner lock. Every call is a new show: its own text (even of the same card text, after an
/// EDIT) and its own timer; only the latest show's timer can approve.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Job`], [`IntakeError::Template`],
/// [`IntakeError::Outbox`], [`IntakeError::Audit`] or [`IntakeError::Database`].
pub async fn show_card(
    db: &mut PgConnection,
    secret: &SecretString,
    p: &Preview<'_>,
) -> Result<Shown, IntakeError> {
    let emergency = models::latest_answer(db, p.conversation, "emergency_choice").await?;
    if emergency.as_deref() == Some("emergency") {
        return Ok(Shown::Emergency);
    }
    if !card_allowed(db, p.conversation).await? {
        return Ok(Shown::NotAllowed);
    }
    let (contact, code) = models::recipient(db, p.conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let number = job_number(db, p.job).await.map_err(|_| IntakeError::Job)?;
    let language = language_from_code(&code);
    let shown = (number.to_string(), p.card.text());
    let (template, body) = preview_text((&shown.0, shown.1), p.link, p.wait, language)?;
    let digest = card_digest(p.card.text());
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if state(&mut tx, p.conversation, p.job)
        .await?
        .1
        .starts_with("sent.")
    {
        return Ok(Shown::AlreadySent);
    }
    let show = next_time(&mut tx, (p.conversation, p.job), p.at).await?;
    let choice = shown_choice(&digest, show);
    models::insert_plain(&mut tx, p.conversation, (&question(p.job), &choice), show).await?;
    let subject = [p.job, card_id(&digest), show_id(show)];
    let queued = queue(&mut tx, secret, (contact, template), &subject, &body).await?;
    let timer = CardTimer {
        job: p.job,
        conversation: p.conversation,
        card: digest,
        show,
        outbox: queued.outbox_id,
        wait: p.wait,
        due: show,
        watch: true,
    };
    queue_timer(&mut tx, &timer).await?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Shown::Texted(queued))
}

/// What [`answer_card`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStep {
    /// SEND: the shown card is approved and the cards go now.
    Sent,
    /// EDIT: the "what should change" text was queued.
    Edit(Queued),
    /// Not a card reply, or no card waiting.
    Nothing,
}

/// INT-041: her reply `msg` to the card of `job` in `conversation`. SEND approves the card last
/// shown; EDIT asks what to change and stops the timer of that card. An opt-out or HELP does
/// nothing here.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Job`], [`IntakeError::Template`],
/// [`IntakeError::Outbox`], [`IntakeError::Audit`] or [`IntakeError::Database`].
pub async fn answer_card(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    (conversation, job): (Uuid, Uuid),
) -> Result<CardStep, IntakeError> {
    let reply = card_reply(msg.inbound.words);
    let Some(reply) = reply.filter(|_| !msg.inbound.opt_out && !msg.inbound.help) else {
        return Ok(CardStep::Nothing);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, now) = state(&mut tx, conversation, job).await?;
    let Some(digest) = shown_digest(&now) else {
        return Ok(CardStep::Nothing);
    };
    let ids = (conversation, job);
    let step = if reply == CardReply::Send {
        let by = (Actor::Homeowner(homeowner), APPROVED_ACTION);
        approve(&mut tx, ids, (digest, by), msg.received_at).await?;
        CardStep::Sent
    } else {
        let at = next_time(&mut tx, ids, msg.received_at).await?;
        let row = (conversation, msg.inbox_id);
        models::insert_answer(&mut tx, row, (&question(job), "edit"), at).await?;
        CardStep::Edit(ask_edit(&mut tx, secret, ids, msg.inbox_id).await?)
    };
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(step)
}

/// One card's no-reply auto-send (INT-041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardTimer {
    /// The job.
    pub job: Uuid,
    /// Its request.
    pub conversation: Uuid,
    /// The [`card_digest`] of the card shown.
    pub card: String,
    /// The time of the show it belongs to (the `<show>` of its `shown.<digest>.<show>` row).
    pub show: Timestamp,
    /// The outbox row of that show's text.
    pub outbox: Uuid,
    /// The wait (the setting, [`AUTO_SEND_AFTER`]), counted from when the text is seen sent.
    pub wait: SignedDuration,
    /// When it runs.
    pub due: Timestamp,
    /// True while the text is not yet seen sent; false once the wait is running.
    pub watch: bool,
}

/// Runs a card timer at `now`. Before `due`, or once its show is no longer the one waiting (a
/// SEND, an EDIT or a newer show), it does nothing. A watching timer reads the text's delivery
/// status: not yet sent (held by quiet hours, say), it looks again after [`RECHECK_AFTER`]; sent,
/// it queues the end of the wait at `now + wait`, so she has the card for the whole wait; failed
/// or undelivered, it stops (D-1278). A waiting timer approves the card as shown. Returns whether
/// it approved. Safe to run twice.
///
/// # Errors
///
/// [`IntakeError::UnknownRequest`], [`IntakeError::Outbox`], [`IntakeError::Audit`] or
/// [`IntakeError::Database`].
pub async fn card_timer_step(
    db: &mut PgConnection,
    t: &CardTimer,
    now: Timestamp,
) -> Result<bool, IntakeError> {
    if now < t.due {
        return Ok(false);
    }
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if state(&mut tx, t.conversation, t.job).await?.1 != shown_choice(&t.card, t.show) {
        return Ok(false);
    }
    let approved = if t.watch {
        watch(&mut tx, t, now).await?;
        false
    } else {
        let by = (Actor::System, AUTO_SENT_ACTION);
        approve(&mut tx, (t.conversation, t.job), (&t.card, by), now).await?;
        true
    };
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(approved)
}

/// A watching timer's next step: look again, start the wait, or stop (D-1278).
async fn watch(tx: &mut PgConnection, t: &CardTimer, now: Timestamp) -> Result<(), IntakeError> {
    let status = delivery_status(tx, t.outbox)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    let Some(watch) = card_text_watch(status) else {
        return Ok(());
    };
    let due = now + if watch { RECHECK_AFTER } else { t.wait };
    let next = CardTimer {
        due,
        watch,
        ..t.clone()
    };
    queue_timer(tx, &next).await
}

/// The [`CARD_TIMER_KIND`] handler `server` registers: [`card_timer_step`] on the payload.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a failed step.
pub async fn run_card_timer(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let timer = parse(&task.payload).ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    card_timer_step(&mut db, &timer, SystemClock.now())
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// CARD-007, TD-AI-17: true only when `card` is, character for character, the card approved for
/// `job`. The card sender checks it before any business gets the card.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn approved_card(
    db: &mut PgConnection,
    (conversation, job): (Uuid, Uuid),
    card: &JobCard,
) -> Result<bool, IntakeError> {
    let now = models::latest_answer(db, conversation, &question(job)).await?;
    Ok(now == Some(format!("sent.{}", card_digest(card.text()))))
}

fn question(job: Uuid) -> String {
    format!("card.{}", job.simple())
}

/// A stable ID for one card text (TXT-011).
fn card_id(digest: &str) -> Uuid {
    Uuid::parse_str(digest).unwrap_or(Uuid::nil())
}

/// An ID for one show, so each show is its own outbox text, even of the same card (TXT-011).
fn show_id(show: Timestamp) -> Uuid {
    Uuid::from_u128(u128::from(show.as_microsecond().unsigned_abs()))
}

/// The state row of a show: `shown.<digest>.<show in microseconds>`.
fn shown_choice(digest: &str, show: Timestamp) -> String {
    format!("shown.{digest}.{}", show.as_microsecond())
}

/// The card digest of a `shown.<digest>.<show>` state, `None` for any other state.
fn shown_digest(state: &str) -> Option<&str> {
    let rest = state.strip_prefix("shown.")?;
    rest.split_once('.').map(|(digest, _)| digest)
}

/// The time to write the job's next card row: `at`, or one microsecond after the latest row when
/// that is not earlier, so rows are written in time order whatever clock each caller used.
async fn next_time(
    tx: &mut PgConnection,
    (conversation, job): (Uuid, Uuid),
    at: Timestamp,
) -> Result<Timestamp, IntakeError> {
    let latest = models::latest_answer_time(tx, conversation, &question(job)).await?;
    Ok(latest.map_or(at, |t| at.max(t + SignedDuration::from_micros(1))))
}

/// Locks the request's homeowner and reads the job's card state ("" when none), with the homeowner.
async fn state(
    tx: &mut PgConnection,
    conversation: Uuid,
    job: Uuid,
) -> Result<(Uuid, String), IntakeError> {
    let (homeowner, _) = models::conversation_state(tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(tx, homeowner).await?;
    let now = models::latest_answer(tx, conversation, &question(job)).await?;
    Ok((homeowner, now.unwrap_or_default()))
}

/// Records the approval of card `digest` by `actor` (audited as `action`) and enqueues the one
/// [`CARDS_APPROVED_KIND`] event of the job (JOB-005).
async fn approve(
    tx: &mut PgConnection,
    (conversation, job): (Uuid, Uuid),
    (digest, (actor, action)): (&str, (Actor, &'static str)),
    at: Timestamp,
) -> Result<(), IntakeError> {
    let at = next_time(tx, (conversation, job), at).await?;
    let choice = format!("sent.{digest}");
    models::insert_plain(tx, conversation, (&question(job), &choice), at).await?;
    let payload = json!({"job": job, "conversation": conversation, "card": digest});
    let mut task = NewTask::new(CARDS_APPROVED_KIND, payload, at);
    task.dedupe_key = Some(format!("{CARDS_APPROVED_KIND}.{job}"));
    enqueue_task(tx, &task)
        .await
        .map_err(|_| IntakeError::Database)?;
    audit_action(tx, actor, action, conversation).await
}

/// Queues timer `t`; its key names the show, the due time and the phase, so a re-run of the same
/// step queues nothing more.
async fn queue_timer(tx: &mut PgConnection, t: &CardTimer) -> Result<(), IntakeError> {
    let (show, due) = (t.show.as_microsecond(), t.due.as_microsecond());
    let payload = json!({
        "job": t.job, "conversation": t.conversation, "card": t.card, "show": show,
        "outbox": t.outbox, "wait": t.wait.as_secs(), "due": due, "watch": t.watch,
    });
    let mut task = NewTask::new(CARD_TIMER_KIND, payload, t.due);
    let phase = if t.watch { "watch" } else { "wait" };
    task.dedupe_key = Some(format!("{CARD_TIMER_KIND}.{}.{show}.{phase}.{due}", t.job));
    enqueue_task(tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| IntakeError::Database)
}

fn parse(p: &Value) -> Option<CardTimer> {
    let id = |key: &str| p[key].as_str().and_then(|s| Uuid::parse_str(s).ok());
    Some(CardTimer {
        job: id("job")?,
        conversation: id("conversation")?,
        card: p["card"].as_str()?.to_owned(),
        show: Timestamp::from_microsecond(p["show"].as_i64()?).ok()?,
        outbox: id("outbox")?,
        wait: SignedDuration::from_secs(p["wait"].as_i64()?),
        due: Timestamp::from_microsecond(p["due"].as_i64()?).ok()?,
        watch: p["watch"].as_bool()?,
    })
}

/// Queues the EDIT question about `job`; each EDIT message gets its own text.
async fn ask_edit(
    tx: &mut PgConnection,
    secret: &SecretString,
    (conversation, job): (Uuid, Uuid),
    inbox: Uuid,
) -> Result<Queued, IntakeError> {
    let (contact, code) = models::recipient(tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    let number = job_number(tx, job).await.map_err(|_| IntakeError::Job)?;
    let language = language_from_code(&code);
    let number = number.to_string();
    let values = [("job", number.as_str())];
    let body = render(
        CARD_EDIT_TEMPLATE,
        language,
        language == Language::Es,
        &values,
    )
    .map_err(|_| IntakeError::Template)?
    .body;
    queue(
        tx,
        secret,
        (contact, CARD_EDIT_TEMPLATE),
        &[job, inbox],
        &body,
    )
    .await
}
