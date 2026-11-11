//! The business's Confirm window, the license re-checks at the pick and at Confirm, the recurring
//! fee line and the reply to a business's YES or CONFIRM text (PICK-019, PICK-020, PICK-033 to
//! PICK-035, PICK-037; D-056, D-196, D-345, D-365).
//!
//! The window is [`ConfirmSettings::window_minutes`] business minutes (4 business hours by
//! default, a setting) from the pick notice's send, so a notice held by the email limit counts
//! from its real send ([`crate::handlers::notice_sent`]). The license is checked once live at the
//! pick ([`check_at_pick`]) and from the stored results at Confirm ([`check_at_confirm`]).
//!
//! T190 added the rules that move the window (PICK-043, PICK-044, BIZ-034): [`hold`] adds 2
//! business hours once on a non-emergency pick, [`away_until`] and [`away_for`] bound "Away
//! until" to 4 business hours ahead and [`window_start`] starts a delayed clock when it ends, and
//! [`emergency_plan`] and [`emergency_fix_by`] time an emergency job's 60 clock minutes.
//!
//! T197 added the bounce grace (PICK-019, D-345): [`bounce_grace`] adds 1 business hour, once, to
//! a non-emergency pick whose notice emails all bounced or failed ([`EmailOutcome`]); a delivery
//! to a spam folder counts as sent. The reminders live in `confirm_reminders`.
//!
//! T157 added [`refused_at_confirm`]: a Confirm refused by the re-check ends the pick and moves
//! it at once to the homeowner's backup (PICK-020, PICK-047, D-400).

use askhava_adapters::api::LicenseSource;
use askhava_businesses::api::{AccountStatus, Roles};
use askhava_core::api::SettingsError;
use askhava_core::api::{add_business_minutes, is_business_hours};
use askhava_licensing::api::{
    Freshness, LookupOutcome, arizona_day, end_date_pauses, lookup_at_pick, roc_check_allows_card,
};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::backup_choice::{Backup, backup_choice};
use crate::confirm_reminders::lapse;
use crate::models::{self, PicksError};
use crate::status::account_confirms;

/// The pick notice's email subject (PICK-037), in `app/messaging/templates/` (en, es).
pub const PICK_NOTICE_TEMPLATE: &str = "pick_notice";
/// The pick notice's subject on a job chosen as an emergency (PICK-037, PICK-044).
pub const PICK_NOTICE_EMERGENCY_TEMPLATE: &str = "pick_notice_emergency";
/// The reply to YES or CONFIRM from a linked phone: the fee and the Confirm link (PICK-033). Its
/// `{number}` value is the [`fee_line`], `{link}` the Confirm review step (D-1501).
pub const FEE_REPLY_TEMPLATE: &str = "pick_fee_reply";
/// The reply to a business that answers after its window ended (PICK-035).
pub const EXPIRED_TEMPLATE: &str = "pick_expired";

/// The setting that sets [`ConfirmSettings::window_minutes`] (PICK-019, D-345, D-1503).
pub const WINDOW_KEY: &str = "ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES";
/// The longest window the setting accepts: five Arizona business days of 12 hours.
const WINDOW_MAX: u32 = 5 * 12 * 60;

/// How long a daily license result stays usable when the pick lookup fails (D-056, LIC-008).
const DAILY_FRESH: SignedDuration = SignedDuration::from_hours(24);

/// The Confirm window's settings (PICK-019, D-345): changing a value moves every new deadline
/// with no code change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmSettings {
    /// Business minutes from the pick notice's send to the end of the window (default 240).
    pub window_minutes: u32,
}

impl Default for ConfirmSettings {
    fn default() -> Self {
        Self {
            window_minutes: 4 * 60,
        }
    }
}

impl ConfirmSettings {
    /// PICK-019, D-345, D-1503: reads [`WINDOW_KEY`] from the process's `ASKHAVA_*` pairs, so the
    /// worker's pick notice and the window's real deadline use one value; absent or empty keeps
    /// the default of 240. Every other key is ignored (`Settings::from_pairs` checks the names).
    ///
    /// # Errors
    ///
    /// [`SettingsError::Invalid`] naming the key, never the value, when it is not a whole number
    /// from 1 to 3,600.
    pub fn from_pairs<I>(pairs: I) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut settings = Self::default();
        for (name, value) in pairs {
            let value = value.trim();
            if name != WINDOW_KEY || value.is_empty() {
                continue;
            }
            settings.window_minutes = value
                .parse::<u32>()
                .ok()
                .filter(|m| (1..=WINDOW_MAX).contains(m))
                .ok_or(SettingsError::Invalid(WINDOW_KEY))?;
        }
        Ok(settings)
    }
}

