//! Property tests of the licensing rules (TD-LIC-06, TD-LIC-12, TD-LIC-17; LIC-011 to LIC-016,
//! LIC-028 to LIC-031, LIC-047; rust-production R17).
//!
//! For any business and any job: a handyman card needs a listed job type of a handyman-allowed
//! category, a handyman on the city list; a no-license card needs the city list; an agency
//! category reaches only its licensees; pest work opens only through an unpaused attestation with
//! the needed record category; a business with no license and no list match gets nothing. The
//! city-list match ignores case and punctuation but never matches a different name, and the
//! inspection fee is waived only for home inspections. Every name and number is fake.

use std::collections::BTreeSet;

use proptest::prelude::*;

use crate::api::{
    Agency, CityListStanding, CityRecord, Eligibility, Holder, InspectionFee, Job, LicenseKind,
    PestAttestation, PestRecordCategory, SaleAnswer, city_list_standing, eligibility,
    inspection_fee,
};

/// Categories the rules treat differently: the four agency ones, a no-license, a handyman-allowed
/// ROC one, the handyman category and one the rules do not map to an agency.
const CATEGORIES: [&str; 8] = [
    "pest_control",
    "home_inspection",
    "alarm_systems",
    "septic_pumping",
    "dryer_vent_cleaning",
    "pool_repair",
    "handyman",
    "fake_unmapped",
];

const JOB_TYPES: [&str; 4] = [
    "termites",
    "pre-emergent weed control",
    "general pest service",
    "fake job type",
];

const KINDS: [LicenseKind; 4] = [
    LicenseKind::None,
    LicenseKind::Roc,
    LicenseKind::StateAgency,
    LicenseKind::RocAndStateAgency,
];

