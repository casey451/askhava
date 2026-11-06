//! After the pick: the loss notice to every other quoter, "This job is no longer active." once,
//! and the reply to a business asking about its live pick (PICK-026, PICK-027, PICK-038, D-044,
//! D-150, D-193, D-263, D-308, D-406).
//!
//! [`lost_notices`] runs when the picked business's payment or $0 fee decision is recorded (the
//! pick is `paid` or `free`): each other business with a quote on the job gets one
//! [`BUSINESS_NOTICE_KIND`] task, by its chosen channels through `messaging` (BIZ-019), to its
//! "Cards and quotes" recipients, deduped per pick and business. The pure [`compare`] and
//! [`loss_template`] choose the words: the first sentence only below [`LossSettings::min_quotes`]
//! quotes, else the lower / about the same / higher line, "Several businesses quoted" when the card
//! went to [`LossSettings::several_at_most`] businesses or fewer, or the different-type line. The
//! payload holds the job number and the quote count only: never an amount or a business name.
//! When the picked quote or the receiving business's own does not read back, the text is the first
//! sentence only. [`no_longer_active`] runs when the job reaches a final status and
//! [`refused_after_confirm`] when a quote is refused after Confirm (D-043); both share one dedupe
//! key per job and business and skip a business whose pick is `paid` or `free`, so each quoter but
//! the connected one hears it once (D-1576). [`live_pick_reply`] answers PICK-027.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{NewTask, enqueue};
use askhava_jobs::api::{current, job_number};
use askhava_quotes::api::{Period, Price, QuotedPrice, card_holders, quoted_prices};
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{PicksError, waiting_pick};
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::status::PickStatus;

/// PICK-026: the first sentence only, for fewer than [`LossSettings::min_quotes`] quotes.
pub const LOST_TEMPLATE: &str = "pick_lost";
/// PICK-026: "<k> businesses quoted; the picked quote was lower than yours."
pub const LOST_LOWER_TEMPLATE: &str = "pick_lost_lower";
/// PICK-026: "<k> businesses quoted; the picked quote was about the same as yours."
pub const LOST_SAME_TEMPLATE: &str = "pick_lost_same";
/// PICK-026: "<k> businesses quoted; the picked quote was higher than yours."
pub const LOST_HIGHER_TEMPLATE: &str = "pick_lost_higher";
/// PICK-026, D-263: "Several businesses quoted; the picked quote was lower than yours."
pub const LOST_SEVERAL_LOWER_TEMPLATE: &str = "pick_lost_several_lower";
/// PICK-026, D-263: "Several businesses quoted; the picked quote was about the same as yours."
pub const LOST_SEVERAL_SAME_TEMPLATE: &str = "pick_lost_several_same";
/// PICK-026, D-263: "Several businesses quoted; the picked quote was higher than yours."
pub const LOST_SEVERAL_HIGHER_TEMPLATE: &str = "pick_lost_several_higher";
/// PICK-038: "The picked quote was a different type of quote from yours."
pub const LOST_OTHER_TYPE_TEMPLATE: &str = "pick_lost_other_type";
/// PICK-026, D-044, D-150: "This job is no longer active."
pub const INACTIVE_TEMPLATE: &str = "job_no_longer_active";
/// PICK-027: the Confirm link and what happens next.
pub const LIVE_PICK_TEMPLATE: &str = "pick_live_reply";

/// The PICK-026 and PICK-038 settings (D-193, D-263).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossSettings {
    /// Quotes received before the second sentence is added (default 3).
    pub min_quotes: usize,
    /// A card sent to this many businesses or fewer says "Several businesses quoted" (default 4).
    pub several_at_most: usize,
    /// "About the same" is within this percent of the receiving business's quote (default 10).
    pub about_same_pct: u32,
}

impl Default for LossSettings {
    fn default() -> Self {
        Self {
            min_quotes: 3,
            several_at_most: 4,
            about_same_pct: 10,
        }
    }
}

