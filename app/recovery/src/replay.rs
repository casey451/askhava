//! The replay after a restore: texts and charges rebuilt from the vendors' records, with the IDs
//! derived from what each identifies, so nothing replayed sends or charges twice (OPS-015,
//! OPS-016, OPS-017; D-156, D-2764, D-3155, D-3753, D-3889, D-4292).
//!
//! Charges: every succeeded charge Stripe lists from [`CHARGE_MARGIN`] before `charges_since`
//! goes to `payments::api::rebuild_charge`, which books a lost one once under the pick's derived
//! charge key and never calls Stripe; an unmatched or refunded one is recorded as a
//! [`ReplayItem`] for Casey. This module moves no money (FEE-032): an unmatched charge's refund
//! is the nightly reconcile's own refund of a charge `payments` has no record of (D-3889).
//!
//! Texts: `messaging::api::replay_window` gives the sending numbers and the span of the
//! outbox rows not yet sent; each number's sent log is listed from the span's start through the
//! run time ([`SmsSentLog::list_sent`], never by a stored key, D-2764) and handed to
//! `messaging::api::replay_match`, which answers row IDs only. A matched row is marked sent with
//! the listed SID (`mark_replayed_sent`), never resent; an ambiguous one is recorded for Casey and
//! never resent; an unmatched one is left to the ordinary sender. No number is logged or stored
//! here. A vendor that cannot be reached is waited for and retried ([`Retry`]); reads only, so a
//! retry repeats nothing outside. The run and its items are written in one transaction at the end.

use std::future::Future;
use std::time::Duration;

use askhava_adapters::api::{AdapterError, ListedCharge, SentMessage, SmsSentLog};
use askhava_messaging::api::{
    Lines, ListedText, ReplayMatch, ReplaySent, mark_replayed_sent, replay_match, replay_window,
};
use askhava_payments::api::{ChargePage, PAGE_SIZE, Rebuilt, StripeCharge, rebuild_charge};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::RecoveryError;

/// Stripe's charge list as the replay reads it, one page at a time (OPS-016). Every
/// `payments::api::ReconcileStripe` client is one, so the composition passes its Stripe client.
pub trait ChargeSource {
    /// Up to `limit` succeeded charges created at or after `since`, after the cursor `after`.
    fn list_page(
        &self,
        since: Timestamp,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = Result<ChargePage, AdapterError>> + Send;
}

impl<T: askhava_payments::api::ReconcileStripe> ChargeSource for T {
    fn list_page(
        &self,
        since: Timestamp,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = Result<ChargePage, AdapterError>> + Send {
        self.list_charges_page(since, after, limit)
    }
}

/// How long before the restored backup's time the charge list starts (OPS-016): a charge created
/// just before the backup but booked just after it is still listed, and Stripe's `created` filter
/// works in whole seconds. Listing more is safe because `rebuild_charge` answers `AlreadyBooked`
/// for a charge the restored database holds; a charge missed here would be refunded by the nightly
/// reconcile as one `payments` has no record of.
pub const CHARGE_MARGIN: SignedDuration = SignedDuration::from_mins(10);

/// How often and how long the replay waits for a vendor it cannot reach (TD-OPS-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Calls in all, at least one.
    pub tries: u32,
    /// The wait between two calls.
    pub wait: Duration,
}

impl Retry {
    /// Ten calls a minute apart.
    pub const STANDARD: Self = Self {
        tries: 10,
        wait: Duration::from_secs(60),
    };
}

/// What one replay reads from: the texting and Stripe clients, the vault and sending numbers
/// `messaging` matches with, the start of the lost window for charges, and the retry rule.
pub struct ReplaySources<'a, S, C> {
    /// Twilio's sent log.
    pub sms: &'a S,
    /// Stripe's charge list.
    pub stripe: &'a C,
    /// The contact vault, for `messaging`'s recipient check.
    pub vault: &'a Vault,
    /// The sending numbers.
    pub lines: &'a Lines,
    /// The restored backup's time; charges created from [`CHARGE_MARGIN`] before it are listed.
    pub charges_since: Timestamp,
    /// The wait-and-retry rule for both vendors.
    pub retry: Retry,
}

