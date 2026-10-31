//! Who receives a job card (CARD-001 to CARD-003, LIC-035, LIC-043). Pure functions, no database.
//!
//! A business receives the card when it is active and verified, serves one of the job's Arizona
//! offer towns (its own town or a neighbouring one, by the same rules, CARD-002), ticked at least
//! one candidate job type, and its licenses cover every candidate job type: ROC cover comes only
//! from the license map (`licensing::api::ClassMap`, so a subclassification gets only its own
//! scope, LIC-035) and every other license rule from `licensing::api::eligibility_for_request`.
//! There is no cap on recipients (CARD-003). The largest-job and recurring-work skips of CARD-001
//! (BIZ-049) are T155's (D-492).
//!
//! T184 added [`match_job`]: an owner pause for everything, a candidate's trade or every offer town
//! a business serves skips it (CARD-016); when no business's checklist gets the card, it goes to
//! the licensed businesses with "jobs nobody else covers" on, ticks aside (CARD-004, D-1018); and
//! [`coverage`] counts the same match for the INT-017 line or offers INT-032's SPLIT (D-1019).

use std::collections::BTreeSet;

use askhava_licensing::api::{ClassMap, Eligibility, Job, JobRequest, eligibility_for_request};

use crate::models::{
    CardBusiness, CardDecision, CardJob, Coverage, OwnerPauses, Pool, Recipient, Skip,
};

/// CARD-001, CARD-002, LIC-035, LIC-043: whether `business` receives `job`'s card, and as a
/// handyman when any candidate is a handyman card. Checks run in the order of [`Skip`].
#[must_use]
pub fn card_for(map: &ClassMap, job: &CardJob, business: &CardBusiness) -> CardDecision {
    gate(map, job, business, true)
}

/// CARD-016, CARD-001, CARD-004: [`card_for`] under the owner pauses; with `overflow` the ticks
/// are not asked, as for a "jobs nobody else covers" card (every other check still applies).
#[must_use]
pub fn card_under(
    map: &ClassMap,
    job: &CardJob,
    pauses: &OwnerPauses,
    (business, overflow): (&CardBusiness, bool),
) -> CardDecision {
    if owner_paused(pauses, job, business) {
        return CardDecision::Skip(Skip::OwnerPaused);
    }
    gate(map, job, business, !overflow)
}

/// CARD-001, CARD-003, CARD-004, CARD-016: every business that receives the card: the checklist
/// match under the owner pauses, or, when that reaches nobody, every licensed business with
/// "jobs nobody else covers" on (D-1018). No cap.
#[must_use]
pub fn match_job(map: &ClassMap, job: &CardJob, pool: &Pool<'_>) -> Vec<Recipient> {
    let under = |overflow: bool| {
        pool.businesses
            .iter()
            .filter(move |p| !overflow || p.overflow_on)
            .filter_map(move |p| {
                chosen(
                    &p.card,
                    card_under(map, job, pool.pauses, (&p.card, overflow)),
                )
            })
            .collect::<Vec<_>>()
    };
    let checklist = under(false);
    if checklist.is_empty() {
        under(true)
    } else {
        checklist
    }
}

/// INT-017, INT-032, D-1019: how many businesses [`match_job`] reaches; when none, SPLIT if the
/// candidates span two or more trades and one trade's part alone reaches a business, otherwise
/// not covered.
#[must_use]
pub fn coverage(map: &ClassMap, job: &CardJob, pool: &Pool<'_>) -> Coverage {
    let n = match_job(map, job, pool).len();
    if n > 0 {
        return Coverage::Send(n);
    }
    let trades: BTreeSet<&str> = job.candidates.iter().map(|c| c.category.as_str()).collect();
    let part_reaches = |trade: &&str| {
        let part = CardJob {
            candidates: job
                .candidates
                .iter()
                .filter(|c| c.category == *trade)
                .cloned()
                .collect(),
            ..job.clone()
        };
        !match_job(map, &part, pool).is_empty()
    };
    if trades.len() > 1 && trades.iter().any(part_reaches) {
        Coverage::Split
    } else {
        Coverage::NotCovered
    }
}

