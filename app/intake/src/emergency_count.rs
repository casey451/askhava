//! The INT-033 night count and the texts that offer the emergency-or-regular choice (INT-006,
//! INT-033, D-271, D-292, D-324, D-346, D-359, D-436, D-491).
//!
//! Between 8 p.m. and 8 a.m. Arizona time the homeowner first gets the count of businesses in the
//! trade that get emergency requests at night, then the choice; with a count of 0 the choice is
//! not offered and the job runs as regular. By day only the choice goes. The count is
//! `businesses::api::night_count(trade, town)`; which businesses it counts (the after-hours tick
//! now, the per-person switch and the alert test later) is `businesses`' alone (T150, D-491).

use askhava_businesses::api::night_count;
use askhava_core::api::{BUSINESS_CLOSE_HOUR, BUSINESS_OPEN_HOUR, arizona_time};
use jiff::Timestamp;
use sqlx::PgConnection;

use crate::models::IntakeError;

/// The INT-006 emergency-or-regular choice text.
pub const CHOICE_TEMPLATE: &str = "emergency_choice";
/// INT-033 for N of 2 or more (`{count}`, `{trade}`).
pub const NIGHT_MANY_TEMPLATE: &str = "night_count_many";
/// INT-033 for N of 1 (`{trade}`): "1 ... business ... gets", never "1 ... businesses".
pub const NIGHT_ONE_TEMPLATE: &str = "night_count_one";
/// INT-033 for N of 0 (`{trade}`): no choice, the request goes out at 8 a.m.
pub const NIGHT_NONE_TEMPLATE: &str = "night_count_none";

/// The texts that offer the choice, in order, and whether the choice is offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Template IDs to send now, in this order.
    pub templates: Vec<&'static str>,
    /// False at night with no after-hours business: the job continues as regular (INT-033).
    pub offered: bool,
}

/// Whether `at` is between 8 p.m. and 8 a.m. Arizona time (INT-006, INT-033).
#[must_use]
pub fn is_night(at: Timestamp) -> bool {
    !(BUSINESS_OPEN_HOUR..BUSINESS_CLOSE_HOUR).contains(&arizona_time(at).hour())
}

/// The choice texts at `at` with `count` after-hours businesses (INT-006, INT-033): by day the
/// choice alone (no count text); at night the count text first, then the choice, or the zero
/// text alone with no choice.
#[must_use]
pub fn offer(at: Timestamp, count: u32) -> Offer {
    let (templates, offered) = match (is_night(at), count) {
        (false, _) => (vec![CHOICE_TEMPLATE], true),
        (true, 0) => (vec![NIGHT_NONE_TEMPLATE], false),
        (true, 1) => (vec![NIGHT_ONE_TEMPLATE, CHOICE_TEMPLATE], true),
        (true, _) => (vec![NIGHT_MANY_TEMPLATE, CHOICE_TEMPLATE], true),
    };
    Offer { templates, offered }
}

/// The INT-033 count N for catalog category `trade` in `town`, from
/// `businesses::api::night_count` (BIZ-007, D-491).
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn count(db: &mut PgConnection, trade: &str, town: &str) -> Result<u32, IntakeError> {
    night_count(db, trade, town)
        .await
        .map_err(|_| IntakeError::Database)
}
