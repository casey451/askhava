//! Twilio's inbound-text and status webhooks, routed to the modules (SEC-016, SEC-039, TXT-012,
//! CON-008, CON-018, TXT-031, INT-019; D-861, D-999, D-1216, T241).
//!
//! [`routes`] adds two POST routes, [`INBOUND_PATH`] and [`STATUS_PATH`]. Each takes only a
//! delivery whose `X-Twilio-Signature` passed `adapters::api::verify_webhook` over
//! `ASKHAVA_PUBLIC_BASE_URL` plus the route path and query (Twilio signs the full public URL it
//! posted to); any other gets 403 and one log line (vendor and reason only). A checked delivery
//! is answered 200 whatever the outcome, except 500 on a database error, so Twilio retries only
//! then; the whole delivery is one transaction, so a retry finds nothing half done. Any other
//! module error that stops a text is logged at warn with the step and the error's variant name
//! only (D-1311), and raises one High [`FAILED_ALERT`] about its inbox row in the delivery's
//! transaction, the step in the alert's audit row, never a body, phone or error text (D-2207).
//!
//! An inbound text runs, in order and once per MessageSid: `messaging::api::receive_inbound`
//! (a repeat stops here), the sender's lookup (blind index only), `privacy::api::read_inbound_for`
//! (a homeowner's text, scrubbed with her vault name and the names she stated before, AI-003,
//! AI-020, D-1752) or `read_inbound` (a business phone's, empty context; the raw `Body` goes no
//! further),
//! `messaging::api::on_inbound_text` (an opt-out stops here, CON-008; a homeowner's read that
//! failed on a vault value other than a database or audit error falls back to the empty-context
//! read for this and the HELP step and then stops before `intake`, D-2088),
//! `messaging::api::reply_to_help` (HELP, INFO, AYUDA: the reply is queued, also to a listed
//! number, and nothing else runs; any other text from a listed number stops here, CON-016),
//! then, for a number that is not a linked business phone, `intake::api::receive_reply`
//! (TXT-031, INT-019, D-999). A business phone's text goes to no homeowner path: its opt-out also
//! calls `businesses::api::phone_stopped`, and any other text for which
//! `businesses::api::auto_reply_due` holds queues `business_auto_reply` (BIZ-017, BIZ-032, D-1715).
//! A first text whose new vault contact loses the unique phone index to a concurrent first text
//! from the same phone continues with the winner's contact; if none is found the whole delivery
//! rolls back, inbox row included, so Twilio sends it again (PRIV-001, TXT-012, D-1751). A status
//! callback runs `messaging::api::receive_status`, once per (MessageSid, MessageStatus).
//! While homeowner texting is switched off or her number suspended (`messaging::api::load_lines`),
//! or Casey's `ops` `texting` or `all` pause is on, a homeowner's emergency text first runs
//! `messaging::api::emergency_plan` (TXT-021 to TXT-024, T279, D-2187; an opted-out or listed
//! homeowner's from the opt-out step, with no second critical alert and no call, D-2189): `raise_emergency` raises the critical alert once per message, `ops::api` turns the
//! WEB-007 banner on, `queue_paused_safety` asks for the INT-004 email or Casey's call list, and
//! intake's safety call is asked for only where the plan keeps it; intake's own step then runs as
//! usual and nothing here texts. Nothing here logs a phone, a body or a URL (PRIV-016). This file is also compiled into
//! `tests/integration/tests/inbound_text_route.rs` with `#[path]`, so it names no `crate::` item.
//!
//! T336 (D-2307, D-3135 to D-3139): a text stopped by `Halt::Failed` also queues one
//! [`RETRY_KIND`] task (payload its inbox row ID only, deduped per row) for `inbound_retry`, which
//! reads the body back from Twilio and runs [`rerun`]: the same steps after the inbox insert. A
//! listed sender's failure (`Halt::Settled`) and a `BadWebhook` delivery are not retried. Each
//! step is safe to run again: the sender lookup and store are keyed by phone, the reads write
//! audit rows only, `on_inbound_text` is retried only when it listed nothing (an `Open` or
//! `Resubscribed` repeat writes nothing), the HELP reply, the business auto reply, the paused
//! emergency claim and the safety call are keyed per inbox row, and SEC-014's count and
//! `receive_reply` share one savepoint, so a failed `receive_reply` keeps no count.

use std::sync::Arc;

