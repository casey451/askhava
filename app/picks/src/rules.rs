//! The pure pick rules: the pick code's clock, tries and cooldown, the Verify send caps, and the
//! reading and plan of a "<number> YES" text pick (PICK-013, PICK-014, PICK-015, PICK-016,
//! PICK-031, PICK-032, PICK-036, PICK-055, CON-029, D-015, D-068, D-147).
//!
//! Nothing here reads a clock or a database: the service passes the stored rows and `now`.

use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;

/// A pick code is good for 10 minutes: valid at 9:59, refused at exactly 10:00 (PICK-013, D-101).
pub const CODE_VALID: SignedDuration = SignedDuration::from_mins(10);
/// Tries per code before the cooldown (PICK-013).
pub const CODE_TRIES: i32 = 5;
/// The cooldown after the fifth wrong try (PICK-013, D-015).
pub const COOLDOWN: SignedDuration = SignedDuration::from_mins(15);
/// Verify sends per destination number per Arizona day (PICK-055).
pub const SENDS_PER_NUMBER: i64 = 5;
/// Verify voice calls per destination number per Arizona day (PICK-055).
pub const CALLS_PER_NUMBER: i64 = 3;
/// Verify sends per job per Arizona day (PICK-055).
pub const SENDS_PER_JOB: i64 = 10;
/// No voice code from 9 p.m. (PICK-055).
pub const CALL_QUIET_FROM: i8 = 21;
/// ... until 8 a.m. local time (PICK-055).
pub const CALL_QUIET_UNTIL: i8 = 8;

/// A typed or pasted 6-digit code; spaces and dashes from a paste are dropped (PICK-014). Its
/// `Debug` hides the digits.
pub struct PickCode(String);

impl PickCode {
    /// The six digits, for the Verify check only.
    #[must_use]
    pub fn digits(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PickCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PickCode(******)")
    }
}

/// Reads a typed, pasted or autofilled code: exactly six ASCII digits once spaces and dashes are
/// removed (PICK-013, PICK-014).
#[must_use]
pub fn parse_code(input: &str) -> Option<PickCode> {
    let digits: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    let six = digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit());
    six.then_some(PickCode(digits))
}

/// One sent code as stored: when it was sent, the tries used, the last try and whether it passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRow {
    /// When the code was sent.
    pub sent_at: Timestamp,
    /// Tries used so far, the passing one included.
    pub tries: i32,
    /// When the latest try was made.
    pub last_try_at: Option<Timestamp>,
    /// Whether a try passed (a code is used once).
    pub verified: bool,
}

/// Where a sent code stands at a given instant (PICK-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeState {
    /// It may be tried; `left` tries remain.
    Open {
        /// Tries remaining.
        left: i32,
    },
    /// Ten minutes have passed, or the cooldown is over: a new code is needed.
    Expired,
    /// Five tries failed; no try and no new code until `until`.
    CoolingDown {
        /// The cooldown's end.
        until: Timestamp,
    },
    /// It already passed.
    Used,
}

/// The state of `row` at `now` (PICK-013, D-015, D-101): five tries used start a 15-minute cooldown
/// from the last try; otherwise the code is open strictly before 10 minutes after it was sent.
#[must_use]
pub fn code_state(row: &CodeRow, now: Timestamp) -> CodeState {
    if row.verified {
        return CodeState::Used;
    }
    if row.tries >= CODE_TRIES {
        let until = row.last_try_at.unwrap_or(row.sent_at) + COOLDOWN;
        return if now < until {
            CodeState::CoolingDown { until }
        } else {
            CodeState::Expired
        };
    }
    if now >= row.sent_at + CODE_VALID {
        return CodeState::Expired;
    }
    CodeState::Open {
        left: CODE_TRIES - row.tries,
    }
}

/// How a code is to reach the homeowner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeVia {
    /// A text, the default.
    Text,
    /// A voice call: only when she taps "Call me with the code instead", never by itself (PICK-013,
    /// D-366).
    CallTapped,
}

