//! The EMERGENCY job card notice and who gets it when (CARD-021, BIZ-007, BIZ-037, QH-004,
//! QH-005, INT-029, INT-030; T150, D-344, D-346, D-492).
//!
//! For a job the homeowner chose as an emergency, [`queue_emergency_card`] queues the card notice
//! by email with the subject [`emergency_card_subject`] and the first line
//! [`EMERGENCY_FIRST_LINE`]. Between 8 a.m. and 8 p.m. Arizona time every card recipient gets it
//! at once; between 8 p.m. and 8 a.m. only the people `businesses::api::night_people` names (the
//! "Emergency emails at any hour" switch on, at a business ticked after-hours and not suspended)
//! get it at once and every other recipient at 8 a.m. ([`split`]). No call is placed (BIZ-039).
//! It runs as soon as the job is released at ZIP-in (INT-029), before the trade questions.

use askhava_businesses::api::{NoticePurpose, may_get_cards, night_people, notice_recipients};
use askhava_core::api::{NewTask, add_business_minutes, brand, enqueue, is_business_hours};
use askhava_messaging::api::BUSINESS_NOTICE_KIND;
use jiff::Timestamp;
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::card_notices::{CardNotice, card_payload};
use crate::models::QuotesError;

/// CARD-021: the first line of every EMERGENCY card notice.
pub const EMERGENCY_FIRST_LINE: &str =
    "EMERGENCY: the homeowner chose emergency. Quote as soon as you can.";

/// CARD-021: "Ask Hava: EMERGENCY <category> job in <town>. Quote as soon as you can."
#[must_use]
pub fn emergency_card_subject(category: &str, town: &str) -> String {
    let b = brand().short_name();
    format!("{b}: EMERGENCY {category} job in {town}. Quote as soon as you can.")
}

/// When each recipient of one EMERGENCY card gets it (CARD-021, QH-004, QH-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardTiming {
    /// The opaque mailbox IDs that get it at once.
    pub now: Vec<Uuid>,
    /// The ones held to [`CardTiming::eight`].
    pub held: Vec<Uuid>,
    /// The next 8 a.m. Arizona time (`now` itself by day).
    pub eight: Timestamp,
}

/// CARD-021, QH-004, QH-005: by day everyone at once; at night only `night_ok` at once and every
/// other recipient at 8 a.m., all together.
#[must_use]
pub fn split(now: Timestamp, everyone: &[Uuid], night_ok: &[Uuid]) -> CardTiming {
    if is_business_hours(now) {
        return CardTiming {
            now: everyone.to_vec(),
            held: Vec::new(),
            eight: now,
        };
    }
    let (at_once, held) = everyone.iter().partition(|m| night_ok.contains(m));
    CardTiming {
        now: at_once,
        held,
        eight: add_business_minutes(now, 0),
    }
}

fn emergency_payload(base: Value, notice: &CardNotice<'_>) -> Value {
    let mut p = base;
    p["emergency"] = Value::Bool(true);
    p["subject"] = Value::String(emergency_card_subject(notice.category, notice.town));
    p["first_line"] = Value::String(EMERGENCY_FIRST_LINE.to_owned());
    p
}

/// The card recipients' mailboxes and those of them with the switch on at night (CARD-021).
async fn mailboxes(
    tx: &mut PgConnection,
    business: Uuid,
) -> Result<(Vec<Uuid>, Vec<Uuid>), QuotesError> {
    let people = notice_recipients(tx, business, NoticePurpose::Cards, false)
        .await
        .map_err(|_| QuotesError::Database)?;
    let everyone: Vec<Uuid> = people.iter().map(|p| p.mailbox).collect();
    let night: Vec<Uuid> = night_people(tx, business)
        .await
        .map_err(|_| QuotesError::Database)?
        .into_iter()
        .filter(|p| p.cards)
        .map(|p| p.mailbox)
        .collect();
    Ok((everyone, night))
}

/// CARD-021, BIZ-037, CON-020: queues `business`'s EMERGENCY card notice for `job` at `now`: one
/// task due now for the people who get it at once and one due at 8 a.m. for the rest, once per job
/// and business; `false` when the business may not get cards yet or nobody gets it.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn queue_emergency_card(
    tx: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    notice: &CardNotice<'_>,
    now: Timestamp,
) -> Result<bool, QuotesError> {
    if !may_get_cards(tx, business)
        .await
        .map_err(|_| QuotesError::Database)?
    {
        return Ok(false);
    }
    let (everyone, night) = mailboxes(tx, business).await?;
    let timing = split(now, &everyone, &night);
    let groups = [(timing.now, now, ""), (timing.held, timing.eight, ".eight")];
    let mut queued = false;
    for (to, at, suffix) in groups.into_iter().filter(|(to, _, _)| !to.is_empty()) {
        let payload = emergency_payload(card_payload((job, business), notice, (&to, &[])), notice);
        let mut task = NewTask::new(BUSINESS_NOTICE_KIND, payload, at);
        task.dedupe_key = Some(format!(
            "{BUSINESS_NOTICE_KIND}.card.{job}.{business}{suffix}"
        ));
        enqueue(tx, &task)
            .await
            .map_err(|_| QuotesError::Database)?;
        queued = true;
    }
    Ok(queued)
}
