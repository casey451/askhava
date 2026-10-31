//! Who receives a job card (CARD-001 to CARD-003, LIC-035, LIC-043). Pure functions, no database.
//!
//! A business receives the card when it is active and verified, serves one of the job's Arizona
//! offer towns (its own town or a neighbouring one, by the same rules, CARD-002), ticked at least
//! one candidate job type, and its licenses cover every candidate job type: ROC cover comes only
//! from the license map (`licensing::api::ClassMap`, so a subclassification gets only its own
//! scope, LIC-035) and every other license rule from `licensing::api::eligibility_for_request`.
//! There is no cap on recipients (CARD-003). The largest-job and recurring-work skips of CARD-001
//! (BIZ-049) are T155's (D-492).

use askhava_licensing::api::{ClassMap, Eligibility, Job, JobRequest, eligibility_for_request};

use crate::models::{CardBusiness, CardDecision, CardJob, Recipient, Skip};

/// CARD-001, CARD-002, LIC-035, LIC-043: whether `business` receives `job`'s card, and as a
/// handyman when any candidate is a handyman card. Checks run in the order of [`Skip`].
#[must_use]
pub fn card_for(map: &ClassMap, job: &CardJob, business: &CardBusiness) -> CardDecision {
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
    if !job.candidates.iter().any(|c| ticked(business, c)) {
        return CardDecision::Skip(Skip::NothingTicked);
    }
    covered(map, job, business)
}

/// CARD-001, CARD-003: every business that receives the card, in the order given; no cap.
#[must_use]
pub fn recipients(map: &ClassMap, job: &CardJob, businesses: &[CardBusiness]) -> Vec<Recipient> {
    businesses
        .iter()
        .filter_map(|b| match card_for(map, job, b) {
            CardDecision::Card => Some((b, false)),
            CardDecision::HandymanCard => Some((b, true)),
            CardDecision::Skip(_) => None,
        })
        .map(|(b, handyman)| Recipient {
            business: b.id,
            handyman,
        })
        .collect()
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