/// PICK-019, D-345, D-482: the end of the Confirm window of a non-emergency pick whose notice was
/// sent (or whose BIZ-034 away time ended) at `start`. Only 8 a.m. to 8 p.m. Arizona time counts:
/// a notice sent at 7:41 p.m. counts 19 minutes that day and ends at 11:41 a.m. the next day.
#[must_use]
pub fn confirm_by(start: Timestamp, settings: ConfirmSettings) -> Timestamp {
    add_business_minutes(start, settings.window_minutes)
}

/// The license answer at the pick (PICK-020, D-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseAtPick {
    /// The live lookup, or a daily result under 24 hours old when it failed, showed Active.
    Eligible,
    /// The live lookup showed the license not Active: the business can't confirm.
    Ineligible,
    /// The lookup failed and no Active daily result under 24 hours old exists: the pick waits.
    Waits,
}

/// PICK-020, D-056: the pick's license answer from the one live lookup and, only when it failed,
/// whether a daily Active result under 24 hours old is on record.
#[must_use]
pub const fn license_at_pick(lookup: LookupOutcome, fresh_daily_active: bool) -> LicenseAtPick {
    match lookup {
        LookupOutcome::Active => LicenseAtPick::Eligible,
        LookupOutcome::Inactive => LicenseAtPick::Ineligible,
        LookupOutcome::Unknown if fresh_daily_active => LicenseAtPick::Eligible,
        LookupOutcome::Unknown => LicenseAtPick::Waits,
    }
}

/// PICK-020, D-056, LIC-007, LIC-008: makes the pick's one live lookup of ROC license `number`
/// (recorded by `licensing` when it reads) and answers [`license_at_pick`].
///
/// # Errors
///
/// [`PicksError::Database`] when the answer or the stored check cannot be read or written.
pub async fn check_at_pick(
    conn: &mut PgConnection,
    source: &impl LicenseSource,
    (business, number): (Uuid, &str),
    now: Timestamp,
) -> Result<LicenseAtPick, PicksError> {
    let lookup = lookup_at_pick(conn, source, (business, number), now)
        .await
        .map_err(|_| PicksError::Database)?;
    let daily = match lookup {
        LookupOutcome::Unknown => fresh_active(conn, (business, number), now).await?,
        LookupOutcome::Active | LookupOutcome::Inactive => false,
    };
    Ok(license_at_pick(lookup, daily))
}

/// Whether the latest stored check of `number` is Active and under 24 hours old.
async fn fresh_active(
    conn: &mut PgConnection,
    (business, number): (Uuid, &str),
    now: Timestamp,
) -> Result<bool, PicksError> {
    let since = now.saturating_sub(DAILY_FRESH).unwrap_or(now);
    let fresh = Freshness {
        checked_since: since,
        listed_since: arizona_day(since),
    };
    roc_check_allows_card(conn, business, number, fresh)
        .await
        .map_err(|_| PicksError::Database)
}

/// Why a Confirm is refused (PICK-020, PICK-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The window ended: the pick expired (PICK-035).
    Expired,
    /// The account is not Active (paused, removed or under review).
    Account,
    /// The latest stored license check is not Active and fresh, or an end or bond date passed.
    License,
}

/// The stored facts a Confirm re-checks with no second live lookup (PICK-020, D-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredFacts {
    /// The business's account status.
    pub account: AccountStatus,
    /// The license-end date on the state record, if any (LIC-009).
    pub license_ends_on: Option<Date>,
    /// The bond-cancellation date on the state record, if any (LIC-009).
    pub bond_cancelled_on: Option<Date>,
}

