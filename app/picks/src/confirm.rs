//! The business's Confirm window, the license re-checks at the pick and at Confirm, the recurring
//! fee line and the reply to a business's YES or CONFIRM text (PICK-019, PICK-020, PICK-033 to
//! PICK-035, PICK-037; D-056, D-196, D-345, D-365).
//!
//! The window is [`ConfirmSettings::window_minutes`] business minutes (4 business hours by
//! default, a setting) from the pick notice's send, so a notice held by the email limit counts
//! from its real send ([`crate::handlers::notice_sent`]). The license is checked once live at the
//! pick ([`check_at_pick`]) and from the stored results at Confirm ([`check_at_confirm`]).

use askhava_adapters::api::LicenseSource;
use askhava_businesses::api::AccountStatus;
use askhava_core::api::SettingsError;
use askhava_core::api::add_business_minutes;
use askhava_licensing::api::{
    Freshness, LookupOutcome, arizona_day, end_date_pauses, lookup_at_pick, roc_check_allows_card,
};
use askhava_messaging::api::Language;
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;
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
fn dollars(cents: i64) -> String {
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
