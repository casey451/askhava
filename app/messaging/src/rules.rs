//! Quiet hours (QH-001 to QH-009, TXT-025): when a text or a business email may go; and the
//! opt-out rules (CON-008 to CON-011, CON-013, CON-030): the word and phrase lists, the detector
//! and which texts may still go to a number on the suppression list.
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

/// The sole opt-out words the texting provider's Advanced Opt-Out also handles, in the order of
/// `stand_ins/provider_optout.json`, which must hold exactly these (CON-008, CON-030, D-493).
pub const PROVIDER_OPT_OUT_WORDS: [&str; 12] = [
    "stop",
    "stopall",
    "unsubscribe",
    "cancel",
    "end",
    "revoke",
    "optout",
    "quit",
    "parar",
    "alto",
    "cancelar",
    "baja",
];
/// The Spanish ones among them; the provider answers these with the Spanish CON-009 wording.
pub const SPANISH_OPT_OUT_WORDS: [&str; 4] = ["parar", "alto", "cancelar", "baja"];
/// A sole opt-out word the provider does not handle, so the app sends its confirmation: "opt out"
/// with a space or hyphen is an FCC word but not a provider keyword (CON-008, CON-030, D-477).
pub const APP_OPT_OUT_WORDS: [&str; 1] = ["opt out"];
/// The only re-subscribe words, the provider's and the app's alike; YES is not one (CON-015, CON-030).
pub const RESUBSCRIBE_WORDS: [&str; 2] = ["start", "unstop"];
/// The revocation phrases, each with the pattern ID stored as how an opt-out was detected (CON-010,
/// CON-036). None also fits a normal request ("can stop by at 3", "please cancel my request").
pub const OPT_OUT_PHRASES: [(&str, &str); 14] = [
    ("en.stop_texting", "stop texting"),
    ("en.stop_messaging", "stop messaging"),
    ("en.stop_sending", "stop sending"),
    ("en.dont_text_me", "don't text me"),
    ("en.do_not_text_me", "do not text me"),
    ("en.no_more_texts", "no more texts"),
    ("en.remove_me", "remove me"),
    ("en.take_me_off", "take me off"),
    ("en.unsubscribe_me", "unsubscribe me"),
    ("es.no_me_escriban", "no me escriban"),
    ("es.no_me_manden_mensajes", "no me manden mensajes"),
    ("es.ya_no_quiero_mensajes", "ya no quiero mensajes"),
    ("es.dejen_de_escribirme", "dejen de escribirme"),
    ("es.quitenme_de_la_lista", "quítenme de la lista"),
];
/// The template of the one opt-out confirmation (CON-009).
pub const CONFIRMATION_TEMPLATE: &str = "opt_out_confirmation";
/// The template of the HELP reply, which still goes to an opted-out number (CON-013, CON-018, D-131).
pub const HELP_TEMPLATE: &str = "help_reply";
/// How long after an opt-out its one confirmation may still go; never later (CON-009, CON-030).
pub const CONFIRM_WITHIN: SignedDuration = SignedDuration::from_mins(5);

/// Which Advanced Opt-Out keyword the provider matched, from the inbound webhook's `OptOutType`
/// (CON-030): the provider has already blocked or unblocked the number and sent its own reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKeyword {
    /// No keyword matched (no `OptOutType`, or `HELP`, which is T030's).
    None,
    /// `OptOutType=STOP`: the provider sent the CON-009 reply.
    OptOut,
    /// `OptOutType=START`: the provider sent the START reply.
    Start,
}

impl ProviderKeyword {
    /// Reads the webhook's `OptOutType` value, if any.
    #[must_use]
    pub fn from_opt_out_type(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_uppercase).as_deref() {
            Some("STOP") => Self::OptOut,
            Some("START") => Self::Start,
            _ => Self::None,
        }
    }
}

