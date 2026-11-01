//! The pick code and the "<number> YES" text pick (PICK-013 to PICK-016, PICK-031, PICK-032,
//! PICK-036, PICK-055, CON-029; rust brain R11, R13).
//!
//! [`send_code`] and [`check_code`] are the page pick's 6-digit Verify code: the caps and the clock
//! are checked and the send or try recorded in a short transaction, and Verify is called after it
//! commits, never inside it. The caller passes the destination number it read for the homeowner's
//! own session; only her vault contact ID is stored. [`text_pick`] runs inside `intake`'s
//! transaction under the homeowner lock: it plans with [`plan_text_pick`] and then picks, replaces
//! the waiting pick, sends the Terms line first, asks which request, or gives the status reply.
//! [`withdraw_paused`] is `intake`'s call on a CANCEL pause (CON-029). Events for the business
//! (the pick notice, the withdrawal notice) are `core.task` rows with IDs only; their emails are
//! other tasks' (D-491).

use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{Guarded, NewTask, enqueue as enqueue_task};
use askhava_jobs::api::{current, job_number, move_to};
use askhava_messaging::api::{Language, NewText, Relation, enqueue, render};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use askhava_quotes::api::page_quotes;
use jiff::Timestamp;
use jiff::tz::{TimeZone, offset};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, PicksError};
use crate::rules::{
    CODE_TRIES, COOLDOWN, CodeState, CodeVia, JobView, Plan, SendAsk, SendRefused, Stage,
    code_state, parse_code, plan_text_pick, read_text_pick, send_allowed,
};
use crate::status::{pick_move, stage};

/// "Reply <n> YES again to pick." with the share sentence and the Terms line (PICK-016, PICK-031).
pub const TERMS_TEMPLATE: &str = "text_pick_terms";
/// Which request? With the share sentence and the Terms line (D-068, PICK-031).
pub const WHICH_TEMPLATE: &str = "text_pick_which";
/// The reply to a pick (PICK-045's words, D-068).
pub const DONE_TEMPLATE: &str = "text_pick_done";
/// No pickable quote has that number.
pub const NO_QUOTE_TEMPLATE: &str = "text_pick_no_quote";
/// The business confirmed; how to close (PICK-032).
pub const CONFIRMED_TEMPLATE: &str = "text_pick_confirmed";
/// A request still waiting for quotes (PICK-036).
pub const WAITING_TEMPLATE: &str = "text_pick_waiting";
/// No open request (PICK-036).
pub const NONE_TEMPLATE: &str = "text_pick_none";
/// The pick-notice event: payload `pick`, `job`, `business`; it goes at once (PICK-031).
pub const PICK_MADE_KIND: &str = "picks.pick_made";
/// The withdrawal-notice event for the business: payload `pick`, `job`, `business`, `reason`
/// (`replaced`, PICK-032; `paused`, CON-029).
pub const PICK_WITHDRAWN_KIND: &str = "picks.pick_withdrawn";
/// The High alert when the global daily Verify count passes the threshold (PICK-055).
pub const VERIFY_CAP_ALERT: &str = "picks.verify_daily_cap";

/// The last line of every text that offers a text pick, by language (PICK-016, D-492).
#[must_use]
pub const fn terms_line(language: Language) -> &'static str {
    match language {
        Language::En => "By picking, you agree to the Terms: askhava.com/terms",
        Language::Es => "Al elegir, usted acepta los Términos: askhava.com/terms",
    }
}

/// One inbound "<number> YES" text as `intake` passes it on (IDs and scrubbed words only).
#[derive(Debug, Clone, Copy)]
pub struct TextPickIn<'a> {
    /// The sender's vault contact, to whom replies go.
    pub contact: Uuid,
    /// The inbound text (`messaging.inbox`), logged with a text pick.
    pub inbox: Uuid,
    /// The scrubbed words.
    pub words: &'a str,
    /// The homeowner's live jobs, each with whether it is paused by the homeowner (CON-029).
    pub jobs: &'a [(Uuid, bool)],
    /// The request's language and whether Spanish conversations are on.
    pub language: (Language, bool),
    /// The homeowner, for the audit rows.
    pub actor: Actor,
    /// When it arrived.
    pub at: Timestamp,
}

