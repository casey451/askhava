//! The public face of `askhava-quotes`: everything other crates may call (D-481).
//!
//! Matching (T044): [`send_cards`] chooses every business that receives a job's card and records
//! one `quotes.job_card` row each, with no cap (CARD-001 to CARD-003); [`card_holders`] reads them
//! back; [`rules::card_for`] and [`rules::recipients`] are the pure decision (LIC-035, LIC-043).
//! T184: [`send_job_cards`] (owner pauses and "jobs nobody else covers", CARD-004, CARD-016),
//! [`send_late_cards`] (CARD-018) and [`coverage`] (INT-017, INT-032).
//! T045: [`build_card`] makes the anonymous [`JobCard`] (CARD-005 to CARD-009, PRIV-006) and
//! [`final_gate`] is the CARD-008 gate it runs last.
//! T185: the gate also removes `catalog`'s carrier-restricted phrases (CARD-010), and
//! [`JobCard::text_for`] adds the Spanish, fee-free and "Sent to <N> businesses." lines from
//! [`CardLines`] (CARD-012, CARD-019, FEE-045).
//! T150: [`queue_emergency_card`] sends the EMERGENCY card by email, at night at once only to
//! people with "Emergency emails at any hour" on, the rest at 8 a.m. ([`split`], CARD-021).
//! T046: [`parse_quote`] checks a [`QuoteForm`] into a [`Quote`] or a [`Bounce`] (PICK-002,
//! PICK-007, FEE-043, FEE-044); [`submit_quote`] stores it inside the business's own 24 hours
//! ([`window_open`], PICK-001); [`confirm_quote`] is the read-back's confirm (PICK-006);
//! T247: a quote confirmed at once by [`submit_quote`], or by [`confirm_quote`] or
//! [`confirm_quote_at`], moves an Open or No quotes yet job to Quotes ready (JOB-009, D-1517);
//! [`shown_quotes`] lists the confirmed ones the homeowner's page may show; [`run_windows_end`]
//! is the [`WINDOWS_KIND`] handler that calls `jobs::api::all_windows_ended` (TXT-042, JOB-025);
//! [`mark_unavailable`] is `picks`' mark on a failed pick's quote (JOB-025, PICK-051).
//! T186: [`quote_card`] reads a shown quote's [`QuoteCard`], whose [`QuoteCard::lines`] are the
//! same for homeowner and business (PICK-005, PICK-040, PICK-041, PICK-044); [`check_summary`]
//! and [`switch_on`] keep AI summaries off unless allowed and claim-free (PICK-003, PICK-004).
//! T047: [`first_quote_text`], [`notice`] (the 3-hour gap), [`list_texts`], [`cards_out_text`] and
//! [`arizona_time`] build the homeowner's quote texts; `intake` sends them (TXT-016, TXT-017,
//! TXT-031 to TXT-033, WORD-011).
//! T052: [`page_quotes`] lists a job's shown quotes in arrival order, unavailable ones included,
//! for the homeowner's quote page (PICK-008, D-1233).
//! T070: [`quoted_prices`] reads each quote's price for `picks`' PICK-026 loss text (PICK-038).
//! T213: [`job_page`] reads a job's business [`JobPage`] from [`PageParts`] (CARD-019, CARD-023,
//! D-434); [`check`] and [`scan`] hold the homeowner update texts in [`UPDATE_TEXTS`] to TXT-040's
//! [`SHORT`] and [`LIMIT`] words a sentence, her page link and no quote details.
//! T091: [`card_photo`] makes a [`CardPhoto`] from a cleaned photo only after the homeowner's
//! [`HomeownerAnswer::Confirmed`] and, when [`PhotoAiGate`] allows AI after zero retention, both
//! [`ScreenCheck`]s through a [`PhotoScreener`] (AI-016, CARD-013, CARD-014).
//! T193: [`open_card_jobs`] lists a business's carded jobs still waiting for quotes (BIZ-031).
//! T148: [`record_quote_minutes`] stores the optional "minutes this quote took" (PICK-040, OPS-047).
//! T209: [`tick_auto_confirm`], [`untick_auto_confirm`] and [`auto_confirm_of`] store and read the
//! quote's "Auto-confirm and pay if picked" tick, worded by [`auto_confirm_authorization`];
//! [`stripe_answer_filed`] reads the owner's filed Stripe answer (PICK-048, PICK-056, D-401).
//! T164: [`format_for`] names a job type's quote format and [`parse_price`] reads the form's
//! [`PriceFields`] for it, a per-visit price with its [`VisitSchedule`] (PICK-053, REC-010);
//! [`check_note`] and [`screen_note`] send a note back with its reason (PICK-052);
//! [`save_quote`] and [`withdraw_quote`] store a quote, an edit or a withdrawal under the job's
//! row lock, never after a pick (PICK-051, D-2912); [`format_definition`] is the plain definition.

