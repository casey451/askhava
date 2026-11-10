//! The capability skips of CARD-001 left to T155 (BIZ-049, CARD-001; D-393, D-492). Pure
//! functions, no database.
//!
//! A business is skipped when its largest-job answer is below a size or budget band the homeowner
//! gave (the band's floor is above the limit, so the band lies entirely above it), or when it
//! answered no to recurring work and a candidate job type is recurring. A business that has not
//! answered (no `businesses.capability` row) is skipped by neither. [`capable_pool`] drops the
//! skipped businesses before `rules::match_job` runs, so the skips hold for checklist and
//! overflow cards alike; the caller reads the answers with `businesses::api::capabilities` and the
//! recurring flag with `businesses::api::is_recurring`.

use std::collections::BTreeMap;

use askhava_businesses::api::Capability;
use sqlx::types::Uuid;

use crate::models::Pooled;

/// What the capability skips know of a job (CARD-001, BIZ-049).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobFit {
    /// The floor of the size or budget band the homeowner gave, in cents; `None` when no band
    /// answer was given.
    pub band_floor_cents: Option<u64>,
    /// A candidate job type is recurring work.
    pub recurring: bool,
}

/// Why a business's capability answers skip a card (CARD-001, BIZ-049).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilitySkip {
    /// The band's floor is above the business's largest job.
    AboveLargestJob,
    /// The job is recurring work and the business answered no to it.
    NoRecurringWork,
}

/// CARD-001, BIZ-049: the skip `answers` give for `fit`, if any; no answers, no skip.
#[must_use]
pub fn capability_skip(fit: JobFit, answers: Option<&Capability>) -> Option<CapabilitySkip> {
    let cap = answers?;
    if fit
        .band_floor_cents
        .is_some_and(|floor| cap.largest.below(floor))
    {
        return Some(CapabilitySkip::AboveLargestJob);
    }
    (fit.recurring && !cap.recurring).then_some(CapabilitySkip::NoRecurringWork)
}

/// CARD-001, BIZ-049: the pool without the businesses [`capability_skip`] skips, in the order
/// given; pass the result to `rules::match_job`.
#[must_use]
pub fn capable_pool(
    pool: &[Pooled],
    fit: JobFit,
    answers: &BTreeMap<Uuid, Capability>,
) -> Vec<Pooled> {
    pool.iter()
        .filter(|p| capability_skip(fit, answers.get(&p.card.id)).is_none())
        .cloned()
        .collect()
}

/// BIZ-049: a band floor as the question bank writes it ("5000.00", "0") in cents; `None` for
/// text that is not a plain dollar amount.
#[must_use]
pub fn band_floor_cents(text: &str) -> Option<u64> {
    let (whole, part) = text.trim().split_once('.').unwrap_or((text.trim(), "0"));
    let digits = |s: &str| !s.is_empty() && s.len() <= 9 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !digits(part) || part.len() > 2 {
        return None;
    }
    let cents = format!("{part:0<2}").parse::<u64>().ok()?;
    Some(whole.parse::<u64>().ok()? * 100 + cents)
}