/// What [`text_pick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPickDone {
    /// The plan it carried out.
    pub plan: Plan,
    /// The pick now waiting for Confirm, for the terms acceptance (TERMS-016, TERMS-033).
    pub pick: Option<Uuid>,
    /// The `messaging.outbox` rows queued, in order.
    pub texts: Vec<Uuid>,
}

/// Handles a "<number> YES" text inside the caller's transaction (PICK-015, PICK-016, PICK-031,
/// PICK-032, PICK-036, CON-029, D-068, D-147). `None` when the words are not a text pick.
///
/// # Errors
///
/// [`PicksError::MovedOn`] when the job moved under the pick (roll back and do nothing), else
/// [`PicksError::Job`], [`PicksError::Quotes`], [`PicksError::Template`], [`PicksError::Outbox`],
/// [`PicksError::Audit`] or [`PicksError::Database`].
pub async fn text_pick(
    tx: &mut PgConnection,
    secret: &SecretString,
    input: &TextPickIn<'_>,
) -> Result<Option<TextPickDone>, PicksError> {
    let Some(read) = read_text_pick(input.words) else {
        return Ok(None);
    };
    let mut views = Vec::with_capacity(input.jobs.len());
    for &(job, paused) in input.jobs {
        views.push(view(tx, job, paused).await?);
    }
    views.sort_by_key(|v| v.number);
    let plan = plan_text_pick(read, &views);
    let mut out = Out {
        secret,
        input,
        texts: Vec::new(),
    };
    let pick = match &plan {
        Plan::Which(numbers) => which(tx, &mut out, numbers, &views).await?,
        Plan::Paused(_) => None,
        Plan::Terms(job) => terms_first(tx, &mut out, find(&views, *job), read.quote).await?,
        Plan::NoQuote(job) => {
            let values = [
                ("quoted", read.quote.to_string()),
                ("job", number(&views, *job)),
            ];
            out.say(tx, NO_QUOTE_TEMPLATE, &[*job, input.inbox], &values)
                .await?;
            None
        }
        Plan::Pick(job) | Plan::Replace(job) | Plan::Same(job) => {
            Some(pick_quote(tx, &mut out, find(&views, *job), read.quote).await?)
        }
        Plan::Status => status_reply(tx, &mut out, &views).await?,
    };
    Ok(Some(TextPickDone {
        plan,
        pick,
        texts: out.texts,
    }))
}

/// The texts of one text pick being handled.
struct Out<'a, 'b> {
    secret: &'a SecretString,
    input: &'a TextPickIn<'b>,
    texts: Vec<Uuid>,
}

impl Out<'_, '_> {
    /// Renders and queues one text to the homeowner; returns its outbox row and template version.
    async fn say(
        &mut self,
        tx: &mut PgConnection,
        template: &'static str,
        subject: &[Uuid],
        values: &[(&str, String)],
    ) -> Result<(Uuid, String), PicksError> {
        let (language, spanish_on) = self.input.language;
        let pairs: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let text =
            render(template, language, spanish_on, &pairs).map_err(|_| PicksError::Template)?;
        let new = NewText {
            template_id: template,
            subject,
            recipient: self.input.contact,
            relation: Relation::OpenRequest,
            body: &text.body,
        };
        let (id, _) = enqueue(tx, self.secret, &new)
            .await
            .map_err(|_| PicksError::Outbox)?;
        self.texts.push(id);
        Ok((id, version(template, text.language, &text.body)))
    }
}

/// The template version of a sent text: template, language and the first 12 hex digits of the
/// SHA256 of the exact words, so any wording change is a new version (PICK-016).
fn version(template: &str, language: Language, body: &str) -> String {
    let hash: String = Sha256::digest(body.as_bytes())
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect();
    let code = match language {
        Language::En => "en",
        Language::Es => "es",
    };
    format!("{template}:{code}:{hash}")
}

