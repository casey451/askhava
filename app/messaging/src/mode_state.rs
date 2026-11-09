//! The stored texting mode of each audience's line and the paused-emergency follow-up (TXT-021 to
//! TXT-024, TXT-035, INT-004; D-144, D-297, D-303, D-1804, D-1807, D-1843, D-2185 to D-2187).
//!
//! `messaging.mode_state` (migration `0169_messaging_mode_state.sql`) is append-only: each change
//! inserts a whole row for one audience and the line is its latest row; with none the homeowner
//! line is on and the business line off (TXT-035). [`load_lines`] builds `mode::Lines` from it with
//! the configured numbers the caller passes. T108's owner portal calls [`switch_on`],
//! [`switch_off`], [`record_suspension`], [`record_restore`] and [`set_launch_channel`]: the provider
//! announces a suspension by email and console, never by webhook, so Casey records it (automatic
//! detection from send refusals is not built, D-2185). Each change takes the table's advisory lock,
//! writes nothing when it changes nothing, and writes its `ops.audit_log` row in the same
//! transaction.
//!
//! An emergency message while the homeowner line is paused asks [`queue_paused_safety`] for a
//! [`PAUSED_SAFETY_KIND`] task, because the vault refuses a read inside the inbound delivery's
//! transaction (PRIV-003). [`run_paused_safety`] then reads her email on a bare connection and
//! queues the INT-004 text by email when one is on file, else puts her on Casey's call list as a
//! High [`CALL_BACK_ALERT`] (TXT-023, TXT-024), once per inbox message.

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_ops::api::{
    Actor, AuditEntry, Change, NewAlert, Severity, audit, audit_once, raise_alert,
};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};
use thiserror::Error;

use crate::email::{
    EmailClass, MailboxKind, NewEmail, Notice, PostalAddress, Recipient, homeowner_email,
    queue_email,
};
use crate::mode::{LaunchChannel, Line, Lines, ModeError, NumberState, SendingNumber};
use crate::models::EmailError;
use crate::retries::CALL_BACK_ALERT;
use crate::templates::{Audience, Language, Registry};

/// The `core.task` kind of one paused-emergency follow-up: the INT-004 email or the call list.
pub const PAUSED_SAFETY_KIND: &str = "messaging.paused_safety";
/// The email key's template ID of the INT-004 text sent by email while texting is paused.
pub const PAUSED_SAFETY_EMAIL: &str = "paused_safety_email";
/// The INT-004 text's template (`intake::api::rules::SAFETY_TEMPLATE`, D-436).
const INT_004_TEMPLATE: &str = "emergency_911";
/// The `ops.audit_log` action claiming one inbox message's follow-up.
const ROUTED_ACTION: &str = "messaging.paused_safety_routed";
/// The `ops.audit_log` action of one stored mode change.
const CHANGED_ACTION: &str = "messaging.mode_changed";

/// Why a stored mode change failed.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ModeStateError {
    /// The change is refused, such as switching on before the provider's restore (D-144).
    #[error(transparent)]
    Refused(#[from] ModeError),
    /// The database refused or dropped a statement.
    #[error("the mode state statement failed")]
    Database,
}

/// One audience's stored mode: its switch, its number's state and its launch channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stored {
    texting_on: bool,
    state: NumberState,
    launch: LaunchChannel,
}

/// TXT-021, TXT-022, TXT-035: both lines from their latest stored rows and the configured
/// `(homeowner, business)` numbers; a line never changed is on for homeowners, off for businesses.
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn load_lines(
    db: &mut PgConnection,
    (homeowner, business): (Option<SendingNumber>, Option<SendingNumber>),
) -> Result<Lines, ModeStateError> {
    let line = |s: Stored, number| Line {
        texting_on: s.texting_on,
        number,
        state: s.state,
    };
    Ok(Lines {
        homeowner: line(stored(db, Audience::Homeowner).await?, homeowner),
        business: line(stored(db, Audience::Business).await?, business),
    })
}

/// TXT-021, D-297: the stored launch channel of `audience` (SMS when never set).
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn launch_channel(
    db: &mut PgConnection,
    audience: Audience,
) -> Result<LaunchChannel, ModeStateError> {
    Ok(stored(db, audience).await?.launch)
}

/// TXT-021, TXT-024, D-144: Casey (`owner`) switches `audience`'s texting on; refused while the
/// number is suspended, and a restored number becomes working. True when anything changed.
///
/// # Errors
///
/// [`ModeStateError::Refused`] ([`ModeError::NotRestored`]) or [`ModeStateError::Database`].
pub async fn switch_on(
    db: &mut PgConnection,
    audience: Audience,
    (owner, now): (Uuid, Timestamp),
) -> Result<bool, ModeStateError> {
    change(db, audience, (owner, now), |s| {
        let state = s.state.switch_on()?;
        Ok(Stored {
            texting_on: true,
            state,
            ..s
        })
    })
    .await
}

