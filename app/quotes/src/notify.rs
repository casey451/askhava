//! The homeowner texts about quotes: the first-quote text, the quote-count updates and their
//! 3-hour gap, the LIST text and the cards-out text (TXT-016, TXT-017, TXT-031 to TXT-033,
//! WORD-011, PICK-002, PICK-003; D-1190 to D-1194).
//!
//! This module only builds words and decides when a quote text is due; it sends nothing and holds
//! no homeowner detail. `intake` (T048) finds the homeowner, her language and phone zones, fills the
//! private quote-page link (never kept here, SEC-037), applies quiet hours and queues the outbox
//! row. Every fixed sentence is a `messaging` template (`first_quote`, `quote_update`, `quote_list`,
//! `quote_list_which`, `request_out` and its `request_out_*` additions), in English and Spanish.
//!
//! A quote's amount and period are printed exactly as the business stored them: this module has no
//! conversion, rounding or suggestion (WORD-011, D-358). The first-quote text carries no amount at
//! all; the amounts are in the LIST text and on the quote card (D-1191).

use askhava_messaging::api::{Language, Text, render};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::intake::{DayPart, Period, Price};
use crate::models::{self, QuotesError};

/// The least gap between two quote texts on one job, measured from the delivery of the previous
/// one (TXT-017, D-012). The gap is a setting; this is its default.
pub const UPDATE_GAP: SignedDuration = SignedDuration::from_hours(3);
/// The most quotes in one LIST text (TXT-031).
pub const LIST_PAGE: usize = 10;
/// The default for "a card went to 1 or 2 businesses" (TXT-032, a setting).
pub const SLOW_AT_MOST: u32 = 2;
/// Every template begins with this; a joined text keeps it once.
const PREFIX: &str = "Ask Hava: ";
/// The clock Ask Hava states every time in (TXT-033).
const ARIZONA: &str = "America/Phoenix";

/// The language a homeowner is texted in: her own, and whether Spanish conversations are on
/// (TXT-039). The same rule `messaging::api::render` applies to each template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reader {
    /// The language she chose.
    pub wanted: Language,
    /// The owner-portal "Spanish conversations" setting.
    pub spanish_on: bool,
}

impl Reader {
    /// The language the texts come out in.
    #[must_use]
    pub fn language(self) -> Language {
        if self.wanted == Language::Es && self.spanish_on {
            Language::Es
        } else {
            Language::En
        }
    }
}

/// The previous quote text on a job: when it was delivered and how many quotes it told of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
    /// The delivery time of the first-quote or update text (TXT-017).
    pub at: Timestamp,
    /// The quotes that text counted.
    pub quoted: u32,
}

/// What the homeowner is due to be told about quotes now (TXT-016, TXT-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// No quote yet, or none beyond what the last text told.
    Nothing,
    /// The first quote is in and no quote text has gone: send the TXT-016 text.
    First,
    /// More quotes are in and the gap has passed: send the update with this count.
    Update {
        /// The quotes now in.
        quoted: u32,
    },
    /// More quotes are in but the gap has not passed; try again at `until`.
    Held {
        /// The earliest delivery time of the next update.
        until: Timestamp,
    },
}

/// TXT-016, TXT-017, D-012: the first quote gets the first-quote text; later counts go at most once
/// per [`UPDATE_GAP`] from the previous text's delivery (the first-quote text counts as the
/// previous text, D-1190), 2:59:59 later is held and 3:00:00 later is sent.
#[must_use]
pub fn notice(quoted: u32, previous: Option<Sent>, now: Timestamp) -> Notice {
    let Some(last) = previous else {
        return if quoted == 0 {
            Notice::Nothing
        } else {
            Notice::First
        };
    };
    if quoted <= last.quoted {
        return Notice::Nothing;
    }
    let due = last.at.saturating_add(UPDATE_GAP).unwrap_or(last.at);
    if now < due {
        Notice::Held { until: due }
    } else {
        Notice::Update { quoted }
    }
}

/// TXT-016, TXT-017: [`notice`] for the job's shown quotes now (confirmed, not withdrawn).
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn notice_for(
    db: &mut PgConnection,
    job: Uuid,
    previous: Option<Sent>,
    now: Timestamp,
) -> Result<Notice, QuotesError> {
    let shown = models::shown_quotes(db, job).await?;
    let quoted = u32::try_from(shown.len()).unwrap_or(u32::MAX);
    Ok(notice(quoted, previous, now))
}

