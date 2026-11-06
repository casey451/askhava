//! The pick code by email when Ask Hava can't text the homeowner, and the "<number> YES" text from
//! an opted-out homeowner's own phone (PICK-029, PICK-030, D-050, D-477, D-478; T095, D-1612 to
//! D-1614).
//!
//! [`code_route`] says where a code goes: by text while texting is open, else by email only to an
//! address from her own text or web-chat conversation, never one typed on the quote page (TXT-018).
//! [`ConversationEmail::new`] refuses an address labelled [`EmailFrom::QuotePage`], but the label
//! is the caller's: until `privacy` stores a source flag for each address (D-1612), the caller
//! must say truthfully where the address came from. The email code keeps PICK-013's limits
//! (6 digits, 10 minutes, 5 tries, 15-minute cooldown) and PICK-055's caps on the same
//! `picks.code_attempt` rows, counted by vault contact, never the address. [`opted_out_yes`]
//! picks with no text back, only once the PICK-016 Terms line was sent for that request.

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_jobs::api::{current, job_number};
use askhava_ops::api::{Actor, NewAlert, Severity, raise_alert};
use askhava_quotes::api::page_quotes;
use jiff::Timestamp;
use jiff::tz::{TimeZone, offset};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, PicksError};
use crate::rules::{
    CodeState, CodeVia, JobView, Plan, SendAsk, SendRefused, code_state, plan_text_pick,
    read_text_pick, send_allowed,
};
use crate::service::{
    CodeCheck, CodeSent, CodeTo, PagePick, VERIFY_CAP_ALERT, check_code, page_pick,
};
use crate::status::stage;

/// Whether Ask Hava may text the homeowner now (PICK-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Texting {
    /// Texting works: codes go by text (PICK-013).
    Open,
    /// She opted out (CON-013): no text goes to her.
    OptedOut,
    /// Texting is switched off or suspended for everyone.
    Off,
}

/// Where an email address came from (PICK-029, TXT-018, D-050).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailFrom {
    /// She gave it in her own text or web-chat conversation.
    Conversation,
    /// She typed it on the quote page for updates: never gets a code (D-477).
    QuotePage,
}

/// Where a pick code goes (PICK-029).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeRoute {
    /// By text, PICK-013's path.
    Text,
    /// By email to the conversation address.
    Email,
    /// Nowhere: no text and no conversation address; the page cannot finish a pick.
    Nowhere,
}

/// PICK-029, D-050: by text while texting is open; otherwise by email only when the address came
/// from her own conversation; a quote-page address or none gets nothing.
#[must_use]
pub fn code_route(texting: Texting, email: Option<EmailFrom>) -> CodeRoute {
    match (texting, email) {
        (Texting::Open, _) => CodeRoute::Text,
        (_, Some(EmailFrom::Conversation)) => CodeRoute::Email,
        _ => CodeRoute::Nowhere,
    }
}

/// An email address from the homeowner's own conversation, the only kind a code may go to
/// (PICK-029). The address is private and its `Debug` shows nothing of it (PRIV-016).
pub struct ConversationEmail(SecretString);

impl ConversationEmail {
    /// `address` when the caller says it came from her own conversation; `None` for a quote-page
    /// address (D-050). The source is the caller's rule, not checked here, until `privacy` stores a
    /// source flag (D-1612).
    #[must_use]
    pub fn new(address: SecretString, from: EmailFrom) -> Option<Self> {
        (from == EmailFrom::Conversation).then_some(Self(address))
    }
}

impl std::fmt::Debug for ConversationEmail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConversationEmail(..)")
    }
}

/// The start of `at`'s Arizona day, from which PICK-055 counts.
fn arizona_day(at: Timestamp) -> Result<(String, Timestamp), PicksError> {
    let zone = TimeZone::fixed(offset(-7));
    let day = at.to_zoned(zone.clone()).date();
    let start = day.to_zoned(zone).map_err(|_| PicksError::Database)?;
    Ok((day.to_string(), start.timestamp()))
}