/// TXT-021: Casey switches `audience`'s texting off (web-and-email mode); the other line is
/// untouched (TXT-035). True when it was on.
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn switch_off(
    db: &mut PgConnection,
    audience: Audience,
    (owner, now): (Uuid, Timestamp),
) -> Result<bool, ModeStateError> {
    let off = |s: Stored| {
        Ok(Stored {
            texting_on: false,
            ..s
        })
    };
    change(db, audience, (owner, now), off).await
}

/// TXT-022: Casey records the provider's suspension of `audience`'s number from `since`; one
/// already running keeps its start (`NumberState::suspend`).
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn record_suspension(
    db: &mut PgConnection,
    audience: Audience,
    since: Timestamp,
    (owner, now): (Uuid, Timestamp),
) -> Result<bool, ModeStateError> {
    let suspend = |s: Stored| {
        Ok(Stored {
            state: s.state.suspend(since),
            ..s
        })
    };
    change(db, audience, (owner, now), suspend).await
}

/// TXT-024, D-144: Casey records the provider's confirmation that the number is restored; texting
/// stays off until [`switch_on`].
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn record_restore(
    db: &mut PgConnection,
    audience: Audience,
    (owner, now): (Uuid, Timestamp),
) -> Result<bool, ModeStateError> {
    let restore = |s: Stored| {
        Ok(Stored {
            state: s.state.provider_restored(),
            ..s
        })
    };
    change(db, audience, (owner, now), restore).await
}

/// TXT-021, D-297: Casey sets `audience`'s launch channel; web-and-email also switches its texting
/// off, while SMS leaves the switch to [`switch_on`].
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn set_launch_channel(
    db: &mut PgConnection,
    audience: Audience,
    launch: LaunchChannel,
    (owner, now): (Uuid, Timestamp),
) -> Result<bool, ModeStateError> {
    let set = |s: Stored| {
        Ok(Stored {
            texting_on: s.texting_on && launch == LaunchChannel::Sms,
            launch,
            ..s
        })
    };
    change(db, audience, (owner, now), set).await
}

/// Applies `f` to `audience`'s latest stored mode under the table's advisory lock and appends the
/// result with its audit row, in one transaction; nothing is written when `f` changes nothing.
async fn change(
    db: &mut PgConnection,
    audience: Audience,
    (owner, now): (Uuid, Timestamp),
    f: impl FnOnce(Stored) -> Result<Stored, ModeError>,
) -> Result<bool, ModeStateError> {
    let mut tx = db.begin().await.map_err(|_| ModeStateError::Database)?;
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtextextended('messaging.mode_state', 0))) AS lock"#
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ModeStateError::Database)?;
    let now_stored = stored(&mut tx, audience).await?;
    let next = f(now_stored)?;
    if next == now_stored {
        return Ok(false);
    }
    let id = insert(&mut tx, audience, next, (owner, now)).await?;
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: CHANGED_ACTION,
        subject_kind: "messaging.mode_state",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| ModeStateError::Database)?;
    tx.commit().await.map_err(|_| ModeStateError::Database)?;
    Ok(true)
}

/// The stored column value of an audience.
fn audience_code(audience: Audience) -> &'static str {
    match audience {
        Audience::Homeowner => "homeowner",
        Audience::Business => "business",
    }
}

/// `audience`'s latest stored mode, or its default.
async fn stored(db: &mut PgConnection, audience: Audience) -> Result<Stored, ModeStateError> {
    let row = sqlx::query!(
        "SELECT texting_on, line_state, suspended_since, launch_channel \
         FROM messaging.mode_state WHERE audience = $1 ORDER BY seq DESC LIMIT 1",
        audience_code(audience)
    )
    .fetch_optional(db)
    .await
    .map_err(|_| ModeStateError::Database)?;
    let Some(row) = row else {
        return Ok(Stored {
            texting_on: audience == Audience::Homeowner,
            state: NumberState::Working,
            launch: LaunchChannel::Sms,
        });
    };
    let since = row.suspended_since.map(jiff_sqlx::Timestamp::to_jiff);
    let state = match (row.line_state.as_str(), since) {
        ("suspended", Some(since)) => NumberState::Suspended { since },
        ("restored", Some(since)) => NumberState::Restored { since },
        ("working", None) => NumberState::Working,
        _ => return Err(ModeStateError::Database),
    };
    let launch = match row.launch_channel.as_str() {
        "web_and_email" => LaunchChannel::WebAndEmail,
        _ => LaunchChannel::Sms,
    };
    Ok(Stored {
        texting_on: row.texting_on,
        state,
        launch,
    })
}

/// Appends `s` for `audience`; returns the new row's ID.
async fn insert(
    db: &mut PgConnection,
    audience: Audience,
    s: Stored,
    (owner, now): (Uuid, Timestamp),
) -> Result<Uuid, ModeStateError> {
    let (state, since) = match s.state {
        NumberState::Working => ("working", None),
        NumberState::Suspended { since } => ("suspended", Some(since.to_sqlx())),
        NumberState::Restored { since } => ("restored", Some(since.to_sqlx())),
    };
    let launch = match s.launch {
        LaunchChannel::Sms => "sms",
        LaunchChannel::WebAndEmail => "web_and_email",
    };
    sqlx::query_scalar!(
        "INSERT INTO messaging.mode_state (audience, texting_on, line_state, suspended_since, \
         launch_channel, actor_id, changed_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        audience_code(audience),
        s.texting_on,
        state,
        since as _,
        launch,
        owner,
        now.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| ModeStateError::Database)
}