use askhava_adapters::api::{Delivery, VerifiedWebhook, WebhookVendor, refuse, verify_webhook};
use askhava_businesses::api::{
    AUTO_REPLY_TEMPLATE, BusinessesError, ConsentError, auto_reply_due, phone_stopped, public_names,
};
use askhava_core::api::{Clock, Environment, NewTask, enqueue as enqueue_task};
use askhava_intake::api::rules::{
    Check, Inbound, Settings as IntakeSettings, check, safety_call_trigger, wanted_language,
};
use askhava_intake::api::{
    Admission, IntakeError, LimitError, SafetyCallAsk, TextIn, admit_new_request,
    enqueue_safety_call, list_ask, open_request_jobs, receive_reply,
};
use askhava_messaging::api::{
    APP_OPT_OUT_WORDS, EmergencyFacts, HelpSettings, InboundText, InboxError, Language,
    ModeStateError, NewText, OutboxError, Outcome, PROVIDER_OPT_OUT_WORDS, ProviderKeyword,
    RESUBSCRIBE_WORDS, Relation, SendingNumber, emergency_plan, enqueue, keyword, load_lines,
    on_inbound_text, queue_paused_safety, raise_emergency, receive_inbound, receive_status, render,
    reply_to_help, sender_business,
};
use askhava_ops::api::{
    Actor, AuditEntry, BannerBy, Change, NewAlert, OpsError, Severity, Switch, audit, is_paused,
    raise_alert, show_banner,
};
use askhava_privacy::api::{
    CommonWords, Inbound as Read, InboundWords, Known, NameMemory, NewContact, Vault, VaultError,
    find_by_phone, read_inbound, read_inbound_for, store_contact,
};
use askhava_quotes::api::{QuotesError, quoted_prices};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::routing::post;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

/// Where Twilio posts an inbound text (the number's messaging webhook).
pub const INBOUND_PATH: &str = "/webhooks/twilio/sms";
/// Where Twilio posts a sent text's status callback.
pub const STATUS_PATH: &str = "/webhooks/twilio/status";
/// The High alert, and its audit action, for a text a module error stopped (D-2207).
pub const FAILED_ALERT: &str = "messaging.inbound_text_failed";
/// The HELP reply times until an owner setting holds them: 2 days, 24 hours if urgent (D-887).
const HELP_TIMES: (u32, u32) = (2, 24);
/// The sole-word HELP words the reader flags (CON-018).
const HELP_WORDS: [&str; 3] = ["help", "info", "ayuda"];
/// The `core.task` kind that retries a text a module error stopped, payload `{"inbox"}` only
/// (TXT-012, D-801, D-2307).
pub const RETRY_KIND: &str = "messaging.inbound_retry";
/// The wait before each of the three tries: 5 minutes, 30 minutes, 2 hours (D-2307).
pub const RETRY_WAITS: [SignedDuration; 3] = [
    SignedDuration::from_mins(5),
    SignedDuration::from_mins(30),
    SignedDuration::from_hours(2),
];
/// Claims of one try before `core` dead-letters it on a database or provider error.
const RETRY_CLAIMS: i32 = 5;
/// The end of a run its [`Reach`] stopped after the opt-out and HELP steps.
const STOPPED: &str = "reach";

/// What the two routes need. Secrets are `SecretString`; no `Debug`, so none can be printed.
pub struct Webhooks {
    /// The app pool.
    pub pool: PgPool,
    /// The injected clock (D-863).
    pub clock: Arc<dyn Clock>,
    /// `ASKHAVA_PUBLIC_BASE_URL`, no trailing slash (`core::api::Settings::public_base_url`).
    pub public_base: String,
    /// The Twilio auth token, the webhook signing secret (SEC-016).
    pub auth_token: SecretString,
    /// The `ID_SECRET` for outbox keys (SEC-018).
    pub id_secret: SecretString,
    /// The vault: phone lookups and the sender directory (PRIV-002).
    pub vault: Vault,
    /// The environment the route runs in (`core::api::Settings::environment`), for the AI gate
    /// T299 wires (D-2065, D-2206).
    #[allow(
        dead_code,
        reason = "T299's AI gate reads it; the test crates that compile this file by #[path] do not (D-3086)"
    )]
    pub environment: Environment,
    /// The owner settings `intake` reads (TXT-039, CALL-019).
    pub intake: IntakeSettings,
}

/// The keys and settings the steps after the inbox insert need, borrowed from [`Webhooks`] by the
/// route or built by the retry's handler (D-2307).
pub struct Ctx<'a> {
    /// The `ID_SECRET` for outbox keys (SEC-018).
    pub id_secret: &'a SecretString,
    /// The vault (PRIV-002).
    pub vault: &'a Vault,
    /// The owner settings `intake` reads.
    pub intake: IntakeSettings,
}

impl Webhooks {
    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            id_secret: &self.id_secret,
            vault: &self.vault,
            intake: self.intake,
        }
    }
}

/// One inbound text's fields, from the delivery or, for a retry, from Twilio's copy (D-2307).
/// `Debug` is not derived: it holds a phone and the body (PRIV-016).
pub struct Fields<'a> {
    /// The sender's number.
    pub from: Option<&'a str>,
    /// The Ask Hava number she texted.
    pub to: Option<&'a str>,
    /// The raw body; read once by the scrubbing reader and never kept (D-801).
    pub body: &'a str,
    /// What the provider's Advanced Opt-Out matched (CON-030).
    pub provider: ProviderKeyword,
}

/// How far a run goes after the opt-out and HELP steps (D-2307).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Every step, as the route runs them.
    All,
    /// Opt-out and HELP only (CON-008, CON-018): a stale retry.
    Screen,
    /// Outside quiet hours (QH-001): opt-out and HELP only, except that an emergency text runs
    /// every step, its INT-004 911 text being an any-hour text (QH-004).
    Night,
}

