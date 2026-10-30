//! Quiet hours (QH-001 to QH-009, TXT-025): when a text or a business email may go.
//!
//! Pure decision logic, no I/O and no clock of its own: the caller passes the instant. A non-urgent
//! message goes only when it is 8:00 a.m. to before 8:00 p.m. in Arizona time (`America/Phoenix`, no
//! daylight saving) and in the zone of the recipient's area code (`area_codes.json`). A number whose
//! area code is not in the table, or that is not +1, is refused and never texted (D-100, D-139).
//! A held message goes at the start of the next instant both zones are inside the window. Held
//! updates for one job and recipient merge, and a held message is dropped when its job closed unless
//! it announces the close (D-158, D-461). The one table is parsed once by [`AreaCodes::embedded`].

use std::collections::{BTreeMap, HashMap};

use jiff::{SignedDuration, Timestamp, Zoned, civil::Date, tz::TimeZone};
use serde_json::from_str;
use thiserror::Error;

/// The Arizona zone every window is also checked in (QH-002).
const ARIZONA: &str = "America/Phoenix";
/// First local hour inside the window (QH-001).
const OPEN_HOUR: i8 = 8;
/// First local hour outside the window, so 19:59:59 is in and 20:00:00 is out (QH-001, D-101).
const CLOSE_HOUR: i8 = 20;
/// A direct reply is allowed at night only while younger than this (QH-004, D-101).
const REPLY_MINUTES: i64 = 15;
/// Days ahead searched for the next window start; a window opens within one day, so 3 is generous.
const SEARCH_DAYS: i64 = 3;

/// Why quiet hours could not decide, or refused a number. Carries no number (R6, PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum QuietHoursError {
    /// Not a +1 number with ten digits after it: never texted (D-100).
    #[error("number is not a North American number")]
    NotNorthAmerican,
    /// The area code is not in the table: never texted (QH-003, D-139).
    #[error("area code is not in the table")]
    UnknownAreaCode,
    /// The compiled-in area-code table is broken.
    #[error("area-code table is invalid")]
    InvalidTable,
    /// The time-zone data could not answer.
    #[error("time zone lookup failed")]
    Zone,
}

impl From<jiff::Error> for QuietHoursError {
    fn from(_: jiff::Error) -> Self {
        Self::Zone
    }
}

/// The area-code table: every code has one or more zones; a code spanning zones is judged in all of
/// them, so the stricter one rules (QH-001, QH-003, D-100, D-139).
#[derive(Debug)]
pub struct AreaCodes {
    zones: HashMap<String, Vec<String>>,
}

impl AreaCodes {
    /// Parses a table shaped `{"<zone>": ["<code>", ...]}`; every zone must be known to the bundled
    /// time-zone data, every code three digits, and no code may repeat within one zone (a code listed under two zones
    /// spans both, QH-003).
    ///
    /// # Errors
    ///
    /// [`QuietHoursError::InvalidTable`] for bad JSON, an unknown zone, a bad code or a code repeated in one zone.
    pub fn parse(json: &str) -> Result<Self, QuietHoursError> {
        let raw: BTreeMap<String, Vec<String>> =
            from_str(json).map_err(|_| QuietHoursError::InvalidTable)?;
        let mut zones = HashMap::new();
        for (zone, codes) in raw {
            TimeZone::get(&zone).map_err(|_| QuietHoursError::InvalidTable)?;
            for code in codes {
                add_code(&mut zones, code, &zone)?;
            }
        }
        Ok(Self { zones })
    }

    /// The compiled-in table (`area_codes.json`, US, territories and Canada only, D-100).
    ///
    /// # Errors
    ///
    /// [`QuietHoursError::InvalidTable`] if the compiled-in file is broken.
    pub fn embedded() -> Result<Self, QuietHoursError> {
        Self::parse(include_str!("../area_codes.json"))
    }

    /// Every (area code, zone) pair in the table, sorted so tests are reproducible.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut pairs: Vec<(&str, &str)> = self
            .zones
            .iter()
            .flat_map(|(c, zs)| zs.iter().map(move |z| (c.as_str(), z.as_str())))
            .collect();
        pairs.sort_unstable();
        pairs.into_iter()
    }

    /// The zones of a `+1` number's area code, at least one (QH-003); a split code lists each zone.
    ///
    /// # Errors
    ///
    /// [`QuietHoursError::NotNorthAmerican`] unless the number is `+1` and ten digits;
    /// [`QuietHoursError::UnknownAreaCode`] when the code is not in the table (D-100, D-139).
    pub fn zones_for(&self, phone: &str) -> Result<&[String], QuietHoursError> {
        let digits = phone
            .strip_prefix("+1")
            .filter(|d| d.len() == 10 && d.bytes().all(|b| b.is_ascii_digit()))
            .ok_or(QuietHoursError::NotNorthAmerican)?;
        self.zones
            .get(&digits[..3])
            .map(Vec::as_slice)
            .ok_or(QuietHoursError::UnknownAreaCode)
    }
}

