//! PIN routing on a masked line, the paid-business read-back, and the who's-calling text
//! (CALL-003, CALL-004, CALL-005, CALL-025, TXT-027, QH-004; D-026, D-480, D-491, D-1684 to
//! D-1688).
//!
//! [`route`] is the pure rule for one turn of an incoming call to a pool number. A caller whose
//! linked business, or whose own vault number, holds exactly one open line on that number is
//! connected; anyone else, and a caller with two or more lines on it, must enter that job's
//! 4-digit PIN ([`job_pin`], derived from the line's ID, D-1684), and a wrong PIN never connects
//! (CALL-003, CALL-004). At most 3 tries per call and 6 per calling number per pool number per day:
//! the third wrong try, or the sixth of the day, hangs up and raises one High [`PIN_ALERT`], and a
//! number that used its 6 is refused before the prompt (CALL-025). The day's count is the voice
//! handler's to keep and pass in (`CallTurn::tries_today`, D-1685). After the second wrong try, a
//! phone linked to the business holding a line on the number hears that line's first name and
//! street, but only on an A-attested call, and is connected only when it confirms (CALL-005,
//! D-026); lines open only at the release of a paid or $0 pick, so that business is the paid one
//! (D-1686). [`answer_call`] reads the caller, the lines and, for a read-back, the vault.
//!
//! [`send_whos_calling`] queues the TXT-027 text once the line is open: at once on an emergency job
//! (QH-004), otherwise when `messaging::api::send_window` allows it, else a [`WHOS_CALLING_KIND`]
//! task at 08:00 that [`run_whos_calling`] runs (D-1687).

use std::sync::Arc;

use askhava_core::api::{
    Claimed, Clock, HandlerError, NewTask, SystemClock, enqueue as enqueue_task,
};
use askhava_messaging::api::{
    Language, NewText, OutboxError, Relation, SendWindow, enqueue, render, send_window,
    sender_business,
};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use askhava_privacy::api::{Readback, Vault, find_by_phone, read_for_readback};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::CallsError;

/// PIN tries one call may make (CALL-025).
pub const TRIES_PER_CALL: u8 = 3;
/// PIN tries one calling number may make on one pool number in a day (CALL-025).
pub const TRIES_PER_DAY: u8 = 6;
/// Wrong tries after which a linked, A-attested caller hears the read-back (CALL-005).
pub const READBACK_AFTER: u8 = 2;
/// The High alert raised when a call is hung up for too many PIN tries (CALL-025).
pub const PIN_ALERT: &str = "calls.pin_tries_exceeded";
/// The who's-calling text (TXT-027).
pub const WHOS_CALLING_TEMPLATE: &str = "whos_calling";
/// The who's-calling text when she chose email contact (TXT-027, PICK-018).
pub const WHOS_CALLING_EMAIL_TEMPLATE: &str = "whos_calling_email";
/// The task that sends a held who's-calling text at the window's start (QH-006, D-1687).
pub const WHOS_CALLING_KIND: &str = "calls.whos_calling";

/// The STIR/SHAKEN attestation the carrier gave the call (CALL-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attestation {
    /// Full attestation: the carrier knows the caller and the number.
    A,
    /// Partial, gateway or missing: PIN routing only, never a read-back.
    Other,
}

impl Attestation {
    /// Reads the carrier's attestation value (`A`, `B`, `C`, or none); only `A` is [`Self::A`].
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("A") => Self::A,
            _ => Self::Other,
        }
    }
}

/// A job's 4-digit PIN. `Debug` hides the digits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JobPin(u16);

impl std::fmt::Debug for JobPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JobPin(****)")
    }
}

impl JobPin {
    /// Exactly four ASCII digits, else `None` (CALL-003).
    #[must_use]
    pub fn parse(digits: &str) -> Option<Self> {
        let ok = digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit());
        ok.then(|| digits.parse().ok().map(Self)).flatten()
    }

    /// The four digits, for the business portal and the release (CALL-003).
    #[must_use]
    pub fn digits(self) -> String {
        format!("{:04}", self.0)
    }
}