/// One job as the plan sees it: stage, number, page quotes, waiting pick and Terms line.
async fn view(tx: &mut PgConnection, job: Uuid, paused: bool) -> Result<JobView, PicksError> {
    let status = current(tx, job).await.map_err(|_| PicksError::Job)?.status;
    let number = job_number(tx, job).await.map_err(|_| PicksError::Job)?;
    let quotes = page_quotes(tx, job)
        .await
        .map_err(|_| PicksError::Quotes)?
        .iter()
        .map(|q| !q.unavailable)
        .collect();
    let waiting = models::waiting_pick(tx, job).await?;
    Ok(JobView {
        job,
        number,
        stage: stage(status),
        paused,
        terms_sent: models::terms_sent(tx, job).await?,
        quotes,
        waiting: waiting.and_then(|w| usize::try_from(w.1).ok()),
    })
}

fn find(views: &[JobView], job: Uuid) -> &JobView {
    views.iter().find(|v| v.job == job).unwrap_or(&views[0])
}

fn number(views: &[JobView], job: Uuid) -> String {
    find(views, job).number.to_string()
}

/// The request numbers as words, "1042, 1043 and 1044" ("y" in Spanish), as "Close which?" lists
/// them (D-068).
fn joined(numbers: &[i64], language: Language) -> String {
    let words: Vec<String> = numbers.iter().map(i64::to_string).collect();
    let and = match language {
        Language::En => " and ",
        Language::Es => " y ",
    };
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}{and}{last}", rest.join(", ")),
        _ => words.concat(),
    }
}

/// Asks which request, listing every request number that takes a pick and giving the first as the
/// example (`{job}` is the list, `{number}` the example; D-068, numbered as for CLOSE); the text
/// ends with the Terms line, recorded for each listed job.
async fn which(
    tx: &mut PgConnection,
    out: &mut Out<'_, '_>,
    numbers: &[i64],
    views: &[JobView],
) -> Result<Option<Uuid>, PicksError> {
    let first = numbers.first().map_or_else(String::new, i64::to_string);
    let values = [
        ("job", joined(numbers, out.input.language.0)),
        ("number", first),
    ];
    let (outbox, version) = out
        .say(tx, WHICH_TEMPLATE, &[out.input.inbox], &values)
        .await?;
    for v in views.iter().filter(|v| numbers.contains(&v.number)) {
        models::insert_terms_line(tx, (v.job, outbox), &version, out.input.at).await?;
    }
    Ok(None)
}

/// No Terms line was sent for the job: send it with "Reply <n> YES again to pick." and record
/// it; nothing is picked (PICK-016, D-492).
async fn terms_first(
    tx: &mut PgConnection,
    out: &mut Out<'_, '_>,
    job: &JobView,
    quote: usize,
) -> Result<Option<Uuid>, PicksError> {
    let values = [
        ("quoted", quote.to_string()),
        ("job", job.number.to_string()),
    ];
    let subject = [job.job, out.input.inbox];
    let (outbox, version) = out.say(tx, TERMS_TEMPLATE, &subject, &values).await?;
    models::insert_terms_line(tx, (job.job, outbox), &version, out.input.at).await?;
    Ok(None)
}