/// Renders one template for `reader`.
fn text(id: &str, reader: Reader, values: &[(&str, &str)]) -> Result<Text, QuotesError> {
    render(id, reader.wanted, reader.spanish_on, values).map_err(|_| QuotesError::Text)
}

/// TXT-016, D-290, D-438: "Your first quote for request <n> is in", the free line, the introduction
/// fee line, the LIST line before the link, the private page `link` and "Terms (18+)". No amount.
///
/// # Errors
///
/// [`QuotesError::Text`].
pub fn first_quote_text(reader: Reader, job: &str, link: &str) -> Result<String, QuotesError> {
    Ok(text("first_quote", reader, &[("job", job), ("link", link)])?.body)
}

/// The numbers a quote-count update tells (TXT-017, TXT-032).
#[derive(Debug, Clone, Copy)]
pub struct Counts<'a> {
    /// The request number.
    pub job: &'a str,
    /// How many quotes are in (<k>).
    pub quoted: u32,
    /// How many businesses the card went to, as CARD-019 counts them (<N>).
    pub sent: u32,
}

/// TXT-017, TXT-032: "You now have <k> quotes ... <k> of <N> businesses have quoted so far" and the
/// page `link`.
///
/// # Errors
///
/// [`QuotesError::Text`].
pub fn update_text(reader: Reader, counts: Counts<'_>, link: &str) -> Result<String, QuotesError> {
    let (quoted, sent) = (counts.quoted.to_string(), counts.sent.to_string());
    let values = [
        ("job", counts.job),
        ("quoted", &quoted),
        ("count", &sent),
        ("link", link),
    ];
    Ok(text("quote_update", reader, &values)?.body)
}

/// A pickable quote as the LIST text states it (TXT-031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The business's name.
    pub name: String,
    /// The price exactly as the business stored it (PICK-002).
    pub price: Price,
    /// The earliest visit the business gave.
    pub earliest_visit: Option<(Date, DayPart)>,
}

/// The dollars of `cents`, whole when there are no cents: "$40", "$1,250", "$40.50".
fn money(cents: u64) -> String {
    let (dollars, rest) = (cents / 100, cents % 100);
    let digits = dollars.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if rest == 0 {
        format!("${out}")
    } else {
        format!("${out}.{rest:02}")
    }
}

/// "per visit", "per week" ... in the reader's language.
fn per(period: Period, es: bool) -> &'static str {
    let (en, sp) = match period {
        Period::Visit => ("per visit", "por visita"),
        Period::Week => ("per week", "por semana"),
        Period::Month => ("per month", "por mes"),
        Period::Quarter => ("per quarter", "por trimestre"),
        Period::Year => ("per year", "por año"),
    };
    if es { sp } else { en }
}

/// WORD-011, D-358: the amount and period as the business stated them, nothing converted, rounded
/// or added. A tier's name is the business's own (it passed the PICK-007 bounce).
fn amount(price: &Price, es: bool) -> String {
    let (to, setup, site) = if es {
        (
            " a ",
            "más {} de instalación única",
            "rango por confirmar en el sitio",
        )
    } else {
        (
            " to ",
            "plus {} one-time setup",
            "Range to be confirmed on site",
        )
    };
    match price {
        Price::Firm(c) => money(*c),
        Price::Range { low, high, on_site } => {
            let base = format!("{}{to}{}", money(*low), money(*high));
            if *on_site {
                format!("{base}, {site}")
            } else {
                base
            }
        }
        Price::Monthly {
            cents,
            period,
            setup: s,
        } => {
            let base = format!("{} {}", money(*cents), per(*period, es));
            s.map_or(base.clone(), |s| {
                format!("{base}, {}", setup.replace("{}", &money(s)))
            })
        }
        Price::Tiers(t) => t
            .iter()
            .map(|(n, c)| format!("{n} {}", money(*c)))
            .collect::<Vec<_>>()
            .join("; "),
        Price::VisitFee(c) => format!(
            "{} {}",
            money(*c),
            if es { "por la visita" } else { "visit fee" }
        ),
        Price::Hourly { rate, hours } => hourly(*rate, *hours, es),
    }
}

