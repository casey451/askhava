//! The records `licensing`'s rules read and return: the category's license kind, the other state
//! agencies, the pest attestation, the city-list standing, the business's licenses and the outcome
//! (LIC-011 to LIC-016, LIC-028 to LIC-031, LIC-047).
//!
//! Nothing here holds a homeowner's detail. A city-list record holds a business's own names, so it
//! has no `Debug` and never reaches a log (PRIV-016).

use std::collections::BTreeSet;

use thiserror::Error;

/// Why a `licensing` value could not be built. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LicensingError {
    /// The catalog's `license.kind` is not one of the four kinds (LIC-001).
    #[error("the license kind is not known")]
    UnknownLicenseKind,
    /// The pest attestation box is unticked or the license number is missing or malformed (LIC-047).
    #[error("the pest-management attestation is not complete")]
    AttestationIncomplete,
}

/// A category's license rule, as `docs/catalog/categories.yaml` names it (LIC-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseKind {
    /// No license: the city business-license list only (LIC-011).
    None,
    /// An ROC class.
    Roc,
    /// Another state agency's license (LIC-012, LIC-015, LIC-016).
    StateAgency,
    /// Both an ROC class and an agency certificate (LIC-013).
    RocAndStateAgency,
}

impl LicenseKind {
    /// Reads the catalog's `license.kind` code.
    ///
    /// # Errors
    ///
    /// [`LicensingError::UnknownLicenseKind`] for any other code.
    pub fn parse(code: &str) -> Result<Self, LicensingError> {
        match code {
            "none" => Ok(Self::None),
            "roc" => Ok(Self::Roc),
            "state_agency" => Ok(Self::StateAgency),
            "roc_and_state_agency" => Ok(Self::RocAndStateAgency),
            _ => Err(LicensingError::UnknownLicenseKind),
        }
    }
}

/// The other state agencies whose license a category needs (`agencies` in
/// `docs/catalog/license_map.yaml`; LIC-012, LIC-013, LIC-015, LIC-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Agency {
    /// Arizona Department of Agriculture pest-management business license, self-attested (LIC-047).
    PestManagement,
    /// Board of Technical Registration certified home inspector (LIC-015).
    HomeInspector,
    /// Board of Technical Registration alarm business certificate (LIC-013).
    AlarmBusiness,
    /// ADEQ septage-hauler vehicle license (LIC-016).
    SeptageHauler,
}

impl Agency {
    /// Every agency, in map order.
    pub const ALL: [Self; 4] = [
        Self::PestManagement,
        Self::HomeInspector,
        Self::AlarmBusiness,
        Self::SeptageHauler,
    ];

    /// The `licensing.business_license.agency` code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::PestManagement => "pest_management",
            Self::HomeInspector => "home_inspector",
            Self::AlarmBusiness => "alarm_business",
            Self::SeptageHauler => "septage_hauler",
        }
    }

    /// The `license` name the license map gives this agency.
    #[must_use]
    pub const fn map_name(self) -> &'static str {
        match self {
            Self::PestManagement => "AZ Department of Agriculture pest-management business license",
            Self::HomeInspector => "Board of Technical Registration certified home inspector",
            Self::AlarmBusiness => {
                "Board of Technical Registration alarm business certificate, with an R-67 or CR-67 ROC class"
            }
            Self::SeptageHauler => "ADEQ septage-hauler vehicle license",
        }
    }

    /// The one catalog category this agency's license opens (LIC-012, LIC-013, LIC-015, LIC-016).
    #[must_use]
    pub const fn category(self) -> &'static str {
        match self {
            Self::PestManagement => "pest_control",
            Self::HomeInspector => "home_inspection",
            Self::AlarmBusiness => "alarm_systems",
            Self::SeptageHauler => "septic_pumping",
        }
    }

    /// The agency a category needs, if any.
    #[must_use]
    pub fn for_category(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.category() == code)
    }

    /// The agency the license map calls `name`, if any.
    #[must_use]
    pub fn from_map_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.map_name() == name)
    }
}

/// A category on a pest-management record that some pest job types need (LIC-012, LIC-043, LIC-047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PestRecordCategory {
    /// Wood-destroying organism: termite job types.
    WoodDestroying,
    /// Weed control: pre-emergent weed control job types.
    WeedControl,
}

impl PestRecordCategory {
    /// The record category a pest job type needs; general pest service and scorpions need none
    /// (`record_category_needed` in the license map, LIC-012).
    #[must_use]
    pub fn needed_for(job_type: &str) -> Option<Self> {
        match job_type {
            "termites" => Some(Self::WoodDestroying),
            "pre-emergent weed control" => Some(Self::WeedControl),
            _ => None,
        }
    }
}

/// What a pest business stated at sign-up: the ticked box, its license number and the record
/// categories it ticked (LIC-047). Self-reported and never checked by Ask Hava (D-417).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PestAttestation {
    number: String,
    wood_destroying: bool,
    weed_control: bool,
    paused: bool,
}

/// The longest pest license number accepted, the `licensing.business_license` check's limit.
const MAX_NUMBER_LEN: usize = 20;