fn job_strategy() -> impl Strategy<Value = Job> {
    (
        0..CATEGORIES.len(),
        0..JOB_TYPES.len(),
        0..KINDS.len(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(c, t, k, handyman_allowed, listed)| Job {
            category: CATEGORIES[c].to_owned(),
            job_type: JOB_TYPES[t].to_owned(),
            license: KINDS[k],
            handyman_allowed,
            listed,
        })
}

fn pest_strategy() -> impl Strategy<Value = Option<PestAttestation>> {
    proptest::option::of((any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
        |(wood, weed, paused)| {
            let attested = PestAttestation::new(true, "PMB-0000", wood, weed).expect("fake");
            if paused {
                attested.reported_wrong()
            } else {
                attested
            }
        },
    ))
}

fn holder_strategy() -> impl Strategy<Value = Holder> {
    let agencies = proptest::sample::subsequence(Agency::ALL.to_vec(), 0..=Agency::ALL.len());
    (
        any::<bool>(),
        agencies,
        pest_strategy(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(roc_covers, agencies, pest, handyman, matched)| Holder {
            roc_covers,
            agencies: agencies.into_iter().collect::<BTreeSet<_>>(),
            pest,
            handyman,
            city_list: if matched {
                CityListStanding::Matched
            } else {
                CityListStanding::Pending
            },
        })
}

/// Whether an outcome lets the business receive the card in any form.
fn receives(outcome: Eligibility) -> bool {
    match outcome {
        Eligibility::Card | Eligibility::HandymanCard => true,
        Eligibility::NoCard(_) => false,
    }
}

/// The LIC-012, LIC-047 oracle: an unpaused attestation holding the job type's record category.
fn pest_may_receive(job: &Job, holder: &Holder) -> bool {
    holder.pest.as_ref().is_some_and(|a| {
        !a.is_paused() && PestRecordCategory::needed_for(&job.job_type).is_none_or(|n| a.holds(n))
    })
}

proptest! {
    /// TD-LIC-12, TD-LIC-06 (LIC-011, LIC-014, LIC-028, LIC-030, LIC-031): a handyman card only
    /// ever goes to a handyman on the city list, for a listed job type of a handyman-allowed
    /// category; a refused job type never reaches a handyman.
    #[test]
    fn handyman_cards_need_a_listed_handyman_job_and_the_city_list(
        job in job_strategy(), holder in holder_strategy(),
    ) {
        if eligibility(&job, &holder) == Eligibility::HandymanCard {
            prop_assert!(holder.handyman && job.handyman_allowed && job.listed);
            prop_assert_eq!(holder.city_list, CityListStanding::Matched);
        }
    }

    /// TD-LIC-06 (LIC-011, D-177): a no-license job never reaches a business still pending on the
    /// city list, and a refused job type of a no-license category reaches nobody (pest is
    /// attestation-only whatever its kind, LIC-012).
    #[test]
    fn no_license_cards_need_the_city_list(job in job_strategy(), holder in holder_strategy()) {
        let job = Job { license: LicenseKind::None, ..job };
        let outcome = eligibility(&job, &holder);
        if receives(outcome) && job.category != "pest_control" {
            prop_assert_eq!(holder.city_list, CityListStanding::Matched);
            prop_assert!(job.listed);
        }
    }

    /// TD-LIC-06, TD-LIC-17 (LIC-012, LIC-013, LIC-015, LIC-016, LIC-047): an agency category
    /// reaches exactly its licensees; pest only through the attestation, never `agencies`, an ROC
    /// class or the city list; alarms also need ROC cover.
    #[test]
    fn agency_categories_reach_exactly_their_licensees(
        job in job_strategy(), holder in holder_strategy(), both in any::<bool>(),
    ) {
        let license = if both { LicenseKind::RocAndStateAgency } else { LicenseKind::StateAgency };
        let job = Job { license, ..job };
        let got = receives(eligibility(&job, &holder));
        let expected = match (Agency::for_category(&job.category), license) {
            (None, _) => false,
            (Some(Agency::PestManagement), _) => pest_may_receive(&job, &holder),
            (Some(agency), LicenseKind::RocAndStateAgency) => {
                holder.agencies.contains(&agency) && holder.roc_covers
            }
            (Some(agency), _) => holder.agencies.contains(&agency),
        };
        prop_assert_eq!(got, expected);
    }

    /// TD-LIC-17 (LIC-012, LIC-047, D-417): a pest job of any license kind reaches exactly the
    /// unpaused attested businesses holding its record category; `agencies`, an ROC class or the
    /// city list never open it.
    #[test]
    fn pest_jobs_of_any_kind_go_only_through_the_attestation(
        job in job_strategy(), holder in holder_strategy(),
    ) {
        let job = Job { category: "pest_control".to_owned(), ..job };
        prop_assert_eq!(receives(eligibility(&job, &holder)), pest_may_receive(&job, &holder));
    }

    /// TD-LIC-06 (LIC-011 to LIC-016): a business with no ROC cover, no agency license, no pest
    /// attestation and no city-list match receives no card of any kind.
    #[test]
    fn an_unlicensed_unlisted_business_gets_nothing(job in job_strategy(), handyman in any::<bool>()) {
        let holder = Holder { handyman, ..Holder::default() };
        prop_assert!(!receives(eligibility(&job, &holder)));
    }

    /// TD-LIC-06 (LIC-011, D-856): a list row naming the business with other case, spacing or
    /// punctuation matches it; a row whose name has one word more never does, and neither does an
    /// empty list.
    #[test]
    fn city_list_match_ignores_form_but_not_words(
        words in proptest::collection::vec("[a-z]{2,8}", 1..4), upper in any::<bool>(),
    ) {
        let own = CityRecord { business_name: words.join(" "), ..CityRecord::default() };
        let styled = if upper { own.business_name.to_uppercase() } else { words.join(",  ") };
        let same = CityRecord { business_name: format!("{styled}."), ..CityRecord::default() };
        let longer = CityRecord { business_name: format!("{} fakeword", own.business_name), ..CityRecord::default() };
        prop_assert_eq!(city_list_standing(&own, &[longer.clone(), same]), CityListStanding::Matched);
        prop_assert_eq!(city_list_standing(&own, &[longer]), CityListStanding::Pending);
        prop_assert_eq!(city_list_standing(&own, &[]), CityListStanding::Pending);
    }

    /// TD-LIC-17 (LIC-015, FEE-045): only a home inspection is ever fee-free; a yes to the sale
    /// question always makes it so.
    #[test]
    fn only_home_inspections_are_fee_free(job in job_strategy(), answer in 0..3usize, fee_free in any::<bool>()) {
        let answer = [SaleAnswer::Yes, SaleAnswer::No, SaleAnswer::NotAnswered][answer];
        let fee = inspection_fee(&job, answer, fee_free);
        if job.category != "home_inspection" {
            prop_assert_eq!(fee, InspectionFee::Normal);
        } else if answer == SaleAnswer::Yes {
            prop_assert_eq!(fee, InspectionFee::FeeFree);
        }
    }
}
