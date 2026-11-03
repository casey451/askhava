//! The business's money notices: the FEE-020 card-fix notice (FEE-020, BIZ-019, BIZ-037, D-308,
//! D-491, D-492).
//!
//! `payments` cannot call `businesses` or `messaging`, so a decline or a bank approval in
//! `payments` raises the `core.task` event `payments::api::CARD_FIX_KIND` with IDs, the attempt
//! number and whether the bank must approve. [`run_card_fix_notice`] is the `picks` handler on
//! it: it first asks `payments::api::card_fix_window` whether the pick still needs a fix (a pick
//! paid or ended since, for example at the window's own end, gets no notice), then resolves the
//! recipients, the owner and every "Confirm and billing" member, through
//! `businesses::api::notice_addresses` (money purpose) and queues one [`BUSINESS_NOTICE_KIND`]
//! task carrying their opaque address IDs, the job number, the pick (the Confirm link's subject)
//! and the window's current deadline, for `messaging` to send by the business's chosen channels
//! through T094's pipeline. A declined card uses [`CARD_FIX_TEMPLATE`], a card waiting on the
//! bank [`CARD_APPROVAL_TEMPLATE`]. It is safe to run twice (JOB-006): the queued notice has a
//! dedupe key per pick and attempt.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_jobs::api::job_number;
use askhava_payments::api::{CardFixWindow, card_fix_window};
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The `core.task` kind through which `picks` asks `messaging` for a business notice (D-491).
pub const BUSINESS_NOTICE_KIND: &str = "messaging.business_notice";
/// The template of the FEE-020 notice for a declined card, in `app/messaging/templates/` (en, es).
pub const CARD_FIX_TEMPLATE: &str = "card_fix_notice";
/// The template of the FEE-020 notice for a card the bank must approve (3-D Secure, en, es).
pub const CARD_APPROVAL_TEMPLATE: &str = "card_approval_notice";

const DB_FAILED: HandlerError = HandlerError("money notice database statement failed");

fn id(payload: &Value, name: &str) -> Result<Uuid, HandlerError> {
    payload[name]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(HandlerError("card-fix event is missing an ID"))
}

/// The `payments.card_needs_fix` handler (FEE-020, D-492): while the pick still needs a fix,
/// queues the card-fix or card-approval notice to the business's money recipients, with the
/// window's deadline (1 business hour, or 60 clock minutes on an emergency job, PICK-044), none
/// while charges are paused; a pick paid or ended since gets nothing.
///
/// # Errors
/// A fixed sentence for a malformed event or a failed statement; the queue tries again.
pub async fn run_card_fix_notice(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (pick, business, job) = (
        id(&task.payload, "pick")?,
        id(&task.payload, "business")?,
        id(&task.payload, "job")?,
    );
    let try_no = task.payload["try"].as_i64().unwrap_or(0);
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let fix_by = match card_fix_window(&mut tx, pick)
        .await
        .map_err(|_| DB_FAILED)?
    {
        CardFixWindow::Closed => return Ok(()),
        CardFixWindow::Running(due) => Some(due.as_second()),
        CardFixWindow::Stopped => None,
    };
    let to = notice_addresses(&mut tx, business, NoticePurpose::Money)
        .await
        .map_err(|_| DB_FAILED)?;
    let number = job_number(&mut tx, job).await.map_err(|_| DB_FAILED)?;
    let template = if task.payload["approval"].as_bool() == Some(true) {
        CARD_APPROVAL_TEMPLATE
    } else {
        CARD_FIX_TEMPLATE
    };
    let payload = json!({
        "template": template, "business": business, "pick": pick, "to": to,
        "values": {"job": number, "time": fix_by},
    });
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, Timestamp::now());
    notice.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{template}.{pick}.{try_no}"));
    enqueue(&mut tx, &notice).await.map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}