impl PestAttestation {
    /// Builds the attestation from the sign-up form (LIC-047): the box must be ticked and the
    /// number given (letters, digits and `-`, at most 20).
    ///
    /// # Errors
    ///
    /// [`LicensingError::AttestationIncomplete`] when the box is unticked or the number is bad.
    pub fn new(
        ticked: bool,
        number: &str,
        wood_destroying: bool,
        weed_control: bool,
    ) -> Result<Self, LicensingError> {
        let number = number.trim();
        let well_formed = !number.is_empty()
            && number.len() <= MAX_NUMBER_LEN
            && number
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !ticked || !well_formed {
            return Err(LicensingError::AttestationIncomplete);
        }
        Ok(Self {
            number: number.to_owned(),
            wood_destroying,
            weed_control,
            paused: false,
        })
    }

    /// The license number the business gave, for the LIC-047 card line.
    #[must_use]
    pub fn number(&self) -> &str {
        &self.number
    }

    /// Whether the business ticked this record category (LIC-012).
    #[must_use]
    pub const fn holds(&self, category: PestRecordCategory) -> bool {
        match category {
            PestRecordCategory::WoodDestroying => self.wood_destroying,
            PestRecordCategory::WeedControl => self.weed_control,
        }
    }

    /// The attestation after a homeowner reports the license wrong: pest jobs pause until a new,
    /// findable number replaces it (LIC-047).
    #[must_use]
    pub fn reported_wrong(self) -> Self {
        Self {
            paused: true,
            ..self
        }
    }

    /// Whether a homeowner's report has paused the business's pest jobs (LIC-047).
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }
}

/// Whether a no-license business matched the city business-license list (LIC-011, D-177).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CityListStanding {
    /// A daily list showed a name match.
    Matched,
    /// No list has matched yet: the business waits and is never active (D-177).
    #[default]
    Pending,
}

/// A business's own names, or one row of the city list: business name, trade name (DBA), owner
/// name and city license number (LIC-011). No `Debug`, so the names never reach a log.
#[derive(Clone, Default)]
pub struct CityRecord {
    /// The listed business name.
    pub business_name: String,
    /// The trade name (DBA), when there is one.
    pub trade_name: Option<String>,
    /// The owner name, when there is one.
    pub owner_name: Option<String>,
    /// The city license number, when the list has one or the business entered it.
    pub license_number: Option<String>,
}

/// What the rules know about one business's licenses (LIC-011 to LIC-016).
#[derive(Debug, Clone, Default)]
pub struct Holder {
    /// Whether its ROC classes cover this job type as the license map says (T153's class map).
    pub roc_covers: bool,
    /// The non-pest agency licenses on its record (inspector, alarm, septage hauler).
    pub agencies: BTreeSet<Agency>,
    /// Its pest attestation; pest jobs read only this, never `agencies` (LIC-047).
    pub pest: Option<PestAttestation>,
    /// Whether it signed up as a handyman (no license, LIC-019).
    pub handyman: bool,
    /// Its city business-license list standing (LIC-011).
    pub city_list: CityListStanding,
}

/// One job type of a category, as the rules need it (LIC-001).
#[derive(Debug, Clone)]
pub struct Job {
    /// The catalog category code, such as `pool_repair`.
    pub category: String,
    /// The job type name.
    pub job_type: String,
    /// The category's license rule.
    pub license: LicenseKind,
    /// Whether the category lets handymen take its listed job types under $1,000.
    pub handyman_allowed: bool,
    /// Whether the job type is in the category's `job_types`; a `refused` job (gas pool heater, a
    /// new outlet, gas appliance work) is licensed-only work (LIC-029 to LIC-031).
    pub listed: bool,
}

/// Whether a business may receive a job's card, on its licenses alone (LIC-011 to LIC-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    /// It may receive the card.
    Card,
    /// It may receive the card as a handyman: under $1,000 total with no permit (LIC-014, LIC-019).
    HandymanCard,
    /// It may not.
    NoCard(NoCardReason),
}

/// Why a business may not receive a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCardReason {
    /// A no-license business not yet matched on the city list (LIC-011, D-177).
    CityListPending,
    /// The category's agency license is not on its record or attested.
    AgencyMissing(Agency),
    /// An agency category with no agency mapped: nobody receives it.
    AgencyUnmapped,
    /// Its ROC classes do not cover the job type.
    RocNotCovered,
    /// A handyman-category job for a business not signed up as a handyman (LIC-014, LIC-028).
    NotHandyman,
    /// A termite or pre-emergent job without the attested record category (LIC-012).
    PestCategoryMissing(PestRecordCategory),
    /// A homeowner reported its pest license wrong (LIC-047).
    PestPaused,
    /// A job type the category refuses: licensed-only work no handyman or no-license business
    /// takes, or work intake sends to another category, such as a vent reroute (LIC-028 to LIC-031).
    RefusedJobType,
}

/// The intake answer to "Is this inspection for a home sale, purchase or refinance?" (LIC-015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleAnswer {
    /// Yes.
    Yes,
    /// No.
    No,
    /// Not answered.
    NotAnswered,
}

/// Whether an inspection pick charges the introduction fee by job size (LIC-015, FEE-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionFee {
    /// A fee-free job type: no introduction fee.
    FeeFree,
    /// A general condition inspection: the normal introduction fee by job size.
    Normal,
}