/// Why a replayed text or charge is listed for Casey (`recovery.replay_item.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// More than one listed text could be this outbox row's, or its recipient cannot be read.
    TextAmbiguous,
    /// `messaging` refused to mark the matched outbox row sent.
    TextRefused,
    /// A Stripe charge no pick holds; the nightly reconcile refunds it (D-3889).
    ChargeUnmatched,
    /// A Stripe charge already refunded; booked nowhere.
    ChargeRefunded,
}

impl ItemKind {
    /// The stored name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextAmbiguous => "text_ambiguous",
            Self::TextRefused => "text_refused",
            Self::ChargeUnmatched => "charge_unmatched",
            Self::ChargeRefunded => "charge_refunded",
        }
    }
}

/// One item listed for Casey: its kind and its subject, an outbox row ID or a Stripe charge ID
/// (never a number or name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayItem {
    /// Why it is listed.
    pub kind: ItemKind,
    /// The outbox row ID or the charge ID.
    pub subject: String,
}

/// What one replay did (OPS-015 to OPS-017); its row is `recovery.replay_run`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayReport {
    /// The `recovery.replay_run` row's ID.
    pub run: Uuid,
    /// Outbox rows found in Twilio's sent log and marked sent, never resent.
    pub texts_marked: i32,
    /// Outbox rows Twilio has no text for, left to the ordinary sender.
    pub texts_unsent: i32,
    /// Charges booked now from Stripe's record.
    pub charges_booked: i32,
    /// Charges the restored database already held.
    pub charges_already: i32,
    /// Everything listed for Casey.
    pub items: Vec<ReplayItem>,
}

/// OPS-015, OPS-016, OPS-017, D-3889: replays the charges, then the texts, and records the run.
///
/// # Errors
/// [`RecoveryError::Vendor`] when a vendor stayed unreachable through every retry;
/// [`RecoveryError::Database`]. What was booked or marked before the error stays, and a second run
/// finds it already done.
pub async fn replay<S, C>(
    conn: &mut PgConnection,
    from: &ReplaySources<'_, S, C>,
    at: Timestamp,
) -> Result<ReplayReport, RecoveryError>
where
    S: SmsSentLog + Sync,
    C: ChargeSource + Sync,
{
    let mut report = ReplayReport::default();
    replay_charges(conn, from, at, &mut report).await?;
    replay_texts(conn, from, at, &mut report).await?;
    report.run = record(conn, &report, at).await?;
    Ok(report)
}

/// TD-OPS-04: calls `call` up to `retry.tries` times, waiting `retry.wait` after each failure.
async fn retried<T, F, Fut>(retry: Retry, mut call: F) -> Result<T, RecoveryError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, AdapterError>>,
{
    for _ in 1..retry.tries {
        if let Ok(answer) = call().await {
            return Ok(answer);
        }
        tokio::time::sleep(retry.wait).await;
    }
    call().await.map_err(|_| RecoveryError::Vendor)
}

/// OPS-016, D-3753, D-4292: every listed charge through `rebuild_charge`, counted by its answer.
async fn replay_charges<S, C: ChargeSource>(
    conn: &mut PgConnection,
    from: &ReplaySources<'_, S, C>,
    at: Timestamp,
    report: &mut ReplayReport,
) -> Result<(), RecoveryError> {
    let since = from.charges_since.saturating_sub(CHARGE_MARGIN);
    let since = since.unwrap_or(Timestamp::MIN);
    let mut after: Option<String> = None;
    loop {
        let page = retried(from.retry, || {
            from.stripe.list_page(since, after.as_deref(), PAGE_SIZE)
        })
        .await?;
        for charge in page.charges {
            let rebuilt = rebuild_charge(conn, &listed(charge), at).await;
            tally(report, rebuilt.map_err(|_| RecoveryError::Database)?);
        }
        match page.next {
            Some(next) => after = Some(next),
            None => return Ok(()),
        }
    }
}

