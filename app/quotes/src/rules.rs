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
//! T184 added [`match_job`]: an owner pause for everything, a candidate's trade, the job's own
//! town or every offer town a business serves skips it (CARD-016); when no business's checklist
//! gets the card, it goes to the licensed businesses with "jobs nobody else covers" on, ticks
//! aside (CARD-004, D-1018); and [`coverage`] counts the same match for the INT-017 line or
//! offers INT-032's SPLIT (D-1019). [`match_job_held`] is the rerun: a card holder whose checklist
//! covers the job keeps overflow off.

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_licensing::api::{ClassMap, Eligibility, Job, JobRequest, eligibility_for_request};
use sqlx::types::Uuid;

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
/// "jobs nobody else covers" on (D-1018). No cap. The first send of a job; a rerun uses
/// [`match_job_held`].
#[must_use]
pub fn match_job(map: &ClassMap, job: &CardJob, pool: &Pool<'_>) -> Vec<Recipient> {
    match_job_held(map, job, pool, &[])
}

/// CARD-004, CARD-018, TD-LIC-03, D-1018: [`match_job`] for a job whose cards `held` (business
/// IDs) already went out. Overflow also stays off when a holder's checklist covers the job, even
/// if that holder is now paused or inactive: one business ticked it, so overflow doesn't apply.
#[must_use]
pub fn match_job_held(
    map: &ClassMap,
    job: &CardJob,
    pool: &Pool<'_>,
    held: &[Uuid],
) -> Vec<Recipient> {
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
    let covered_by_holder = || {
        pool.businesses
            .iter()
            .any(|p| held.contains(&p.card.id) && checklist_covers(map, job, &p.card))
    };
    if checklist.is_empty() && !covered_by_holder() {
        under(true)
    } else {
        checklist
    }
}

/// CARD-004, D-1018: the business's checklist covers the job whatever its status or the owner
/// pauses: verified, serving an offer town, licensed and ticking a candidate.
fn checklist_covers(map: &ClassMap, job: &CardJob, business: &CardBusiness) -> bool {
    let active = CardBusiness {
        status: AccountStatus::Active,
        ..business.clone()
    };
    chosen(business, gate(map, job, &active, true)).is_some()
}

/// INT-017, INT-032, D-1019: how many businesses [`match_job`] reaches; when none, SPLIT only if
/// no active, verified business serving an offer town is licensed for every candidate job type
/// (ticks, overflow switch and owner pauses aside), the candidates span two or more trades and one
/// trade's part alone reaches a business; otherwise not covered.
#[must_use]
pub fn coverage(map: &ClassMap, job: &CardJob, pool: &Pool<'_>) -> Coverage {
    let n = match_job(map, job, pool).len();
    if n > 0 {
        return Coverage::Send(n);
    }
    let one_license_covers_all = pool
        .businesses
        .iter()
        .any(|p| chosen(&p.card, gate(map, job, &p.card, false)).is_some());
    if one_license_covers_all {
        return Coverage::NotCovered;
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

/// CARD-016, OPS-003, D-1018: the "all" switch, a candidate's trade switch, the switch of the
/// job's own town (the first offer town: a paused town takes no new jobs), or the switch of every
/// Arizona offer town the business serves is on.
fn owner_paused(pauses: &OwnerPauses, job: &CardJob, business: &CardBusiness) -> bool {
    let own_town_paused = job
        .towns
        .first()
        .is_some_and(|t| pauses.towns.contains(&t.code));
    let mut served = job
        .towns
        .iter()
        .filter(|t| t.state.trim().eq_ignore_ascii_case("AZ") && business.towns.contains(&t.code))
        .peekable();
    let towns_paused = served.peek().is_some() && served.all(|t| pauses.towns.contains(&t.code));
    pauses.all
        || own_town_paused
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