/// Today's Verify sends, counted from the start of the Arizona day (PICK-055).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SendCounts {
    /// Codes sent to this destination number, any channel.
    pub to_number: i64,
    /// Voice codes sent to this destination number.
    pub calls_to_number: i64,
    /// Codes sent for this job.
    pub for_job: i64,
    /// Codes sent by Ask Hava in all.
    pub global: i64,
}

/// Why a code was not sent (PICK-013, PICK-055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendRefused {
    /// The last code's five tries failed; wait for the cooldown's end.
    CoolingDown(Timestamp),
    /// 5 codes to this number today.
    NumberCap,
    /// 3 voice codes to this number today.
    CallCap,
    /// 10 codes for this job today.
    JobCap,
    /// A voice code between 9 p.m. and 8 a.m. from outside the phone's own session.
    CallQuietHours,
    /// The global daily count is above the threshold: sends stop and a High alert goes.
    GlobalCap,
}

/// One send request: the channel, whether it came from that phone's own session and the local hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendAsk {
    /// Text, or a call she tapped for.
    pub via: CodeVia,
    /// Whether the request came from the destination phone's own session.
    pub own_session: bool,
    /// The hour (0 to 23) at the destination, Arizona time (D-1244).
    pub local_hour: i8,
}

/// Whether a Verify code may go (PICK-055, D-480): the global threshold first, then the job, the
/// number and the voice caps, then the 9 p.m. to 8 a.m. voice rule. `threshold` is the configured
/// global daily count; at or above it nothing more goes.
///
/// # Errors
///
/// The [`SendRefused`] reason.
pub fn send_allowed(ask: SendAsk, n: SendCounts, threshold: i64) -> Result<(), SendRefused> {
    let call = ask.via == CodeVia::CallTapped;
    let quiet = !(CALL_QUIET_UNTIL..CALL_QUIET_FROM).contains(&ask.local_hour);
    if n.global >= threshold {
        Err(SendRefused::GlobalCap)
    } else if n.for_job >= SENDS_PER_JOB {
        Err(SendRefused::JobCap)
    } else if n.to_number >= SENDS_PER_NUMBER {
        Err(SendRefused::NumberCap)
    } else if call && n.calls_to_number >= CALLS_PER_NUMBER {
        Err(SendRefused::CallCap)
    } else if call && quiet && !ask.own_session {
        Err(SendRefused::CallQuietHours)
    } else {
        Ok(())
    }
}

/// A "<number> YES" text: the quote number and, when two numbers lead, the request number first
/// ("1042 2 YES"), the answer D-068's which-request text asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPick {
    /// The request number, when named.
    pub job: Option<i64>,
    /// The quote number on the page and in the LIST text (1-based).
    pub quote: usize,
}

/// Reads "<number> YES", "<number> SÍ" or "<number> SI" in any case, with any words after ("2 yes
/// please"), or "<request> <number> YES" (PICK-015, D-068, D-290). Anything else is not a pick.
#[must_use]
pub fn read_text_pick(words: &str) -> Option<TextPick> {
    let lower = words.to_lowercase();
    let mut tokens = lower
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| c.is_ascii_punctuation()));
    let first: u64 = tokens.next()?.parse().ok()?;
    let second = tokens.next()?;
    let (job, quote, yes) = match second.parse::<u64>() {
        Ok(quote) => (Some(first), quote, tokens.next()?),
        Err(_) => (None, first, second),
    };
    if !matches!(yes, "yes" | "sí" | "si") || quote == 0 {
        return None;
    }
    let job = match job {
        Some(j) => Some(i64::try_from(j).ok()?),
        None => None,
    };
    Some(TextPick {
        job,
        quote: usize::try_from(quote).ok()?,
    })
}

