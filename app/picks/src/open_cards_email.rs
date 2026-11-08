//! The daily open-cards email's words and the monthly missed-cards line (CARD-022, BIZ-044,
//! BIZ-038; T198, D-330, D-331, D-345, D-380, D-491, D-1993, D-1994).
//!
//! `messaging` sits below `quotes` and `picks`, so the words are built here. [`chosen_minutes`]
//! reads a person's chosen times through `businesses::api` for `messaging::api::digest_goes`;
//! the caller passes only active team members who get job cards (BIZ-037). [`open_cards_email`]
//! lists the unquoted cards with at least `min_left` of their window left (trade, town, end time,
//! Quote link) and, for a person who gets picks, each pick waiting for Confirm, first, with its
//! end and Confirm link; with a pick waiting the subject names it. [`missed_cards_line`] is
//! BIZ-044's account email. No homeowner detail, no marketing; wording from the brand file
//! (D-1971), English and Spanish.

use askhava_businesses::api::{OpenCardsTime, open_cards_times};
use askhava_core::api::{arizona_time, brand};
use askhava_messaging::api::{Language, MonthCards, listable};
use jiff::{SignedDuration, Timestamp, Zoned};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::PicksError;

/// One unquoted job card as the composer read it from `quotes` (CARD-022): no homeowner detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCard<'a> {
    /// The trade, as on the card.
    pub trade: &'a str,
    /// The town, as on the card.
    pub town: &'a str,
    /// When the quote window ends (PICK-001, unchanged).
    pub ends: Timestamp,
    /// The person's signed Quote link (BIZ-038).
    pub quote_link: &'a str,
}

/// One pick waiting for this person's business to Confirm (CARD-022, D-345).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitingPick<'a> {
    /// The job number.
    pub job: u32,
    /// When the Confirm window ends.
    pub ends: Timestamp,
    /// The person's signed Confirm link.
    pub confirm_link: &'a str,
}

/// A composed email: its subject and its lines, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedEmail {
    /// The subject line.
    pub subject: String,
    /// The body lines.
    pub lines: Vec<String>,
}

/// CARD-022: `member`'s chosen times as minutes after midnight, Arizona time; empty is "off".
///
/// # Errors
/// [`PicksError::Database`].
pub async fn chosen_minutes(db: &mut PgConnection, member: Uuid) -> Result<Vec<u16>, PicksError> {
    let times = open_cards_times(db, member)
        .await
        .map_err(|_| PicksError::Database)?;
    Ok(times
        .times()
        .into_iter()
        .map(OpenCardsTime::minute)
        .collect())
}

/// "5 p.m." or "9:30 p.m.", Arizona time; Spanish adds the article ("las 5 p.m.", "la 1 p.m.").
fn clock(t: &Zoned, language: Language) -> String {
    let (h, m) = (t.hour(), t.minute());
    let half = if h < 12 { "a.m." } else { "p.m." };
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let time = match m {
        0 => format!("{h12} {half}"),
        _ => format!("{h12}:{m:02} {half}"),
    };
    match (language, h12) {
        (Language::En, _) => time,
        (Language::Es, 1) => format!("la {time}"),
        (Language::Es, _) => format!("las {time}"),
    }
}

/// The day an end falls on, after its clock: nothing today, "tomorrow", the weekday within a
/// week, else the date (Arizona time).
fn day_word(t: &Zoned, days: i64, language: Language) -> String {
    let weekday = t.weekday().to_monday_zero_offset();
    let (month, day) = (usize::try_from(t.month() - 1).unwrap_or(0), t.day());
    match (days, language) {
        (0, _) => String::new(),
        (1, Language::En) => " tomorrow".to_owned(),
        (1, Language::Es) => " de mañana".to_owned(),
        (2..=6, Language::En) => format!(" {}", WEEKDAYS_EN[usize::from(weekday.unsigned_abs())]),
        (2..=6, Language::Es) => {
            format!(" del {}", WEEKDAYS_ES[usize::from(weekday.unsigned_abs())])
        }
        (_, Language::En) => format!(" {} {day}", MONTHS_EN[month]),
        (_, Language::Es) => format!(" del {day} de {}", MONTHS_ES[month]),
    }
}

const WEEKDAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const WEEKDAYS_ES: [&str; 7] = [
    "lunes",
    "martes",
    "miércoles",
    "jueves",
    "viernes",
    "sábado",
    "domingo",
];
const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_ES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// An end's time and day as the email words it ("5 p.m.", "9 a.m. tomorrow", "5 p.m. Thursday";
/// Spanish "las 5 p.m. del jueves"), Arizona time, counted in calendar days from `now`.
fn end_time(at: Timestamp, now: Timestamp, language: Language) -> String {
    let (t, today) = (arizona_time(at), arizona_time(now));
    let days = (t.date() - today.date()).get_days();
    format!(
        "{}{}",
        clock(&t, language),
        day_word(&t, i64::from(days), language)
    )
}

fn short(language: Language) -> &'static str {
    match language {
        Language::En => brand().short_name(),
        Language::Es => brand().es().short_name(),
    }
}

fn pick_line(p: &WaitingPick<'_>, now: Timestamp, language: Language) -> String {
    let (job, by, link) = (p.job, end_time(p.ends, now, language), p.confirm_link);
    match language {
        Language::En => format!("Job {job}: a homeowner chose your quote. Confirm by {by}: {link}"),
        Language::Es => format!(
            "Trabajo {job}: un cliente eligió su cotización. Confirme antes de {by}: {link}"
        ),
    }
}

fn card_line(c: &OpenCard<'_>, now: Timestamp, language: Language) -> String {
    let (trade, town) = (c.trade, c.town);
    let until = end_time(c.ends, now, language);
    match language {
        Language::En => format!("{trade} in {town}. Quote by {until}: {}", c.quote_link),
        Language::Es => format!(
            "{trade} en {town}. Cotice antes de {until}: {}",
            c.quote_link
        ),
    }
}

/// CARD-022, D-330, D-345, D-380: the open-cards email at `now`, or `None` when no card has
/// `min_left` left and no pick waits. A pick whose Confirm window already ended is left out. Picks
/// come first, earliest end first; the subject names the
/// first pick, else "Your job requests still open".
#[must_use]
pub fn open_cards_email(
    (cards, picks): (&[OpenCard<'_>], &[WaitingPick<'_>]),
    now: Timestamp,
    min_left: SignedDuration,
    language: Language,
) -> Option<ComposedEmail> {
    let mut picks: Vec<_> = picks.iter().filter(|p| p.ends > now).copied().collect();
    picks.sort_by_key(|p| p.ends);
    let listed = cards.iter().filter(|c| listable(c.ends, now, min_left));
    let lines: Vec<String> = (picks.iter().map(|p| pick_line(p, now, language)))
        .chain(listed.map(|c| card_line(c, now, language)))
        .collect();
    let b = short(language);
    let subject = match (picks.first(), language) {
        (Some(p), Language::En) => format!(
            "{b}: Pick waiting: confirm job {} by {}",
            p.job,
            end_time(p.ends, now, language)
        ),
        (Some(p), Language::Es) => format!(
            "{b}: Selección en espera: confirme el trabajo {} antes de {}",
            p.job,
            end_time(p.ends, now, language)
        ),
        (None, Language::En) => format!("{b}: Your job requests still open"),
        (None, Language::Es) => format!("{b}: Sus solicitudes de trabajo siguen abiertas"),
    };
    (!lines.is_empty()).then_some(ComposedEmail { subject, lines })
}

/// BIZ-044, D-331, D-406: the missed-cards account email for `month`, with the BIZ-042 inbox
/// steps link and the BIZ-043 backup-address link, exactly as the spec words it.
#[must_use]
pub fn missed_cards_line(
    month: MonthCards,
    (fix, backup): (&str, &str),
    language: Language,
) -> String {
    let (b, n, q, k) = (
        short(language),
        month.got,
        month.quoted,
        month.ended_unquoted,
    );
    match language {
        Language::En => format!(
            "{b}: Last month you got {n} job requests and quoted {q}. {k} closed before you quoted. If our emails land in Spam or Promotions, this 1-minute fix moves them to your inbox: {fix}. Or add a second address: {backup}."
        ),
        Language::Es => format!(
            "{b}: El mes pasado recibió {n} solicitudes de trabajo y cotizó {q}. {k} se cerraron antes de que cotizara. Si nuestros correos llegan a Spam o Promociones, este arreglo de 1 minuto los pasa a su bandeja de entrada: {fix}. O agregue una segunda dirección: {backup}."
        ),
    }
}