/// The PIN of the line with ID `line`: its ID's value modulo 10,000 (D-1684).
#[must_use]
pub fn job_pin(line: Uuid) -> JobPin {
    JobPin(u16::try_from(line.as_u128() % 10_000).unwrap_or_default())
}

/// Who is calling, as the sender directory and the vault know the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    /// A main or staff phone linked to this business.
    Business(Uuid),
    /// The homeowner with this vault contact ID.
    Homeowner(Uuid),
    /// A number neither knows.
    Unlinked,
}

/// One open line on the called pool number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedLine {
    /// The line's `calls.number_pool` ID.
    pub line: Uuid,
    /// The homeowner's vault contact ID.
    pub homeowner: Uuid,
    /// The connected business.
    pub business: Uuid,
}

/// One turn of a call, as the voice handler tracks it (CALL-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallTurn {
    /// The carrier's attestation.
    pub attestation: Attestation,
    /// Wrong tries already made on this call.
    pub tries_call: u8,
    /// Tries already made today from this number on this pool number (D-1685).
    pub tries_today: u8,
    /// The PIN entered this turn, if any.
    pub entered: Option<JobPin>,
}

/// What the line does next (CALL-003 to CALL-005, CALL-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Bridge to this line.
    Connect(Uuid),
    /// Ask for the 4-digit PIN.
    AskPin,
    /// Wrong PIN: say so and ask again.
    WrongPin,
    /// Read back this line's first name and street; connect only on the caller's confirm.
    ReadBack(Uuid),
    /// Too many tries: hang up and raise one alert.
    HangUp,
    /// The number used its tries today: refused before any prompt.
    Refuse,
}

/// The lines `party` may reach without a PIN: its own when it has any, else none.
fn own_lines(party: Party, lines: &[RoutedLine]) -> Vec<RoutedLine> {
    let mine = |l: &&RoutedLine| match party {
        Party::Business(b) => l.business == b,
        Party::Homeowner(h) => l.homeowner == h,
        Party::Unlinked => false,
    };
    lines.iter().filter(mine).copied().collect()
}

/// The pure routing rule for one turn (CALL-003, CALL-004, CALL-005, CALL-025, D-026); `lines`
/// in assignment order. See the module docs.
#[must_use]
pub fn route(party: Party, lines: &[RoutedLine], turn: CallTurn) -> Step {
    let mine = own_lines(party, lines);
    if let [only] = mine.as_slice() {
        return Step::Connect(only.line);
    }
    if turn.tries_today >= TRIES_PER_DAY {
        return Step::Refuse;
    }
    let Some(pin) = turn.entered else {
        return Step::AskPin;
    };
    let reach = if mine.is_empty() { lines } else { &mine };
    let hits: Vec<_> = reach.iter().filter(|l| job_pin(l.line) == pin).collect();
    if let [hit] = hits.as_slice() {
        return Step::Connect(hit.line);
    }
    let (call, day) = (turn.tries_call + 1, turn.tries_today + 1);
    if call >= TRIES_PER_CALL || day >= TRIES_PER_DAY {
        return Step::HangUp;
    }
    let paid = matches!(party, Party::Business(_))
        .then(|| mine.last())
        .flatten();
    match paid {
        Some(line) if call == READBACK_AFTER && turn.attestation == Attestation::A => {
            Step::ReadBack(line.line)
        }
        _ => Step::WrongPin,
    }
}

/// An incoming call: the pool number called and the caller's number (never logged).
pub struct IncomingCall<'a> {
    /// The pool number called.
    pub pool_number: &'a str,
    /// The caller's number.
    pub caller: &'a SecretString,
}

/// What [`answer_call`] decided, with the read-back words when it reads back.
#[derive(Debug)]
pub enum Answer {
    /// Any step but a read-back.
    Step(Step),
    /// Read these words back for this line; connect only on the caller's confirm (CALL-005).
    ReadBack(Uuid, Readback),
}