/// PICK-020, PICK-035, D-056: whether the business may Confirm at `now` before `deadline`, from
/// the stored results only: the latest check of ROC license `number` (the pick lookup or a later
/// daily check) Active and under 24 hours old, no end or bond date reached, the account Active.
///
/// # Errors
///
/// [`PicksError::Database`] when the stored check cannot be read.
pub async fn check_at_confirm(
    conn: &mut PgConnection,
    (business, number): (Uuid, &str),
    facts: StoredFacts,
    (now, deadline): (Timestamp, Timestamp),
) -> Result<Result<(), Refusal>, PicksError> {
    if now >= deadline {
        return Ok(Err(Refusal::Expired));
    }
    if !account_confirms(facts.account) {
        return Ok(Err(Refusal::Account));
    }
    let dated = end_date_pauses(facts.license_ends_on, facts.bond_cancelled_on, now);
    if dated || !fresh_active(conn, (business, number), now).await? {
        return Ok(Err(Refusal::License));
    }
    Ok(Ok(()))
}

/// PICK-020, PICK-035, PICK-047, D-400: ends waiting pick `pick` after [`check_at_confirm`]
/// refused its Confirm for `refusal`, inside the caller's transaction, and moves it at once to
/// the homeowner's backup when still pickable. An ended window is the lapse
/// ([`crate::confirm_reminders::lapse`]); an account or license refusal cancels the pick as
/// `ineligible`. [`Backup::NoBackup`] means the caller sends PICK-046's text; a pick no longer
/// waiting is [`Backup::Done`]. `paused` is the job's homeowner pause (CON-029).
///
/// # Errors
///
/// [`PicksError::Database`], [`PicksError::MovedOn`] (roll back) or the page pick's errors.
pub async fn refused_at_confirm(
    tx: &mut PgConnection,
    (pick, refusal): (Uuid, Refusal),
    paused: bool,
    by: (Actor, Timestamp),
) -> Result<Backup, PicksError> {
    if refusal == Refusal::Expired {
        return lapse(tx, pick, paused, by).await;
    }
    if !models::end_pick(tx, pick, "ineligible", by.1).await? {
        return Ok(Backup::Done);
    }
    backup_choice(tx, pick, paused, by).await
}

/// How a recurring service's fee was set (PICK-034, FEE-039, D-196).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeBasis {
    /// The quote's monthly amount in cents.
    pub monthly_cents: i64,
    /// Four months of it in cents, as `payments` priced it.
    pub basis_cents: i64,
    /// Whether the recurring cap set the fee below the basis's tier.
    pub capped: bool,
}

/// Whole dollars as `$1,234`, otherwise `$1,234.56`.
pub(crate) fn dollars(cents: i64) -> String {
    let whole = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match cents % 100 {
        0 => format!("${grouped}"),
        part => format!("${grouped}.{part:02}"),
    }
}

/// PICK-034, D-196, D-1503: the fee line of the pick text and the Confirm review step in
/// `language`, for example "Fee $75 (ongoing services count as 4 months: $125 x 4 = $500)" or
/// "Fee $75 (ongoing services count as 4 months, capped at $75)"; a one-time job's is "Fee $75".
/// No fee (zero or less) has no fee line.
#[must_use]
pub fn fee_line(fee_cents: i64, basis: Option<FeeBasis>, language: Language) -> Option<String> {
    if fee_cents <= 0 {
        return None;
    }
    let fee = dollars(fee_cents);
    let (word, ongoing, cap) = match language {
        Language::En => ("Fee", "ongoing services count as 4 months", "capped at"),
        Language::Es => (
            "Tarifa",
            "los servicios continuos cuentan como 4 meses",
            "con tope de",
        ),
    };
    Some(match basis {
        None => format!("{word} {fee}"),
        Some(b) if b.capped => format!("{word} {fee} ({ongoing}, {cap} {fee})"),
        Some(b) => format!(
            "{word} {fee} ({ongoing}: {} x 4 = {})",
            dollars(b.monthly_cents),
            dollars(b.basis_cents)
        ),
    })
}

/// A business's answer word to a pick text (PICK-033, D-259).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessWord {
    /// YES, SÍ or SI.
    Yes,
    /// CONFIRM or CONFIRMAR.
    Confirm,
}

/// PICK-033, D-259: reads "YES 1042", "SÍ 1042", "CONFIRM 1042" or "CONFIRMAR 1042" (any case),
/// or the word alone, with any trailing periods or exclamation marks ("YES 1042."); anything else
/// is none.
#[must_use]
pub fn read_business_word(words: &str) -> Option<(BusinessWord, Option<u32>)> {
    let mut parts = words
        .trim_end()
        .trim_end_matches(['.', '!'])
        .split_whitespace();
    let word = match parts.next()?.to_lowercase().as_str() {
        "yes" | "sí" | "si" => BusinessWord::Yes,
        "confirm" | "confirmar" => BusinessWord::Confirm,
        _ => return None,
    };
    let number = match parts.next() {
        None => None,
        Some(n) => Some(n.parse().ok()?),
    };
    parts.next().is_none().then_some((word, number))
}