/// How a [`rerun`] ended (D-2307).
#[allow(
    dead_code,
    reason = "only inbound_retry reads it; the test crates that compile this file by #[path] do not (D-3136)"
)]
pub enum Rerun {
    /// The steps ran, or ended normally (a repeat-safe end such as `opted_out`, `help`).
    Done,
    /// Stopped after the opt-out and HELP steps by its [`Reach`].
    Stopped,
    /// A module error: the step's code name and the error's variant name; `retry` is false for a
    /// listed sender (not retried).
    Failed(&'static str, String, bool),
    /// A database error: nothing of this run is kept.
    Database,
}

/// The two POST routes over `hooks`, for `build_router` to merge (SEC-016, SEC-039).
pub fn routes(hooks: Arc<Webhooks>) -> Router {
    Router::new()
        .route(INBOUND_PATH, post(inbound))
        .route(STATUS_PATH, post(status))
        .with_state(hooks)
}

/// Why a delivery stopped before the end: `Database` is answered 500, `Ends` (a normal end, info)
/// and `Failed` (a module error after the duplicate check, warn) 200. `Failed` holds the step and
/// the error's variant name only, never a value (PRIV-016, D-1311).
enum Halt {
    Database,
    Ends(&'static str),
    Failed(&'static str, String),
    /// A module error for a sender on the suppression list: alerted, never retried (D-2307).
    Settled(&'static str, String),
}

type Step<T> = Result<T, Halt>;

async fn inbound(
    State(h): State<Arc<Webhooks>>,
    uri: Uri,
    head: HeaderMap,
    body: Bytes,
) -> StatusCode {
    match verified(&h, &uri, &head, &body) {
        Ok(hook) => answer(in_transaction(&h, &hook, Kind::Text).await),
        Err(code) => code,
    }
}

async fn status(
    State(h): State<Arc<Webhooks>>,
    uri: Uri,
    head: HeaderMap,
    body: Bytes,
) -> StatusCode {
    match verified(&h, &uri, &head, &body) {
        Ok(hook) => answer(in_transaction(&h, &hook, Kind::Status).await),
        Err(code) => code,
    }
}

/// The delivery checked against the URL Twilio signed: the public base, path and query (SEC-016).
fn verified(
    h: &Webhooks,
    uri: &Uri,
    head: &HeaderMap,
    body: &[u8],
) -> Result<VerifiedWebhook, StatusCode> {
    let tail = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
    let url = format!("{}{tail}", h.public_base);
    let pairs: Vec<(&str, &str)> = head
        .iter()
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.as_str(), v)))
        .collect();
    let delivery = Delivery {
        url: &url,
        body,
        headers: &pairs,
    };
    let now = h.clock.now().as_second();
    verify_webhook(WebhookVendor::Twilio, &h.auth_token, delivery, now).map_err(|e| {
        StatusCode::from_u16(refuse(WebhookVendor::Twilio, e)).unwrap_or(StatusCode::FORBIDDEN)
    })
}

fn answer(done: Step<()>) -> StatusCode {
    match done {
        Ok(()) => StatusCode::OK,
        Err(Halt::Ends(step)) => {
            tracing::info!(step, "twilio webhook ended");
            StatusCode::OK
        }
        Err(Halt::Failed(step, error) | Halt::Settled(step, error)) => {
            tracing::warn!(
                step,
                error,
                "twilio webhook step failed; the text was not handled"
            );
            StatusCode::OK
        }
        Err(Halt::Database) => {
            tracing::error!("twilio webhook database error");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Status,
}

/// Runs one delivery in one transaction: committed unless the database failed (TXT-012).
async fn in_transaction(h: &Webhooks, hook: &VerifiedWebhook, kind: Kind) -> Step<()> {
    let mut conn = h.pool.acquire().await.map_err(|_| Halt::Database)?;
    let mut tx = conn.begin().await.map_err(|_| Halt::Database)?;
    let now = h.clock.now();
    let done = match kind {
        Kind::Text => text(h, &mut tx, hook, now).await,
        Kind::Status => match receive_status(&mut tx, hook, now).await {
            Ok(_) => Ok(()),
            Err(e) => Err(inbox_halt(e)),
        },
    };
    if matches!(done, Err(Halt::Database)) {
        return done;
    }
    tx.commit().await.map_err(|_| Halt::Database)?;
    done
}

/// The inbound-text steps, in order (TXT-012, PRIV-026, CON-008, CON-018, D-999). A homeowner's
/// text whose contact-aware read failed on a vault value (D-2088) still runs the opt-out and HELP
/// steps on its empty-context read, then stops before `intake` with `Halt::Failed`.
async fn text(
    h: &Webhooks,
    db: &mut PgConnection,
    hook: &VerifiedWebhook,
    now: Timestamp,
) -> Step<()> {
    let inbox_id = receive_inbound(db, hook, now)
        .await
        .map_err(inbox_halt)?
        .ok_or(Halt::Ends("repeat"))?;
    let fields = Fields {
        from: field(hook, "From"),
        to: field(hook, "To"),
        body: field(hook, "Body").unwrap_or_default(),
        provider: ProviderKeyword::from_opt_out_type(field(hook, "OptOutType")),
    };
    let done = handle(&h.ctx(), db, &fields, (inbox_id, now), Reach::All).await;
    match &done {
        Err(Halt::Failed(step, _)) => {
            failure_alert(db, inbox_id, step).await?;
            queue_retry(db, (inbox_id, fields.provider), now).await?;
        }
        Err(Halt::Settled(step, _)) => failure_alert(db, inbox_id, step).await?,
        _ => {}
    }
    done
}

/// TXT-012, D-801, D-2307: one [`RETRY_KIND`] task for inbox row `inbox_id`, due after the first
/// wait, in the delivery's transaction; its payload is the row ID and the webhook's `OptOutType`
/// code (no body text, D-3140) and its dedupe key [`retry_key`] `(inbox_id, 1)`, so a row is
/// queued once. An insert error is `Halt::Database`.
async fn queue_retry(
    db: &mut PgConnection,
    inbox: (Uuid, ProviderKeyword),
    now: Timestamp,
) -> Step<()> {
    queue_try(db, inbox, (1, now + RETRY_WAITS[0]))
        .await
        .map(|_| ())
}

/// CON-030, D-866, D-3140: a retry's payload: the inbox row ID and, when Twilio's Advanced
/// Opt-Out matched, its `OptOutType` (`STOP` or `START`), so the retry never re-matches the body.
#[must_use]
pub fn retry_payload(inbox_id: Uuid, provider: ProviderKeyword) -> serde_json::Value {
    match provider {
        ProviderKeyword::OptOut => json!({ "inbox": inbox_id, "opt_out_type": "STOP" }),
        ProviderKeyword::Start => json!({ "inbox": inbox_id, "opt_out_type": "START" }),
        ProviderKeyword::None => json!({ "inbox": inbox_id }),
    }
}

/// Queues try `n` of inbox row `inbox_id` at `at`; true when it was new (D-2307).
async fn queue_try(
    db: &mut PgConnection,
    (inbox_id, provider): (Uuid, ProviderKeyword),
    (n, at): (usize, Timestamp),
) -> Step<bool> {
    let mut task = NewTask::new(RETRY_KIND, retry_payload(inbox_id, provider), at);
    task.max_attempts = RETRY_CLAIMS;
    task.dedupe_key = Some(retry_key(inbox_id, n));
    enqueue_task(db, &task)
        .await
        .map(|id| id.is_some())
        .map_err(|_| Halt::Database)
}

/// The dedupe key of try `n` (1 to 3) of inbox row `inbox_id`; try 1 is the route's (D-2307).
#[must_use]
pub fn retry_key(inbox_id: Uuid, n: usize) -> String {
    if n == 1 {
        format!("{RETRY_KIND}:{inbox_id}")
    } else {
        format!("{RETRY_KIND}:{inbox_id}:{n}")
    }
}

/// D-2307: after a failed try, queues the next one (try 2 after 30 minutes, try 3 after 2 hours
/// from `now`, its payload as [`retry_payload`]), the first whose dedupe key is free; false when
/// try 3 already exists, so the last failure adds nothing and T325's alert stays the record.
///
/// # Errors
///
/// [`Rerun::Database`] on an insert error.
#[allow(
    dead_code,
    reason = "only inbound_retry calls it; the test crates that compile this file by #[path] do not (D-3136)"
)]
pub async fn queue_next_try(
    db: &mut PgConnection,
    inbox: (Uuid, ProviderKeyword),
    now: Timestamp,
) -> Result<bool, Rerun> {
    for n in 2..=RETRY_WAITS.len() {
        let queued = queue_try(db, inbox, (n, now + RETRY_WAITS[n - 1])).await;
        match queued {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(_) => return Err(Rerun::Database),
        }
    }
    Ok(false)
}

/// D-2307: queues the night-held run of inbox row `inbox_id` at `until`, the next instant QH-001
/// opens, once per row; false when it was queued before.
///
/// # Errors
///
/// [`Rerun::Database`] on an insert error.
#[allow(
    dead_code,
    reason = "only inbound_retry calls it; the test crates that compile this file by #[path] do not (D-3136)"
)]
pub async fn queue_held(
    db: &mut PgConnection,
    (inbox_id, provider): (Uuid, ProviderKeyword),
    until: Timestamp,
) -> Result<bool, Rerun> {
    let mut task = NewTask::new(RETRY_KIND, retry_payload(inbox_id, provider), until);
    task.max_attempts = RETRY_CLAIMS;
    task.dedupe_key = Some(format!("{RETRY_KIND}:{inbox_id}:held"));
    enqueue_task(db, &task)
        .await
        .map(|id| id.is_some())
        .map_err(|_| Rerun::Database)
}

