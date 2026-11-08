//! When the daily open-cards email goes, and to whom (CARD-022; T198, D-330, D-345, D-359, D-380,
//! D-1993).
//!
//! `messaging` cannot read `businesses`, `quotes` or `picks`, so the composer in `picks`
//! (`open_cards_email.rs`) gathers the facts and asks [`digest_goes`]. A person's chosen times are
//! minutes after midnight Arizona time (`businesses::api::OpenCardsTime::minute`: 480, 720 or
//! 1020); [`slot_due`] names the chosen time whose hour `now` falls in, so a scheduled run at
//! 17:00 or a little after sends the 5 p.m. email. A card is listed only with at least
//! [`MIN_LEFT`] of its quote window left ([`listable`]); the window itself is never changed
//! (PICK-001). Never to a paused business or one set to busy. Email only, class bulk (OPS-038).

use askhava_core::api::arizona_time;
use jiff::{SignedDuration, Timestamp};

/// CARD-022: the least quote-window time a card needs to be listed, 1 hour (a setting).
pub const MIN_LEFT: SignedDuration = SignedDuration::from_hours(1);
/// How long after a chosen time a run still counts as that time's run.
const SLOT_SPAN: u16 = 60;

/// The facts of one person's open-cards email at one run (CARD-022).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestFacts {
    /// The person's chosen times, minutes after midnight Arizona time; empty is "off".
    pub chosen: Vec<u16>,
    /// The business is paused (BIZ-034's pause or an account pause).
    pub paused: bool,
    /// The business is set to busy (BIZ-038).
    pub busy: bool,
    /// The end of each card the business has not quoted.
    pub card_ends: Vec<Timestamp>,
    /// How many picks wait for this person's Confirm (only for a person who gets picks).
    pub picks_waiting: usize,
}

/// CARD-022: the chosen time (minutes after midnight, Arizona time) whose hour holds `now`, or
/// `None`: at 17:00 and up to 17:59 the 5 p.m. time is due, at 16:59 none is.
#[must_use]
pub fn slot_due(chosen: &[u16], now: Timestamp) -> Option<u16> {
    let t = arizona_time(now);
    let minute = u16::try_from(i32::from(t.hour()) * 60 + i32::from(t.minute())).ok()?;
    chosen
        .iter()
        .copied()
        .find(|slot| minute >= *slot && minute < slot.saturating_add(SLOT_SPAN))
}

/// CARD-022: whether a card ending at `end` is listed at `now`: 60 minutes left is, 59 is not.
#[must_use]
pub fn listable(end: Timestamp, now: Timestamp, min_left: SignedDuration) -> bool {
    end.duration_since(now) >= min_left
}

/// CARD-022, D-330, D-345, D-359: whether `facts`' person gets the open-cards email at `now`: a
/// chosen time is due, the business is neither paused nor busy, and a card is listable or a pick
/// waits.
#[must_use]
pub fn digest_goes(facts: &DigestFacts, now: Timestamp, min_left: SignedDuration) -> bool {
    let has_cards = facts.card_ends.iter().any(|e| listable(*e, now, min_left));
    slot_due(&facts.chosen, now).is_some()
        && !facts.paused
        && !facts.busy
        && (has_cards || facts.picks_waiting > 0)
}