/// Sends a 6-digit pick code by Verify's email channel to `to` for `(job, contact)` (PICK-029,
/// PICK-013, PICK-055): the cooldown and the caps are checked and the send recorded in one short
/// transaction (the row's channel is `sms`, a non-voice send, D-1613); a global cap hit raises
/// the one High alert a day. Verify is called after the commit, keyed by the row (SEC-018).
///
/// # Errors
///
/// [`PicksError::Verify`] (the send still counts), [`PicksError::Database`] or
/// [`PicksError::Audit`].
pub async fn send_email_code<V: VerifyAdapter>(
    db: &mut PgConnection,
    (verify, secret): (&V, &SecretString),
    (job, contact): (Uuid, Uuid),
    to: &ConversationEmail,
    (at, threshold): (Timestamp, i64),
) -> Result<CodeSent, PicksError> {
    let (day, since) = arizona_day(at)?;
    let mut tx = db.begin().await.map_err(|_| PicksError::Database)?;
    models::lock_contact(&mut tx, contact).await?;
    if let Some((_, row)) = models::latest_code(&mut tx, (job, contact)).await?
        && let CodeState::CoolingDown { until } = code_state(&row, at)
    {
        return Ok(CodeSent::Refused(SendRefused::CoolingDown(until)));
    }
    let counts = models::send_counts(&mut tx, (job, contact), since).await?;
    let ask = SendAsk {
        via: CodeVia::Text,
        own_session: true,
        local_hour: 12,
    };
    if let Err(refused) = send_allowed(ask, counts, threshold) {
        if refused == SendRefused::GlobalCap && models::cap_hit(&mut tx, &day, at).await? {
            let alert = NewAlert {
                kind: VERIFY_CAP_ALERT,
                severity: Severity::High,
                subject: None,
            };
            raise_alert(&mut tx, &alert)
                .await
                .map_err(|_| PicksError::Audit)?;
        }
        tx.commit().await.map_err(|_| PicksError::Database)?;
        return Ok(CodeSent::Refused(refused));
    }
    let row = models::insert_code(&mut tx, (job, contact), "sms", at).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    let key = IdempotencyKey::derive(secret, "verify.send", &[&row.to_string()]);
    verify
        .send_code(&key, to.0.expose_secret(), CodeChannel::Email)
        .await
        .map_err(|_| PicksError::Verify)?;
    Ok(CodeSent::Sent)
}

/// Checks a code typed on the quote page against the emailed one (PICK-029, PICK-013, PICK-014):
/// [`check_code`] with the conversation address as the destination, so tries, expiry and the
/// cooldown are the texted code's.
///
/// # Errors
///
/// As [`check_code`].
pub async fn check_email_code<V: VerifyAdapter>(
    db: &mut PgConnection,
    verify: &V,
    (job, contact): (Uuid, Uuid),
    to: &ConversationEmail,
    (input, at): (&str, Timestamp),
) -> Result<CodeCheck, PicksError> {
    let to = CodeTo {
        job,
        contact,
        to: &to.0,
    };
    check_code(db, verify, &to, input, at).await
}

/// What an opted-out homeowner's "<number> YES" did; no text goes back in any case (PICK-030,
/// CON-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptedOutYes {
    /// The pick is stored and waits for Confirm; the caller must send its confirmation by email and
    /// show it on the page (no sender exists yet, D-1612).
    Picked(Uuid),
    /// That quote already waits for Confirm: nothing new.
    Same(Uuid),
    /// No Terms line was sent for this request: nothing is picked; the caller must put the Terms
    /// line and a one-time Pick button on her quote page and in her conversation email (D-478; no
    /// sender exists yet, D-1612).
    TermsFirst(Uuid),
    /// Nothing to pick: more than one request, a paused one, no such quote, or none takes a pick.
    Nothing,
}

/// One job as the text-pick rule reads it.
async fn job_view(tx: &mut PgConnection, job: Uuid, paused: bool) -> Result<JobView, PicksError> {
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    let quotes = page_quotes(tx, job).await.map_err(|_| PicksError::Quotes)?;
    let waiting = models::waiting_pick(tx, job).await?;
    Ok(JobView {
        job,
        number: job_number(tx, job).await.map_err(|_| PicksError::Job)?,
        stage: stage(status),
        paused,
        terms_sent: models::terms_sent(tx, job).await?,
        quotes: quotes.iter().map(|q| !q.unavailable).collect(),
        waiting: waiting.and_then(|w| usize::try_from(w.1).ok()),
    })
}

/// PICK-030, D-050, D-478: a "<number> YES" from an opted-out homeowner's own phone, inside the
/// caller's transaction, over her live `jobs` (each with its CON-029 pause, which the caller must
/// read in the same transaction with `intake::api::paused_under_lock`, under the homeowner lock,
/// D-1605). The text-pick rule chooses the job; a pick goes through the page pick's core with that
/// job's own pause (no text queued; stored with channel `page`, D-1614). `None` when the words are
/// not a text pick.
///
/// # Errors
///
/// As [`page_pick`]: [`PicksError::MovedOn`] means roll back.
pub async fn opted_out_yes(
    tx: &mut PgConnection,
    words: &str,
    jobs: &[(Uuid, bool)],
    (actor, at): (Actor, Timestamp),
) -> Result<Option<OptedOutYes>, PicksError> {
    let Some(read) = read_text_pick(words) else {
        return Ok(None);
    };
    let mut views = Vec::with_capacity(jobs.len());
    for &(job, paused) in jobs {
        views.push(job_view(tx, job, paused).await?);
    }
    views.sort_by_key(|v| v.number);
    let done = match plan_text_pick(read, &views) {
        Plan::Terms(job) => OptedOutYes::TermsFirst(job),
        Plan::Pick(job) | Plan::Replace(job) | Plan::Same(job) => {
            let paused = jobs.iter().any(|&(j, p)| j == job && p);
            match page_pick(tx, (job, paused), read.quote, actor, at).await? {
                PagePick::Picked(pick) => OptedOutYes::Picked(pick),
                PagePick::Same(pick) => OptedOutYes::Same(pick),
                PagePick::Taken => OptedOutYes::Nothing,
            }
        }
        _ => OptedOutYes::Nothing,
    };
    Ok(Some(done))
}