/// Picks quote `quote` of `job` by text (PICK-015, PICK-031, PICK-032): the same quote again only
/// repeats the reply; another quote cancels the waiting pick and enqueues the business's
/// withdrawal notice; from Quotes ready the job moves to Awaiting confirm. The new pick is logged
/// with the channel `text` and its inbound text, and the pick notice is enqueued at once. A waiting
/// pick that is no longer waiting when it is ended (the business confirmed it meanwhile) is
/// [`PicksError::MovedOn`]: the caller rolls back and no second pick is made.
async fn pick_quote(
    tx: &mut PgConnection,
    out: &mut Out<'_, '_>,
    job: &JobView,
    quote: usize,
) -> Result<Uuid, PicksError> {
    let input = out.input;
    let waiting = models::waiting_pick(tx, job.job).await?;
    let pick = match waiting {
        Some((id, q, _)) if usize::try_from(q).ok() == Some(quote) => id,
        _ => {
            if let Some((old, _, business)) = waiting {
                let by = (input.actor, input.at);
                if !end(tx, (job.job, old, business), "replaced", by).await? {
                    return Err(PicksError::MovedOn);
                }
            }
            if let Some(arrow) = pick_move(job.stage) {
                let moved = move_to(tx, job.job, arrow, input.at, input.actor).await;
                if !matches!(moved, Ok(Guarded::Applied(()))) {
                    return Err(moved.map_or(PicksError::Job, |_| PicksError::MovedOn));
                }
            }
            new_pick(tx, job.job, quote, input).await?
        }
    };
    let values = [
        ("quoted", quote.to_string()),
        ("job", job.number.to_string()),
    ];
    out.say(tx, DONE_TEMPLATE, &[pick], &values).await?;
    Ok(pick)
}

/// Stores the text pick, its audit row and the pick-notice event.
async fn new_pick(
    tx: &mut PgConnection,
    job: Uuid,
    quote: usize,
    input: &TextPickIn<'_>,
) -> Result<Uuid, PicksError> {
    let quotes = page_quotes(tx, job).await.map_err(|_| PicksError::Quotes)?;
    let business = quotes.get(quote - 1).ok_or(PicksError::Quotes)?.business;
    let number = i32::try_from(quote).map_err(|_| PicksError::Quotes)?;
    let row = (job, business, number, Some(input.inbox));
    let pick = models::insert_pick(tx, row, input.at).await?;
    pick_audit(tx, input.actor, "picks.picked", pick).await?;
    let payload = serde_json::json!({ "pick": pick, "job": job, "business": business });
    event(tx, PICK_MADE_KIND, pick, payload, input.at).await?;
    Ok(pick)
}

/// Cancels a waiting pick with `reason` and enqueues the business's withdrawal notice; after this
/// the business can no longer confirm it (PICK-032, CON-029). A pick no longer waiting (the
/// business confirmed it meanwhile) is left as it is and `false` returned, so no notice goes.
async fn end(
    tx: &mut PgConnection,
    (job, pick, business): (Uuid, Uuid, Uuid),
    reason: &'static str,
    (actor, at): (Actor, Timestamp),
) -> Result<bool, PicksError> {
    if !models::end_pick(tx, pick, reason, at).await? {
        return Ok(false);
    }
    pick_audit(tx, actor, "picks.withdrawn", pick).await?;
    let payload = serde_json::json!({
        "pick": pick, "job": job, "business": business, "reason": reason,
    });
    event(tx, PICK_WITHDRAWN_KIND, pick, payload, at).await?;
    Ok(true)
}

/// Withdraws the pick waiting for Confirm on each of `jobs`, paused by the homeowner's CANCEL, and
/// enqueues each business's notice; no job status changes and timers keep running (CON-029,
/// D-129, D-1245). Returns the withdrawn picks. `intake::on_opt_out` calls it in its transaction.
///
/// # Errors
///
/// [`PicksError::Database`] or [`PicksError::Audit`].
pub async fn withdraw_paused(
    tx: &mut PgConnection,
    jobs: &[Uuid],
    actor: Actor,
    at: Timestamp,
) -> Result<Vec<Uuid>, PicksError> {
    let mut withdrawn = Vec::new();
    for &job in jobs {
        if let Some((pick, _, business)) = models::waiting_pick(tx, job).await?
            && end(tx, (job, pick, business), "paused", (actor, at)).await?
        {
            withdrawn.push(pick);
        }
    }
    Ok(withdrawn)
}

