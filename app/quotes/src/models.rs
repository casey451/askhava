//! The records matching reads and returns, and the `quotes.job_card` query (migration
//! `0067_quotes_matching.sql`; CARD-001 to CARD-004, CARD-016, CARD-018, LIC-035); T046 adds
//! the `quotes.quote` queries (migration `0070_quotes_quote.sql`; PICK-001, PICK-002, TXT-042);
//! T186 adds [`stored_card`], what a quote card reads of a shown quote (PICK-003, PICK-040);
//! T047 adds [`list_rows`], the pickable quotes the LIST text states (TXT-031, PICK-002); T213 adds
//! [`page_counts`], the business job page's counts and card check (CARD-019, CARD-023).
//!
//! Nothing here holds a homeowner's detail: a job is its offer towns, its candidate job types and
//! its scrubbed text (PRIV-001); a business is IDs, codes and ticks.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_core::api::Guarded;
use askhava_jobs::api::rules::Windows;
use askhava_licensing::api::{Holder, Job, NoCardReason};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::{DayPart, Format, Period, Price, Quote};

/// Why a quotes call failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QuotesError {
    /// A database call failed.
    #[error("quotes database call failed")]
    Database,
    /// The shipped license map did not load (LIC-043).
    #[error("the license map did not load")]
    LicenseMap,
    /// The job could not be read through `jobs::api` (CARD-018).
    #[error("the job could not be read")]
    Job,
    /// The business could not be read through `businesses::api` (PICK-044).
    #[error("the business could not be read")]
    Business,
    /// A homeowner text could not be built from its templates (TXT-016, TXT-031, TXT-032).
    #[error("the text could not be built")]
    Text,
}

/// A town a job is offered in: the job's own town first, then any neighbouring towns (CARD-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferTown {
    /// The catalog town code, as `businesses.town` stores it.
    pub code: String,
    /// Its two-letter state; only `AZ` towns are ever offered (INT-022, D-382).
    pub state: String,
}

/// What a card says beyond its stored text when it is sent to one business (CARD-012, CARD-019,
/// FEE-045). The caller fills it; nothing here caps or filters who gets the card (CARD-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardLines {
    /// The N of "Sent to <N> businesses.": the count of businesses whose card notice the
    /// provider accepted, which the card-notice sender passes (CARD-019, T147).
    pub sent_to: u32,
    /// The homeowner prefers Spanish; shown only to a business that ticked "We speak Spanish"
    /// (CARD-012). It is never a matching input, so no business can skip a job by language.
    pub prefers_spanish: bool,
    /// The job type is one of the catalog's `fee_free_job_types`, read from
    /// `catalog::api::JobType::fee_free` (FEE-045, D-403).
    pub fee_free: bool,
}

/// What matching knows of one job (CARD-001, CARD-002).
#[derive(Debug, Clone)]
pub struct CardJob {
    /// The towns the job is offered in.
    pub towns: Vec<OfferTown>,
    /// Every remaining candidate job type, each with its category's license rule (LIC-043).
    pub candidates: Vec<Job>,
    /// The scrubbed request text, read only for the handyman block (AI-012, LIC-021).
    pub text: String,
}

/// What matching knows of one business (CARD-001).
#[derive(Debug, Clone)]
pub struct CardBusiness {
    /// The business ID.
    pub id: Uuid,
    /// Its account status; only `Active` receives cards (CARD-016).
    pub status: AccountStatus,
    /// Whether its identity and license or city-list checks stand verified (BIZ-016, CARD-015).
    pub verified: bool,
    /// The town codes it serves (BIZ-010).
    pub towns: BTreeSet<String>,
    /// The (category, job type) pairs it ticked (BIZ-004).
    pub ticked: BTreeSet<(String, String)>,
    /// The ROC classes and subclassifications on its state record, as named there (LIC-035).
    pub classes: Vec<String>,
    /// Its other licenses; `roc_covers` is ignored and filled per job type from `classes`.
    pub licenses: Holder,
}

/// Whether one business receives the card (CARD-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardDecision {
    /// It receives the card.
    Card,
    /// It receives the card as a handyman: under $1,000 with no permit (LIC-014, LIC-019).
    HandymanCard,
    /// It does not.
    Skip(Skip),
}