/// Adds one code under `zone`, refusing a malformed code or one repeated in the same zone (QH-003).
fn add_code(
    zones: &mut HashMap<String, Vec<String>>,
    code: String,
    zone: &str,
) -> Result<(), QuietHoursError> {
    let digits = code.len() == 3 && code.bytes().all(|b| b.is_ascii_digit());
    let list = zones.entry(code).or_default();
    if !digits || list.iter().any(|z| z == zone) {
        return Err(QuietHoursError::InvalidTable);
    }
    list.push(zone.to_owned());
    Ok(())
}

/// How a message travels. Business notices go by email or text; quiet hours treat both alike (QH-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// A text message.
    Text,
    /// A business email.
    Email,
}

/// What a message is, which decides whether QH-004 lets it through at night.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The INT-004 911 text.
    Safety911,
    /// A STOP, HELP or START reply.
    KeywordReply,
    /// A pick code.
    PickCode,
    /// A direct reply to the person's own inbound text, sent at `inbound_at` (15-minute rule).
    DirectReply {
        /// When the person's own text arrived.
        inbound_at: Timestamp,
    },
    /// A job card (CARD-021 emergency rules).
    JobCard,
    /// A confirm request to a business.
    ConfirmRequest,
    /// The first-quote text (TXT-016, D-136).
    Quote,
    /// A quote-count update such as "You now have N quotes" (TXT-017, D-136); held ones merge (TXT-025).
    QuoteCount,
    /// A question INT-029 asks after the cards (D-164).
    JobQuestion,
    /// A pick, confirm or connect text: who's-calling, missed-call, PICK-044, PICK-046 (D-270).
    PickStep,
    /// The INT-033 after-hours notice (D-271).
    AfterHoursNotice,
    /// An update text about a job; held updates and quote-count updates merge (TXT-025).
    Update,
    /// Any other non-urgent message.
    Other,
}

/// One message to judge: who gets it, what it is and which job state it belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Outgoing<'a> {
    /// The recipient's `+1` number; its area code gives the zone, for emails too (QH-005).
    pub phone: &'a str,
    /// What the message is.
    pub kind: Kind,
    /// Text or email.
    pub channel: Channel,
    /// True when the homeowner chose emergency for the job (D-054).
    pub emergency_job: bool,
    /// True for a business that ticked "after-hours emergencies" (BIZ-007).
    pub after_hours_business: bool,
    /// True for a person with "Emergency emails at any hour" on (BIZ-037).
    pub emergency_emails: bool,
}

/// The answer: send now, or hold until the start of the next allowed window (QH-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Send at the instant asked about.
    Send,
    /// Send no earlier than `until`.
    Hold {
        /// The start of the next window both zones allow.
        until: Timestamp,
    },
}

/// True when the local time is 8:00:00 to 19:59:59, the QH-001 window (D-101).
fn in_window(at: &Zoned) -> bool {
    (OPEN_HOUR..CLOSE_HOUR).contains(&at.hour())
}

