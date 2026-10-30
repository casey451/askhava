//! Who may receive a card on licenses alone: agency-licensed categories go only to their licensees,
//! no-license categories only to businesses on the city list, and the boundary job types split
//! between no-license, handyman and licensed work (LIC-011 to LIC-016, LIC-028 to LIC-031, LIC-047).
//!
//! Pure functions, no database. Towns, ticked job types (CARD-001) and the handyman price and
//! permit checks (LIC-019 to LIC-025, T035) are decided elsewhere; a `HandymanCard` carries the
//! under-$1,000, no-permit condition for those checks.

use crate::models::{
    Agency, CityListStanding, CityRecord, Eligibility, Holder, InspectionFee, Job, LicenseKind,
    NoCardReason, PestRecordCategory, SaleAnswer,
};

/// Whether `holder` may receive `job`'s card on its licenses (LIC-011 to LIC-016, LIC-028 to
/// LIC-031, LIC-047).
///
/// - No license: only a business matched on the city list, and only for a listed job type
///   (dryer-vent cleaning, plug-in lights, weekly pool cleaning; LIC-011, LIC-028, LIC-029,
///   LIC-031); the handyman category's jobs, a vent reroute among them, are handyman cards (LIC-028).
/// - ROC: a business whose classes cover the job type, or a handyman on the city list for a listed
///   job type of a handyman-allowed category (cameras, appliances, pool repair; LIC-014, LIC-030,
///   LIC-031). Refused job types (gas pool heater, gas or line work) go to licensed businesses only.
/// - Agency: only that agency's licensees (LIC-012, LIC-015, LIC-016); alarms need the Board
///   certificate and an ROC low-voltage class (LIC-013).
#[must_use]
pub fn eligibility(job: &Job, holder: &Holder) -> Eligibility {
    match job.license {
        LicenseKind::None => no_license(job, holder),
        LicenseKind::Roc if holder.roc_covers => Eligibility::Card,
        LicenseKind::Roc => handyman(job, holder),
        LicenseKind::StateAgency => agency_only(job, holder),
        LicenseKind::RocAndStateAgency => roc_and_agency(job, holder),
    }
}

/// LIC-011, LIC-028, LIC-029, LIC-031: a no-license job needs a city-list match; a job type the
/// category refuses (a new outlet for holiday lights, a vent reroute for vent cleaning) is not
/// no-license work.
fn no_license(job: &Job, holder: &Holder) -> Eligibility {
    if !job.listed {
        return Eligibility::NoCard(NoCardReason::RefusedJobType);
    }
    match holder.city_list {
        CityListStanding::Pending => Eligibility::NoCard(NoCardReason::CityListPending),
        CityListStanding::Matched if job.handyman_allowed => Eligibility::HandymanCard,
        CityListStanding::Matched => Eligibility::Card,
    }
}

/// LIC-014, LIC-030, LIC-031: a handyman takes a listed job type of a handyman-allowed ROC category,
/// under $1,000 with no permit, once it is on the city list (LIC-011).
fn handyman(job: &Job, holder: &Holder) -> Eligibility {
    if !job.listed {
        return Eligibility::NoCard(NoCardReason::RefusedJobType);
    }
    if !(job.handyman_allowed && holder.handyman) {
        return Eligibility::NoCard(NoCardReason::RocNotCovered);
    }
    match holder.city_list {
        CityListStanding::Matched => Eligibility::HandymanCard,
        CityListStanding::Pending => Eligibility::NoCard(NoCardReason::CityListPending),
    }
}

/// LIC-012, LIC-015, LIC-016: only the category's agency licensees; pest by attestation alone.
fn agency_only(job: &Job, holder: &Holder) -> Eligibility {
    match Agency::for_category(&job.category) {
        Some(Agency::PestManagement) => pest(job, holder),
        Some(agency) if holder.agencies.contains(&agency) => Eligibility::Card,
        Some(agency) => Eligibility::NoCard(NoCardReason::AgencyMissing(agency)),
        None => Eligibility::NoCard(NoCardReason::AgencyUnmapped),
    }
}