/// Where each open request stands (PICK-032, PICK-036): a confirmed pick gets the confirmed line
/// with how to close, a request still waiting for quotes says so, none open says that.
async fn status_reply(
    tx: &mut PgConnection,
    out: &mut Out<'_, '_>,
    views: &[JobView],
) -> Result<Option<Uuid>, PicksError> {
    let inbox = out.input.inbox;
    let mut said = false;
    for v in views {
        let template = match v.stage {
            Stage::Collecting => WAITING_TEMPLATE,
            Stage::Accepted => CONFIRMED_TEMPLATE,
            Stage::Ready | Stage::Waiting | Stage::Final => continue,
        };
        let values = [("job", v.number.to_string())];
        out.say(tx, template, &[v.job, inbox], &values).await?;
        said = true;
    }
    if !said {
        out.say(tx, NONE_TEMPLATE, &[inbox], &[]).await?;
    }
    Ok(None)
}

async fn event(
    tx: &mut PgConnection,
    kind: &'static str,
    pick: Uuid,
    payload: serde_json::Value,
    at: Timestamp,
) -> Result<(), PicksError> {
    let mut task = NewTask::new(kind, payload, at);
    task.dedupe_key = Some(format!("{kind}.{pick}"));
    enqueue_task(tx, &task)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(())
}

/// One audit row about a pick (JOB-005, PRIV-028: IDs only).
async fn pick_audit(
    tx: &mut PgConnection,
    actor: Actor,
    action: &'static str,
    pick: Uuid,
) -> Result<(), PicksError> {
    let entry = AuditEntry {
        actor,
        action,
        subject_kind: "picks.pick",
        subject_id: pick,
        change: Change::Action,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| PicksError::Audit)
}

/// Whether a text ending with the Terms line was sent for `job` (PICK-016; PICK-030 reads it).
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn terms_line_sent(db: &mut PgConnection, job: Uuid) -> Result<bool, PicksError> {
    models::terms_sent(db, job).await
}

/// Records a queued text ending with the Terms line for `job`: the outbox row, the template
/// version and the time (PICK-016, D-492). The LIST text's sender calls it for `quote_list_end`.
///
/// # Errors
///
/// [`PicksError::Database`].
pub async fn record_terms_line(
    db: &mut PgConnection,
    (job, outbox): (Uuid, Uuid),
    version: &str,
    at: Timestamp,
) -> Result<(), PicksError> {
    models::insert_terms_line(db, (job, outbox), version, at).await
}

/// Where a pick-code send or check is about: the job, the homeowner's vault contact and the
/// destination number the caller read for her own session (never stored or logged).
pub struct CodeTo<'a> {
    /// The job being picked on.
    pub job: Uuid,
    /// Her vault contact ID: the per-number caps count by it (PICK-055, PRIV-002).
    pub contact: Uuid,
    /// The destination number, E.164.
    pub to: &'a SecretString,
}

/// What [`send_code`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeSent {
    /// Verify accepted the send.
    Sent,
    /// Nothing was sent (PICK-013, PICK-055).
    Refused(SendRefused),
}

/// The Arizona time zone (no daylight saving).
fn arizona() -> TimeZone {
    TimeZone::fixed(offset(-7))
}

/// Sends a 6-digit pick code by Verify (PICK-013, PICK-055, D-366): a text, or a voice call only
/// when `ask.via` is her tap. The cooldown, then the caps of [`send_allowed`] are checked and the
/// send recorded in one short transaction; a global cap hit raises one High alert per Arizona
/// day. Verify is called after the commit, under a key derived from the send's row (SEC-018).
///
/// # Errors
///
/// [`PicksError::Verify`] when Verify fails (the send still counts), [`PicksError::Database`] or
/// [`PicksError::Audit`].
pub async fn send_code<V: VerifyAdapter>(
    db: &mut PgConnection,
    (verify, secret): (&V, &SecretString),
    to: &CodeTo<'_>,
    ask: SendAsk,
    (at, threshold): (Timestamp, i64),
) -> Result<CodeSent, PicksError> {
    let local = at.to_zoned(arizona());
    let day = local.date();
    let since = day
        .to_zoned(arizona())
        .map_err(|_| PicksError::Database)?
        .timestamp();
    let mut tx = db.begin().await.map_err(|_| PicksError::Database)?;
    models::lock_contact(&mut tx, to.contact).await?;
    let ids = (to.job, to.contact);
    if let Some((_, row)) = models::latest_code(&mut tx, ids).await?
        && let CodeState::CoolingDown { until } = code_state(&row, at)
    {
        return Ok(CodeSent::Refused(SendRefused::CoolingDown(until)));
    }
    let counts = models::send_counts(&mut tx, ids, since).await?;
    let ask = SendAsk {
        local_hour: local.hour(),
        ..ask
    };
    if let Err(refused) = send_allowed(ask, counts, threshold) {
        if refused == SendRefused::GlobalCap
            && models::cap_hit(&mut tx, &day.to_string(), at).await?
        {
            cap_alert(&mut tx).await?;
        }
        tx.commit().await.map_err(|_| PicksError::Database)?;
        return Ok(CodeSent::Refused(refused));
    }
    let (channel, stored) = match ask.via {
        CodeVia::Text => (CodeChannel::Sms, "sms"),
        CodeVia::CallTapped => (CodeChannel::Call, "call"),
    };
    let row = models::insert_code(&mut tx, ids, stored, at).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    let key = IdempotencyKey::derive(secret, "verify.send", &[&row.to_string()]);
    verify
        .send_code(&key, to.to.expose_secret(), channel)
        .await
        .map_err(|_| PicksError::Verify)?;
    Ok(CodeSent::Sent)
}