/// Why a business does not receive a card, first failing check first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// An owner pause switch covers everything, a candidate's trade or every offer town it
    /// serves (CARD-016, OPS-003); checked only by [`crate::rules::card_under`].
    OwnerPaused,
    /// Its status is not `Active` (CARD-001, CARD-016).
    NotActive,
    /// It is not verified (CARD-001).
    NotVerified,
    /// It serves none of the job's Arizona offer towns (CARD-001, CARD-002).
    OutOfTown,
    /// The job has no candidate job type left.
    NoCandidates,
    /// It ticked none of the candidate job types (CARD-001).
    NothingTicked,
    /// Its licenses do not cover one candidate job type (CARD-001, LIC-035, LIC-043).
    NotCovered(NoCardReason),
}

/// One business as full matching sees it: its matching record and its "also send me jobs nobody
/// else covers" switch (BIZ-005, CARD-004).
#[derive(Debug, Clone)]
pub struct Pooled {
    /// What the checklist match reads (CARD-001).
    pub card: CardBusiness,
    /// "Also send me jobs nobody else covers" is on (CARD-004).
    pub overflow_on: bool,
}

/// The owner pause switches that are on (OPS-003), as codes: the caller reads them through
/// `ops::api::is_paused` and maps each town or trade switch's record to its code (CARD-016).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerPauses {
    /// The "all" switch is on.
    pub all: bool,
    /// Town codes whose switch is on.
    pub towns: BTreeSet<String>,
    /// Category codes whose switch is on.
    pub trades: BTreeSet<String>,
}

/// Every business that may receive a job's card, and the owner pauses in force (CARD-004,
/// CARD-016).
#[derive(Debug, Clone, Copy)]
pub struct Pool<'a> {
    /// The businesses, in any order.
    pub businesses: &'a [Pooled],
    /// The owner pause switches that are on.
    pub pauses: &'a OwnerPauses,
}

/// What the homeowner is told once the trade is known (INT-017, INT-032), counted as CARD-001
/// and CARD-004 would send the card then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// This many businesses would receive the card (at least 1).
    Send(usize),
    /// Nobody is licensed for every candidate, but the job spans two or more trades and one
    /// trade's part alone would reach a business: offer SPLIT (INT-032, D-1019).
    Split,
    /// No active business in the town covers it (INT-017).
    NotCovered,
}

/// One business chosen to receive the card (CARD-001, CARD-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipient {
    /// The business ID.
    pub business: Uuid,
    /// Whether the card carries the handyman condition (LIC-014, LIC-019).
    pub handyman: bool,
}

/// Records one card per recipient; a card already sent for this job and business is kept, so a
/// second run adds nothing (JOB-006). Returns how many rows were added.
pub(crate) async fn insert_cards(
    db: &mut PgConnection,
    job: Uuid,
    recipients: &[Recipient],
    now: Timestamp,
) -> Result<u64, QuotesError> {
    let businesses: Vec<Uuid> = recipients.iter().map(|r| r.business).collect();
    let handyman: Vec<bool> = recipients.iter().map(|r| r.handyman).collect();
    let done = sqlx::query!(
        "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) \
         SELECT $1, b, h, $4 FROM unnest($2::uuid[], $3::bool[]) AS t(b, h) \
         ON CONFLICT (job_id, business_id) DO NOTHING",
        job,
        &businesses,
        &handyman,
        now.to_sqlx()
    )
    .execute(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(done.rows_affected())
}

/// The businesses holding a card for this job, sorted by ID.
pub(crate) async fn card_holders(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Vec<Uuid>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT business_id FROM quotes.job_card WHERE job_id = $1 ORDER BY business_id",
        job
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)
}