/// TXT-012, CON-008, CON-018, D-801, D-2307: the steps after the inbox insert for inbox row
/// `inbox_id`, run again by the retry from Twilio's copy of the body in the caller's transaction;
/// `reach` other than [`Reach::All`] stops after the opt-out and HELP steps. Nothing here stores or logs
/// the body; no alert is raised (T325's stays the record).
#[allow(
    dead_code,
    reason = "only inbound_retry calls it; the test crates that compile this file by #[path] do not (D-3136)"
)]
pub async fn rerun(
    ctx: &Ctx<'_>,
    db: &mut PgConnection,
    fields: &Fields<'_>,
    (inbox_id, now, reach): (Uuid, Timestamp, Reach),
) -> Rerun {
    match handle(ctx, db, fields, (inbox_id, now), reach).await {
        Err(Halt::Ends(STOPPED)) => Rerun::Stopped,
        Ok(()) | Err(Halt::Ends(_)) => Rerun::Done,
        Err(Halt::Failed(step, error)) => Rerun::Failed(step, error, true),
        Err(Halt::Settled(step, error)) => Rerun::Failed(step, error, false),
        Err(Halt::Database) => Rerun::Database,
    }
}

/// TXT-012, D-801, D-1311, D-2207 (fourth audit F25): a text stopped by a module error after the
/// repeat check (`Halt::Failed`) is answered 200, so Twilio never resends it, and its body is not
/// kept; one High [`FAILED_ALERT`] about its inbox row, with one system audit row (subject the
/// alert, field the failed step's code name), lets Casey see it and ask the sender to write again.
/// Both commit in the delivery's transaction; a resend stops at the repeat check and raises none.
/// Neither holds a body, phone, address or error text (PRIV-016, PRIV-028). A write that fails is
/// `Halt::Database`: the delivery rolls back and Twilio resends, so no failure is left unseen.
async fn failure_alert(db: &mut PgConnection, inbox_id: Uuid, step: &'static str) -> Step<()> {
    let alert = NewAlert {
        kind: FAILED_ALERT,
        severity: Severity::High,
        subject: Some(("messaging.inbox", inbox_id)),
    };
    let id = raise_alert(db, &alert).await.map_err(|_| Halt::Database)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: FAILED_ALERT,
        subject_kind: "ops.alert",
        subject_id: id,
        change: Change::Field(step),
    };
    audit(db, &entry).await.map_err(|_| Halt::Database)?;
    Ok(())
}