/// How the picked quote compares with the receiving business's own (PICK-038).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// Below 100% minus the setting of yours.
    Lower,
    /// Within the setting of yours, both edges included.
    AboutSame,
    /// Above 100% plus the setting of yours.
    Higher,
    /// The two quotes cannot be compared on one figure.
    OtherType,
}

/// The one figure a quote compares on (PICK-038, D-1572).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Figure {
    OneTime(u64),
    Recurring(u64, Period),
    Visit(u64),
}

/// FEE-003, D-033: a range's midpoint, a half cent rounded down; never overflows.
const fn midpoint(low: u64, high: u64) -> u64 {
    low / 2 + high / 2 + (low % 2 + high % 2) / 2
}

/// PICK-038, D-1572: a firm amount, a range's (or an hourly quote's) midpoint, tier `tier` of a
/// tiers quote, a recurring figure with its period or a visit fee; none for an hourly quote with no
/// hours, or a tiers quote without that tier.
fn figure(price: &Price, tier: Option<usize>) -> Option<Figure> {
    match price {
        Price::Firm(v) => Some(Figure::OneTime(*v)),
        Price::Range { low, high, .. } => Some(Figure::OneTime(midpoint(*low, *high))),
        Price::Hourly {
            rate,
            hours: Some((lo, hi)),
        } => {
            let (lo, hi) = (
                rate.checked_mul((*lo).into())?,
                rate.checked_mul((*hi).into())?,
            );
            Some(Figure::OneTime(midpoint(lo, hi)))
        }
        Price::Hourly { hours: None, .. } => None,
        Price::VisitFee(v) => Some(Figure::Visit(*v)),
        Price::Monthly { cents, period, .. } => Some(Figure::Recurring(*cents, *period)),
        Price::Tiers(tiers) => tiers.get(tier?).map(|(_, v)| Figure::OneTime(*v)),
    }
}

/// PICK-038, D-193: "about the same" when `picked` is within `pct` percent of `yours`, edges in.
fn near(picked: u64, yours: u64, pct: u32) -> Comparison {
    let (picked, yours) = (u128::from(picked) * 100, u128::from(yours));
    if picked < yours * u128::from(100_u32.saturating_sub(pct)) {
        Comparison::Lower
    } else if picked > yours * u128::from(100 + pct) {
        Comparison::Higher
    } else {
        Comparison::AboutSame
    }
}

/// PICK-038, D-193, D-1572: compares the `picked` quote (on `tier`, the tier the homeowner picked,
/// for a tiers quote) with `yours`. One-time prices (firm, range midpoint, hourly with hours, the
/// tier picked) compare with each other, a recurring figure with one of the same period, a visit
/// fee with a visit fee; a tiers quote of yours compares on its tier in the picked tier's place when
/// the picked quote is a tiers quote; anything else is [`Comparison::OtherType`].
#[must_use]
pub fn compare(
    picked: &Price,
    tier: Option<usize>,
    yours: &Price,
    about_same_pct: u32,
) -> Comparison {
    let your_tier = tier.filter(|_| matches!(picked, Price::Tiers(_)));
    match (figure(picked, tier), figure(yours, your_tier)) {
        (Some(Figure::OneTime(p)), Some(Figure::OneTime(y)))
        | (Some(Figure::Visit(p)), Some(Figure::Visit(y))) => near(p, y, about_same_pct),
        (Some(Figure::Recurring(p, a)), Some(Figure::Recurring(y, b))) if a == b => {
            near(p, y, about_same_pct)
        }
        _ => Comparison::OtherType,
    }
}