/// LIC-012, LIC-047: a pest job goes only to a business that attested a Department license and is
/// not paused by a homeowner's report; termite and pre-emergent job types also need the attested
/// record category. An ROC class or `agencies` entry never opens pest work.
fn pest(job: &Job, holder: &Holder) -> Eligibility {
    let Some(attestation) = &holder.pest else {
        return Eligibility::NoCard(NoCardReason::AgencyMissing(Agency::PestManagement));
    };
    if attestation.is_paused() {
        return Eligibility::NoCard(NoCardReason::PestPaused);
    }
    match PestRecordCategory::needed_for(&job.job_type) {
        Some(needed) if !attestation.holds(needed) => {
            Eligibility::NoCard(NoCardReason::PestCategoryMissing(needed))
        }
        Some(_) | None => Eligibility::Card,
    }
}

/// LIC-013: an alarm job needs the Board alarm certificate and an ROC class covering it (R-67,
/// CR-67 or broader, as the license map says).
fn roc_and_agency(job: &Job, holder: &Holder) -> Eligibility {
    let Some(agency) = Agency::for_category(&job.category) else {
        return Eligibility::NoCard(NoCardReason::AgencyUnmapped);
    };
    if !holder.agencies.contains(&agency) {
        return Eligibility::NoCard(NoCardReason::AgencyMissing(agency));
    }
    if holder.roc_covers {
        Eligibility::Card
    } else {
        Eligibility::NoCard(NoCardReason::RocNotCovered)
    }
}

/// LIC-015, FEE-045: an inspection is fee-free when intake's sale question was answered yes or the
/// request names a fee-free job type (buyer, pre-listing or refinance inspection, a termite report
/// for a sale or loan: the catalog's `fee_free_job_types`); a general condition inspection keeps
/// the normal introduction fee by job size.
#[must_use]
pub fn inspection_fee(answer: SaleAnswer, job_type_fee_free: bool) -> InspectionFee {
    match answer {
        SaleAnswer::Yes => InspectionFee::FeeFree,
        SaleAnswer::No | SaleAnswer::NotAnswered if job_type_fee_free => InspectionFee::FeeFree,
        SaleAnswer::No | SaleAnswer::NotAnswered => InspectionFee::Normal,
    }
}

/// LIC-011, D-177: a no-license business is `Matched` when one row of today's city list shares a
/// name with it (business name, trade name or owner name, compared without case, spacing or
/// punctuation) and no city license number on both sides disagrees; otherwise it stays `Pending`,
/// to be read again against the next daily list. Activation with the identity code, keeping the
/// join date, is `businesses`' (T081).
#[must_use]
pub fn city_list_standing(business: &CityRecord, list: &[CityRecord]) -> CityListStanding {
    let own = names(business);
    let matched = list.iter().any(|row| {
        numbers_agree(business, row) && names(row).iter().any(|name| own.contains(name))
    });
    if matched {
        CityListStanding::Matched
    } else {
        CityListStanding::Pending
    }
}

/// The non-empty compared forms of a record's names.
fn names(record: &CityRecord) -> Vec<String> {
    [
        Some(&record.business_name),
        record.trade_name.as_ref(),
        record.owner_name.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|name| compared(name))
    .filter(|name| !name.is_empty())
    .collect()
}

/// A license number entered by the business must equal the list's, when the list has one.
fn numbers_agree(business: &CityRecord, row: &CityRecord) -> bool {
    match (&business.license_number, &row.license_number) {
        (Some(own), Some(listed)) => compared(own) == compared(listed),
        (Some(_) | None, None) | (None, Some(_)) => true,
    }
}

/// Lower case, letters and digits only, words joined by one space.
fn compared(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
