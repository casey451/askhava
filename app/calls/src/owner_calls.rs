//! Calls to Casey's own phone about a critical safety alert (T484; OPS-028, OPS-035, CALL-006;
//! D-2762, D-3753, D-3880, D-3890).
//!
//! Two calls, each from a pool number, never the main number, and never showing Casey's number:
//!
//! - OPS-035's automated call ([`place_owner_alert_call`]): one say-only call per alert, keyed by
//!   the alert ID, naming the alert kind and the job number ([`owner_alert_words`]) and holding no
//!   homeowner detail. It is placed at any hour and while the texting number is suspended: it goes
//!   to Casey, not a homeowner, so no quiet hours or suppression list apply.
//! - OPS-028's bridge ([`bridge_owner`]): the owner portal's call press ("Call from the" brand "number").
//!   It reads her phone inside `calls` under `(Reader::Calls, Purpose::BridgeCall)` and calls
//!   [`ring_owner`], which reads Casey the INT-004 words and dials her with the pool number shown.
//!
//! [`alert_facts`] gives both of `server`'s handlers the alert's kind and, for an inbound-text
//! subject, the contact ID (`messaging::api::alert_contact`). The contact ID stays inside the
//! handler and this crate: no page, payload or log holds it.

use askhava_adapters::api::{CallPlan, IdempotencyKey, PoolNumber, VoiceAdapter};
use askhava_core::api::brand;
use askhava_messaging::api::alert_contact;
use askhava_ops::api::alert_subject;
use askhava_privacy::api::{Field, Purpose, Reader, Vault, read_contact};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::CallsError;
use crate::routing::xml_escape;
use crate::voice::{SAFETY_STATUS_PATH, VoiceError, ring_owner};

/// OPS-028, D-3753: the task kind of one owner-portal call press (alert and press IDs).
pub const OWNER_BRIDGE_KIND: &str = "calls.owner_bridge";

/// OPS-035: the idempotency scope of the automated call, the kind T204 queues it under.
const ALERT_CALL_SCOPE: &str = "calls.owner_alert_call";

/// What a call to Casey dials with: the voice client, the `ID_SECRET` for its key, Casey's phone
/// (`ASKHAVA_OWNER_PHONE`), the pool number shown and the public base URL of its status callback.
pub struct OwnerDialer<'a, V> {
    /// The voice provider client.
    pub voice: &'a V,
    /// The `ID_SECRET` the idempotency key derives from.
    pub secret: &'a SecretString,
    /// Casey's own phone, dialled and never shown.
    pub owner_phone: &'a SecretString,
    /// The pool number the call comes from.
    pub from: &'a PoolNumber,
    /// The app's public origin.
    pub public_base: &'a str,
}

/// D-3890: an alert's kind and, when its subject is a `messaging.inbox` text with a recorded
/// sender, that sender's contact ID; `None` for an unknown alert.
///
/// # Errors
///
/// [`CallsError::Ops`] or [`CallsError::Messaging`] for a failed read.
pub async fn alert_facts(
    db: &mut PgConnection,
    alert: Uuid,
) -> Result<Option<(String, Option<Uuid>)>, CallsError> {
    let Some(subject) = alert_subject(db, alert)
        .await
        .map_err(|_| CallsError::Ops)?
    else {
        return Ok(None);
    };
    let inbox = match (subject.subject_kind.as_deref(), subject.subject_id) {
        (Some("messaging.inbox"), Some(id)) => id,
        _ => return Ok(Some((subject.kind, None))),
    };
    let contact = alert_contact(db, inbox)
        .await
        .map_err(|_| CallsError::Messaging)?;
    Ok(Some((subject.kind, contact)))
}

/// OPS-035: the automated call's words: the brand, "critical safety alert", the job number (none
/// when she has no job) and the alert kind's code name spoken as words, never a homeowner detail.
#[must_use]
pub fn owner_alert_words(kind: &str, job: Option<i64>) -> String {
    let spoken = kind.replace(['.', '_'], " ");
    let job = job.map_or_else(|| ", no job number".to_owned(), |n| format!(" on job {n}"));
    format!(
        "This is {}. Critical safety alert{job}. Alert type: {spoken}. Open the owner portal now.",
        brand().display_name()
    )
}

/// OPS-035: the say-only script: [`owner_alert_words`] twice, then a hang-up; nothing gathered,
/// nothing dialled, never recorded.
#[must_use]
pub fn owner_alert_twiml(words: &str) -> SecretString {
    let words = xml_escape(words);
    SecretString::from(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Say loop=\"2\">{words}</Say>\
         <Hangup/></Response>"
    ))
}

/// OPS-035, D-3880: rings Casey's phone once for `alert` (the key is the alert ID, so a rerun is
/// the provider's same call) with [`owner_alert_twiml`] for the alert `kind` and `job` number. Its
/// status goes to [`SAFETY_STATUS_PATH`]`?owner_alert=<alert>`, which changes nothing.
///
/// # Errors
///
/// [`VoiceError::Script`] for a status URL the plan refuses, else [`VoiceError::Provider`].
pub async fn place_owner_alert_call<V: VoiceAdapter>(
    dialer: &OwnerDialer<'_, V>,
    alert: Uuid,
    (kind, job): (&str, Option<i64>),
) -> Result<String, VoiceError> {
    let status = format!(
        "{}{SAFETY_STATUS_PATH}?owner_alert={alert}",
        dialer.public_base
    );
    let twiml = owner_alert_twiml(&owner_alert_words(kind, job));
    let plan = CallPlan::new(twiml, status).ok_or(VoiceError::Script)?;
    let id = alert.to_string();
    let key = IdempotencyKey::derive(dialer.secret, ALERT_CALL_SCOPE, &[&id]);
    let state = dialer
        .voice
        .open_call(&key, dialer.owner_phone.expose_secret(), dialer.from, &plan)
        .await
        .map_err(|_| VoiceError::Provider)?;
    Ok(state.sid)
}

/// OPS-028, PRIV-003, D-3890: the press's bridge: reads `contact`'s phone under
/// `(Reader::Calls, Purpose::BridgeCall)`, then [`ring_owner`] for `(alert, press)` with the
/// INT-004 `safety_words`. Call it outside a transaction (the vault read).
///
/// # Errors
///
/// [`VoiceError::Calls`] when her phone cannot be read, else as [`ring_owner`].
pub async fn bridge_owner<V: VoiceAdapter>(
    db: &mut PgConnection,
    vault: &Vault,
    dialer: &OwnerDialer<'_, V>,
    (alert, press, contact): (Uuid, Uuid, Uuid),
    safety_words: &str,
) -> Result<String, VoiceError> {
    let access = (Reader::Calls, Purpose::BridgeCall);
    let details = read_contact(db, vault, access, contact, &[Field::Phone])
        .await
        .map_err(|_| CallsError::Messaging)?;
    let her = details.phone.ok_or(CallsError::Messaging)?;
    let who = (dialer.owner_phone, dialer.from);
    let base = dialer.public_base;
    ring_owner(
        dialer.voice,
        dialer.secret,
        who,
        (alert, press),
        (&her, safety_words),
        base,
    )
    .await
}