/// CARD-016, OPS-003: the "all" switch, a candidate's trade switch, or every Arizona offer town
/// the business serves is paused.
fn owner_paused(pauses: &OwnerPauses, job: &CardJob, business: &CardBusiness) -> bool {
    let mut served = job
        .towns
        .iter()
        .filter(|t| t.state.trim().eq_ignore_ascii_case("AZ") && business.towns.contains(&t.code))
        .peekable();
    let towns_paused = served.peek().is_some() && served.all(|t| pauses.towns.contains(&t.code));
    pauses.all
        || towns_paused
        || job
            .candidates
            .iter()
            .any(|c| pauses.trades.contains(&c.category))
}

/// The checks of CARD-001 in the order of [`Skip`]; `checklist` false leaves out the ticks.
fn gate(map: &ClassMap, job: &CardJob, business: &CardBusiness, checklist: bool) -> CardDecision {
    if !business.status.receives_cards() {
        return CardDecision::Skip(Skip::NotActive);
    }
    if !business.verified {
        return CardDecision::Skip(Skip::NotVerified);
    }
    if !serves(job, business) {
        return CardDecision::Skip(Skip::OutOfTown);
    }
    if job.candidates.is_empty() {
        return CardDecision::Skip(Skip::NoCandidates);
    }
    if checklist && !job.candidates.iter().any(|c| ticked(business, c)) {
        return CardDecision::Skip(Skip::NothingTicked);
    }
    covered(map, job, business)
}

/// CARD-001, CARD-003: every business that receives the card, in the order given; no cap.
#[must_use]
pub fn recipients(map: &ClassMap, job: &CardJob, businesses: &[CardBusiness]) -> Vec<Recipient> {
    businesses
        .iter()
        .filter_map(|b| chosen(b, card_for(map, job, b)))
        .collect()
}

/// The recipient a decision makes, if any.
fn chosen(business: &CardBusiness, decision: CardDecision) -> Option<Recipient> {
    let handyman = match decision {
        CardDecision::Card => false,
        CardDecision::HandymanCard => true,
        CardDecision::Skip(_) => return None,
    };
    Some(Recipient {
        business: business.id,
        handyman,
    })
}

/// CARD-001, CARD-002, INT-022: the business serves one of the job's offer towns that is in
/// Arizona; a town outside Arizona is never offered.
fn serves(job: &CardJob, business: &CardBusiness) -> bool {
    job.towns
        .iter()
        .any(|t| t.state.trim().eq_ignore_ascii_case("AZ") && business.towns.contains(&t.code))
}

/// CARD-001, BIZ-004: the business ticked this job type in this category.
fn ticked(business: &CardBusiness, job: &Job) -> bool {
    business
        .ticked
        .contains(&(job.category.clone(), job.job_type.clone()))
}

/// CARD-001, LIC-035, LIC-043: every candidate must be a card; one handyman card makes the whole
/// card a handyman card, and the first uncovered candidate's reason is the skip.
fn covered(map: &ClassMap, job: &CardJob, business: &CardBusiness) -> CardDecision {
    let candidates: Vec<String> = job.candidates.iter().map(|c| c.job_type.clone()).collect();
    let request = JobRequest {
        text: &job.text,
        candidates: &candidates,
    };
    let classes: Vec<&str> = business.classes.iter().map(String::as_str).collect();
    let mut handyman = false;
    for candidate in &job.candidates {
        let mut holder = business.licenses.clone();
        holder.roc_covers = map.covers(&classes, &candidate.category, &candidate.job_type);
        match eligibility_for_request(candidate, &holder, &request) {
            Eligibility::Card => {}
            Eligibility::HandymanCard => handyman = true,
            Eligibility::NoCard(reason) => return CardDecision::Skip(Skip::NotCovered(reason)),
        }
    }
    if handyman {
        CardDecision::HandymanCard
    } else {
        CardDecision::Card
    }
}