/// Answers one turn of `call` (CALL-003 to CALL-005, CALL-025): finds the caller, the open lines
/// on the number, applies [`route`], raises one High [`PIN_ALERT`] on a hang-up and reads the
/// first name and street from the vault for a read-back. Call it outside a transaction.
///
/// # Errors
///
/// [`CallsError::Messaging`] or [`CallsError::Database`] when a lookup or the vault fails,
/// [`CallsError::Ops`] when the alert cannot be written.
pub async fn answer_call(
    db: &mut PgConnection,
    vault: &Vault,
    call: &IncomingCall<'_>,
    turn: CallTurn,
) -> Result<Answer, CallsError> {
    let party = party_of(db, vault, call.caller).await?;
    let lines = lines_on(db, call.pool_number).await?;
    match route(party, &lines, turn) {
        Step::HangUp => {
            let subject = lines.first().map(|l| ("calls.number_pool", l.line));
            let alert = NewAlert {
                kind: PIN_ALERT,
                severity: Severity::High,
                subject,
            };
            raise_alert(db, &alert).await.map_err(|_| CallsError::Ops)?;
            Ok(Answer::Step(Step::HangUp))
        }
        Step::ReadBack(id) => {
            let line = lines.iter().find(|l| l.line == id);
            let homeowner = line.ok_or(CallsError::Database)?.homeowner;
            let words = read_for_readback(db, vault, homeowner)
                .await
                .map_err(|_| CallsError::Database)?;
            Ok(Answer::ReadBack(id, words))
        }
        step => Ok(Answer::Step(step)),
    }
}

/// The caller's party: a linked business phone first, then a vault homeowner, else unlinked.
async fn party_of(
    db: &mut PgConnection,
    vault: &Vault,
    caller: &SecretString,
) -> Result<Party, CallsError> {
    let phone = caller.expose_secret();
    match sender_business(db, vault, phone).await {
        Ok(Some(business)) => return Ok(Party::Business(business)),
        Ok(None) | Err(OutboxError::BadPhone) => {}
        Err(_) => return Err(CallsError::Messaging),
    }
    Ok(match find_by_phone(db, vault, phone).await {
        Ok(Some(contact)) => Party::Homeowner(contact),
        Ok(None) | Err(askhava_privacy::api::VaultError::BadPhone) => Party::Unlinked,
        Err(_) => return Err(CallsError::Database),
    })
}

/// The open lines on `number`, in assignment order.
async fn lines_on(db: &mut PgConnection, number: &str) -> Result<Vec<RoutedLine>, CallsError> {
    let rows = sqlx::query!(
        "SELECT id, homeowner_id, business_id FROM calls.number_pool \
         WHERE pool_number = $1 AND closed_at IS NULL ORDER BY seq",
        number
    )
    .fetch_all(db)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| RoutedLine {
            line: r.id,
            homeowner: r.homeowner_id,
            business: r.business_id,
        })
        .collect())
}

/// The who's-calling text of one open line (TXT-027). `business` is the business's public name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhosCalling {
    /// The line's masked number.
    pub number: String,
    /// The job.
    pub job: Uuid,
    /// The request number she knows.
    pub job_number: i64,
    /// Her vault contact ID.
    pub contact: Uuid,
    /// The picked business's name.
    pub business: String,
    /// She chose emergency (QH-004).
    pub emergency: bool,
    /// She chose email contact at the pick (PICK-018).
    pub email_chosen: bool,
    /// Her language, and whether Spanish conversations are on (TXT-039).
    pub language: (Language, bool),
}

/// What [`send_whos_calling`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhosCallingSent {
    /// Queued in the outbox (or already there).
    Queued,
    /// Held to this instant by a [`WHOS_CALLING_KIND`] task (QH-006).
    Held(Timestamp),
    /// Opted out or not textable: no text (CON-013, QH-003).
    Skipped,
}

