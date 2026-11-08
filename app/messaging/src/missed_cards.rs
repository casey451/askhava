//! The monthly missed-cards account email's rule (BIZ-044, BIZ-025; T198, D-331, D-406, D-1994).
//!
//! On the first day of each month (Arizona time, [`first_of_month`]) a business that got at least
//! [`MissedSettings::min_cards`] job cards the month before and saw at least
//! [`MissedSettings::share_percent`] of them end without a quote gets one account email
//! ([`missed_email_due`]), unless the owner switched it off. It is an account notice under the
//! business's consent (CON-019), never marketing; its words are composed in `picks`
//! (`open_cards_email.rs`), since only `quotes` knows the counts.

use askhava_core::api::arizona_time;
use jiff::Timestamp;

/// BIZ-044's two settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissedSettings {
    /// The fewest cards last month that can trigger it: 3.
    pub min_cards: u32,
    /// The least share of them, in percent, that ended without a quote: 50.
    pub share_percent: u32,
}

impl Default for MissedSettings {
    fn default() -> Self {
        Self {
            min_cards: 3,
            share_percent: 50,
        }
    }
}

/// One business's cards last month (BIZ-025's "cards that ended without a quote").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthCards {
    /// Cards it got.
    pub got: u32,
    /// Cards it quoted.
    pub quoted: u32,
    /// Cards that ended without its quote.
    pub ended_unquoted: u32,
}

/// BIZ-044: whether `now` is the first day of a month, Arizona time.
#[must_use]
pub fn first_of_month(now: Timestamp) -> bool {
    arizona_time(now).day() == 1
}

/// BIZ-044, D-331: whether the business gets the email for `month`: switched on, at least
/// `min_cards` cards, and at least `share_percent` of them ended without a quote (6 and 3 does,
/// 6 and 2 does not, 2 and 2 does not).
#[must_use]
pub fn missed_email_due(month: MonthCards, settings: MissedSettings, switched_on: bool) -> bool {
    let ended = u64::from(month.ended_unquoted) * 100;
    let needed = u64::from(month.got) * u64::from(settings.share_percent);
    switched_on && month.got >= settings.min_cards && month.got > 0 && ended >= needed
}