/// When the business's card for this job was sent, or none when it holds no card (PICK-001).
pub(crate) async fn card_sent_at(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Option<Timestamp>, QuotesError> {
    let row = sqlx::query_scalar!(
        "SELECT sent_at FROM quotes.job_card WHERE job_id = $1 AND business_id = $2",
        job,
        business
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(row.map(jiff_sqlx::Timestamp::to_jiff))
}

/// The `quotes.quote` columns one checked quote fills (PICK-002, FEE-043, FEE-044).
#[derive(Default)]
struct Columns {
    amount: Option<i64>,
    low: Option<i64>,
    high: Option<i64>,
    tier_names: Option<Vec<String>>,
    tier_cents: Option<Vec<i64>>,
    period: Option<&'static str>,
    setup: Option<i64>,
    rate: Option<i64>,
    hours: (Option<i32>, Option<i32>),
    on_site: bool,
}

/// Cents as the database stores them; [`crate::intake::parse_quote`] already capped every amount.
fn cents(v: u64) -> Option<i64> {
    i64::try_from(v).ok()
}

/// Spreads `quote`'s price over the columns.
fn columns(quote: &Quote) -> Columns {
    let (low, high) = quote
        .range()
        .map_or((None, None), |(l, h)| (cents(l), cents(h)));
    let mut c = Columns {
        low,
        high,
        ..Columns::default()
    };
    match &quote.price {
        Price::Firm(v) | Price::VisitFee(v) => c.amount = cents(*v),
        Price::Range { on_site, .. } => c.on_site = *on_site,
        Price::Monthly {
            cents: v,
            period,
            setup,
        } => {
            (c.amount, c.setup) = (cents(*v), setup.and_then(cents));
            c.period = Some(period.code());
        }
        Price::Tiers(t) => {
            c.tier_names = Some(t.iter().map(|(n, _)| n.clone()).collect());
            c.tier_cents = Some(t.iter().filter_map(|(_, v)| cents(*v)).collect());
        }
        Price::Hourly { rate, hours } => {
            c.rate = cents(*rate);
            let (lo, hi) = hours.map_or((None, None), |(l, h)| (Some(l), Some(h)));
            c.hours = (
                lo.and_then(|v| i32::try_from(v).ok()),
                hi.and_then(|v| i32::try_from(v).ok()),
            );
        }
    }
    c
}

/// PICK-002, PICK-051: stores the quote; a second quote from the same business replaces the first
/// as an edit (`edited_at`), and one after a withdrawal clears `withdrawn_at` (D-1049). A form
/// quote is confirmed as sent; an AI-read one waits for the business's confirm on the read-back
/// screen, after every edit too (PICK-006, D-149). The caller refuses an edit after a pick.
pub(crate) async fn upsert_quote(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    quote: &Quote,
    now: Timestamp,
) -> Result<(), QuotesError> {
    let c = columns(quote);
    let form = &quote.form;
    let (visit, part) = form
        .earliest_visit
        .map_or((None, None), |(d, p)| (Some(d), Some(p.code())));
    sqlx::query!(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, low_cents, \
         high_cents, tier_names, tier_cents, period, setup_cents, hourly_rate_cents, hours_low, \
         hours_high, confirm_on_site, notes, original, ai_read, confirmed, earliest_visit, \
         visit_part, received_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
         $14, $15, $16, $17, NOT $17, $18, $19, $20) \
         ON CONFLICT (job_id, business_id) DO UPDATE SET format = EXCLUDED.format, \
         amount_cents = EXCLUDED.amount_cents, low_cents = EXCLUDED.low_cents, \
         high_cents = EXCLUDED.high_cents, tier_names = EXCLUDED.tier_names, \
         tier_cents = EXCLUDED.tier_cents, period = EXCLUDED.period, \
         setup_cents = EXCLUDED.setup_cents, hourly_rate_cents = EXCLUDED.hourly_rate_cents, \
         hours_low = EXCLUDED.hours_low, hours_high = EXCLUDED.hours_high, \
         confirm_on_site = EXCLUDED.confirm_on_site, notes = EXCLUDED.notes, \
         original = EXCLUDED.original, ai_read = EXCLUDED.ai_read, \
         confirmed = EXCLUDED.confirmed, earliest_visit = EXCLUDED.earliest_visit, \
         visit_part = EXCLUDED.visit_part, edited_at = EXCLUDED.received_at, \
         withdrawn_at = NULL",
        job,
        business,
        quote.format.code(),
        c.amount,
        c.low,
        c.high,
        c.tier_names.as_deref(),
        c.tier_cents.as_deref(),
        c.period,
        c.setup,
        c.rate,
        c.hours.0,
        c.hours.1,
        c.on_site,
        form.notes,
        form.original,
        form.ai_read,
        visit.map(|d| d.to_sqlx()) as _,
        part,
        now.to_sqlx()
    )
    .execute(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(())
}

/// PICK-051, JOB-025: whether `business` already has a quote on `job` (a new one is then an
/// edit), and whether any quote on the job is marked unavailable by a failed pick.
pub(crate) async fn edit_facts(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<(bool, bool), QuotesError> {
    let row = sqlx::query!(
        "SELECT EXISTS (SELECT 1 FROM quotes.quote WHERE job_id = $1 AND business_id = $2) \
         AS \"own!\", EXISTS (SELECT 1 FROM quotes.quote WHERE job_id = $1 \
         AND unavailable_at IS NOT NULL) AS \"picked!\"",
        job,
        business
    )
    .fetch_one(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok((row.own, row.picked))
}

/// JOB-025: marks the business's quote on the job unavailable at `at`, once (a failed pick).
pub(crate) async fn mark_unavailable(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
    at: Timestamp,
) -> Result<Guarded<()>, QuotesError> {
    let row = sqlx::query_scalar!(
        "UPDATE quotes.quote SET unavailable_at = $3 \
         WHERE job_id = $1 AND business_id = $2 AND unavailable_at IS NULL RETURNING true",
        job,
        business,
        at.to_sqlx()
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(Guarded::from_returning(row.map(|_| ())))
}

/// PICK-006: marks the business's quote on the job confirmed, unless it is withdrawn.
pub(crate) async fn confirm_quote(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Guarded<()>, QuotesError> {
    let row = sqlx::query_scalar!(
        "UPDATE quotes.quote SET confirmed = true \
         WHERE job_id = $1 AND business_id = $2 AND withdrawn_at IS NULL RETURNING true",
        job,
        business
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(Guarded::from_returning(row.map(|_| ())))
}

/// TD-PICK-02: the businesses whose quote the homeowner's page may show: confirmed by the
/// business and not withdrawn, sorted by ID (PICK-002, PICK-006, PICK-051).
pub(crate) async fn shown_quotes(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Vec<Uuid>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT business_id FROM quotes.quote \
         WHERE job_id = $1 AND confirmed AND withdrawn_at IS NULL ORDER BY business_id",
        job
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)
}

/// A job's cards and quotes when its windows are judged (PICK-001, TXT-042, JOB-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowState {
    /// When the last card was sent; none when no card was.
    pub last_sent: Option<Timestamp>,
    /// The counts `jobs::api::all_windows_ended` takes.
    pub windows: Windows,
}

/// Counts the job's cards (`sent`), its confirmed quotes (`quoted`) and those neither withdrawn
/// nor marked unavailable by a failed pick (`choosable`, JOB-025, D-1051).
pub(crate) async fn window_state(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<WindowState, QuotesError> {
    let row = sqlx::query!(
        "SELECT max(c.sent_at) AS last_sent, count(*) AS \"sent!\", \
         (SELECT count(*) FROM quotes.quote q WHERE q.job_id = $1 AND q.confirmed) AS \"quoted!\", \
         (SELECT count(*) FROM quotes.quote q WHERE q.job_id = $1 AND q.confirmed \
          AND q.withdrawn_at IS NULL AND q.unavailable_at IS NULL) AS \"choosable!\" \
         FROM quotes.job_card c WHERE c.job_id = $1",
        job
    )
    .fetch_one(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let count = |v: i64| u32::try_from(v).unwrap_or(u32::MAX);
    Ok(WindowState {
        last_sent: row.last_sent.map(jiff_sqlx::Timestamp::to_jiff),
        windows: Windows {
            quoted: count(row.quoted),
            sent: count(row.sent),
            choosable: count(row.choosable),
        },
    })
}

/// What a quote card reads of one shown quote: its format, the business's original wording and
/// the earliest visit (PICK-003, PICK-040). No AI output is stored with a quote (PICK-003, D-373).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredCard {
    pub format: Format,
    pub original: String,
    pub earliest_visit: Option<(jiff::civil::Date, DayPart)>,
}

/// PICK-003: the stored card of a confirmed, unwithdrawn quote, or none (PICK-002, PICK-006).
pub(crate) async fn stored_card(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Option<StoredCard>, QuotesError> {
    let row = sqlx::query!(
        "SELECT format, original, earliest_visit::text AS visit, visit_part FROM quotes.quote \
         WHERE job_id = $1 AND business_id = $2 AND confirmed AND withdrawn_at IS NULL",
        job,
        business
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let Some(row) = row else { return Ok(None) };
    let format = Format::from_code(&row.format).ok_or(QuotesError::Database)?;
    let part = row.visit_part.as_deref().and_then(DayPart::from_code);
    let date = row.visit.and_then(|d| d.parse::<jiff::civil::Date>().ok());
    let visit = date.zip(part);
    Ok(Some(StoredCard {
        format,
        original: row.original,
        earliest_visit: visit,
    }))
}

/// One quote the LIST text states: who made it, the price exactly as stored and the earliest visit
/// (TXT-031, PICK-002, WORD-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListRow {
    pub business: Uuid,
    pub price: Price,
    pub earliest_visit: Option<(jiff::civil::Date, DayPart)>,
}

/// The period a stored `period` code names.
fn period_of(code: &str) -> Option<Period> {
    [
        Period::Visit,
        Period::Week,
        Period::Month,
        Period::Quarter,
        Period::Year,
    ]
    .into_iter()
    .find(|p| p.code() == code)
}

/// The columns of one stored quote that make up its price.
struct PriceColumns {
    format: Format,
    amount: Option<i64>,
    range: (Option<i64>, Option<i64>),
    tiers: (Option<Vec<String>>, Option<Vec<i64>>),
    period: Option<String>,
    setup: Option<i64>,
    rate: Option<i64>,
    hours: (Option<i32>, Option<i32>),
    on_site: bool,
}

/// Puts a stored quote's price back together exactly as the business entered it (PICK-002).
fn price_of(c: PriceColumns) -> Option<Price> {
    let cents = |v: Option<i64>| v.and_then(|v| u64::try_from(v).ok());
    if let Some(rate) = cents(c.rate) {
        let hours = c.hours.0.zip(c.hours.1);
        let hours = hours.and_then(|(l, h)| Some((u32::try_from(l).ok()?, u32::try_from(h).ok()?)));
        return Some(Price::Hourly { rate, hours });
    }
    match c.format {
        Format::Firm => cents(c.amount).map(Price::Firm),
        Format::DiagnosticVisit => cents(c.amount).map(Price::VisitFee),
        Format::Range => Some(Price::Range {
            low: cents(c.range.0)?,
            high: cents(c.range.1)?,
            on_site: c.on_site,
        }),
        Format::Monthly => Some(Price::Monthly {
            cents: cents(c.amount)?,
            period: period_of(c.period.as_deref()?)?,
            setup: cents(c.setup),
        }),
        Format::Tiers => {
            let (names, prices) = (c.tiers.0?, c.tiers.1?);
            let prices = prices.into_iter().map(|v| u64::try_from(v).ok());
            let tiers: Option<Vec<(String, u64)>> = names
                .into_iter()
                .zip(prices)
                .map(|(n, p)| Some((n, p?)))
                .collect();
            tiers.map(Price::Tiers)
        }
    }
}

/// TXT-031, PICK-002: the job's pickable quotes in arrival order: confirmed, not withdrawn and
/// not marked unavailable by a failed pick, oldest first (ties by business ID).
pub(crate) async fn list_rows(
    db: &mut PgConnection,
    job: Uuid,
) -> Result<Vec<ListRow>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT business_id, format, amount_cents, low_cents, high_cents, tier_names, tier_cents, \
         period, setup_cents, hourly_rate_cents, hours_low, hours_high, confirm_on_site, \
         earliest_visit::text AS visit, visit_part FROM quotes.quote \
         WHERE job_id = $1 AND confirmed AND withdrawn_at IS NULL AND unavailable_at IS NULL \
         ORDER BY received_at, business_id",
        job
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    rows.into_iter()
        .map(|r| {
            let format = Format::from_code(&r.format).ok_or(QuotesError::Database)?;
            let price = price_of(PriceColumns {
                format,
                amount: r.amount_cents,
                range: (r.low_cents, r.high_cents),
                tiers: (r.tier_names, r.tier_cents),
                period: r.period,
                setup: r.setup_cents,
                rate: r.hourly_rate_cents,
                hours: (r.hours_low, r.hours_high),
                on_site: r.confirm_on_site,
            })
            .ok_or(QuotesError::Database)?;
            let part = r.visit_part.as_deref().and_then(DayPart::from_code);
            let date = r.visit.and_then(|d| d.parse::<jiff::civil::Date>().ok());
            Ok(ListRow {
                business: r.business_id,
                price,
                earliest_visit: date.zip(part),
            })
        })
        .collect()
}

/// CARD-023, CARD-019: what the business job page counts for `job`, and whether `business` holds a
/// card for it (T213).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageCounts {
    /// `business` holds a `quotes.job_card` row for the job, so the page may open.
    pub holds_card: bool,
    /// The businesses the card went to (one `quotes.job_card` row each).
    pub sent: u32,
    /// The quotes shown so far: confirmed and not withdrawn (PICK-002, PICK-006).
    pub quoted: u32,
}

/// CARD-023, CARD-019: [`PageCounts`] in one statement, so the counts agree with each other.
pub(crate) async fn page_counts(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<PageCounts, QuotesError> {
    let row = sqlx::query!(
        "SELECT EXISTS (SELECT 1 FROM quotes.job_card WHERE job_id = $1 AND business_id = $2) \
         AS \"holds!\", \
         (SELECT count(*) FROM quotes.job_card WHERE job_id = $1) AS \"sent!\", \
         (SELECT count(*) FROM quotes.quote WHERE job_id = $1 AND confirmed \
         AND withdrawn_at IS NULL) AS \"quoted!\"",
        job,
        business
    )
    .fetch_one(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let count = |n: i64| u32::try_from(n).unwrap_or(u32::MAX);
    Ok(PageCounts {
        holds_card: row.holds,
        sent: count(row.sent),
        quoted: count(row.quoted),
    })
}