use sqlx::types::Uuid;

pub use crate::auto_confirm_box::{
    AUTO_CONFIRM_VERSION, AutoConfirmError, AutoConfirmTick, Untick, auto_confirm_authorization,
    auto_confirm_of, stripe_answer_filed, tick_auto_confirm, untick_auto_confirm,
};
pub use crate::capability_match::{
    CapabilitySkip, JobFit, band_floor_cents, capability_skip, capable_pool,
};
pub use crate::card_notices::{
    CardNotice, QUOTE_HOURS, card_payload, job_page_prefix, queue_card_notice, sent_count,
};
pub use crate::cards::{CardParts, DEFAULT_MIN_WORDS, Danger, JobCard, Place, Timing, build_card};
pub use crate::edit_withdraw::{Saved, Withdrawal, save_quote, withdraw_quote};
pub use crate::emergency_card_notice::{
    CardTiming, EMERGENCY_FIRST_LINE, emergency_card_subject, queue_emergency_card, split,
};
pub use crate::formats::{
    OTHER_SCHEDULE, PriceFields, PriceProblem, VisitSchedule, format_for, parse_price,
    visit_schedule_of,
};
pub use crate::gate::final_gate;
pub use crate::handlers::{WINDOWS_KIND, WindowsCheck, run_windows_end, windows_ended};
pub use crate::intake::{
    Bounce, DayPart, Format, Period, Price, QUOTE_WINDOW, Quote, QuoteForm, QuoteRules, Submitted,
    confirm_quote, confirm_quote_at, mark_unavailable, parse_quote, shown_quotes, submit_quote,
    window_open,
};
pub use crate::job_page::{JobPage, PageParts, job_page};
pub use crate::models::{
    CardBusiness, CardDecision, CardJob, CardLines, Coverage, OfferTown, OwnerPauses, Pool, Pooled,
    QuotesError, Recipient, Skip,
};
pub use crate::notes::{
    NOTE_LIMIT, NoteFlag, NoteProblem, NoteScreener, OFF_PLATFORM, check_note, screen_note,
};
pub use crate::notify::{
    CardsOut, Counts, LIST_PAGE, Listed, Notice, Reader, SLOW_AT_MOST, Sent, UPDATE_GAP,
    arizona_time, cards_out_text, first_quote_text, list_texts, list_texts_from, notice,
    notice_for, update_text, which_job_text,
};
pub use crate::photos::{
    CardPhoto, CardPhotoOutcome, HomeownerAnswer, LeftOff, PhotoAiGate, PhotoScreener, ScreenCheck,
    ScreenFailed, ScreenVerdict, card_photo,
};
pub use crate::quote_card::{
    CardSource, POLICY_SUMMARY_SENTENCE, Parts, Permit, QuoteCard, QuoteFacts, Spanish, Summary,
    SummarySetting, SwitchRefused, TripFee, check_summary, format_definition, quote_card,
    switch_on,
};
pub use crate::service::{card_holders, coverage, send_cards, send_job_cards, send_late_cards};
pub use crate::update_texts::{
    LIMIT, NOT_UPDATE_TEXTS, Problem, SHORT, Shape, UPDATE_TEXTS, check, scan,
};