/// An hourly rate with the hours the business estimated, if any (FEE-043).
fn hourly(rate: u64, hours: Option<(u32, u32)>, es: bool) -> String {
    let base = format!(
        "{} {}",
        money(rate),
        if es { "por hora" } else { "per hour" }
    );
    let (about, unit) = if es {
        ("unas", "horas")
    } else {
        ("about", "hours")
    };
    match hours {
        None => base,
        Some((lo, hi)) if lo == hi => format!("{base}, {about} {lo} {unit}"),
        Some((lo, hi)) => {
            let to = if es { "a" } else { "to" };
            format!("{base}, {about} {lo} {to} {hi} {unit}")
        }
    }
}

/// "Nov 9, evening" in the reader's language (TXT-031, PICK-040).
fn visit(date: Date, part: DayPart, es: bool) -> String {
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const ES: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
    ];
    let i = usize::try_from(date.month() - 1).unwrap_or_default();
    let (months, parts) = if es {
        (ES, ["mañana", "tarde", "noche", "esta noche"])
    } else {
        (EN, ["morning", "afternoon", "evening", "tonight"])
    };
    let day = format!("{} {}", months[i.min(11)], date.day());
    let day = if es {
        format!("{} {}", date.day(), months[i.min(11)])
    } else {
        day
    };
    format!("{day}, {}", parts[part as usize])
}

/// One LIST line: its number, the business, the amount and the earliest visit (TXT-031).
fn list_line(n: usize, item: &Listed, es: bool) -> String {
    let head = format!("{n}. {}: {}.", item.name, amount(&item.price, es));
    match item.earliest_visit {
        Some((d, p)) => {
            let label = if es {
                "Primera visita:"
            } else {
                "Earliest visit:"
            };
            format!("{head} {label} {}.", visit(d, p, es))
        }
        None => head,
    }
}

/// TXT-031, D-239, D-290, D-358, D-420, D-443, D-446: the pickable quotes in the order given (arrival
/// order), numbered from 1, at most [`LIST_PAGE`] to a text; every text ends with the "2 YES" line,
/// the share sentence and the Terms line (PICK-016). Twelve quotes make two texts, numbered 1 to 10
/// and 11 to 12. No quotes make no text.
///
/// # Errors
///
/// [`QuotesError::Text`].
pub fn list_texts_from(
    reader: Reader,
    job: &str,
    items: &[Listed],
) -> Result<Vec<String>, QuotesError> {
    let es = reader.language() == Language::Es;
    items
        .chunks(LIST_PAGE)
        .enumerate()
        .map(|(page, chunk)| {
            let lines: Vec<String> = chunk
                .iter()
                .enumerate()
                .map(|(i, item)| list_line(page * LIST_PAGE + i + 1, item, es))
                .collect();
            let head = text("quote_list", reader, &[("job", job)])?.body;
            let end = text("quote_list_end", reader, &[])?.body;
            let end = end.strip_prefix(PREFIX).unwrap_or(&end);
            Ok(format!("{head}\n{}\n{end}", lines.join("\n")))
        })
        .collect()
}

/// TXT-031, PICK-002: [`list_texts_from`] for the job's pickable quotes (confirmed, not withdrawn,
/// not marked unavailable), in arrival order, naming each business from `names`.
///
/// # Errors
///
/// [`QuotesError::Database`], or [`QuotesError::Business`] when a quote's business has no name in
/// `names`, or [`QuotesError::Text`].
pub async fn list_texts(
    db: &mut PgConnection,
    (job_id, job): (Uuid, &str),
    reader: Reader,
    names: &[(Uuid, String)],
) -> Result<Vec<String>, QuotesError> {
    let rows = models::list_rows(db, job_id).await?;
    let items: Result<Vec<Listed>, QuotesError> = rows
        .into_iter()
        .map(|r| {
            let name = names.iter().find(|(id, _)| *id == r.business);
            Ok(Listed {
                name: name.ok_or(QuotesError::Business)?.1.clone(),
                price: r.price,
                earliest_visit: r.earliest_visit,
            })
        })
        .collect();
    list_texts_from(reader, job, &items?)
}

/// TXT-031, D-239: with more than one job in Quotes ready the homeowner is first asked which job,
/// with one of her request numbers (`example`) as the example (D-1192).
///
/// # Errors
///
/// [`QuotesError::Text`].
pub fn which_job_text(reader: Reader, example: &str) -> Result<String, QuotesError> {
    Ok(text("quote_list_which", reader, &[("job", example)])?.body)
}