/// PICK-026, PICK-038, D-263: the loss text's template for `quoted` quotes received on a card sent
/// to `sent` businesses: the first sentence only below `min_quotes`, else the comparison's line,
/// with "Several businesses quoted" when `sent` is at most `several_at_most`.
#[must_use]
pub const fn loss_template(
    (quoted, sent): (usize, usize),
    compared: Comparison,
    settings: LossSettings,
) -> &'static str {
    if quoted < settings.min_quotes {
        return LOST_TEMPLATE;
    }
    match (compared, sent <= settings.several_at_most) {
        (Comparison::OtherType, _) => LOST_OTHER_TYPE_TEMPLATE,
        (Comparison::Lower, false) => LOST_LOWER_TEMPLATE,
        (Comparison::AboutSame, false) => LOST_SAME_TEMPLATE,
        (Comparison::Higher, false) => LOST_HIGHER_TEMPLATE,
        (Comparison::Lower, true) => LOST_SEVERAL_LOWER_TEMPLATE,
        (Comparison::AboutSame, true) => LOST_SEVERAL_SAME_TEMPLATE,
        (Comparison::Higher, true) => LOST_SEVERAL_HIGHER_TEMPLATE,
    }
}

/// PICK-026, PICK-038, D-1576: the template for `yours` against the `picked` price; the first
/// sentence only when either does not read back (no comparison is made up).
fn template_for(
    (picked, tier): (Option<&Price>, Option<usize>),
    yours: Option<&Price>,
    counts: (usize, usize),
    settings: LossSettings,
) -> &'static str {
    match (picked, yours) {
        (Some(p), Some(y)) => loss_template(
            counts,
            compare(p, tier, y, settings.about_same_pct),
            settings,
        ),
        _ => LOST_TEMPLATE,
    }
}

/// BIZ-019, BIZ-037, D-308: queues one notice to `business`'s "Cards and quotes" recipients under
/// `key`; false when it was queued before or the business has no recipient.
async fn queue_notice(
    conn: &mut PgConnection,
    (template, business, job): (&str, Uuid, Uuid),
    values: Value,
    key: String,
) -> Result<bool, PicksError> {
    let to = notice_addresses(conn, business, NoticePurpose::Cards)
        .await
        .map_err(|_| PicksError::Database)?;
    if to.is_empty() {
        return Ok(false);
    }
    let payload = json!({
        "template": template, "business": business, "job": job, "to": to, "values": values,
    });
    let mut task = NewTask::new(BUSINESS_NOTICE_KIND, payload, Timestamp::now());
    task.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.{key}"));
    let queued = enqueue(conn, &task)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(queued.is_some())
}