/// OPS-016, D-4292: counts one `rebuild_charge` answer, or lists it for Casey.
pub(crate) fn tally(report: &mut ReplayReport, rebuilt: Rebuilt) {
    match rebuilt {
        Rebuilt::Booked => report.charges_booked += 1,
        Rebuilt::AlreadyBooked => report.charges_already += 1,
        Rebuilt::Unmatched(id) => item(report, ItemKind::ChargeUnmatched, id),
        Rebuilt::Refunded(id) => item(report, ItemKind::ChargeRefunded, id),
    }
}

/// A charge as the reconcile reads it, back in the list's shape `rebuild_charge` takes.
fn listed(charge: StripeCharge) -> ListedCharge {
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    ListedCharge {
        id: charge.id,
        payment_intent: some(charge.payment_intent),
        pick_id: charge.pick.map(|p| p.to_string()),
        amount: charge.amount_cents,
        amount_refunded: charge.refunded_cents,
        balance_transaction: some(charge.balance_transaction).map(|t| (t, charge.fee_cents)),
        dispute: charge.dispute,
    }
}

fn item(report: &mut ReplayReport, kind: ItemKind, subject: String) {
    report.items.push(ReplayItem { kind, subject });
}

/// OPS-017, D-2764, D-3889: the unsent outbox rows matched to Twilio's sent log inside
/// `messaging`; a matched row is marked sent, never resent.
async fn replay_texts<S: SmsSentLog + Sync, C>(
    conn: &mut PgConnection,
    from: &ReplaySources<'_, S, C>,
    at: Timestamp,
    report: &mut ReplayReport,
) -> Result<(), RecoveryError> {
    let db = |_| RecoveryError::Database;
    let Some(window) = replay_window(conn, from.lines).await.map_err(db)? else {
        return Ok(());
    };
    let mut sent = Vec::new();
    for number in &window.numbers {
        let listing = retried(from.retry, || {
            from.sms.list_sent(number, (window.earliest, at))
        });
        sent.extend(listing.await?.into_iter().filter_map(text));
    }
    let (times, listed): (Vec<Timestamp>, Vec<ListedText>) = sent.into_iter().unzip();
    let keys = (from.vault, from.lines);
    for (row, found) in replay_match(conn, keys, &listed).await.map_err(db)? {
        match found {
            ReplayMatch::Matched(sid) => {
                let at = listed.iter().position(|t| t.sid() == sid);
                let Some(i) = at else { continue };
                let marked = mark_replayed_sent(conn, keys, row, &listed[i], times[i]).await;
                match marked.map_err(db)? {
                    ReplaySent::Marked => report.texts_marked += 1,
                    ReplaySent::AlreadySent => {}
                    ReplaySent::Refused(_) => item(report, ItemKind::TextRefused, row.to_string()),
                }
            }
            ReplayMatch::Ambiguous => item(report, ItemKind::TextAmbiguous, row.to_string()),
            ReplayMatch::Unmatched => report.texts_unsent += 1,
        }
    }
    Ok(())
}

/// A listed message in `messaging`'s shape with its send time; `None` for a SID it refuses.
fn text(m: SentMessage) -> Option<(Timestamp, ListedText)> {
    let text = ListedText::new((&m.sid, m.to, &m.from), m.body, m.sent_at)?;
    Some((m.sent_at, text))
}

/// The run's row and its items, in one transaction.
async fn record(
    conn: &mut PgConnection,
    report: &ReplayReport,
    at: Timestamp,
) -> Result<Uuid, RecoveryError> {
    let db = |_| RecoveryError::Database;
    let mut tx = conn.begin().await.map_err(db)?;
    let run = sqlx::query_scalar!(
        "INSERT INTO recovery.replay_run \
         (ran_at, texts_marked, texts_unsent, charges_booked, charges_already) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        at.to_sqlx() as _,
        report.texts_marked,
        report.texts_unsent,
        report.charges_booked,
        report.charges_already
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    for it in &report.items {
        sqlx::query!(
            "INSERT INTO recovery.replay_item (run_id, kind, subject) VALUES ($1, $2, $3) \
             ON CONFLICT DO NOTHING",
            run,
            it.kind.as_str(),
            it.subject
        )
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(run)
}