/// What a business's YES or CONFIRM text gets (PICK-033, PICK-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmReply {
    /// [`FEE_REPLY_TEMPLATE`]: the fee and the Confirm link; nothing is confirmed or charged.
    FeeAndLink,
    /// [`EXPIRED_TEMPLATE`]: the window ended and the quote can't be picked again for this job.
    Expired,
    /// Nothing: the phone is not linked to the picked business.
    Nothing,
}

/// PICK-033, PICK-035, D-365: a text never confirms or charges, so YES and CONFIRM get the same
/// answer: from a phone linked to the picked business, the fee and the Confirm link while the
/// window is open and the expired reply after; another phone or business gets nothing.
#[must_use]
pub const fn business_reply(linked: bool, open: bool) -> ConfirmReply {
    match (linked, open) {
        (false, _) => ConfirmReply::Nothing,
        (true, true) => ConfirmReply::FeeAndLink,
        (true, false) => ConfirmReply::Expired,
    }
}

/// The HOLD, Away and emergency lengths (PICK-043, PICK-044, BIZ-034): each a setting, so a test
/// or a later caller passes other values with no code change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRules {
    /// Business minutes a HOLD adds to the Confirm window, once per pick (default 120).
    pub hold_minutes: u32,
    /// The most business minutes ahead "Away until" may be set (default 240).
    pub away_max_minutes: u32,
    /// Clock minutes of an emergency job's Confirm and card-fix windows (default 60).
    pub emergency_minutes: i64,
    /// Clock minutes after the emergency pick notice's send of its one reminder (default 30).
    pub emergency_reminder_minutes: i64,
    /// Business minutes between a non-emergency pick's reminders (PICK-037, default 60).
    pub reminder_minutes: u32,
    /// Business minutes the all-bounced grace adds, once (PICK-019, default 60).
    pub bounce_grace_minutes: u32,
}

impl Default for WindowRules {
    fn default() -> Self {
        Self {
            hold_minutes: 2 * 60,
            away_max_minutes: 4 * 60,
            emergency_minutes: 60,
            emergency_reminder_minutes: 30,
            reminder_minutes: 60,
            bounce_grace_minutes: 60,
        }
    }
}

/// A business's HOLD or AWAY text word (PICK-043, BIZ-034, D-285).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWord {
    /// HOLD or ESPERA, with the job number.
    Hold(u32),
    /// AWAY or AUSENTE, with the hours.
    Away(u32),
}

/// PICK-043, BIZ-034, D-285, D-309: reads "HOLD 1042", "ESPERA 1042", "AWAY 3" or "AUSENTE 3"
/// (any case, trailing periods or exclamation marks dropped); the number is required. These are
/// never opt-out words; anything else is none.
#[must_use]
pub fn read_time_word(words: &str) -> Option<TimeWord> {
    let mut parts = words
        .trim_end()
        .trim_end_matches(['.', '!'])
        .split_whitespace();
    let word = parts.next()?.to_lowercase();
    let number: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    match word.as_str() {
        "hold" | "espera" => Some(TimeWord::Hold(number)),
        "away" | "ausente" => Some(TimeWord::Away(number)),
        _ => None,
    }
}

/// The facts a HOLD is judged on (PICK-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldAsk {
    /// The asking person's roles; the owner holds both (BIZ-023).
    pub by: Roles,
    /// Whether the homeowner chose emergency at INT-006 (a "Today" answer alone is not).
    pub emergency: bool,
    /// Whether this pick was already HOLDed.
    pub held: bool,
    /// Whether the pick's Confirm clock started late because the business was away (BIZ-034).
    pub away_delayed: bool,
}

/// Why a HOLD or an "Away until" is refused (PICK-043, BIZ-034).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRefusal {
    /// The person is neither the owner nor a "Confirm and billing" member.
    Role,
    /// HOLD on a job the homeowner chose as an emergency.
    Emergency,
    /// A second HOLD on the same pick.
    AlreadyHeld,
    /// HOLD on a pick whose clock started late under BIZ-034.
    AwayDelayed,
    /// The Confirm window already ended (PICK-035).
    Ended,
    /// "Away until" not after now or more than the setting's business minutes ahead.
    TooFar,
}