/// The steps after the repeat check for inbox row `inbox_id`, in order.
async fn handle(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    fields: &Fields<'_>,
    (inbox_id, now): (Uuid, Timestamp),
    reach: Reach,
) -> Step<()> {
    let (from, to) = match (fields.from, fields.to) {
        (Some(from), Some(to)) => (from, to),
        _ => return Err(Halt::Ends("no_number")),
    };
    let (contact, relation) = sender(db, h.vault, from).await?;
    let (read, unread) = read(h, db, fields.body, (contact, relation)).await?;
    let words = read.text.as_str();
    let emergency = matches!(check(words, false), Check::Match(_));
    let msg = InboundText {
        inbox_id,
        contact,
        relation,
        phone: from,
        text: words,
        provider: fields.provider,
        emergency,
    };
    let resubscribed = screen(h, db, &msg, (to, now)).await?;
    if reach == Reach::Screen || (reach == Reach::Night && !emergency) {
        return Err(Halt::Ends(STOPPED));
    }
    if relation == Relation::Business {
        return auto_reply(h, db, &msg).await;
    }
    paused_emergency(h, db, &msg, (to, now), false).await?;
    if let Some(error) = unread {
        return Err(Halt::Failed("vault", error));
    }
    intake(h, db, (&msg, &read), (resubscribed, now)).await
}

/// TXT-021 to TXT-024, INT-004, WEB-007, CALL-019, D-1805, D-2187: a homeowner's emergency text
/// while her line is paused (switched off, suspended, restored and waiting for Casey, or the `ops`
/// `texting` or `all` pause on, which `send_on_line` also obeys; the line's number is the `To` she
/// texted, so a `To` that is no +1 number counts as paused). Once per inbox message
/// (`raise_emergency`'s claim; a resent delivery does nothing more): the critical alert (none for
/// an `opted_out` number, whose alert is optout's), the banner on, the INT-004 email or call-list
/// follow-up, and intake's safety call only when the plan keeps it. An open line or no 911-list
/// match changes nothing here.
async fn paused_emergency(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    msg: &InboundText<'_>,
    (to, now): (&str, Timestamp),
    opted_out: bool,
) -> Step<()> {
    if !msg.emergency || msg.relation == Relation::Business {
        return Ok(());
    }
    let mut line = load_lines(db, (SendingNumber::parse(to).ok(), None))
        .await
        .map_err(mode_halt)?
        .homeowner;
    line.texting_on &= !owner_paused(db).await?;
    let facts = EmergencyFacts {
        matched: true,
        call_trigger: safety_call_trigger(msg.text),
        arrived_at: now,
        has_email: false,
        opted_out,
        safety_switch_on: h.intake.safety_call_on,
    };
    let Some(plan) = emergency_plan(&line, &facts) else {
        return Ok(());
    };
    let pair = (msg.contact, msg.inbox_id);
    let Some(raised) = raise_emergency(db, (&plan, &line), pair)
        .await
        .map_err(ops_halt)?
    else {
        return Ok(());
    };
    if plan.banner_on {
        let cause = BannerBy::System("messaging.inbox", msg.inbox_id);
        show_banner(db, cause, now).await.map_err(ops_halt)?;
    }
    let language = wanted_language(msg.text, None);
    queue_paused_safety(db, pair, language, now)
        .await
        .map_err(mode_halt)?;
    let Some(alert) = raised.alert else {
        return Ok(());
    };
    let ask = SafetyCallAsk {
        contact: msg.contact,
        message: msg.inbox_id,
        arrived_at: now,
        alert,
    };
    enqueue_safety_call(db, msg.text, ask, raised.voice_call)
        .await
        .map(|_| ())
        .map_err(intake_halt)
}

