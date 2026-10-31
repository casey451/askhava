//! Property tests of the handyman block and limits (TD-LIC-09, TD-LIC-08, TD-LIC-10; AI-012,
//! LIC-019, LIC-021, LIC-022, LIC-023; rust-production R17).
//!
//! For any job, any business and any AI reading: a request whose text holds a gas keyword, or
//! whose candidates include a gas, smoke-alarm, sprinkler or permit-type job type, never gets a
//! handyman card. A handyman construction quote is accepted exactly when certified and under
//! $1,000 at its top, and two near jobs whose tops reach $1,000 are always blocked. Every text is
//! fake.

use std::collections::BTreeSet;

use proptest::prelude::*;

use crate::api::{
    CityListStanding, Eligibility, HANDYMAN_LIMIT_CENTS, HandymanJob, Holder, Job, JobRequest,
    LicenseKind, QuotePrice, QuoteRefusal, QuoteSubmission, SPLIT_JOB_WINDOW_DAYS, check_quote,
    eligibility_for_request, split_job,
};

/// Words from the gas list (AI-012), English and Spanish, as homeowners might write them.
const GAS_WORDS: [&str; 8] = [
    "gas",
    "Propane",
    "pilot light",
    "won't light",
    "furnace",
    "propano",
    "piloto",
    "GAS LINE",
];

/// Job types never sent to a handyman (LIC-021, LIC-022).
const BLOCKED_TYPES: [&str; 6] = [
    "hardwired smoke alarm",
    "fire sprinklers",
    "water heater replacement",
    "panel upgrade",
    "new circuit",
    "re-roof",
];

/// Handyman-category job types: what an AI might wrongly read the job as.
const HANDYMAN_TYPES: [&str; 5] = [
    "drywall patch",
    "door repair",
    "shelves",
    "fence repair",
    "dryer vent reroute",
];

fn handyman_job(job_type: &str) -> Job {
    Job {
        category: "handyman".to_owned(),
        job_type: job_type.to_owned(),
        license: LicenseKind::None,
        handyman_allowed: true,
        listed: true,
    }
}

fn handyman() -> Holder {
    Holder {
        handyman: true,
        city_list: CityListStanding::Matched,
        ..Holder::default()
    }
}

proptest! {
    /// TD-LIC-09 (AI-012, LIC-021, D-157): a gas word anywhere in the request text blocks the
    /// handyman card, whatever job type the AI chose.
    #[test]
    fn gas_text_never_reaches_a_handyman(
        before in "[a-z ]{0,20}",
        after in "[a-z ]{0,20}",
        word in 0..GAS_WORDS.len(),
        ai_type in 0..HANDYMAN_TYPES.len(),
    ) {
        let text = format!("{before} {} {after}", GAS_WORDS[word]);
        let candidates = vec![HANDYMAN_TYPES[ai_type].to_owned()];
        let request = JobRequest { text: &text, candidates: &candidates };
        let job = handyman_job(HANDYMAN_TYPES[ai_type]);
        prop_assert_ne!(eligibility_for_request(&job, &handyman(), &request), Eligibility::HandymanCard);
    }

    /// TD-LIC-09 (LIC-021, LIC-022): any candidate list holding a blocked job type never gives a
    /// handyman card, in any position and beside any handyman job types.
    #[test]
    fn blocked_candidates_never_reach_a_handyman(
        blocked in 0..BLOCKED_TYPES.len(),
        others in prop::collection::vec(0..HANDYMAN_TYPES.len(), 0..4),
        at in 0usize..4,
        ai_type in 0..HANDYMAN_TYPES.len(),
    ) {
        let mut candidates: Vec<String> = others.iter().map(|i| HANDYMAN_TYPES[*i].to_owned()).collect();
        candidates.insert(at.min(candidates.len()), BLOCKED_TYPES[blocked].to_owned());
        let request = JobRequest { text: "please help", candidates: &candidates };
        let job = handyman_job(HANDYMAN_TYPES[ai_type]);
        prop_assert_ne!(eligibility_for_request(&job, &handyman(), &request), Eligibility::HandymanCard);
    }

    /// TD-LIC-08 (LIC-019, LIC-025): a handyman construction quote passes exactly when it is
    /// certified and its top is under $1,000.00.
    #[test]
    fn handyman_quote_accepted_only_certified_and_under_the_limit(
        low in 0u64..300_000,
        spread in 0u64..300_000,
        certified in any::<bool>(),
    ) {
        let price = QuotePrice::Range { low, high: low + spread };
        let quote = QuoteSubmission {
            construction_type: true,
            license: LicenseKind::None,
            handyman: true,
            price,
            certified,
        };
        let accepted = certified && low + spread < HANDYMAN_LIMIT_CENTS;
        prop_assert_eq!(check_quote(&quote).is_ok(), accepted);
    }

    /// TD-LIC-10 (LIC-023): two jobs under 30 days apart whose tops reach $1,000 are blocked;
    /// 30 days or more apart they never are.
    #[test]
    fn split_job_blocks_near_jobs_reaching_the_limit(
        first in 0u64..100_000,
        second in 0u64..100_000,
        gap in 0i64..(60 * 86_400),
    ) {
        let job = |name: &str, cents: u64, at: i64| HandymanJob {
            job_types: BTreeSet::from([name.to_owned()]),
            price: QuotePrice::Fixed(cents),
            submitted_at: at,
        };
        let earlier = job("shelves", first, 0);
        let later = job("door repair", second, gap);
        let blocked = gap < SPLIT_JOB_WINDOW_DAYS * 86_400 && first + second >= HANDYMAN_LIMIT_CENTS;
        let outcome = split_job(&later, &[earlier], SPLIT_JOB_WINDOW_DAYS);
        prop_assert_eq!(outcome == Err(QuoteRefusal::SplitJob), blocked);
    }
}