/// How the app's detector matched an opt-out (CON-008, CON-010, CON-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    /// The whole message is this sole opt-out word.
    Word(&'static str),
    /// The message contains the revocation phrase with this pattern ID.
    Phrase(&'static str),
}

impl Detected {
    /// Whether it was a Spanish word or phrase, so its confirmation is in Spanish (CON-009).
    #[must_use]
    pub fn spanish(self) -> bool {
        match self {
            Self::Word(word) => SPANISH_OPT_OUT_WORDS.contains(&word),
            Self::Phrase(id) => id.starts_with("es."),
        }
    }

    /// What is stored as how it was detected: `word:<word>` or `phrase:<pattern ID>` (CON-036).
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Word(word) => format!("word:{word}"),
            Self::Phrase(id) => format!("phrase:{id}"),
        }
    }
}

/// What one inbound message means for the suppression list (CON-008, CON-010, CON-015, CON-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classified {
    /// An opt-out. `detected` is `None` only when the provider matched a word the app does not
    /// list; `by_provider` means the provider already sent the confirmation, so the app sends none.
    OptOut {
        /// The app's own match, if any.
        detected: Option<Detected>,
        /// The provider matched it and replied (CON-030).
        by_provider: bool,
    },
    /// A sole START or UNSTOP (CON-015).
    Resubscribe,
    /// Anything else, "yes" included (CON-011, CON-030).
    Other,
}

/// The app's opt-out detector, the same for texts, help emails, email replies and form messages
/// (CON-035): a message that is only an opt-out word in any case, spacing and punctuation
/// (CON-008), or contains a listed revocation phrase (CON-010); a word used in another sense ("can
/// stop by at 3") is not one (CON-011).
#[must_use]
pub fn detect_opt_out(text: &str) -> Option<Detected> {
    let folded = fold(text);
    if let Some(word) = PROVIDER_OPT_OUT_WORDS
        .iter()
        .chain(&APP_OPT_OUT_WORDS)
        .find(|w| **w == folded)
    {
        return Some(Detected::Word(word));
    }
    let padded = format!(" {folded} ");
    OPT_OUT_PHRASES
        .iter()
        .find(|(_, phrase)| padded.contains(&format!(" {} ", fold(phrase))))
        .map(|(id, _)| Detected::Phrase(id))
}

/// Classifies an inbound text from what the provider matched and the app's own detector (CON-008,
/// CON-010, CON-015, CON-030): a sole START or UNSTOP re-subscribes; nothing else does.
#[must_use]
pub fn classify(text: &str, provider: ProviderKeyword) -> Classified {
    let folded = fold(text);
    if provider == ProviderKeyword::Start || RESUBSCRIBE_WORDS.contains(&folded.as_str()) {
        return Classified::Resubscribe;
    }
    let detected = detect_opt_out(text);
    let by_provider = provider == ProviderKeyword::OptOut;
    if detected.is_some() || by_provider {
        return Classified::OptOut {
            detected,
            by_provider,
        };
    }
    Classified::Other
}

/// Whether text `template_id` may go at `now` to a number on the suppression list since `since`
/// (`None`: not on it). Only the one confirmation, within [`CONFIRM_WITHIN`] of the opt-out, and the
/// HELP reply go to a suppressed number (CON-009, CON-013, CON-030, CON-035, D-131).
#[must_use]
pub fn may_send_to(since: Option<Timestamp>, template_id: &str, now: Timestamp) -> bool {
    let Some(since) = since else {
        return true;
    };
    template_id == HELP_TEMPLATE
        || (template_id == CONFIRMATION_TEMPLATE
            && now >= since
            && now.duration_since(since) <= CONFIRM_WITHIN)
}

/// Lower-cases, folds Spanish accents, drops apostrophes, turns every other non-letter, non-digit
/// into a space and collapses spaces, so "Don't TEXT me!!" reads "dont text me" (CON-010).
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            '\'' | '\u{2018}' | '\u{2019}' => continue,
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