/// Whether Casey's `ops` `texting` or `all` pause is on (OPS-003, TXT-021), which closes every
/// line as `send_on_line` does.
async fn owner_paused(db: &mut PgConnection) -> Step<bool> {
    for switch in [Switch::Texting, Switch::All] {
        if is_paused(db, switch).await.map_err(ops_halt)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Hands a homeowner's scrubbed text to `intake::api::receive_reply` (TXT-031, INT-019, D-999).
/// With an open request, a LIST text gets its quoted businesses' public names read now
/// ([`list_names`]); with none, SEC-014's new-request limit is counted first ([`admitted`]),
/// except for a START that just took her number off the opt-out list (`resubscribed`, CON-015)
/// and a 911-list text, which is never counted or refused, so the INT-002 safety text always
/// runs (INT-002, INT-003, D-2206, D-3083, D-3087). The count and `receive_reply` share one
/// savepoint, rolled back on a module error, so a text that fails here and is retried is counted
/// once (SEC-014, D-2307).
async fn intake(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    text: (&InboundText<'_>, &Read),
    at: (bool, Timestamp),
) -> Step<()> {
    let mut savepoint = db.begin().await.map_err(|_| Halt::Database)?;
    let done = intake_steps(h, &mut savepoint, text, at).await;
    match done {
        Err(Halt::Failed(..) | Halt::Settled(..)) => {
            savepoint.rollback().await.map_err(|_| Halt::Database)?
        }
        Err(Halt::Database) => {}
        Ok(()) | Err(Halt::Ends(_)) => savepoint.commit().await.map_err(|_| Halt::Database)?,
    }
    done
}

/// The read of her open request, the count and `receive_reply`, inside [`intake`]'s savepoint.
async fn intake_steps(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    (msg, read): (&InboundText<'_>, &Read),
    (resubscribed, now): (bool, Timestamp),
) -> Step<()> {
    let names = match open_request_jobs(db, msg.contact)
        .await
        .map_err(intake_halt)?
    {
        Some(jobs) if list_ask(msg.text).is_some() => list_names(db, &jobs).await?,
        Some(_) => Vec::new(),
        None if resubscribed || msg.emergency => Vec::new(),
        None => {
            admitted(db, msg.contact, now).await?;
            Vec::new()
        }
    };
    let inbound = Inbound {
        words: msg.text,
        opt_out: read.flags.opt_out,
        help: read.flags.help,
        emergency: msg.emergency,
        ai_spanish: None,
    };
    let reply = TextIn {
        inbox_id: msg.inbox_id,
        contact: msg.contact,
        inbound,
        received_at: now,
    };
    receive_reply(db, h.id_secret, &reply, (h.intake, names.as_slice()))
        .await
        .map(|_| ())
        .map_err(intake_halt)
}

/// SEC-014, SEC-015, D-1951, D-2206, D-3083: the new-request limit for a homeowner with no open
/// request, keyed by her vault contact (no IP: Twilio posts the text), counted on the delivery's
/// transaction as its last step before `receive_reply`, so a rolled-back delivery counts nothing
/// and nothing waits on the network while the shared hourly row is held. A refused text ends here
/// (200, its inbox row kept, so Twilio's resend is a repeat): it opens nothing and is answered by
/// nothing; its alert (one per window, no body or phone) is `intake`'s. A counter or alert error
/// is a database error, so the delivery rolls back and Twilio resends it.
async fn admitted(db: &mut PgConnection, contact: Uuid, now: Timestamp) -> Step<()> {
    match admit_new_request(&mut *db, (contact, None), now).await {
        Ok(Admission::Admitted) => Ok(()),
        Ok(Admission::Refused(_)) => Err(Halt::Ends("limited")),
        Err(LimitError::Counter | LimitError::Alert) => Err(Halt::Database),
    }
}

/// TXT-031, PICK-041, D-1310, D-2206, D-3083: the public names, read now through
/// `businesses::api::public_names`, of every business that quoted one of `jobs`, for the LIST
/// reply; a business with no usable name is left out, so `receive_reply` keeps D-1310's fallback.
async fn list_names(db: &mut PgConnection, jobs: &[Uuid]) -> Step<Vec<(Uuid, String)>> {
    let mut quoted: Vec<Uuid> = Vec::new();
    for &job in jobs {
        let prices = quoted_prices(db, job).await.map_err(quotes_halt)?;
        quoted.extend(prices.iter().map(|p| p.business));
    }
    quoted.sort_unstable();
    quoted.dedup();
    public_names(db, &quoted).await.map_err(businesses_halt)
}

/// The opt-out check, then the HELP reply (CON-008, CON-013, CON-016, CON-018): an opt-out ends
/// the text; a listed number's text ends after the HELP reply, which reaches it too (D-131); a
/// HELP text ends after its reply. `to` is the Ask Hava number Twilio delivered to; the HELP
/// settings are built from it only for a HELP, INFO or AYUDA text (`keyword`), so a bad `To`
/// never stops any other text (D-1311). A text that carries on answers whether it was the
/// provider's START that just took the number off the list (`Outcome::Resubscribed`, D-3087).
async fn screen(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    msg: &InboundText<'_>,
    (to, now): (&str, Timestamp),
) -> Step<bool> {
    let keys = (h.id_secret, h.vault);
    let outcome = on_inbound_text(db, keys, msg, now)
        .await
        .map_err(outbox_halt)?;
    let listed = match outcome {
        Outcome::OptedOut { .. } => {
            if msg.relation == Relation::Business {
                phone_stopped(db, msg.contact, msg.inbox_id, now)
                    .await
                    .map_err(|e| settled(consent_halt(e)))?;
            }
            paused_emergency(h, db, msg, (to, now), true)
                .await
                .map_err(settled)?;
            return Err(Halt::Ends("opted_out"));
        }
        Outcome::Suppressed { .. } => {
            paused_emergency(h, db, msg, (to, now), true)
                .await
                .map_err(settled)?;
            true
        }
        Outcome::Resubscribed | Outcome::Open => false,
    };
    help(h, db, msg, (to, listed)).await?;
    Ok(outcome == Outcome::Resubscribed)
}

/// The HELP reply (CON-018, D-131) and the listed number's end (CON-016); a module error for a
/// listed sender is `Halt::Settled`, never retried (D-2307).
async fn help(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    msg: &InboundText<'_>,
    (to, listed): (&str, bool),
) -> Step<()> {
    let keep = |e: Halt| if listed { settled(e) } else { e };
    let replied = if keyword(msg.text).is_some() {
        let help = HelpSettings::new(HELP_TIMES, None, (None, to))
            .map_err(|e| keep(Halt::Failed("help_settings", variant(&e))))?;
        reply_to_help(db, h.id_secret, msg, &help)
            .await
            .map_err(|e| keep(outbox_halt(e)))?
    } else {
        None
    };
    match (replied, listed) {
        (Some(_), _) => Err(Halt::Ends("help")),
        (None, true) => Err(Halt::Ends("suppressed")),
        (None, false) => Ok(()),
    }
}

/// A module error for a listed sender: alerted, not retried (D-2307).
fn settled(e: Halt) -> Halt {
    match e {
        Halt::Failed(step, error) => Halt::Settled(step, error),
        other => other,
    }
}

/// The reader over the raw text: the sole-word flags and the scrubbed words (PRIV-026, AI-002).
/// A homeowner's text is scrubbed with her vault name and the names she stated in earlier texts,
/// inside this delivery's transaction, so a resent delivery is read and audited again (AI-003,
/// AI-020, D-1752); a business phone's text with an empty context. The emergency flag is
/// `intake`'s own screen of the scrubbed words (INT-002, INT-026).
///
/// A vault database or audit failure is `Halt::Database` (500, the delivery rolls back and Twilio
/// resends). Any other vault error (a value that will not open, a contact not found) falls back to
/// the empty-context read, so an opt-out or HELP still runs (CON-008, CON-018), and returns the
/// error's variant name so the text stops before `intake` and the AI (AI-003, D-2088).
async fn read(
    h: &Ctx<'_>,
    db: &mut PgConnection,
    raw: &str,
    (contact, relation): (Uuid, Relation),
) -> Step<(Read, Option<String>)> {
    let opt_out: Vec<&str> = PROVIDER_OPT_OUT_WORDS
        .iter()
        .chain(&APP_OPT_OUT_WORDS)
        .copied()
        .collect();
    let words = InboundWords {
        emergency: &[],
        opt_out: &opt_out,
        help: &HELP_WORDS,
        start: &RESUBSCRIBE_WORDS,
    };
    if relation == Relation::Business {
        return Ok((plain_read(raw, &words), None));
    }
    match read_inbound_for(db, h.vault, contact, raw, &words).await {
        Ok(read) => Ok((read, None)),
        Err(e @ (VaultError::Database | VaultError::Audit)) => Err(vault_halt(e)),
        Err(e) => Ok((plain_read(raw, &words), Some(variant(&e)))),
    }
}

/// `raw` read with an empty context: no vault name, no stated names (PRIV-026).
fn plain_read(raw: &str, words: &InboundWords<'_>) -> Read {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    read_inbound(raw, words, &known, &mut NameMemory::default())
}

/// Who wrote from `from`: a linked business phone's business (D-492), else the homeowner's vault
/// contact. She just wrote, so TXT-009's "written first" holds for any reply (D-1308).
async fn sender(db: &mut PgConnection, vault: &Vault, from: &str) -> Step<(Uuid, Relation)> {
    match sender_business(db, vault, from)
        .await
        .map_err(outbox_halt)?
    {
        Some(id) => Ok((id, Relation::Business)),
        None => Ok((homeowner(db, vault, from).await?, Relation::OpenRequest)),
    }
}

/// BIZ-032, D-1715: the reply to a business phone's text that `auto_reply_due` says needs one,
/// queued once per inbound text through the outbox. The route expects no business text yet, so
/// `expected` is false; HELP and opt-outs already ended in [`screen`] (D-2039). The row's
/// `recipient` is the business ID, not the phone's line ID that `business_notices` stores and
/// `businesses::api::may_text` takes: `sender_business` answers only the business, so no sender
/// can check `may_text` for this row or resolve its phone until `messaging` also returns the line
/// ID (open item, D-2042).
async fn auto_reply(h: &Ctx<'_>, db: &mut PgConnection, msg: &InboundText<'_>) -> Step<()> {
    if !auto_reply_due(msg.text, false) {
        return Err(Halt::Ends("business"));
    }
    let body = render(AUTO_REPLY_TEMPLATE, Language::En, false, &[])
        .map_err(|e| Halt::Failed("auto_reply", variant(&e)))?;
    let text = NewText {
        template_id: AUTO_REPLY_TEMPLATE,
        subject: &[msg.inbox_id],
        recipient: msg.contact,
        relation: Relation::Business,
        body: &body.body,
    };
    enqueue(db, h.id_secret, &text).await.map_err(outbox_halt)?;
    Err(Halt::Ends("business_auto_reply"))
}

/// The vault contact of homeowner phone `from`, stored (phone only) on her first text (PRIV-001).
/// Two first texts from one new phone race here (D-1751): the loser's `store_contact` answers
/// `PhoneTaken` after its savepoint rolled back, so the winner's contact is looked up again in
/// this transaction and used; none found is `Halt::Database`, so the delivery, inbox row
/// included, rolls back and Twilio resends it (TXT-012). The unique phone index stays.
async fn homeowner(db: &mut PgConnection, vault: &Vault, from: &str) -> Step<Uuid> {
    if let Some(id) = find_by_phone(db, vault, from).await.map_err(vault_halt)? {
        return Ok(id);
    }
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(from.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    match store_contact(db, vault, &new).await {
        Err(VaultError::PhoneTaken) => find_by_phone(db, vault, from)
            .await
            .map_err(vault_halt)?
            .ok_or(Halt::Database),
        stored => stored.map_err(vault_halt),
    }
}

/// The value of form field `name`, if present and not blank.
fn field<'a>(hook: &'a VerifiedWebhook, name: &str) -> Option<&'a str> {
    let found = hook.params().iter().find(|(n, _)| n == name);
    found
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.trim().is_empty())
}

fn inbox_halt(e: InboxError) -> Halt {
    match e {
        InboxError::Database => Halt::Database,
        InboxError::BadWebhook => Halt::Failed("inbox", variant(&e)),
    }
}

fn outbox_halt(e: OutboxError) -> Halt {
    if matches!(e, OutboxError::Database) {
        Halt::Database
    } else {
        Halt::Failed("messaging", variant(&e))
    }
}

/// A vault database or audit-row failure is a database error (500, Twilio resends), so no text is
/// kept unhandled because an audit row could not be written (PRIV-003, D-2088).
fn vault_halt(e: VaultError) -> Halt {
    if matches!(e, VaultError::Database | VaultError::Audit) {
        Halt::Database
    } else {
        Halt::Failed("vault", variant(&e))
    }
}

/// A database or audit-row failure in `businesses` is a database error (500, Twilio resends), so
/// a business STOP is never lost before its owner email is queued (BIZ-017, D-2039).
fn consent_halt(e: ConsentError) -> Halt {
    if matches!(e, ConsentError::Database | ConsentError::Audit) {
        Halt::Database
    } else {
        Halt::Failed("businesses", variant(&e))
    }
}

fn quotes_halt(e: QuotesError) -> Halt {
    if matches!(e, QuotesError::Database) {
        Halt::Database
    } else {
        Halt::Failed("quotes", variant(&e))
    }
}

fn businesses_halt(e: BusinessesError) -> Halt {
    if matches!(e, BusinessesError::Database) {
        Halt::Database
    } else {
        Halt::Failed("businesses", variant(&e))
    }
}

fn mode_halt(e: ModeStateError) -> Halt {
    if matches!(e, ModeStateError::Database) {
        Halt::Database
    } else {
        Halt::Failed("mode", variant(&e))
    }
}

fn ops_halt(e: OpsError) -> Halt {
    if matches!(e, OpsError::Database) {
        Halt::Database
    } else {
        Halt::Failed("ops", variant(&e))
    }
}

fn intake_halt(e: IntakeError) -> Halt {
    if matches!(e, IntakeError::Database) {
        Halt::Database
    } else {
        Halt::Failed("intake", variant(&e))
    }
}

/// The variant name of error `e` (its `Debug` text up to the first non-name character), so a
/// log line names the error and never a value it may carry (PRIV-016).
fn variant(e: &impl std::fmt::Debug) -> String {
    let text = format!("{e:?}");
    text.chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect()
}
