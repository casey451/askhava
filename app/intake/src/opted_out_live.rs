//! The handler on `intake.opted_out_live_job` (CON-028, CON-029; D-129, D-372, D-2288; T330).
//!
//! `service::on_opt_out` queues the event when a homeowner with a live job opts out (payload
//! `conversation`, `jobs`, `paused`, `inbox`). [`run_opted_out_live`] then, for a sole-word CANCEL
//! or CANCELAR (`paused`), withdraws through `picks::api::withdraw_paused` the waiting pick of each
//! job that is still paused, under the homeowner lock in one transaction; `on_opt_out` already did
//! so in its own transaction, so this repeat finds nothing unless a pick waits again, and a job
//! whose pause has ended keeps its pick (CON-029). Then, only with an email on file, it stores one
//! CON-028 email per job still live: "Your request <number> is still open. Close it on your quote
//! page." (`messaging`'s `Notice::StillOpen`), keyed on the inbox row and the job, so a rerun stores
//! nothing more. It never texts: the opt-out confirmation stays bare (CON-009). The quote page reads
//! the stored state itself. The email read runs outside any transaction (the vault refuses one).

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError};
use askhava_jobs::api::{current, job_number};
use askhava_messaging::api::{
    Audience, EmailClass, EmailError, MailboxKind, NewEmail, Notice, OnDomain, PostalAddress,
    Recipient, homeowner_has_email, queue_email,
};
use askhava_ops::api::Actor;
use askhava_picks::api::withdraw_paused;
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::job_texts::paused_under_lock;
use crate::models;
use crate::rules::language_from_code;

/// The CON-028 email's template ID, part of its key (`email.send`).
pub const STILL_OPEN_EMAIL: &str = "still_open_email";

/// Her portal page, the email's "Manage email notices" link.
const MANAGE: OnDomain = OnDomain("/h");

const FAILED: HandlerError = HandlerError("the opted-out live job step failed");
const MALFORMED: HandlerError = HandlerError("opted-out live job payload is malformed");

/// What the CON-028 email needs: the `ID_SECRET` (its key), the footer's checked postal line and
/// the vault, through which `messaging` alone reads whether she has an email (PRIV-004).
pub struct StillOpenSender {
    secret: SecretString,
    postal: PostalAddress,
    vault: Vault,
}

impl StillOpenSender {
    /// A sender with a checked postal line (`kind`: a P.O. box or private mailbox, SEC-028).
    ///
    /// # Errors
    /// The postal line's refusal.
    pub fn new(
        secret: SecretString,
        (postal, kind): (&str, MailboxKind),
        vault: Vault,
    ) -> Result<Self, EmailError> {
        Ok(Self {
            secret,
            postal: PostalAddress::new(postal, kind)?,
            vault,
        })
    }
}

/// The event's fields, parsed once.
struct Event {
    conversation: Uuid,
    jobs: Vec<Uuid>,
    paused: bool,
    inbox: Uuid,
}

fn parse(p: &Value) -> Option<Event> {
    let id = |v: &Value| v.as_str().and_then(|s| Uuid::parse_str(s).ok());
    let jobs = p["jobs"]
        .as_array()?
        .iter()
        .map(id)
        .collect::<Option<_>>()?;
    Some(Event {
        conversation: id(&p["conversation"])?,
        jobs,
        paused: p["paused"].as_bool()?,
        inbox: id(&p["inbox"])?,
    })
}

/// CON-028, CON-029, D-129, D-2288: the `intake.opted_out_live_job` handler. A CANCEL's waiting
/// picks are withdrawn first (each business told by `picks`), then the "still open" email goes
/// by email only; `sender` is `None` until the footer postal setting and both vault keys exist,
/// when no email is stored. Safe to run twice.
///
/// # Errors
/// A fixed sentence for a malformed payload or a failed step (the queue tries again); never a
/// value.
pub async fn run_opted_out_live(
    pool: PgPool,
    sender: Option<Arc<StillOpenSender>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let event = parse(&task.payload).ok_or(MALFORMED)?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    if event.paused {
        withdraw_still_paused(&mut db, &event).await?;
    }
    match sender {
        Some(sender) => still_open_emails(&mut db, &sender, &event).await,
        None => Ok(()),
    }
}

/// CON-029: under the homeowner lock, withdraws the waiting pick of each event job still paused.
async fn withdraw_still_paused(db: &mut PgConnection, event: &Event) -> Result<(), HandlerError> {
    let mut tx = db.begin().await.map_err(|_| FAILED)?;
    let (homeowner, _) = models::conversation_state(&mut tx, event.conversation)
        .await
        .map_err(|_| FAILED)?
        .ok_or(MALFORMED)?;
    let mut paused = Vec::new();
    for &job in &event.jobs {
        if paused_under_lock(&mut tx, event.conversation, job)
            .await
            .map_err(|_| FAILED)?
        {
            paused.push(job);
        }
    }
    withdraw_paused(
        &mut tx,
        &paused,
        Actor::Homeowner(homeowner),
        Timestamp::now(),
    )
    .await
    .map_err(|_| FAILED)?;
    tx.commit().await.map_err(|_| FAILED)
}

/// CON-028: with an email on file, one "still open" email per event job not yet final, in her
/// language, keyed on the inbox row and the job.
async fn still_open_emails(
    db: &mut PgConnection,
    sender: &StillOpenSender,
    event: &Event,
) -> Result<(), HandlerError> {
    let Some((contact, language)) = models::recipient(db, event.conversation)
        .await
        .map_err(|_| FAILED)?
    else {
        return Ok(());
    };
    if !homeowner_has_email(db, &sender.vault, contact)
        .await
        .map_err(|_| FAILED)?
    {
        return Ok(());
    }
    let manage = MANAGE.to_string();
    let to = Recipient {
        id: contact,
        audience: Audience::Homeowner,
        language: language_from_code(&language),
        manage_url: &manage,
    };
    for &job in &event.jobs {
        if current(db, job)
            .await
            .map_err(|_| FAILED)?
            .status
            .is_final()
        {
            continue;
        }
        let number = job_number(db, job).await.map_err(|_| FAILED)?;
        let subject = [event.inbox, job];
        let email = NewEmail {
            template_id: STILL_OPEN_EMAIL,
            subject: &subject,
            class: EmailClass::Notice,
            notice: Notice::StillOpen(number),
        };
        let keys = (&sender.secret, &sender.postal);
        queue_email(db, keys, &to, &email, Timestamp::now())
            .await
            .map_err(|_| FAILED)?;
    }
    Ok(())
}
