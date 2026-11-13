//! The emergency address asked again, and the event once it arrives (INT-027, INT-029, PICK-022;
//! T255, D-1563, D-1739, D-3431).
//!
//! On an emergency job a release that comes before the street address stores one
//! `picks.address_wait` row and queues one `picks.address_needed` event. [`run_address_needed`]
//! (registered by `server`'s `register_handlers`) asks the homeowner the name-and-address question
//! again at once through [`ask_name_address`], once per pick: it first stores one
//! `address_wait` answer row naming the pick on the request that opened the job, and a pick
//! already marked asks nothing. When `take_name_address` has written her answer onto her own
//! contact, it calls [`address_arrived`], which queues one [`ADDRESS_IN_KIND`] event per marked
//! pick of the request (deduped per pick, so a second address message queues nothing new);
//! `server`'s handler re-clears, settles and releases. No address, name or phone is stored or
//! queued here: the answer rows hold the pick ID only.

use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, SystemClock, enqueue};
use askhava_messaging::api::Language;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::conversation::ask_name_address;
use crate::job_texts::job_homeowner;
use crate::models::{self, IntakeError};
use crate::rules::Inbound;
use crate::service::TextIn;

/// The event `take_name_address` queues, through [`address_arrived`], for each pick whose release
/// waited for the address (INT-029, D-1739); payload `{"pick": …}`, the pick ID only.
pub const ADDRESS_IN_KIND: &str = "intake.address_in";
/// The stored question naming a pick whose release waits for the address (INT-029).
const WAIT_MARK: &str = "address_wait";
const BAD_PAYLOAD: &str = "bad picks.address_needed payload";
const FAILED: &str = "the address question could not be asked again";

/// What [`ask_again`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskedAgain {
    /// The question was queued for her now.
    Asked,
    /// This pick was marked before; nothing is asked twice (once per pick).
    Already,
    /// No open request opened the job, or it has no property yet; nothing asked.
    NoRequest,
}

/// The `picks.address_needed` handler (INT-029, TD-INT-34): payload `{"job", "pick"}`, run at
/// the system clock's now. Safe to run twice: the mark makes the second run ask nothing.
///
/// # Errors
/// A [`HandlerError`] for a malformed payload or a failed step (the task is retried).
pub async fn run_address_needed(
    pool: PgPool,
    secret: SecretString,
    task: Claimed,
) -> Result<(), HandlerError> {
    let id = |key: &str| {
        task.payload[key]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
    };
    let (Some(job), Some(pick)) = (id("job"), id("pick")) else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut db = pool.acquire().await.map_err(|_| HandlerError(FAILED))?;
    ask_again(&mut db, &secret, (job, pick), SystemClock.now())
        .await
        .map(|_| ())
        .map_err(|_| HandlerError(FAILED))
}

/// Asks the homeowner of `job`'s request for her first name and street address again at once,
/// once per `pick` (INT-027, INT-029, D-164): in one transaction the pick's `address_wait` mark
/// and [`ask_name_address`]'s text (its outbox subject is the request and the pick), in the
/// request's language.
///
/// # Errors
/// [`IntakeError::Template`], [`IntakeError::Outbox`] or [`IntakeError::Database`]; nothing kept.
pub async fn ask_again(
    db: &mut PgConnection,
    secret: &SecretString,
    (job, pick): (Uuid, Uuid),
    at: Timestamp,
) -> Result<AskedAgain, IntakeError> {
    let Some(home) = job_homeowner(db, job).await? else {
        return Ok(AskedAgain::NoRequest);
    };
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    if waiting_picks(&mut tx, home.conversation)
        .await?
        .contains(&pick)
    {
        return Ok(AskedAgain::Already);
    }
    let hex = pick.simple().to_string();
    models::insert_plain(&mut tx, home.conversation, (WAIT_MARK, &hex), at).await?;
    let msg = TextIn {
        inbox_id: pick,
        contact: home.contact,
        inbound: Inbound {
            words: "",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at,
    };
    let spanish = home.language == Language::Es;
    let asked = ask_name_address(&mut tx, secret, &msg, home.conversation, spanish).await?;
    if asked.is_none() {
        return Ok(AskedAgain::NoRequest);
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(AskedAgain::Asked)
}

/// Queues one [`ADDRESS_IN_KIND`] event per pick of `conversation` whose release waits for the
/// address, in its own transaction (INT-029, D-1739, D-3431); returns how many were new. Its one
/// caller is `take_name_address`, after it stored her answer; a pick's event is deduped for good,
/// so a second answer queues nothing for it.
///
/// # Errors
/// [`IntakeError::Database`]; nothing queued.
pub(crate) async fn address_arrived(
    db: &mut PgConnection,
    conversation: Uuid,
    at: Timestamp,
) -> Result<usize, IntakeError> {
    let mut tx = db.begin().await.map_err(|_| IntakeError::Database)?;
    let mut queued = 0;
    for pick in waiting_picks(&mut tx, conversation).await? {
        let mut task = NewTask::new(ADDRESS_IN_KIND, address_in(pick), at);
        task.dedupe_key = Some(format!("{ADDRESS_IN_KIND}.{pick}"));
        let id = enqueue(&mut tx, &task)
            .await
            .map_err(|_| IntakeError::Database)?;
        queued += usize::from(id.is_some());
    }
    tx.commit().await.map_err(|_| IntakeError::Database)?;
    Ok(queued)
}

/// The [`ADDRESS_IN_KIND`] payload: the pick ID only.
fn address_in(pick: Uuid) -> Value {
    json!({ "pick": pick })
}

/// The picks marked as waiting for the address on `conversation`.
async fn waiting_picks(
    db: &mut PgConnection,
    conversation: Uuid,
) -> Result<Vec<Uuid>, IntakeError> {
    let marks = sqlx::query_scalar!(
        "SELECT choice FROM intake.intake_answer WHERE conversation_id = $1 AND question_id = $2",
        conversation,
        WAIT_MARK
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)?;
    Ok(marks
        .into_iter()
        .flatten()
        .filter_map(|m| Uuid::parse_str(&m).ok())
        .collect())
}