/// QH-001: true when `at` is inside the window in Arizona (QH-002) and in every zone of the area code.
fn allowed_at(zones: &[String], at: Timestamp) -> Result<bool, QuietHoursError> {
    if !in_window(&at.in_tz(ARIZONA)?) {
        return Ok(false);
    }
    for zone in zones {
        if !in_window(&at.in_tz(zone.as_str())?) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The instant `zone` reaches 08:00 on `day`, the only moment an allowed stretch can begin (QH-006).
fn opens_on(zone: &str, day: Date) -> Result<Timestamp, QuietHoursError> {
    Ok(day.at(OPEN_HOUR, 0, 0, 0).in_tz(zone)?.timestamp())
}

/// The first instant after `now` at which both Arizona and `zone` are inside 8:00 to 20:00. An
/// allowed stretch always begins when one zone opens at 08:00 local, so those are the only candidates
/// (QH-001, QH-006, QH-009).
fn next_window_start(zones: &[String], now: Timestamp) -> Result<Timestamp, QuietHoursError> {
    let mut starts = Vec::new();
    let names = std::iter::once(ARIZONA).chain(zones.iter().map(String::as_str));
    for name in names {
        let today = now.in_tz(name)?.date();
        for offset in 0..=SEARCH_DAYS {
            let day = today.checked_add(jiff::Span::new().days(offset))?;
            starts.push(opens_on(name, day)?);
        }
    }
    starts.sort();
    for start in starts.into_iter().filter(|s| *s > now) {
        if allowed_at(zones, start)? {
            return Ok(start);
        }
    }
    Err(QuietHoursError::Zone)
}

/// QH-004, D-101: a direct reply is fresh when 0 <= age < 15 minutes since the person's own text.
fn is_fresh_reply(inbound_at: Timestamp, now: Timestamp) -> bool {
    let age = now.duration_since(inbound_at);
    age >= SignedDuration::ZERO && age < SignedDuration::from_mins(REPLY_MINUTES)
}

/// Whether QH-004 lets this message go outside the window.
fn night_exception(msg: &Outgoing<'_>, now: Timestamp) -> bool {
    match msg.kind {
        Kind::Safety911 | Kind::KeywordReply | Kind::PickCode | Kind::AfterHoursNotice => true,
        Kind::DirectReply { inbound_at } => is_fresh_reply(inbound_at, now),
        Kind::Quote | Kind::QuoteCount | Kind::JobQuestion | Kind::PickStep => msg.emergency_job,
        Kind::JobCard | Kind::ConfirmRequest => {
            msg.emergency_job
                && msg.channel == Channel::Email
                && msg.after_hours_business
                && msg.emergency_emails
        }
        Kind::Update | Kind::Other => false,
    }
}

/// Decides whether `msg` may go at `now` (QH-001 to QH-006). Applies to homeowners and businesses and
/// to texts and emails alike (QH-005); the night exceptions are exactly QH-004's. A held message is
/// told when to try again: the start of the next window in both zones.
///
/// # Errors
///
/// [`QuietHoursError::NotNorthAmerican`] or [`QuietHoursError::UnknownAreaCode`]: such a number is
/// never texted, whatever the kind (QH-003, D-100, D-139).
pub fn decide(
    table: &AreaCodes,
    msg: &Outgoing<'_>,
    now: Timestamp,
) -> Result<Decision, QuietHoursError> {
    let zones = table.zones_for(msg.phone)?;
    if allowed_at(zones, now)? || night_exception(msg, now) {
        return Ok(Decision::Send);
    }
    Ok(Decision::Hold {
        until: next_window_start(zones, now)?,
    })
}

/// Decides a retry of a failed send: the same rule as the first send, judged at the retry instant
/// (QH-008). A retried direct reply stays judged against the person's own inbound time.
///
/// # Errors
///
/// As [`decide`].
pub fn retry(
    table: &AreaCodes,
    msg: &Outgoing<'_>,
    now: Timestamp,
) -> Result<Decision, QuietHoursError> {
    decide(table, msg, now)
}

/// A message that quiet hours held, as the sender remembers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// The sender's own id for the held message.
    pub id: u64,
    /// The job it is about.
    pub job_id: u64,
    /// The person it goes to.
    pub recipient_id: u64,
    /// What it is; only [`Kind::Update`] and [`Kind::QuoteCount`] merge.
    pub kind: Kind,
    /// True for a text that announces the close itself (JOB-012, INT-019).
    pub announces_close: bool,
}

/// What to do with held messages at send time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    /// Send this message on its own.
    Send(u64),
    /// Send one merged text standing for all these held updates, oldest first (TXT-025).
    Merged(Vec<u64>),
    /// Drop: the job closed meanwhile (QH-007).
    Drop(u64),
}

/// Plans the release of held messages at the start of a window. `job_closed` is the sending module's
/// own check (`messaging` cannot read `jobs`): a held message for a closed job is dropped unless it
/// announces the close (QH-007, D-461); the rest of the held updates for one job and recipient merge
/// into one text (TXT-025, D-158). Order follows each group's first message.
pub fn plan_release(held: &[Held], job_closed: impl Fn(u64) -> bool) -> Vec<Release> {
    let mut out: Vec<Slot> = Vec::new();
    let mut groups: HashMap<(u64, u64), usize> = HashMap::new();
    for h in held {
        let mergeable = matches!(h.kind, Kind::Update | Kind::QuoteCount) && !h.announces_close;
        if job_closed(h.job_id) && !h.announces_close {
            out.push(Slot::Done(Release::Drop(h.id)));
        } else if mergeable {
            let slot = *groups
                .entry((h.job_id, h.recipient_id))
                .or_insert(out.len());
            match out.get_mut(slot) {
                Some(Slot::Group(ids)) => ids.push(h.id),
                _ => out.push(Slot::Group(vec![h.id])),
            }
        } else {
            out.push(Slot::Done(Release::Send(h.id)));
        }
    }
    out.into_iter().map(Slot::finish).collect()
}

/// A release being built: either settled, or a group of mergeable updates still collecting ids.
enum Slot {
    Done(Release),
    Group(Vec<u64>),
}

impl Slot {
    /// A group of one is sent alone; a larger group becomes one merged text (TXT-025).
    fn finish(self) -> Release {
        match self {
            Self::Done(release) => release,
            Self::Group(ids) if ids.len() == 1 => Release::Send(ids[0]),
            Self::Group(ids) => Release::Merged(ids),
        }
    }
}
