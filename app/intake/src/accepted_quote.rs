//! The `picks.accepted_quote` handler: the homeowner's copy of the quote she accepted, by email
//! after the Confirm (PICK-042, PICK-054, D-244, D-493, T388, D-3074, D-3077).
//!
//! `picks::api::settle` queues the task (pick, job and business IDs only) in the transaction that
//! moves the pick to `paid` or `free`. [`run_accepted_quote`] checks the pick is still the job's
//! `paid` or `free` one (`picks::api::accepted_pick`), finds her contact and language
//! (`job_homeowner`), the accepted quote's words (`quotes::api::accepted_quote`) and the request
//! number, and hands them to `messaging::api::queue_accepted_quote`, which stores the email only
//! when an email is on file. A pick that moved on, a job no open request opened, a quote that no
//! longer reads, or no email on file sends nothing and the task ends done; a second run stores
//! nothing new (JOB-006). Without a sender (no footer postal setting yet, D-3077) it sends nothing.

use std::sync::Arc;

use askhava_core::api::{Claimed, HandlerError};
use askhava_jobs::api::job_number;
use askhava_messaging::api::{
    AcceptedQuoteFacts, AcceptedQuoteSender, Language, queue_accepted_quote,
};
use askhava_picks::api::accepted_pick;
use askhava_quotes::api::accepted_quote;
use jiff::Timestamp;
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::job_texts::job_homeowner;

const FAILED: HandlerError = HandlerError("the accepted-quote email step failed");

/// The task's pick, job and business, parsed once.
fn parse(payload: &Value) -> Option<(Uuid, Uuid, Uuid)> {
    let id = |name: &str| payload[name].as_str().and_then(|v| Uuid::parse_str(v).ok());
    Some((id("pick")?, id("job")?, id("business")?))
}

/// PICK-042, D-493, D-3074: the `picks.accepted_quote` handler. Stores one accepted-quote email in
/// her language when the pick is still `paid` or `free` and an email is on file; otherwise
/// nothing, and the task is done. `sender` is `None` until the footer postal setting exists
/// (D-3077): nothing is sent then.
///
/// # Errors
/// A fixed sentence for a malformed payload, or when a read or the store fails (the queue tries
/// again); never a value.
pub async fn run_accepted_quote(
    pool: sqlx::PgPool,
    sender: Option<Arc<AcceptedQuoteSender>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let (pick, job, business) =
        parse(&task.payload).ok_or(HandlerError("accepted-quote payload is malformed"))?;
    let Some(sender) = sender else {
        return Ok(());
    };
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let accepted = accepted_pick(&mut db, job).await.map_err(|_| FAILED)?;
    if accepted.is_none_or(|a| a.pick != pick || a.business != business) {
        return Ok(());
    }
    let Some(her) = job_homeowner(&mut db, job).await.map_err(|_| FAILED)? else {
        return Ok(());
    };
    send(
        &mut db,
        &sender,
        (pick, job, business),
        (her.contact, her.language),
    )
    .await
}

/// Reads the quote's words and the request number and stores the email (PICK-042).
async fn send(
    db: &mut PgConnection,
    sender: &AcceptedQuoteSender,
    (pick, job, business): (Uuid, Uuid, Uuid),
    to: (Uuid, Language),
) -> Result<(), HandlerError> {
    let quote = accepted_quote(db, (job, business))
        .await
        .map_err(|_| FAILED)?;
    let Some(quote) = quote else {
        return Ok(());
    };
    let job_number = job_number(db, job).await.map_err(|_| FAILED)?;
    let es = to.1 == Language::Es;
    let (amount, visit) = (quote.amount(es), quote.visit(es));
    let facts = AcceptedQuoteFacts {
        pick,
        job_number,
        number: quote.number,
        kind: quote.kind(es),
        amount: &amount,
        visit: visit.as_deref(),
        words: &quote.original,
    };
    queue_accepted_quote(db, sender, to, &facts, Timestamp::now())
        .await
        .map(|_| ())
        .map_err(|_| FAILED)
}
