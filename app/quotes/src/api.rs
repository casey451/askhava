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
//! T046: [`parse_quote`] checks a [`QuoteForm`] into a [`Quote`] or a [`Bounce`] (PICK-002,
//! PICK-007, FEE-043, FEE-044); [`submit_quote`] stores it inside the business's own 24 hours
//! ([`window_open`], PICK-001); [`confirm_quote`] is the read-back's confirm (PICK-006);
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

use sqlx::types::Uuid;

pub use crate::cards::{CardParts, DEFAULT_MIN_WORDS, Danger, JobCard, Place, Timing, build_card};
pub use crate::gate::final_gate;
pub use crate::handlers::{WINDOWS_KIND, WindowsCheck, run_windows_end, windows_ended};
pub use crate::intake::{
    Bounce, DayPart, Format, Period, Price, QUOTE_WINDOW, Quote, QuoteForm, QuoteRules, Submitted,
    confirm_quote, mark_unavailable, parse_quote, shown_quotes, submit_quote, window_open,
};
pub use crate::models::{
    CardBusiness, CardDecision, CardJob, CardLines, Coverage, OfferTown, OwnerPauses, Pool, Pooled,
    QuotesError, Recipient, Skip,
};
pub use crate::notify::{
    CardsOut, Counts, LIST_PAGE, Listed, Notice, Reader, SLOW_AT_MOST, Sent, UPDATE_GAP,
    arizona_time, cards_out_text, first_quote_text, list_texts, list_texts_from, notice,
    notice_for, update_text, which_job_text,
};
pub use crate::quote_card::{
    CardSource, POLICY_SUMMARY_SENTENCE, Parts, Permit, QuoteCard, QuoteFacts, Spanish, Summary,
    SummarySetting, SwitchRefused, TripFee, check_summary, quote_card, switch_on,
};
pub use crate::service::{card_holders, coverage, send_cards, send_job_cards, send_late_cards};

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

/// The pure matching rules, for the rules bridge and other crates' property tests.
pub mod rules {
    pub use crate::rules::{card_for, card_under, coverage, match_job, match_job_held, recipients};
}
