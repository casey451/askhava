//! The public face of `askhava-licensing`: everything other crates may call (D-481).
//!
//! [`eligibility`] says whether a business may receive a job's card on its licenses alone
//! (LIC-011 to LIC-016, LIC-028 to LIC-031, LIC-047); [`city_list_standing`] matches a no-license
//! business on the city list (LIC-011, D-177); [`inspection_fee`] says whether an inspection is a
//! fee-free job type (LIC-015, FEE-045). The handyman limits (T035): [`eligibility_for_request`]
//! withholds a handyman card from gas, smoke-alarm, sprinkler and permit-type requests (AI-012,
//! LIC-021, LIC-022); [`check_quote`] refuses an uncertified handyman quote or one at $1,000 or
//! more (LIC-019, LIC-020, LIC-025, LIC-036); [`split_job`] runs the split-job check (LIC-023);
//! [`HANDYMAN_CARD_LINE`] is the LIC-024 line. `quotes` combines these with towns and ticked job types
//! (CARD-001) when it chooses recipients.
//!
//! The daily lists (T036): [`import_roc_list`] and [`import_city_list`] record the day's lists,
//! skipping and counting malformed rows; [`check_license`] records a business's daily check and
//! [`daily_check_allows_card`] lets a card go only after an Active one (CARD-015, LIC-007);
//! [`read_back`] gives the sign-up read-back of the classes on the state record (LIC-006, LIC-027);
//! [`city_standing`] matches a business on the latest city list (LIC-010, LIC-011).

pub use crate::handyman::{
    HANDYMAN_CARD_LINE, HANDYMAN_LIMIT_CENTS, HandymanJob, JobRequest, NeverHandyman, QuotePrice,
    QuoteRefusal, QuoteSubmission, SPLIT_JOB_WINDOW_DAYS, check_quote, never_handyman, split_job,
};
pub use crate::imports::{
    CITY_HEADER, CityRow, ImportError, ParsedList, ROC_HEADER, RocRow, RocStatus, parse_city_list,
    parse_roc_list,
};
pub use crate::models::{
    Agency, CityListStanding, CityRecord, Eligibility, Holder, InspectionFee, Job, LicenseKind,
    LicensingError, NoCardReason, PestAttestation, PestRecordCategory, SaleAnswer,
};
pub use crate::rules::{city_list_standing, eligibility, eligibility_for_request, inspection_fee};
pub use crate::service::{
    DailyCheck, ImportSummary, ReadBack, check_license, city_standing, daily_check_allows_card,
    import_city_list, import_roc_list, read_back,
};