/// Where a job stands for a text pick, from its status (`status::stage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Intake, Open or No quotes yet: no quote to pick yet.
    Collecting,
    /// Quotes ready: waiting for a pick.
    Ready,
    /// Awaiting confirm: a pick waits for the business's Confirm (PICK-032).
    Waiting,
    /// Payment pending or Connected: the business tapped Confirm.
    Accepted,
    /// A final status.
    Final,
}

/// One of the homeowner's jobs as the text-pick plan sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    /// The job.
    pub job: Uuid,
    /// Its request number.
    pub number: i64,
    /// Its stage.
    pub stage: Stage,
    /// Paused by the homeowner (CON-029): no new pick.
    pub paused: bool,
    /// Whether a text ending with the Terms line was sent for it (PICK-016).
    pub terms_sent: bool,
    /// The quotes on its page in arrival order: `true` where still pickable.
    pub quotes: Vec<bool>,
    /// The quote number of the pick waiting for Confirm, if any.
    pub waiting: Option<usize>,
}

/// What a text pick does (PICK-015, PICK-016, PICK-031, PICK-032, PICK-036, CON-029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// More than one job takes a pick: ask which, listing their numbers (D-068).
    Which(Vec<i64>),
    /// The job is paused by the homeowner: no pick, no text (CON-029).
    Paused(Uuid),
    /// No Terms line was sent for the job: send it with "Reply <n> YES again" (PICK-016).
    Terms(Uuid),
    /// No pickable quote has that number.
    NoQuote(Uuid),
    /// A new pick of the quote.
    Pick(Uuid),
    /// A pick of another quote replaces the one waiting for Confirm (PICK-032).
    Replace(Uuid),
    /// The waiting pick is this quote already.
    Same(Uuid),
    /// Nothing waits for a pick: say where each open request stands (PICK-032, PICK-036).
    Status,
}

/// The text-pick rule (PICK-031, PICK-032, PICK-036, PICK-016, CON-029, D-068, D-147): a job takes a
/// pick in Quotes ready or Awaiting confirm. A job paused by the homeowner takes no new pick
/// (CON-029), so "exactly one job" counts only the jobs not paused: with more than one of them (or
/// a named number that is none of the open jobs) Ask Hava asks which, listing them; with none open
/// it gives the status reply; with only paused ones open the YES is refused as paused. With one
/// chosen job, a pause stops it, an unknown or unavailable quote is refused, a missing Terms line
/// is sent first, and otherwise it picks, replaces the waiting pick or repeats it.
#[must_use]
pub fn plan_text_pick(pick: TextPick, jobs: &[JobView]) -> Plan {
    let open: Vec<&JobView> = jobs
        .iter()
        .filter(|j| matches!(j.stage, Stage::Ready | Stage::Waiting))
        .collect();
    let live: Vec<&JobView> = open.iter().copied().filter(|j| !j.paused).collect();
    let chosen: Vec<&JobView> = match pick.job {
        Some(n) => open.iter().copied().filter(|j| j.number == n).collect(),
        None => live.clone(),
    };
    let [job] = chosen.as_slice() else {
        return match (live.as_slice(), open.first()) {
            ([], None) => Plan::Status,
            ([], Some(paused)) => Plan::Paused(paused.job),
            (many, _) => Plan::Which(many.iter().map(|j| j.number).collect()),
        };
    };
    let index = pick.quote.checked_sub(1);
    let pickable = index
        .and_then(|i| job.quotes.get(i))
        .copied()
        .unwrap_or(false);
    match (job.paused, pickable, job.terms_sent, job.waiting) {
        (true, ..) => Plan::Paused(job.job),
        (_, false, ..) => Plan::NoQuote(job.job),
        (_, _, false, _) => Plan::Terms(job.job),
        (.., Some(q)) if q == pick.quote => Plan::Same(job.job),
        (.., Some(_)) => Plan::Replace(job.job),
        (.., None) => Plan::Pick(job.job),
    }
}
