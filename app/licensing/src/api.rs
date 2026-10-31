//! The public face of `askhava-licensing`: everything other crates may call (D-481).
//!
//! [`eligibility`] says whether a business may receive a job's card on its licenses alone
//! (LIC-011 to LIC-016, LIC-028 to LIC-031, LIC-047); [`city_list_standing`] matches a no-license
//! business on the city list (LIC-011, D-177); [`inspection_fee`] says whether an inspection is a
//! fee-free job type (LIC-015, FEE-045). `quotes` combines these with towns and ticked job types
//! (CARD-001) when it chooses recipients.

pub use crate::models::{
    Agency, CityListStanding, CityRecord, Eligibility, Holder, InspectionFee, Job, LicenseKind,
    LicensingError, NoCardReason, PestAttestation, PestRecordCategory, SaleAnswer,
};
pub use crate::rules::{city_list_standing, eligibility, inspection_fee};
