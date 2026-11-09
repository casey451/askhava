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
//! only (D-1311).
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
//! a homeowner's emergency text first runs `messaging::api::emergency_plan` (TXT-021 to TXT-024,
//! T279, D-2187): `raise_emergency` raises the critical alert once per message, `ops::api` turns the
//! WEB-007 banner on, `queue_paused_safety` asks for the INT-004 email or Casey's call list, and
//! intake's safety call is asked for only where the plan keeps it; intake's own step then runs as
//! usual and nothing here texts. Nothing here logs a phone, a body or a URL (PRIV-016). This file is also compiled into
//! `tests/integration/tests/inbound_text_route.rs` with `#[path]`, so it names no `crate::` item.

use std::sync::Arc;

use askhava_adapters::api::{Delivery, VerifiedWebhook, WebhookVendor, refuse, verify_webhook};
use askhava_businesses::api::{AUTO_REPLY_TEMPLATE, ConsentError, auto_reply_due, phone_stopped};
use askhava_core::api::Clock;
use askhava_intake::api::rules::{
    Check, Inbound, Settings as IntakeSettings, check, safety_call_trigger, wanted_language,
};
use askhava_intake::api::{IntakeError, SafetyCallAsk, TextIn, enqueue_safety_call, receive_reply};
use askhava_messaging::api::{
    APP_OPT_OUT_WORDS, EmergencyFacts, HelpSettings, InboundText, InboxError, Language,
    ModeStateError, NewText, OutboxError, Outcome, PROVIDER_OPT_OUT_WORDS, ProviderKeyword,
    RESUBSCRIBE_WORDS, Relation, SendingNumber, emergency_plan, enqueue, keyword, load_lines,
    on_inbound_text, queue_paused_safety, raise_emergency, receive_inbound, receive_status, render,
    reply_to_help, sender_business,
};
use askhava_ops::api::{BannerBy, OpsError, show_banner};
use askhava_privacy::api::{
    CommonWords, Inbound as Read, InboundWords, Known, NameMemory, NewContact, Vault, VaultError,
    find_by_phone, read_inbound, read_inbound_for, store_contact,
};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::routing::post;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

/// Where Twilio posts an inbound text (the number's messaging webhook).
pub const INBOUND_PATH: &str = "/webhooks/twilio/sms";
/// Where Twilio posts a sent text's status callback.
pub const STATUS_PATH: &str = "/webhooks/twilio/status";
/// The HELP reply times until an owner setting holds them: 2 days, 24 hours if urgent (D-887).
const HELP_TIMES: (u32, u32) = (2, 24);
/// The sole-word HELP words the reader flags (CON-018).
const HELP_WORDS: [&str; 3] = ["help", "info", "ayuda"];

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
    /// The businesses' public names for LIST (TXT-031); empty until `businesses` holds names.
    pub names: Vec<(Uuid, String)>,
    /// The owner settings `intake` reads (TXT-039, CALL-019).
    pub intake: IntakeSettings,
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
        Err(Halt::Failed(step, error)) => {
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
    let (from, to) = match (field(hook, "From"), field(hook, "To")) {
        (Some(from), Some(to)) => (from, to),
        _ => return Err(Halt::Ends("no_number")),
    };
    let (contact, relation) = sender(db, &h.vault, from).await?;
    let raw = field(hook, "Body").unwrap_or_default();
    let (read, unread) = read(h, db, raw, (contact, relation)).await?;
    let words = read.text.as_str();
    let emergency = matches!(check(words, false), Check::Match(_));
    let msg = InboundText {
        inbox_id,
        contact,
        relation,
        phone: from,
        text: words,
        provider: ProviderKeyword::from_opt_out_type(field(hook, "OptOutType")),
        emergency,
    };
    screen(h, db, &msg, (to, now)).await?;
    if relation == Relation::Business {
        return auto_reply(h, db, &msg).await;
    }
    paused_emergency(h, db, &msg, (to, now)).await?;
    if let Some(error) = unread {
        return Err(Halt::Failed("vault", error));
    }
    intake(h, db, (&msg, &read), now).await
}