/// Queues the who's-calling text (TXT-027, QH-004, D-1687): at once on an emergency job, else
/// inside the window `messaging::api::send_window` gives, else held to its start. Always a text,
/// whatever her update channel; the email-contact wording when she chose email (PICK-018). One
/// outbox row per template, contact and line. Call it outside a transaction.
///
/// # Errors
///
/// [`CallsError::Messaging`] or [`CallsError::Database`].
pub async fn send_whos_calling(
    db: &mut PgConnection,
    (vault, outbox): (&Vault, &SecretString),
    text: &WhosCalling,
    now: Timestamp,
) -> Result<WhosCallingSent, CallsError> {
    if !text.emergency {
        let window = send_window(db, vault, text.contact, now).await;
        match window.map_err(|_| CallsError::Messaging)? {
            SendWindow::Now => {}
            SendWindow::Hold { until } => return hold(db, text, until).await,
            SendWindow::OptedOut | SendWindow::NotTextable => return Ok(WhosCallingSent::Skipped),
        }
    }
    let id = if text.email_chosen {
        WHOS_CALLING_EMAIL_TEMPLATE
    } else {
        WHOS_CALLING_TEMPLATE
    };
    let job = text.job_number.to_string();
    let values = [
        ("business", text.business.as_str()),
        ("job", &job),
        ("number", &text.number),
    ];
    let body =
        render(id, text.language.0, text.language.1, &values).map_err(|_| CallsError::Messaging)?;
    let new = NewText {
        template_id: id,
        subject: &[text.job],
        recipient: text.contact,
        relation: Relation::OpenRequest,
        body: &body.body,
    };
    enqueue(db, outbox, &new)
        .await
        .map_err(|_| CallsError::Messaging)?;
    Ok(WhosCallingSent::Queued)
}

/// Queues the held text as a task due at `until`, once per job and instant (QH-006).
async fn hold(
    db: &mut PgConnection,
    text: &WhosCalling,
    until: Timestamp,
) -> Result<WhosCallingSent, CallsError> {
    let spanish = text.language.0 == Language::Es;
    let payload = json!({"number": text.number, "job": text.job, "job_number": text.job_number,
        "contact": text.contact, "business": text.business, "email_chosen": text.email_chosen,
        "spanish": spanish, "spanish_on": text.language.1});
    let mut task = NewTask::new(WHOS_CALLING_KIND, payload, until);
    let at = until.as_microsecond();
    task.dedupe_key = Some(format!("{WHOS_CALLING_KIND}.{}.{at}", text.job));
    enqueue_task(db, &task)
        .await
        .map_err(|_| CallsError::Database)?;
    Ok(WhosCallingSent::Held(until))
}

/// The held text from a [`WHOS_CALLING_KIND`] payload, or `None` when malformed.
#[must_use]
pub fn parse_whos_calling(payload: &Value) -> Option<WhosCalling> {
    let id = |k: &str| payload.get(k)?.as_str()?.parse::<Uuid>().ok();
    let flag = |k: &str| payload.get(k)?.as_bool();
    let language = if flag("spanish")? {
        Language::Es
    } else {
        Language::En
    };
    Some(WhosCalling {
        number: payload.get("number")?.as_str()?.to_owned(),
        job: id("job")?,
        job_number: payload.get("job_number")?.as_i64()?,
        contact: id("contact")?,
        business: payload.get("business")?.as_str()?.to_owned(),
        emergency: false,
        email_chosen: flag("email_chosen")?,
        language: (language, flag("spanish_on")?),
    })
}

/// The [`WHOS_CALLING_KIND`] handler: sends the held text at the window's start (QH-006); `keys`
/// are the vault and the outbox key secret.
///
/// # Errors
///
/// A fixed sentence for a malformed payload or a failed step; the queue tries again.
pub async fn run_whos_calling(
    pool: PgPool,
    keys: Arc<(Vault, SecretString)>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let text = parse_whos_calling(&task.payload).ok_or(HandlerError("bad who's-calling task"))?;
    let mut db = pool
        .acquire()
        .await
        .map_err(|_| HandlerError("no connection"))?;
    send_whos_calling(&mut db, (&keys.0, &keys.1), &text, SystemClock.now())
        .await
        .map(|_| ())
        .map_err(|_| HandlerError("who's-calling text failed"))
}