/// PICK-026, PICK-038, D-150, D-193, D-1576: once the pick's payment or $0 fee decision is
/// recorded (status `paid` or `free`), queues the loss notice to every other business with an
/// unwithdrawn quote on the job, once per pick and business; `tier` is the tier the homeowner
/// picked of a tiers quote. Every unwithdrawn quote counts, one whose price does not read back
/// included. Returns how many were queued now; a pick in any other status queues none.
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::Quotes`] or [`PicksError::Job`].
pub async fn lost_notices(
    conn: &mut PgConnection,
    pick: Uuid,
    tier: Option<usize>,
    settings: LossSettings,
) -> Result<usize, PicksError> {
    let decided = [PickStatus::Paid.code(), PickStatus::Free.code()];
    let Some(row) = sqlx::query!(
        "SELECT job_id, business_id FROM picks.pick WHERE id = $1 AND status = ANY($2)",
        pick,
        &decided as &[&str],
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| PicksError::Database)?
    else {
        return Ok(0);
    };
    let quotes: Vec<QuotedPrice> = quoted_prices(conn, row.job_id)
        .await
        .map_err(|_| PicksError::Quotes)?
        .into_iter()
        .filter(|q| !q.withdrawn)
        .collect();
    let sent = card_holders(conn, row.job_id)
        .await
        .map_err(|_| PicksError::Quotes)?
        .len();
    let number = job_number(conn, row.job_id)
        .await
        .map_err(|_| PicksError::Job)?;
    let picked = quotes
        .iter()
        .find(|q| q.business == row.business_id)
        .and_then(|q| q.price.as_ref());
    let counts = (quotes.len(), sent);
    let mut queued = 0;
    for q in quotes.iter().filter(|q| q.business != row.business_id) {
        let template = template_for((picked, tier), q.price.as_ref(), counts, settings);
        let values = json!({"job": number, "count": quotes.len()});
        let key = format!("{LOST_TEMPLATE}.{pick}.{}", q.business);
        let target = (template, q.business, row.job_id);
        queued += usize::from(queue_notice(conn, target, values, key).await?);
    }
    Ok(queued)
}

/// D-150, D-1576: the businesses whose pick on `job` is `paid` or `free` now (connected); a pick
/// moved on to no-show or fraud-refunded is not.
async fn connected(conn: &mut PgConnection, job: Uuid) -> Result<Vec<Uuid>, PicksError> {
    let decided = [PickStatus::Paid.code(), PickStatus::Free.code()];
    sqlx::query_scalar!(
        "SELECT business_id FROM picks.pick WHERE job_id = $1 AND status = ANY($2)",
        job,
        &decided as &[&str],
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| PicksError::Database)
}

/// PICK-026, D-044, D-150: "This job is no longer active." to `business`, once per job.
async fn inactive_to(
    conn: &mut PgConnection,
    (job, number): (Uuid, i64),
    business: Uuid,
) -> Result<bool, PicksError> {
    let key = format!("{INACTIVE_TEMPLATE}.{job}.{business}");
    queue_notice(
        conn,
        (INACTIVE_TEMPLATE, business, job),
        json!({"job": number}),
        key,
    )
    .await
}

/// PICK-026, D-044, D-150, D-1576: when `job` is in a final status (Done, Closed, Cancelled and the
/// other final ones), queues "This job is no longer active." once to every business that quoted on
/// it, one sent after the pick and one withdrawn since included, except a business whose pick is
/// connected (`paid` or `free`). Returns how many were queued now; a job not yet final queues none.
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::Quotes`] or [`PicksError::Job`].
pub async fn no_longer_active(conn: &mut PgConnection, job: Uuid) -> Result<usize, PicksError> {
    let row = current(conn, job).await.map_err(|_| PicksError::Job)?;
    if !row.status.is_final() {
        return Ok(0);
    }
    let connected = connected(conn, job).await?;
    let quotes = quoted_prices(conn, job)
        .await
        .map_err(|_| PicksError::Quotes)?;
    let number = job_number(conn, job).await.map_err(|_| PicksError::Job)?;
    let mut queued = 0;
    for q in quotes.iter().filter(|q| !connected.contains(&q.business)) {
        queued += usize::from(inactive_to(conn, (job, number), q.business).await?);
    }
    Ok(queued)
}

/// PICK-026, D-043, D-044, D-1576: a business whose quote is refused after Confirm gets "This job is
/// no longer active." at once; under the same key as [`no_longer_active`], so never twice. False
/// when it already had it, or when its pick on the job is connected (`paid` or `free`).
///
/// # Errors
///
/// [`PicksError::Database`] or [`PicksError::Job`].
pub async fn refused_after_confirm(
    conn: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<bool, PicksError> {
    if connected(conn, job).await?.contains(&business) {
        return Ok(false);
    }
    let number = job_number(conn, job).await.map_err(|_| PicksError::Job)?;
    inactive_to(conn, (job, number), business).await
}

/// The PICK-027 reply: [`LIVE_PICK_TEMPLATE`] with the job number; `pick` is the Confirm link's
/// subject, which `messaging` signs (D-1565).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveReply {
    /// The template ID.
    pub template: &'static str,
    /// The job number.
    pub job: i64,
    /// The pick waiting for this business's Confirm.
    pub pick: Uuid,
}

/// PICK-027: `business` asking about `job` gets the Confirm link and what happens next while its
/// pick waits for Confirm; none for another business, or once the pick no longer waits.
///
/// # Errors
///
/// [`PicksError::Database`] or [`PicksError::Job`].
pub async fn live_pick_reply(
    conn: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Option<LiveReply>, PicksError> {
    let Some((pick, _, picked)) = waiting_pick(conn, job).await? else {
        return Ok(None);
    };
    if picked != business {
        return Ok(None);
    }
    let number = job_number(conn, job).await.map_err(|_| PicksError::Job)?;
    Ok(Some(LiveReply {
        template: LIVE_PICK_TEMPLATE,
        job: number,
        pick,
    }))
}