/// A zone's name in a time: "Mountain", "Central" ... or its own abbreviation for any other zone.
fn zone_name(zone: &str, es: bool, at: &jiff::Zoned) -> String {
    let (en, sp) = match zone {
        "America/New_York" => ("Eastern", "Este"),
        "America/Chicago" => ("Central", "Central"),
        "America/Denver" => ("Mountain", "Montaña"),
        "America/Los_Angeles" => ("Pacific", "Pacífico"),
        "America/Anchorage" => ("Alaska", "Alaska"),
        "Pacific/Honolulu" => ("Hawaii", "Hawái"),
        _ => return at.strftime("%Z").to_string(),
    };
    (if es { sp } else { en }).to_owned()
}

/// "8 a.m." or "5:30 p.m.", the clock of `at`.
fn clock(at: &jiff::Zoned) -> String {
    let (h, m) = (at.hour(), at.minute());
    let hour = match h % 12 {
        0 => 12,
        n => n,
    };
    let half = if h < 12 { "a.m." } else { "p.m." };
    if m == 0 {
        format!("{hour} {half}")
    } else {
        format!("{hour}:{m:02} {half}")
    }
}

/// TXT-033, D-293: `at` as Arizona time, plus the clock time in each of the recipient's area-code
/// `zones` that shows a different one: "5 p.m. Arizona time (6 p.m. Mountain)".
///
/// # Errors
///
/// [`QuotesError::Text`] when a time zone is unknown.
pub fn arizona_time(at: Timestamp, zones: &[String], es: bool) -> Result<String, QuotesError> {
    let az = at.in_tz(ARIZONA).map_err(|_| QuotesError::Text)?;
    let label = if es {
        "hora de Arizona"
    } else {
        "Arizona time"
    };
    let mut out = format!("{} {label}", clock(&az));
    let mut others: Vec<String> = Vec::new();
    for zone in zones {
        let local = at.in_tz(zone).map_err(|_| QuotesError::Text)?;
        let shown = format!("{} {}", clock(&local), zone_name(zone, es, &local));
        if clock(&local) != clock(&az) && !others.contains(&shown) {
            others.push(shown);
        }
    }
    if !others.is_empty() {
        out = format!("{out} ({})", others.join(", "));
    }
    Ok(out)
}

/// What the cards-out text tells (TXT-032).
#[derive(Debug, Clone, Copy)]
pub struct CardsOut<'a> {
    /// The request number.
    pub job: &'a str,
    /// The businesses the card went to, as CARD-019 counts them (<N>).
    pub count: u32,
    /// The trade's name, as the card says it.
    pub trade: &'a str,
    /// When quiet hours let the cards go, when they hold them (D-241).
    pub held_until: Option<Timestamp>,
    /// The job was answered "Today, it's urgent" (D-292).
    pub today: bool,
    /// "Quotes may be slow" shows for 1 up to this many businesses ([`SLOW_AT_MOST`], a setting).
    pub slow_at_most: u32,
}

/// TXT-032, TXT-033, D-241, D-292, D-435: "Request <n> is out to <N> <trade> businesses ...", then
/// "It goes out at <time>." when quiet hours hold the cards, the "pick as soon as the first quote
/// arrives" lines for a "Today" job and "Quotes may be slow with only <N> on Ask Hava so far." for
/// a small count. Each time is Arizona time, with the recipient's `zones` clock beside it when it
/// differs.
///
/// # Errors
///
/// [`QuotesError::Text`].
pub fn cards_out_text(
    reader: Reader,
    out: &CardsOut<'_>,
    zones: &[String],
) -> Result<String, QuotesError> {
    let count = out.count.to_string();
    let base = [("job", out.job), ("count", &count), ("trade", out.trade)];
    let mut parts = vec![text("request_out", reader, &base)?];
    if let Some(until) = out.held_until {
        let time = arizona_time(until, zones, reader.language() == Language::Es)?;
        parts.push(text("request_out_held", reader, &[("time", &time)])?);
    }
    if out.today {
        parts.push(text("request_out_today", reader, &[])?);
    }
    if (1..=out.slow_at_most).contains(&out.count) {
        parts.push(text("request_out_slow", reader, &[("count", &count)])?);
    }
    let bodies: Vec<&str> = parts
        .iter()
        .map(|t| t.body.strip_prefix(PREFIX).unwrap_or(&t.body))
        .collect();
    Ok(format!("{PREFIX}{}", bodies.join(" ")))
}