/// TXT-021 to TXT-024, INT-004, WEB-007, CALL-019, D-1805, D-2187: a homeowner's emergency text
/// while her line is paused (switched off, suspended, or restored and waiting for Casey; the line's
/// number is the `To` she texted, so a `To` that is no +1 number counts as paused). Once per inbox
/// message (`raise_emergency`'s claim; a resent delivery does nothing more): the critical alert,
/// the banner on, the INT-004 email or call-list follow-up, and intake's safety call only when the
/// plan keeps it. An open line or no 911-list match changes nothing here.
async fn paused_emergency(
    h: &Webhooks,
    db: &mut PgConnection,
    msg: &InboundText<'_>,
    (to, now): (&str, Timestamp),
) -> Step<()> {
    if !msg.emergency {
        return Ok(());
    }
    let lines = load_lines(db, (SendingNumber::parse(to).ok(), None))
        .await
        .map_err(mode_halt)?;
    let facts = EmergencyFacts {
        matched: true,
        call_trigger: safety_call_trigger(msg.text),
        arrived_at: now,
        has_email: false,
        opted_out: false,
        safety_switch_on: h.intake.safety_call_on,
    };
    let Some(plan) = emergency_plan(&lines.homeowner, &facts) else {
        return Ok(());
    };
    let pair = (msg.contact, msg.inbox_id);
    let line = &lines.homeowner;
    let Some(raised) = raise_emergency(db, (&plan, line), pair)
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

/// Hands a homeowner's scrubbed text to `intake::api::receive_reply` (TXT-031, INT-019, D-999).
async fn intake(
    h: &Webhooks,
    db: &mut PgConnection,
    (msg, read): (&InboundText<'_>, &Read),
    now: Timestamp,
) -> Step<()> {
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
    let pass = (h.intake, h.names.as_slice());
    receive_reply(db, &h.id_secret, &reply, pass)
        .await
        .map(|_| ())
        .map_err(intake_halt)
}

/// The opt-out check, then the HELP reply (CON-008, CON-013, CON-016, CON-018): an opt-out ends
/// the text; a listed number's text ends after the HELP reply, which reaches it too (D-131); a
/// HELP text ends after its reply. `to` is the Ask Hava number Twilio delivered to; the HELP
/// settings are built from it only for a HELP, INFO or AYUDA text (`keyword`), so a bad `To`
/// never stops any other text (D-1311).
async fn screen(
    h: &Webhooks,
    db: &mut PgConnection,
    msg: &InboundText<'_>,
    (to, now): (&str, Timestamp),
) -> Step<()> {
    let keys = (&h.id_secret, &h.vault);
    let listed = match on_inbound_text(db, keys, msg, now)
        .await
        .map_err(outbox_halt)?
    {
        Outcome::OptedOut { .. } => {
            if msg.relation == Relation::Business {
                phone_stopped(db, msg.contact, msg.inbox_id, now)
                    .await
                    .map_err(consent_halt)?;
            }
            return Err(Halt::Ends("opted_out"));
        }
        Outcome::Suppressed { .. } => true,
        Outcome::Resubscribed | Outcome::Open => false,
    };
    let replied = if keyword(msg.text).is_some() {
        let help = HelpSettings::new(HELP_TIMES, None, (None, to))
            .map_err(|e| Halt::Failed("help_settings", variant(&e)))?;
        reply_to_help(db, &h.id_secret, msg, &help)
            .await
            .map_err(outbox_halt)?
    } else {
        None
    };
    match (replied, listed) {
        (Some(_), _) => Err(Halt::Ends("help")),
        (None, true) => Err(Halt::Ends("suppressed")),
        (None, false) => Ok(()),
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
    h: &Webhooks,
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
    match read_inbound_for(db, &h.vault, contact, raw, &words).await {
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
async fn auto_reply(h: &Webhooks, db: &mut PgConnection, msg: &InboundText<'_>) -> Step<()> {
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
    enqueue(db, &h.id_secret, &text)
        .await
        .map_err(outbox_halt)?;
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
