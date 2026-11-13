//! START into a still-open request (CON-001, CON-015; T357, D-2629).
//!
//! A homeowner who opted out and texts the provider's START (`messaging`'s
//! `Outcome::Resubscribed`) while her request is still open is a new request as CON-015 says:
//! she gets the consent text again before anything else, and "START" is never heard as an
//! answer to the open question. [`restart_after_start`] queues the request's own consent text
//! once more under the START inbox row, records it as sent (the CON-005 record and the
//! `intake.consent_sent` audit row) and passes nothing on. With no open request it does nothing,
//! and the caller's usual path opens a new request from the consent text.

use askhava_messaging::api::{NewText, Relation, enqueue, render};
use askhava_ops::api::Actor;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, IntakeError};
use crate::rules::{CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, Settings, language_from_code};
use crate::service::{TextIn, audit_action, sha256_hex};

/// CON-001, CON-015, CON-005, TXT-011, D-2629: after the provider's START took contact
/// `msg.contact` off the opt-out list, queues her open request's consent text again (the
/// template and language the request was opened with) in one transaction under the homeowner
/// lock, subject the request and the START inbox row (so a resent delivery queues nothing more),
/// stores one consent record for that inbox row and the `intake.consent_sent` audit row. Answers
/// the outbox row; `None` when she has no open request (nothing written). Never hears the text.
///
/// # Errors
///
/// [`IntakeError::Template`], [`IntakeError::Outbox`], [`IntakeError::Audit`] or
/// [`IntakeError::Database`]; nothing is kept on an error.
pub async fn restart_after_start(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &TextIn<'_>,
    settings: Settings,
) -> Result<Option<Uuid>, IntakeError> {
    let Some(id) = models::open_conversation(db, msg.contact).await? else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let row = models::conversation(&mut tx, id)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, row.homeowner_id).await?;
    let template = if row.consent_template == CONSENT_CALL_TEMPLATE {
        CONSENT_CALL_TEMPLATE
    } else {
        CONSENT_TEMPLATE
    };
    let language = language_from_code(&row.language);
    let body = render(template, language, settings.spanish_conversations_on, &[])
        .map_err(|_| IntakeError::Template)?
        .body;
    let text = NewText {
        template_id: template,
        subject: &[id, msg.inbox_id],
        recipient: msg.contact,
        relation: Relation::OpenRequest,
        body: &body,
    };
    let (outbox, added) = enqueue(&mut tx, secret, &text)
        .await
        .map_err(|_| IntakeError::Outbox)?;
    if added {
        let code = &row.language;
        let version = format!("{template}:{code}:{}", &sha256_hex(&body)[..12]);
        let ids = (row.homeowner_id, id, msg.inbox_id);
        models::record_consent(&mut tx, ids, &version, msg.received_at).await?;
        let actor = Actor::Homeowner(row.homeowner_id);
        audit_action(&mut tx, actor, "intake.consent_sent", id).await?;
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Some(outbox))
}