/// PICK-043, D-252, D-284, D-344: whether the "Hold for 2 more hours" link is offered and a HOLD
/// text can extend the window; the reasons are checked in this order.
///
/// # Errors
///
/// The [`TimeRefusal`] the business is told: the window can't be extended.
pub fn hold_allowed(ask: HoldAsk) -> Result<(), TimeRefusal> {
    if !ask.by.confirm_and_billing() {
        return Err(TimeRefusal::Role);
    }
    if ask.emergency {
        return Err(TimeRefusal::Emergency);
    }
    if ask.held {
        return Err(TimeRefusal::AlreadyHeld);
    }
    if ask.away_delayed {
        return Err(TimeRefusal::AwayDelayed);
    }
    Ok(())
}

/// PICK-043, D-252, D-477: the Confirm window's new end when HOLD arrives at `now` on a window
/// ending at `due`: [`WindowRules::hold_minutes`] business minutes past `due`. The business is
/// told the new end; the homeowner gets no text (PICK-045), and she can still pick another quote.
///
/// # Errors
///
/// [`hold_allowed`]'s refusal, or [`TimeRefusal::Ended`] at or after `due`.
pub fn hold(
    ask: HoldAsk,
    (now, due): (Timestamp, Timestamp),
    rules: WindowRules,
) -> Result<Timestamp, TimeRefusal> {
    hold_allowed(ask)?;
    if now >= due {
        return Err(TimeRefusal::Ended);
    }
    Ok(add_business_minutes(due, rules.hold_minutes))
}

/// BIZ-034, D-285: "Away until `until`", set at `now` by a person with `by`'s roles, from the
/// portal or the Away link; allowed after `now` and at most [`WindowRules::away_max_minutes`]
/// business minutes ahead.
///
/// # Errors
///
/// [`TimeRefusal::Role`] for a "Cards and quotes"-only person, [`TimeRefusal::TooFar`] otherwise.
pub fn away_until(
    by: Roles,
    (now, until): (Timestamp, Timestamp),
    rules: WindowRules,
) -> Result<Timestamp, TimeRefusal> {
    if !by.confirm_and_billing() {
        return Err(TimeRefusal::Role);
    }
    if until <= now || until > add_business_minutes(now, rules.away_max_minutes) {
        return Err(TimeRefusal::TooFar);
    }
    Ok(until)
}

/// BIZ-034, D-285: AWAY <hours> (AUSENTE <hours>) at `now`: away until `hours` business hours
/// ahead; AWAY 0 (at any hour, even at night when no business minute has passed yet) or more
/// than the setting allows is refused.
///
/// # Errors
///
/// As [`away_until`]; [`TimeRefusal::TooFar`] for AWAY 0 once the role is allowed.
pub fn away_for(
    by: Roles,
    now: Timestamp,
    hours: u32,
    rules: WindowRules,
) -> Result<Timestamp, TimeRefusal> {
    if !by.confirm_and_billing() {
        return Err(TimeRefusal::Role);
    }
    if hours == 0 {
        return Err(TimeRefusal::TooFar);
    }
    let minutes = hours.saturating_mul(60).min(rules.away_max_minutes + 1);
    away_until(by, (now, add_business_minutes(now, minutes)), rules)
}

/// BIZ-034, PICK-019, D-285: where a pick's Confirm clock starts, and whether Away delayed it: a
/// non-emergency pick whose notice goes at `sent` while the business is away until `away` starts
/// at `away`, with no text to the homeowner; away never delays an emergency pick.
#[must_use]
pub fn window_start(
    sent: Timestamp,
    away: Option<Timestamp>,
    emergency: bool,
) -> (Timestamp, bool) {
    match away {
        Some(until) if !emergency && until > sent => (until, true),
        _ => (sent, false),
    }
}

/// What the homeowner is told about an emergency pick at night (PICK-044, D-346).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightLine {
    /// Nothing about the night: the business ticked after-hours emergencies, or it's daytime.
    None,
    /// "doesn't get emergency requests at night and will see your pick at 8 a.m."
    SeesAtEight,
    /// "doesn't get emergency requests at night and may not see your pick after 8 p.m."
    MayMissAfterEight,
}