/// TXT-023, TXT-024: in the caller's transaction, asks once per inbox `message` for homeowner
/// `her`'s paused-emergency follow-up in `language` (the INT-004 email or the call list).
///
/// # Errors
///
/// [`ModeStateError::Database`].
pub async fn queue_paused_safety(
    db: &mut PgConnection,
    (her, message): (Uuid, Uuid),
    language: Language,
    now: Timestamp,
) -> Result<(), ModeStateError> {
    let code = if language == Language::Es { "es" } else { "en" };
    let payload = json!({ "contact": her, "message": message, "language": code });
    let mut task = NewTask::new(PAUSED_SAFETY_KIND, payload, now);
    task.dedupe_key = Some(format!("{PAUSED_SAFETY_KIND}.{message}"));
    enqueue(db, &task)
        .await
        .map(|_| ())
        .map_err(|_| ModeStateError::Database)
}

/// The email key's secret, the checked footer postal line, the vault and the homeowner's
/// "Manage email notices" link for the [`PAUSED_SAFETY_KIND`] handler (SEC-018, SEC-028).
pub struct SafetySender {
    secret: SecretString,
    postal: PostalAddress,
    vault: Vault,
    manage_url: String,
}

impl SafetySender {
    /// Checks `postal` as `kind` of mailbox (SEC-028).
    ///
    /// # Errors
    ///
    /// [`EmailError::PostalAddress`].
    pub fn new(
        (secret, vault): (SecretString, Vault),
        (postal, kind): (&str, MailboxKind),
        manage_url: &str,
    ) -> Result<Self, EmailError> {
        Ok(Self {
            secret,
            postal: PostalAddress::new(postal, kind)?,
            vault,
            manage_url: manage_url.to_owned(),
        })
    }
}

/// TXT-023, TXT-024, INT-004, D-2186: the [`PAUSED_SAFETY_KIND`] handler. Reads her email on a bare
/// connection (any vault failure counts as none, so she is called rather than missed), then in one
/// transaction claims the inbox message once and queues the INT-004 text by email, or raises the
/// High [`CALL_BACK_ALERT`] that puts her on Casey's call list. A second run writes nothing.
///
/// # Errors
///
/// A fixed sentence for a malformed payload or a failed statement; the queue tries again.
pub async fn run_paused_safety(
    pool: PgPool,
    sender: Arc<SafetySender>,
    task: Claimed,
) -> Result<(), HandlerError> {
    const FAILED: HandlerError = HandlerError("paused safety follow-up could not be stored");
    let ids = |k: &str| {
        task.payload[k]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
    };
    let (Some(her), Some(message)) = (ids("contact"), ids("message")) else {
        return Err(HandlerError("paused safety event is malformed"));
    };
    let language = match task.payload["language"].as_str() {
        Some("es") => Language::Es,
        _ => Language::En,
    };
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    let on_file = homeowner_email(&mut conn, &sender.vault, her)
        .await
        .is_ok_and(|e| e.is_some());
    let mut tx = conn.begin().await.map_err(|_| FAILED)?;
    let claim = AuditEntry {
        actor: Actor::System,
        action: ROUTED_ACTION,
        subject_kind: "messaging.inbox",
        subject_id: message,
        change: Change::Action,
    };
    if !audit_once(&mut tx, &claim).await.map_err(|_| FAILED)? {
        return Ok(());
    }
    if on_file {
        safety_email(&mut tx, &sender, (her, message), language)
            .await
            .map_err(|_| FAILED)?;
    } else {
        let alert = NewAlert {
            kind: CALL_BACK_ALERT,
            severity: Severity::High,
            subject: Some(("messaging.inbox", message)),
        };
        raise_alert(&mut tx, &alert).await.map_err(|_| FAILED)?;
    }
    tx.commit().await.map_err(|_| FAILED)
}

/// Queues the INT-004 text, in `language` (its Spanish stays on), to homeowner `her` by email as
/// the existing text-by-email notice (D-2186), keyed on the inbox `message`.
async fn safety_email(
    db: &mut PgConnection,
    sender: &SafetySender,
    (her, message): (Uuid, Uuid),
    language: Language,
) -> Result<(), EmailError> {
    let registry = Registry::embedded().map_err(|_| EmailError::Template)?;
    let text = registry
        .render(INT_004_TEMPLATE, language, true, &[])
        .map_err(|_| EmailError::Template)?;
    let to = Recipient {
        id: her,
        audience: Audience::Homeowner,
        language,
        manage_url: &sender.manage_url,
    };
    let email = NewEmail {
        template_id: PAUSED_SAFETY_EMAIL,
        subject: &[message],
        class: EmailClass::Notice,
        notice: Notice::TextFailed(&text.body),
    };
    let keys = (&sender.secret, &sender.postal);
    queue_email(db, keys, &to, &email, Timestamp::now()).await?;
    Ok(())
}