async fn cap_alert(tx: &mut PgConnection) -> Result<(), PicksError> {
    let alert = NewAlert {
        kind: VERIFY_CAP_ALERT,
        severity: Severity::High,
        subject: None,
    };
    raise_alert(tx, &alert)
        .await
        .map(|_| ())
        .map_err(|_| PicksError::Audit)
}

/// What [`check_code`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCheck {
    /// The code passed: the page may take the pick (T055).
    Accepted,
    /// Wrong; `left` tries remain.
    Wrong {
        /// Tries remaining.
        left: i32,
    },
    /// The code is 10 minutes old or more, or the cooldown ended: send a new one.
    Expired,
    /// Five wrong tries: wait until this instant (D-015).
    CoolingDown(Timestamp),
    /// Not six digits; no try counted (PICK-014).
    Malformed,
    /// No code was sent for this job and number, or it was already used.
    NoCode,
}

/// Checks a typed, pasted or autofilled code (PICK-013, PICK-014, D-015, D-101): the try is
/// counted in a short locked transaction before Verify is asked, so concurrent tries never pass 5;
/// the fifth wrong try starts the 15-minute cooldown.
///
/// # Errors
///
/// [`PicksError::Verify`] or [`PicksError::Database`].
pub async fn check_code<V: VerifyAdapter>(
    db: &mut PgConnection,
    verify: &V,
    to: &CodeTo<'_>,
    input: &str,
    at: Timestamp,
) -> Result<CodeCheck, PicksError> {
    let Some(code) = parse_code(input) else {
        return Ok(CodeCheck::Malformed);
    };
    let mut tx = db.begin().await.map_err(|_| PicksError::Database)?;
    models::lock_contact(&mut tx, to.contact).await?;
    let Some((id, row)) = models::latest_code(&mut tx, (to.job, to.contact)).await? else {
        return Ok(CodeCheck::NoCode);
    };
    match code_state(&row, at) {
        CodeState::Open { .. } => {}
        CodeState::Expired => return Ok(CodeCheck::Expired),
        CodeState::CoolingDown { until } => return Ok(CodeCheck::CoolingDown(until)),
        CodeState::Used => return Ok(CodeCheck::NoCode),
    }
    let tries = models::count_try(&mut tx, id, at).await?;
    tx.commit().await.map_err(|_| PicksError::Database)?;
    let valid = verify
        .check_code(to.to.expose_secret(), code.digits())
        .await
        .map_err(|_| PicksError::Verify)?;
    if valid {
        models::mark_verified(db, id, at).await?;
        return Ok(CodeCheck::Accepted);
    }
    let left = CODE_TRIES - tries;
    Ok(if left > 0 {
        CodeCheck::Wrong { left }
    } else {
        CodeCheck::CoolingDown(at + COOLDOWN)
    })
}