/// One shown quote as the homeowner's quote page lists it (PICK-008, D-1233). No homeowner or
/// business contact detail: the wording passed the PICK-007 bounce before it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageQuote {
    /// The business that quoted.
    pub business: Uuid,
    /// When the quote first arrived; an edit keeps it, so its place never moves (PICK-051).
    pub received_at: jiff::Timestamp,
    /// `picks` marked it after a failed pick, so it shows "no longer available" (JOB-025, D-084).
    pub unavailable: bool,
    /// The business's own words (PICK-003).
    pub original: String,
}

/// PICK-008, D-084: `job`'s confirmed, unwithdrawn quotes in arrival order (time received, then
/// business ID), the unavailable ones kept in their place so no number moves.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn page_quotes(
    db: &mut sqlx::PgConnection,
    job: Uuid,
) -> Result<Vec<PageQuote>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT business_id, received_at, unavailable_at IS NOT NULL AS \"unavailable!\", \
         original FROM quotes.quote WHERE job_id = $1 AND confirmed AND withdrawn_at IS NULL \
         ORDER BY received_at, business_id",
        job
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| PageQuote {
            business: r.business_id,
            received_at: r.received_at.to_jiff(),
            unavailable: r.unavailable,
            original: r.original,
        })
        .collect())
}

/// One stored quote's price columns, as [`quoted_prices`] reads them.
struct StoredPrice {
    business_id: Uuid,
    format: String,
    amount_cents: Option<i64>,
    low_cents: Option<i64>,
    high_cents: Option<i64>,
    tier_names: Option<Vec<String>>,
    tier_cents: Option<Vec<i64>>,
    period: Option<String>,
    setup_cents: Option<i64>,
    hourly_rate_cents: Option<i64>,
    hours_low: Option<i32>,
    hours_high: Option<i32>,
    confirm_on_site: bool,
    withdrawn: bool,
}

/// One business's confirmed quote on a job, as [`quoted_prices`] returns it (T070, D-1572, D-1576).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotedPrice {
    /// The business that quoted.
    pub business: Uuid,
    /// Its price as entered; none for a row whose stored price does not read back.
    pub price: Option<Price>,
    /// Whether the business withdrew the quote (PICK-051).
    pub withdrawn: bool,
}

fn cents(v: Option<i64>) -> Option<u64> {
    v.and_then(|v| u64::try_from(v).ok())
}

impl StoredPrice {
    /// The price as the business entered it (PICK-002), or none for a row that does not read back.
    fn price(self) -> Option<Price> {
        let hours = |v: Option<i32>| v.and_then(|v| u32::try_from(v).ok());
        if let Some(rate) = cents(self.hourly_rate_cents) {
            let hours = hours(self.hours_low).zip(hours(self.hours_high));
            return Some(Price::Hourly { rate, hours });
        }
        let periods = [
            Period::Visit,
            Period::Week,
            Period::Month,
            Period::Quarter,
            Period::Year,
        ];
        Some(match Format::from_code(&self.format)? {
            Format::Firm => Price::Firm(cents(self.amount_cents)?),
            Format::DiagnosticVisit => Price::VisitFee(cents(self.amount_cents)?),
            Format::Range => Price::Range {
                low: cents(self.low_cents)?,
                high: cents(self.high_cents)?,
                on_site: self.confirm_on_site,
            },
            Format::Monthly => Price::Monthly {
                cents: cents(self.amount_cents)?,
                period: periods
                    .into_iter()
                    .find(|p| Some(p.code()) == self.period.as_deref())?,
                setup: cents(self.setup_cents),
            },
            Format::Tiers => {
                let prices = self.tier_cents?.into_iter().map(|v| cents(Some(v)));
                let tiers = self.tier_names?.into_iter().zip(prices);
                Price::Tiers(tiers.map(|(n, v)| Some((n, v?))).collect::<Option<_>>()?)
            }
        })
    }
}

