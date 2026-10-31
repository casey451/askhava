//! Routing confidence (INT-040, AI-017, D-395, D-477, D-478): when intake goes on, when it asks
//! the INT-013 trade question and then the INT-014 job-type question, and when a job goes out
//! marked "Job type not certain".
//!
//! The keyword rules, and the AI reading while AI-008 allows it, each give a trade and job type
//! with a confidence ([`keyword_scored`] for the rules). [`decide`] goes on only when the readings
//! agree at or above the high-confidence setting ([`HIGH_CONFIDENCE`] by default); otherwise it
//! builds the trade question (`matching::second_opinion`'s options). After both questions,
//! [`after_fit`] turns `Candidates::fit_step`'s result into what goes out: one chosen job type, or
//! every remaining candidate marked [`NOT_CERTAIN`], which `quotes::api::rules::match_job` sends
//! only to a business whose license covers every candidate (CARD-001), so never to one covering
//! part of the job or outside the trade. [`unsure_reach`] says when none does: no card goes, and
//! the homeowner's text and the owner's OPS-041 email are T215's (D-493). No license class is
//! read or guessed here (AI-017).

use crate::matching::{FitStep, KeywordReading, Opinion, Reading, TradeQuestion, second_opinion};

/// The high-confidence setting's default (INT-040, D-1038): both readings at or above it.
pub const HIGH_CONFIDENCE: f64 = 0.8;
/// The card marker for a job sent with every remaining candidate (INT-040, CARD-007).
pub const NOT_CERTAIN: &str = "Job type not certain";
/// The keyword rules' confidence for one trade and one job type named (D-1038).
const KEYWORD_SURE: f64 = 1.0;
/// The keyword rules' confidence for a trade with no single job type (D-1038).
const KEYWORD_TRADE_ONLY: f64 = 0.5;

/// One reading with its confidence, 0 to 1 (INT-040).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored<'a> {
    /// The trade and job type read.
    pub reading: Reading<'a>,
    /// How sure the reading is.
    pub confidence: f64,
}

/// INT-040: the keyword rules' reading: one trade naming exactly one job type is sure; one trade
/// with none or several is trade-only; several trades are the first named at no confidence.
#[must_use]
pub fn keyword_scored(kw: &KeywordReading) -> Option<Scored<'static>> {
    let trade = *kw.trades.first()?;
    let mut jobs: Vec<&'static str> = Vec::new();
    for &(t, j) in kw.job_types.iter().chain(&kw.licensed_only) {
        if t == trade && !jobs.contains(&j) {
            jobs.push(j);
        }
    }
    let (job_type, confidence) = match (kw.trades.len(), jobs.as_slice()) {
        (1, [one]) => (Some(*one), KEYWORD_SURE),
        (1, _) => (None, KEYWORD_TRADE_ONLY),
        _ => (None, 0.0),
    };
    Some(Scored {
        reading: Reading { trade, job_type },
        confidence,
    })
}

/// What routing does before any trade question (INT-040).
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// The readings agree at high confidence: go on with this trade and job type.
    GoOn {
        /// The catalog category ID.
        trade: String,
        /// The job type.
        job_type: String,
    },
    /// Ask INT-013's trade question, then INT-014's "Which fits best?".
    Ask(TradeQuestion),
    /// Neither the keywords nor the AI name a trade: the INT-016 path decides.
    NoTrade,
}

fn sure(s: &Scored<'_>, high: f64) -> bool {
    s.reading.job_type.is_some() && s.confidence >= high
}

/// INT-040: with AI on, both readings agree on trade and job type at or above `high`; with AI off
/// (`ai` is `None`), the keyword reading alone at or above it. Anything else asks, offering every
/// distinct trade of the readings and the keywords (`matching::second_opinion`, D-104).
#[must_use]
pub fn decide(kw: &KeywordReading, ai: Option<Scored<'_>>, high: f64) -> Decision {
    let keyword = keyword_scored(kw);
    let Some(first) = ai.or(keyword) else {
        return Decision::NoTrade;
    };
    let agreed = match (keyword, ai) {
        (Some(k), Some(a)) => sure(&k, high) && sure(&a, high) && k.reading == a.reading,
        (Some(k), None) => sure(&k, high),
        _ => false,
    };
    if let (true, Some(job_type)) = (agreed, first.reading.job_type) {
        return Decision::GoOn {
            trade: first.reading.trade.to_owned(),
            job_type: job_type.to_owned(),
        };
    }
    let second = keyword.unwrap_or(first).reading;
    Decision::Ask(match second_opinion([first.reading, second], kw) {
        Opinion::Ask(question) => question,
        Opinion::Settled(trade) => TradeQuestion {
            options: vec![trade.clone()],
            fallback: trade,
        },
    })
}

/// What goes out after both questions (INT-040, CARD-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// The (trade, job type) candidates the card carries; a business must cover every one.
    pub candidates: Vec<(String, String)>,
    /// True when the card is marked [`NOT_CERTAIN`].
    pub not_certain: bool,
}

/// INT-040: a chosen job type goes as itself; "Not sure" or a second unusable reply keeps every
/// candidate and marks the card when more than one remains; `None` while the question is re-asked.
#[must_use]
pub fn after_fit(step: &FitStep) -> Option<Outgoing> {
    let (candidates, kept) = match step {
        FitStep::Chosen(c) => (c, false),
        FitStep::KeepAll(c) => (c, true),
        FitStep::AskAgain(_) => return None,
    };
    Some(Outgoing {
        not_certain: kept && candidates.job_types.len() > 1,
        candidates: candidates.job_types.clone(),
    })
}

/// Whether an unsure job reaches anyone (INT-040, D-477, D-478).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsureReach {
    /// The marked card goes to this many businesses covering every candidate.
    Cards(usize),
    /// No business covers every candidate: no card goes; the homeowner is asked for more and the
    /// owner gets the OPS-041 email (T215's).
    NoneCovers,
}

/// INT-040: from the number of businesses `quotes::api::rules::match_job` reached.
#[must_use]
pub fn unsure_reach(recipients: usize) -> UnsureReach {
    if recipients == 0 {
        UnsureReach::NoneCovers
    } else {
        UnsureReach::Cards(recipients)
    }
}
