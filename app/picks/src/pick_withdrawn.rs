//! The withdrawn-pick notice to the business (PICK-032, CON-029, BIZ-037; D-2288; T330).
//!
//! [`run_pick_withdrawn`] is the handler on `picks.pick_withdrawn`, which `service::end` queues in
//! the transaction that cancels a waiting pick as `replaced` (a second "<number> YES" or a page
//! re-pick, PICK-032) or `paused` (the homeowner's sole-word CANCEL, CON-029). It resolves the
//! owner and every "Confirm and billing" member through `businesses::api::notice_addresses`
//! (picks purpose, BIZ-037) and queues one `messaging.business_notice` task with their opaque
//! address IDs and the job number, deduped per pick, so a repeat or a retried event sends nothing
//! more. The cancel is final (`cancelled` never moves on), so the event's own facts still hold
//! when it runs. The words are [`WITHDRAWN_TEMPLATE`]; no homeowner detail goes in the payload.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_jobs::api::job_number;
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::money_notices::BUSINESS_NOTICE_KIND;

/// The withdrawn-pick notice's template (`pick_withdrawn`, en and es; PICK-032, CON-029).
pub const WITHDRAWN_TEMPLATE: &str = "pick_withdrawn";

const DB_FAILED: HandlerError = HandlerError("withdrawn pick notice database statement failed");
const BAD_EVENT: HandlerError = HandlerError("withdrawn pick event is malformed");

fn id(payload: &Value, name: &str) -> Result<Uuid, HandlerError> {
    payload[name]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(BAD_EVENT)
}

/// The `picks.pick_withdrawn` handler (PICK-032, CON-029, BIZ-037, D-2288): tells the business
/// whose waiting pick was withdrawn (`reason` `replaced` or `paused`), once per pick, through one
/// `messaging.business_notice` task to its owner and "Confirm and billing" members. Any other
/// `reason` is a malformed event.
///
/// # Errors
/// A fixed sentence for a malformed event or a failed statement; the queue tries again.
pub async fn run_pick_withdrawn(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let p = &task.payload;
    let (pick, business, job) = (id(p, "pick")?, id(p, "business")?, id(p, "job")?);
    if !matches!(p["reason"].as_str(), Some("replaced" | "paused")) {
        return Err(BAD_EVENT);
    }
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let to = notice_addresses(&mut tx, business, NoticePurpose::PicksAndRelease)
        .await
        .map_err(|_| DB_FAILED)?;
    let number = job_number(&mut tx, job).await.map_err(|_| DB_FAILED)?;
    let payload = json!({
        "template": WITHDRAWN_TEMPLATE, "business": business, "pick": pick, "to": to,
        "values": {"job": number},
    });
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, Timestamp::now());
    notice.dedupe_key = Some(format!(
        "{BUSINESS_NOTICE_KIND}.{WITHDRAWN_TEMPLATE}.{pick}"
    ));
    enqueue(&mut tx, &notice).await.map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}