/// PICK-026, PICK-038 (T070, D-1572, D-1576): every business with a confirmed quote on `job`,
/// withdrawn and unavailable ones included, with its price as entered (none when the stored price
/// does not read back) and whether it was withdrawn, in arrival order; no amount ever reaches
/// `picks`' loss text.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn quoted_prices(
    db: &mut sqlx::PgConnection,
    job: Uuid,
) -> Result<Vec<QuotedPrice>, QuotesError> {
    let rows = sqlx::query_as!(
        StoredPrice,
        r#"SELECT business_id, format, amount_cents, low_cents, high_cents, tier_names, tier_cents,
         period, setup_cents, hourly_rate_cents, hours_low, hours_high, confirm_on_site,
         withdrawn_at IS NOT NULL AS "withdrawn!"
         FROM quotes.quote WHERE job_id = $1 AND confirmed
         ORDER BY received_at, business_id"#,
        job
    )
    .fetch_all(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| QuotedPrice {
            business: r.business_id,
            withdrawn: r.withdrawn,
            price: r.price(),
        })
        .collect())
}

/// PICK-040, OPS-047, D-492 (T148): stores the optional "minutes this quote took" on `business`'s
/// quote for `job`, for the pilot scorecard only (never on a quote card); `false` when there is
/// no such quote. Call it in the submitting transaction.
///
/// # Errors
///
/// [`QuotesError::Database`], also for minutes outside 1 to 600 (the column's check).
pub async fn record_quote_minutes(
    db: &mut sqlx::PgConnection,
    (job, business): (Uuid, Uuid),
    minutes: i32,
) -> Result<bool, QuotesError> {
    let done = sqlx::query!(
        "UPDATE quotes.quote SET quote_minutes = $3 WHERE job_id = $1 AND business_id = $2",
        job,
        business,
        minutes
    )
    .execute(db)
    .await
    .map_err(|_| QuotesError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// BIZ-031, CARD-018, D-462 (T193): the jobs `business` holds a card for that are still waiting for
/// quotes (Open or No quotes yet, `JobStatus::takes_late_cards`), oldest card first, for the
/// sign-up done screen's "<k> open <trade> jobs you can quote now".
///
/// # Errors
///
/// [`QuotesError::Database`], or [`QuotesError::Job`] when a job cannot be read.
pub async fn open_card_jobs(
    db: &mut sqlx::PgConnection,
    business: Uuid,
) -> Result<Vec<Uuid>, QuotesError> {
    let jobs = sqlx::query_scalar!(
        "SELECT job_id FROM quotes.job_card WHERE business_id = $1 ORDER BY sent_at, job_id",
        business
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| QuotesError::Database)?;
    let mut open = Vec::new();
    for job in jobs {
        let row = askhava_jobs::api::current(db, job).await;
        if row.map_err(|_| QuotesError::Job)?.status.takes_late_cards() {
            open.push(job);
        }
    }
    Ok(open)
}

/// JOB-015 (T278, D-1842): the start of `business`'s quote's earliest visit date on `job`, in
/// Arizona time, for the day-3 check-in, which counts from the later of the release and this
/// date; `None` when the quote gave no date or there is no such quote.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn earliest_visit(
    db: &mut sqlx::PgConnection,
    (job, business): (Uuid, Uuid),
) -> Result<Option<jiff::Timestamp>, QuotesError> {
    let visit = sqlx::query_scalar!(
        "SELECT earliest_visit::text FROM quotes.quote WHERE job_id = $1 AND business_id = $2",
        job,
        business
    )
    .fetch_optional(db)
    .await
    .map_err(|_| QuotesError::Database)?
    .flatten();
    let day = visit.and_then(|d| d.parse::<jiff::civil::Date>().ok());
    Ok(day.and_then(|d| d.in_tz("America/Phoenix").ok().map(|z| z.timestamp())))
}

/// The pure matching rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{card_for, card_under, coverage, match_job, match_job_held, recipients};
}
