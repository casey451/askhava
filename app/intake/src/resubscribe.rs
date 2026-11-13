//! START into a still-open request (CON-001, CON-015; T357, D-2629).
//!
//! A homeowner who opted out and texts the provider's START (`messaging`'s
//! `Outcome::Resubscribed`) while her request is still open is a new request as CON-015 says:
//! she gets the consent text again before anything else, and "START" is never heard as an
//! answer to the open question. [`restart_after_start`] queues the request's own consent text
//! once more under the START inbox row, records it as sent (the CON-005 record and the
//! `intake.consent_sent` audit row) and passes nothing on. With no open request it does nothing,
//! and the caller's usual path opens a new request from the consent text.
//!
//! [`pause_on_repeat_cancel`] (D-3480) is the other half of an opt-out from a number already
//! listed: a sole CANCEL or CANCELAR still pauses her live jobs and withdraws a waiting pick as
//! `on_opt_out` does, but queues no `intake.opted_out_live_job`.

use askhava_jobs::api::current;
use askhava_messaging::api::{NewText, Relation, enqueue, render};
use askhava_ops::api::Actor;
use askhava_picks::api::withdraw_paused;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, IntakeError};
use crate::rules::{CONSENT_CALL_TEMPLATE, CONSENT_TEMPLATE, Settings, language_from_code};
use crate::service::{OptOutJobs, TextIn, audit_action, sha256_hex, sole_cancel};

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

/// A sole-word CANCEL or CANCELAR from a number an earlier opt-out already listed (CON-029,
/// D-3480): pauses her live jobs and withdraws a waiting pick as `on_opt_out` does (same
/// `intake.homeowner_pause` rows, `intake.paused_by_homeowner` audit row and
/// `picks::withdraw_paused`), but enqueues no `intake.opted_out_live_job` task, so the earlier
/// opt-out's "still open" email is not sent twice (`task` is `None`). Any other text, no open
/// request or no live job does nothing (`None`).
///
/// # Errors
///
/// [`IntakeError::Job`], [`IntakeError::Audit`] or [`IntakeError::Database`].
pub async fn pause_on_repeat_cancel(
    db: &mut PgConnection,
    msg: &TextIn<'_>,
) -> Result<Option<OptOutJobs>, IntakeError> {
    if !msg.inbound.opt_out || !sole_cancel(msg.inbound.words) {
        return Ok(None);
    }
    let Some(conversation) = models::open_conversation(db, msg.contact).await? else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let (homeowner, _) = models::conversation_state(&mut tx, conversation)
        .await?
        .ok_or(IntakeError::UnknownRequest)?;
    models::lock_homeowner(&mut tx, homeowner).await?;
    let jobs = live_jobs(&mut tx, conversation).await?;
    if jobs.is_empty() {
        return Ok(None);
    }
    let mut inserted = false;
    for job in &jobs {
        let ids = (conversation, *job);
        inserted |= models::insert_pause(&mut tx, ids, msg.inbox_id, msg.received_at).await?;
    }
    let actor = Actor::Homeowner(homeowner);
    if inserted {
        audit_action(&mut tx, actor, "intake.paused_by_homeowner", conversation).await?;
    }
    withdraw_paused(&mut tx, &jobs, actor, msg.received_at)
        .await
        .map_err(|_| IntakeError::Database)?;
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(Some(OptOutJobs {
        conversation,
        jobs,
        paused: true,
        task: None,
    }))
}

/// The request's jobs before a final status (CON-028's "live job"), as `on_opt_out` reads them.
async fn live_jobs(db: &mut PgConnection, conversation: Uuid) -> Result<Vec<Uuid>, IntakeError> {
    let mut live = Vec::new();
    for job in models::request_jobs(db, conversation).await? {
        let status = current(db, job).await.map_err(|_| IntakeError::Job)?.status;
        if !status.is_final() {
            live.push(job);
        }
    }
    Ok(live)
}
