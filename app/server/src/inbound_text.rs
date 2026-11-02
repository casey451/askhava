//! Twilio's inbound-text and status webhooks, routed to the modules (SEC-016, SEC-039, TXT-012,
//! CON-008, CON-018, TXT-031, INT-019; D-861, D-999, D-1216, T241).
//!
//! [`routes`] adds two POST routes, [`INBOUND_PATH`] and [`STATUS_PATH`]. Each takes only a
//! delivery whose `X-Twilio-Signature` passed `adapters::api::verify_webhook` over
//! `ASKHAVA_PUBLIC_BASE_URL` plus the route path and query (Twilio signs the full public URL it
//! posted to); any other gets 403 and one log line (vendor and reason only). A checked delivery
//! is answered 200 whatever the outcome, except 500 on a database error, so Twilio retries only
//! then; the whole delivery is one transaction, so a retry finds nothing half done.
//!
//! An inbound text runs, in order and once per MessageSid: `messaging::api::receive_inbound`
//! (a repeat stops here), `privacy::api::read_inbound` (the raw `Body` goes no further),
//! `messaging::api::on_inbound_text` (an opt-out stops here, CON-008),
//! `messaging::api::reply_to_help` (HELP, INFO, AYUDA: the reply is queued, also to a listed
//! number, and nothing else runs; any other text from a listed number stops here, CON-016),
//! then, for a number that is not a linked business phone, `intake::api::receive_reply`
//! (TXT-031, INT-019, D-999). A business phone's text is recorded and goes to no homeowner path. A status
//! callback runs `messaging::api::receive_status`, once per (MessageSid, MessageStatus).
//! Nothing here logs a phone, a body or a URL (PRIV-016). This file is also compiled into
//! `tests/integration/tests/inbound_text_route.rs` with `#[path]`, so it names no `crate::` item.

use std::sync::Arc;

use askhava_adapters::api::{Delivery, VerifiedWebhook, WebhookVendor, refuse, verify_webhook};
use askhava_core::api::Clock;
use askhava_intake::api::rules::{Check, Inbound, Settings as IntakeSettings, check};
use askhava_intake::api::{IntakeError, TextIn, receive_reply};
use askhava_messaging::api::{
    APP_OPT_OUT_WORDS, HelpSettings, InboundText, InboxError, OutboxError, Outcome,
    PROVIDER_OPT_OUT_WORDS, ProviderKeyword, RESUBSCRIBE_WORDS, Relation, on_inbound_text,
    receive_inbound, receive_status, reply_to_help, sender_business,
};
use askhava_privacy::api::{
    CommonWords, InboundWords, Known, NameMemory, NewContact, Vault, VaultError, find_by_phone,
    read_inbound, store_contact,
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

/// Why a delivery stopped before the end: `Database` is answered 500, `Ends` 200.
enum Halt {
    Database,
    Ends(&'static str),
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

/// The inbound-text steps, in order (TXT-012, PRIV-026, CON-008, CON-018, D-999).
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
    let read = read(field(hook, "Body").unwrap_or_default());
    let words = read.text.as_str();
    let (contact, relation) = sender(db, &h.vault, from).await?;
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
        return Err(Halt::Ends("business"));
    }
    let inbound = Inbound {
        words,
        opt_out: read.flags.opt_out,
        help: read.flags.help,
        emergency,
        ai_spanish: None,
    };
    let reply = TextIn {
        inbox_id,
        contact,
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
/// HELP text ends after its reply. `to` is the Ask Hava number Twilio delivered to.
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
        Outcome::OptedOut { .. } => return Err(Halt::Ends("opted_out")),
        Outcome::Suppressed { .. } => true,
        Outcome::Resubscribed | Outcome::Open => false,
    };
    let help =
        HelpSettings::new(HELP_TIMES, None, (None, to)).map_err(|_| Halt::Ends("help_settings"))?;
    let replied = reply_to_help(db, &h.id_secret, msg, &help)
        .await
        .map_err(outbox_halt)?;
    match (replied, listed) {
        (Some(_), _) => Err(Halt::Ends("help")),
        (None, true) => Err(Halt::Ends("suppressed")),
        (None, false) => Ok(()),
    }
}

/// The reader over the raw text: the sole-word flags and the scrubbed words (PRIV-026, AI-002).
/// The emergency flag is `intake`'s own screen of the scrubbed words (INT-002, INT-026).
fn read(raw: &str) -> askhava_privacy::api::Inbound {
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
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    read_inbound(raw, &words, &known, &mut NameMemory::default())
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

/// The vault contact of homeowner phone `from`, stored (phone only) on her first text (PRIV-001).
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
    store_contact(db, vault, &new).await.map_err(vault_halt)
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
        InboxError::BadWebhook => Halt::Ends("bad_webhook"),
    }
}

fn outbox_halt(e: OutboxError) -> Halt {
    if matches!(e, OutboxError::Database) {
        Halt::Database
    } else {
        Halt::Ends("messaging")
    }
}

fn vault_halt(e: VaultError) -> Halt {
    if matches!(e, VaultError::Database) {
        Halt::Database
    } else {
        Halt::Ends("vault")
    }
}

fn intake_halt(e: IntakeError) -> Halt {
    if matches!(e, IntakeError::Database) {
        Halt::Database
    } else {
        Halt::Ends("intake")
    }
}
