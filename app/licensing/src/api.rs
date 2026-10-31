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
//! skipping and counting malformed rows and refusing an empty or mostly malformed list;
//! [`check_license`] records a business's daily check and [`roc_check_allows_card`] lets an
//! ROC-licensed card go only after a fresh Active check of the covering license (CARD-015, LIC-007);
//! [`read_back`] gives the sign-up read-back of the classes on the state record (LIC-006, LIC-027);
//! [`city_standing`] matches a business on the latest city list (LIC-010, LIC-011).
//!
//! The card lines (T037): [`licensed_lines`] gives a licensed card's record-check line with its link
//! and its "license since" line; [`no_license_lines`] gives a no-license card's city-license line and
//! business-reported insurance after the handyman line (LIC-026, LIC-034, LIC-037, LIC-038, LIC-032).
//!
//! The live lookup (T180): [`lookup_at_pick`] makes the one lookup at a pick, a failed one is
//! `Unknown` and can't Confirm; [`unknown_standing`] keeps, warns and pauses (LIC-008);
//! [`end_date_pauses`] applies LIC-009; [`list_failure_alerts`], [`alert_list_failure`] and
//! [`lookup_while_list_fails`] cover a failing list (LIC-039).
//!
//! The class map (T153): [`ClassMap`] loads `docs/catalog/license_map.yaml`; [`ClassMap::covers`]
//! fills `Holder::roc_covers` (LIC-043, LIC-035), [`ClassMap::sign_up`] stops a commercial-only
//! record (LIC-044), [`ClassMap::check_project_limit`] applies the R-61 and R-62 limits (LIC-045),
//! [`ClassMap::opening`] and [`request_intake`] honor `launch_open` (LIC-046).

pub use crate::class_map::{
    ClassMap, ClassMapError, JobTypeKey, Opening, ProjectLimitRefusal, RequestIntake,
    STRUCTURAL_JOB_TYPES, SignUpOutcome, request_intake,
};
pub use crate::handlers::{
    FailureLimits, LIST_FAILING_ALERT, LIST_STILL_FAILING_ALERT, ListFailureAlert, LookupOutcome,
    UnknownStanding, alert_list_failure, arizona_day, end_date_pauses, list_failure_alerts,
    lookup_at_pick, lookup_while_list_fails, unknown_standing,
};
pub use crate::handyman::{
    HANDYMAN_CARD_LINE, HANDYMAN_LIMIT_CENTS, HandymanJob, JobRequest, NeverHandyman, QuotePrice,
    QuoteRefusal, QuoteSubmission, SPLIT_JOB_WINDOW_DAYS, check_quote, never_handyman, split_job,
};
pub use crate::imports::{
    CITY_HEADER, CityRow, ImportError, ParsedList, ROC_HEADER, RocRow, RocStatus,
    normalise_license_number, parse_city_list, parse_roc_list,
};
pub use crate::lines::{
    CardLanguage, CardLine, Issuer, LicenseFacts, LinesError, NoLicenseFacts, licensed_lines,
    no_license_lines,
};
pub use crate::models::{
    Agency, CityListStanding, CityRecord, Eligibility, Holder, InspectionFee, Job, LicenseKind,
    LicensingError, NoCardReason, PestAttestation, PestRecordCategory, SaleAnswer,
};
pub use crate::rules::{city_list_standing, eligibility, eligibility_for_request, inspection_fee};
pub use crate::service::{
    DailyCheck, Freshness, ImportSummary, ReadBack, check_license, city_standing, import_city_list,
    import_roc_list, read_back, roc_check_allows_card,
};
