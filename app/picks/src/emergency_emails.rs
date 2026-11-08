//! Who gets an EMERGENCY pick email and when (PICK-037, PICK-044, BIZ-007, BIZ-037, CARD-021;
//! T150, D-344, D-346).
//!
//! An emergency pick goes by email only, marked EMERGENCY ([`emergency_pick_subject`], the
//! `pick_notice_emergency` words). By day it goes to every person who gets picks; at night only to
//! the owner and "Confirm and billing" members with "Emergency emails at any hour" on at a
//! business ticked after-hours and not suspended (`businesses::api::night_people`); with nobody
//! such, the business is not an after-hours one and its notice waits for 8 a.m.
//! ([`emergency_pick_plan`] over `confirm::emergency_plan`: the 60-minute window and its one
//! reminder 30 minutes after the send). No call is placed (BIZ-039).

use askhava_businesses::api::night_people;
use askhava_core::api::{brand, is_business_hours};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::confirm::{EmergencyPlan, WindowRules, emergency_plan};
use crate::models::PicksError;

/// PICK-037, D-346: "Ask Hava: EMERGENCY: a homeowner picked your quote for job <n>. Accept by
/// <time>.", with `time` as the sender writes it (Arizona time).
#[must_use]
pub fn emergency_pick_subject(job: i64, time: &str) -> String {
    let b = brand().short_name();
    format!("{b}: EMERGENCY: a homeowner picked your quote for job {job}. Accept by {time}.")
}

/// The people an emergency pick goes to and its timing (PICK-044, BIZ-037).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmergencyPick {
    /// The opaque mailbox IDs it goes to at the plan's send time.
    pub to: Vec<Uuid>,
    /// The send, Confirm end and reminder times.
    pub plan: EmergencyPlan,
}

/// PICK-044, BIZ-037, CARD-021: the emergency pick of `business` made at `picked`: by day, or for
/// a business with nobody switched on, every `day_people` (picks recipients) under the plan; at
/// night with someone switched on, only those people, at once.
///
/// # Errors
/// [`PicksError::Database`].
pub async fn emergency_pick_plan(
    conn: &mut PgConnection,
    business: Uuid,
    (day_people, picked): (&[Uuid], Timestamp),
    rules: WindowRules,
) -> Result<EmergencyPick, PicksError> {
    let night: Vec<Uuid> = night_people(conn, business)
        .await
        .map_err(|_| PicksError::Database)?
        .into_iter()
        .filter(|p| p.picks)
        .map(|p| p.mailbox)
        .collect();
    let after_hours = !night.is_empty();
    let to = if after_hours && !is_business_hours(picked) {
        night
    } else {
        day_people.to_vec()
    };
    Ok(EmergencyPick {
        to,
        plan: emergency_plan(picked, after_hours, rules),
    })
}
