//! The business's money notices: the FEE-020 card-fix notice (FEE-020, BIZ-019, BIZ-037, D-308,
//! D-491, D-492).
//!
//! `payments` cannot call `businesses` or `messaging`, so a decline or a bank approval in
//! `payments` raises the `core.task` event `payments::api::CARD_FIX_KIND` with IDs and the
//! deadline only. [`run_card_fix_notice`] is the `picks` handler on it: it resolves the
//! recipients, the owner and every "Confirm and billing" member, through
//! `businesses::api::notice_addresses` (money purpose) and queues one [`BUSINESS_NOTICE_KIND`]
//! task carrying their opaque address IDs, the job number, the pick (the Confirm link's subject)
//! and the deadline, for `messaging` to send by the business's chosen channels through T094's
//! pipeline. It is safe to run twice (JOB-006): the queued notice has a dedupe key per pick and
//! deadline.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_jobs::api::job_number;
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The `core.task` kind through which `picks` asks `messaging` for a business notice (D-491).
pub const BUSINESS_NOTICE_KIND: &str = "messaging.business_notice";
/// The template of the FEE-020 card-fix notice, in `app/messaging/templates/` (en, es).
pub const CARD_FIX_TEMPLATE: &str = "card_fix_notice";

const DB_FAILED: HandlerError = HandlerError("money notice database statement failed");

fn id(payload: &Value, name: &str) -> Result<Uuid, HandlerError> {
    payload[name]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(HandlerError("card-fix event is missing an ID"))
}

/// The `payments.card_needs_fix` handler (FEE-020, D-492): queues the card-fix notice to the
/// business's money recipients, with the Confirm deadline (1 business hour, or 60 clock minutes
/// on an emergency job, PICK-044) when charges are not paused.
///
/// # Errors
/// A fixed sentence for a malformed event or a failed statement; the queue tries again.
pub async fn run_card_fix_notice(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (pick, business, job) = (
        id(&task.payload, "pick")?,
        id(&task.payload, "business")?,
        id(&task.payload, "job")?,
    );
    let fix_by = task.payload["fix_by"].as_i64();
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let to = notice_addresses(&mut tx, business, NoticePurpose::Money)
        .await
        .map_err(|_| DB_FAILED)?;
    let number = job_number(&mut tx, job).await.map_err(|_| DB_FAILED)?;
    let payload = json!({
        "template": CARD_FIX_TEMPLATE, "business": business, "pick": pick, "to": to,
        "values": {"job": number, "time": fix_by},
    });
    let at = Timestamp::now();
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, at);
    let deadline = fix_by.map_or_else(|| "paused".to_owned(), |s| s.to_string());
    notice.dedupe_key = Some(format!(
        "{BUSINESS_NOTICE_KIND}.{CARD_FIX_TEMPLATE}.{pick}.{deadline}"
    ));
    enqueue(&mut tx, &notice).await.map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}