/// An emergency pick's timings (PICK-044).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmergencyPlan {
    /// When the pick notice goes; the window counts from here.
    pub send_at: Timestamp,
    /// The Confirm window's end: [`WindowRules::emergency_minutes`] clock minutes after the send.
    pub confirm_by: Timestamp,
    /// The one reminder email, held to 8 a.m. by quiet hours for a business without after-hours;
    /// none when that held time is at or after `confirm_by` (the window would already be over).
    pub reminder_at: Option<Timestamp>,
    /// The homeowner's night line.
    pub night: NightLine,
}

impl EmergencyPlan {
    /// PICK-044: whether the Confirm window is open at `at`: from the send, strictly before
    /// `confirm_by`.
    #[must_use]
    pub fn open_at(&self, at: Timestamp) -> bool {
        self.send_at <= at && at < self.confirm_by
    }
}

/// PICK-044, D-270, D-346: the emergency pick made at `picked` of a business that ticked
/// after-hours emergencies (BIZ-007) when `after_hours`. Clock minutes at any hour; a business
/// without the tick picked between 8 p.m. and 8 a.m. gets its notice at 8 a.m. and the window
/// counts from then; picked before 8 p.m. with the window ending after it, its notice goes at once
/// and its reminder waits for 8 a.m., or is dropped when 8 a.m. is past the window's end.
#[must_use]
pub fn emergency_plan(picked: Timestamp, after_hours: bool, rules: WindowRules) -> EmergencyPlan {
    let night_held = !after_hours && !is_business_hours(picked);
    let send_at = if night_held {
        add_business_minutes(picked, 0)
    } else {
        picked
    };
    let confirm_by = clock_minutes(send_at, rules.emergency_minutes);
    let reminder = clock_minutes(send_at, rules.emergency_reminder_minutes);
    let last_second = confirm_by.saturating_sub(SignedDuration::from_secs(1));
    let ends_at_night = !is_business_hours(last_second.unwrap_or(confirm_by));
    let (night, reminder_at) = match (after_hours, night_held, ends_at_night) {
        (true, _, _) => (NightLine::None, reminder),
        (false, true, _) => (NightLine::SeesAtEight, reminder),
        (false, false, true) => (
            NightLine::MayMissAfterEight,
            add_business_minutes(reminder, 0),
        ),
        (false, false, false) => (NightLine::None, reminder),
    };
    EmergencyPlan {
        send_at,
        confirm_by,
        reminder_at: (reminder_at < confirm_by).then_some(reminder_at),
        night,
    }
}

/// PICK-044, JOB-014, FEE-020: the end of an emergency job's card-fix window for a card declined
/// at `declined`: [`WindowRules::emergency_minutes`] clock minutes at any hour.
#[must_use]
pub fn emergency_fix_by(declined: Timestamp, rules: WindowRules) -> Timestamp {
    clock_minutes(declined, rules.emergency_minutes)
}

fn clock_minutes(from: Timestamp, minutes: i64) -> Timestamp {
    from.saturating_add(SignedDuration::from_mins(minutes))
        .unwrap_or(Timestamp::MAX)
}

/// What the email provider reported for one email of a pick notice, to one address (PICK-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailOutcome {
    /// No final word yet.
    Waiting,
    /// The receiving mail server accepted it.
    Delivered,
    /// Delivered to a spam folder: counts as sent (D-332).
    Spam,
    /// Bounced.
    Bounced,
    /// The provider could not send it.
    Failed,
}

/// PICK-019, D-345, D-477: the window's new end when the pick notice of a window ending at `due`
/// had `outcomes`, one per email and address: [`WindowRules::bounce_grace_minutes`] business
/// minutes more when every one bounced or failed, the job is not an emergency, the grace was not
/// `granted` before and `now` is still before `due` (as [`hold`], D-1974); none otherwise (a spam
/// delivery or a wait counts as sent). The homeowner gets no text about it (PICK-045).
#[must_use]
pub fn bounce_grace(
    outcomes: &[EmailOutcome],
    (emergency, granted): (bool, bool),
    (now, due): (Timestamp, Timestamp),
    rules: WindowRules,
) -> Option<Timestamp> {
    let all_bounced = !outcomes.is_empty()
        && outcomes
            .iter()
            .all(|o| matches!(o, EmailOutcome::Bounced | EmailOutcome::Failed));
    (all_bounced && !emergency && !granted && now < due)
        .then(|| add_business_minutes(due, rules.bounce_grace_minutes))
}
