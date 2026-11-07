//! The job card as a business notice, and the card's sent count (CARD-019, CARD-020, BIZ-019,
//! BIZ-036, BIZ-037, PRIV-027, TXT-036; D-308, D-491, D-493, D-1715).
//!
//! [`queue_card_notice`] runs for each business a card goes to: it checks
//! `businesses::api::may_get_cards` (no card before the welcome email, CON-020), resolves the owner
//! and every "Cards and quotes" member with their channels through
//! `businesses::api::notice_recipients`, and queues one `messaging.business_notice` task with
//! their opaque IDs, the card text (no homeowner detail, CARD-005), the WORD-002 business
//! statement and the signed-in job page's link; never an image or a photo link (PRIV-027).
//! `messaging` sends it by email, and by text only to phones that may get one at send time.
//! The "Quote by" time is set when the notice is sent ([`QUOTE_HOURS`] on), never when queued.
//! [`sent_count`] is CARD-019's N: the card holders whose card notice the email or texting
//! provider accepted and that did not all hard-bounce (`messaging::api::card_notice_accepted`).

use askhava_businesses::api::{
    BUSINESS_STATEMENT, NoticePurpose, may_get_cards, notice_recipients,
};
use askhava_core::api::{NewTask, enqueue};
use askhava_messaging::api::{BUSINESS_NOTICE_KIND, JOB_CARD_TEMPLATE, card_notice_accepted};
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, QuotesError};

/// The signed-in job page a card email links to; the job number follows (PRIV-027, D-493).
pub const JOB_PAGE_PREFIX: &str = "https://askhava.com/b/jobs/";

/// PICK-001, CARD-020, D-1828: the quote window's length; `messaging` writes "Quote by" as the
/// send time plus this, so a card held to 8 a.m. (QH-005) counts from 8 a.m.
pub const QUOTE_HOURS: i64 = 24;

/// What one card notice says (CARD-020): no homeowner name, address, contact or photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardNotice<'a> {
    /// The job's request number.
    pub number: i64,
    /// The category's plain name, as in the subject.
    pub category: &'a str,
    /// The town.
    pub town: &'a str,
    /// The stored card text sent to this business (`JobCard::text_for`).
    pub card: &'a str,
}

/// The payload of one business's card notice (CARD-020, PRIV-027); public for its test.
#[must_use]
pub fn card_payload(
    (job, business): (Uuid, Uuid),
    notice: &CardNotice<'_>,
    (to, texts): (&[Uuid], &[Value]),
) -> Value {
    json!({
        "template": JOB_CARD_TEMPLATE,
        "business": business, "job": job, "to": to, "texts": texts,
        "values": {"trade": notice.category, "town": notice.town, "job": notice.number,
                   "quote_hours": QUOTE_HOURS},
        "card": notice.card, "statement": BUSINESS_STATEMENT,
        "page": format!("{JOB_PAGE_PREFIX}{}", notice.number),
    })
}

/// CARD-020, BIZ-037, CON-020, D-1715: queues `business`'s card notice for `job` at `now`, once
/// per job and business; `false` when the business may not get cards yet or has nobody to send to.
/// `business_texting_on` is the business-texting switch (TXT-035).
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn queue_card_notice(
    tx: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    notice: &CardNotice<'_>,
    (business_texting_on, now): (bool, Timestamp),
) -> Result<bool, QuotesError> {
    if !may_get_cards(tx, business)
        .await
        .map_err(|_| QuotesError::Database)?
    {
        return Ok(false);
    }
    let people = notice_recipients(tx, business, NoticePurpose::Cards, business_texting_on)
        .await
        .map_err(|_| QuotesError::Database)?;
    if people.is_empty() {
        return Ok(false);
    }
    let to: Vec<Uuid> = people
        .iter()
        .filter(|p| p.email)
        .map(|p| p.mailbox)
        .collect();
    let texts: Vec<Value> = people
        .iter()
        .flat_map(|p| {
            p.phones
                .iter()
                .map(|ph| json!({"mailbox": p.mailbox, "phone": ph}))
        })
        .collect();
    let payload = card_payload((job, business), notice, (&to, &texts));
    let mut task = NewTask::new(BUSINESS_NOTICE_KIND, payload, now);
    task.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.card.{job}.{business}"));
    enqueue(tx, &task)
        .await
        .map_err(|_| QuotesError::Database)?;
    Ok(true)
}

/// CARD-019, TD-TXT-22: how many of `job`'s card holders had their card notice accepted and not
/// every one hard-bounced; the cards-out text's N and the job page's count.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn sent_count(db: &mut PgConnection, job: Uuid) -> Result<u32, QuotesError> {
    let mut n = 0;
    for business in models::card_holders(db, job).await? {
        if card_notice_accepted(db, job, business)
            .await
            .map_err(|_| QuotesError::Database)?
        {
            n += 1;
        }
    }
    Ok(n)
}
