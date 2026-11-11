//! A random stored PIN per line and the stored PIN-try counts behind the inbound voice webhook
//! (CALL-002, CALL-003, CALL-004, CALL-005, CALL-025, SEC-017, FEE-022; D-1684, D-1685, D-1755,
//! D-1836, D-2091 to D-2095).
//!
//! [`allocate_pin`] draws a line's four digits from the OS random source inside `assign_number`'s
//! transaction, under the pool lock, and retries a PIN already used by an open line of the same
//! pool number at most [`PIN_DRAWS`] times; the partial unique index on (number, PIN) of open
//! lines backs it. The line ID never gives the PIN: [`stored_pin`] reads the stored column.
//!
//! [`voice_turn`] answers one turn of an incoming call. Wrong tries are counted in
//! `calls.pin_try` per calling number (an HMAC-SHA256 digest under `ID_SECRET` with the label
//! [`PIN_TRY_LABEL`], never the number, PRIV-002, PRIV-024), pool number and Arizona day, by one
//! atomic upsert that never passes [`TRIES_PER_DAY`], so concurrent wrong tries cannot pass 6 a
//! day. Each try of a call is recorded by (CallSid, try number) with the count it was judged on,
//! so a repeated Twilio callback answers the same step and counts nothing (SEC-017). On a connect
//! the homeowner's phone is read for the bridge and the call's start is stored once per parent
//! CallSid in `calls.call_start` (D-1836).
//!
//! The line a read-back names is bound to its try: `calls.pin_call.readback_line` is written by
//! the insert that records the try, Twilio's resend of that turn reads the stored line back, never
//! a newly chosen one, and [`voice_confirm`] connects only that line, only while it is still open on
//! the call's pool number and still the caller's business's (CALL-005, CALL-002, D-2201).

use askhava_adapters::api::PoolNumber;
use askhava_privacy::api::{
    Field, Purpose, Readback, Reader, Vault, read_contact, read_for_readback,
};
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::{TimeZone, offset};
use rand::TryRng;
use rand::rngs::SysRng;
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::handlers::dial_twiml;
use crate::models::{self, CallsError, TryKey};
use crate::routing::{
    Answer, Attestation, CallTurn, IncomingCall, JobPin, Party, READBACK_AFTER, RoutedLine, Step,
    TRIES_PER_DAY, answer_for, lines_on, party_of, route_with,
};

/// How many PINs one assignment draws before it gives up (CALL-003, D-1755).
pub const PIN_DRAWS: u8 = 8;
/// The domain label of the calling-number digest in `calls.pin_try` (PRIV-024).
pub const PIN_TRY_LABEL: &str = "pin-try|v1";

/// Two-byte values drawn at most for one number; each is rejected with odds 5,536 in 65,536, so
/// all 64 rejected (about 2e-69) is never the reason a draw fails (D-2451).
const NUMBER_TRIES: u8 = 64;
/// The largest multiple of 10,000 two bytes hold; values from it up are drawn again (D-2451).
const UNBIASED_BELOW: u16 = 60_000;

/// The first unused PIN of at most [`PIN_DRAWS`] numbers from `source`; a failed or invalid draw
/// costs that one draw, not the rest; `None` when no draw gave a free PIN (CALL-003, CALL-017,
/// D-1755, D-2451).
pub(crate) fn draw_pin(used: &[JobPin], mut source: impl FnMut() -> Option<u16>) -> Option<JobPin> {
    (0..PIN_DRAWS)
        .filter_map(|_| source().and_then(JobPin::from_number))
        .find(|pin| !used.contains(pin))
}

/// One number below 10,000 from the OS random source, without modulo bias (D-2451).
fn os_number() -> Option<u16> {
    number_from(|bytes| SysRng.try_fill_bytes(bytes).ok())
}

/// One number below 10,000 from two-byte values `fill` writes, drawing again while a value is at
/// least [`UNBIASED_BELOW`]; `None` only when `fill` fails (or, never in practice, after
/// [`NUMBER_TRIES`] rejections) (CALL-003, D-2451).
fn number_from(mut fill: impl FnMut(&mut [u8; 2]) -> Option<()>) -> Option<u16> {
    for _ in 0..NUMBER_TRIES {
        let mut bytes = [0_u8; 2];
        fill(&mut bytes)?;
        let n = u16::from_be_bytes(bytes);
        if n < UNBIASED_BELOW {
            return Some(n % 10_000);
        }
    }
    None
}

/// A PIN no open line on `number` holds, drawn under the pool lock in `tx` (CALL-003, D-1755).
pub(crate) async fn allocate_pin(
    tx: &mut PgConnection,
    number: &str,
) -> Result<Option<JobPin>, CallsError> {
    let used: Vec<JobPin> = models::used_pins(tx, number)
        .await?
        .iter()
        .filter_map(|p| JobPin::parse(p))
        .collect();
    Ok(draw_pin(&used, os_number))
}

/// The stored PIN of line `line`, open or closed; `None` for a line assigned before T262
/// (CALL-003, CALL-011, D-1755). Never derived from the ID.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn stored_pin(db: &mut PgConnection, line: Uuid) -> Result<Option<JobPin>, CallsError> {
    Ok(models::line_pin(db, line)
        .await?
        .as_deref()
        .and_then(JobPin::parse))
}

/// The Arizona calendar day of `at` (UTC-7 all year), the day CALL-025's count runs over.
#[must_use]
pub fn arizona_day(at: Timestamp) -> Date {
    at.to_zoned(TimeZone::fixed(offset(-7))).date()
}

/// The digest of a calling number: HMAC-SHA256 under `ID_SECRET` over the version, the label
/// [`PIN_TRY_LABEL`] and the E.164 number, every field length-prefixed as `IdempotencyKey::derive`
/// and `core`'s ID derivation do. A number that is not E.164 (withheld, anonymous) is digested as
/// the one word `anonymous`, so all such callers share one count (PRIV-002, PRIV-024, CALL-025).
#[must_use]
pub fn caller_digest(secret: &SecretString, version: u16, caller: &SecretString) -> [u8; 32] {
    let number = caller.expose_secret().trim();
    let e164 = number
        .strip_prefix('+')
        .is_some_and(|d| (8..=15).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()));
    let normal = if e164 { number } else { "anonymous" };
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.expose_secret().as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
    mac.update(&version.to_be_bytes());
    for part in [PIN_TRY_LABEL.as_bytes(), normal.as_bytes()] {
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// The wrong PIN tries counted today (Arizona day of `now`) from `caller` on `number`
/// (CALL-025): what the next turn is judged on. `keys` gives the `ID_SECRET` and its version.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn pin_tries_today(
    db: &mut PgConnection,
    keys: &VoiceKeys<'_>,
    (caller, number): (&SecretString, &str),
    now: Timestamp,
) -> Result<u8, CallsError> {
    let digest = caller_digest(keys.id_secret, keys.version, caller);
    let key = TryKey {
        digest: &digest,
        version: i16::try_from(keys.version).unwrap_or(i16::MAX),
        number,
        day: arizona_day(now),
    };
    Ok(count(models::tries_today(db, &key).await?))
}

/// The stored start of the call with parent CallSid `parent`: its line and when the webhook
/// answered with the dial (FEE-022, D-1836); T271 and T282 read it.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn call_start(
    db: &mut PgConnection,
    parent: &str,
) -> Result<Option<(Uuid, Timestamp)>, CallsError> {
    models::call_start(db, parent).await
}

/// One webhook turn of an incoming call to a pool number (CALL-003, CALL-025, SEC-017).
pub struct VoiceCall<'a> {
    /// The pool number called and the caller's number.
    pub call: IncomingCall<'a>,
    /// Twilio's `CallSid` of this (parent) call.
    pub call_sid: &'a str,
    /// The carrier's STIR/SHAKEN attestation.
    pub attestation: Attestation,
    /// Which try of the call this is: 0 before any digits, else 1 to 3.
    pub try_no: u8,
    /// The digits entered this turn, if any.
    pub entered: Option<JobPin>,
}

/// What the webhook answers (CALL-002 to CALL-005, CALL-025).
pub enum VoiceReply {
    /// Bridge with this TwiML (holds her number: answer it, never log it); the start is stored.
    Dial(SecretString),
    /// Ask for the PIN (again).
    AskPin,
    /// Read these words back; connect only on the caller's confirm (CALL-005).
    ReadBack(Readback),
    /// Hang up: too many tries, refused for the day, or nothing to dial.
    HangUp,
}

/// The keys `voice_turn` needs: the `ID_SECRET` and its version, the masked pool.
pub struct VoiceKeys<'a> {
    /// `ID_SECRET`, the key of the calling-number digest.
    pub id_secret: &'a SecretString,
    /// Its version, stored with each count so a rotation re-derives (PRIV-024).
    pub version: u16,
    /// The masked pool, whose numbers the dial shows (CALL-002, CALL-021).
    pub pool: &'a [PoolNumber],
}

/// Answers one turn of `v` at `now` (CALL-002 to CALL-005, CALL-025, SEC-017, D-1755): routes on
/// the stored PINs with the stored counts, counts a wrong try once per (CallSid, try), and on a
/// connect dials the homeowner from the pool number and stores the call's start (D-1836). Call it
/// outside a transaction (the vault refuses a read inside one).
///
/// # Errors
///
/// [`CallsError::Database`], [`CallsError::Messaging`] or [`CallsError::Ops`].
pub async fn voice_turn(
    db: &mut PgConnection,
    vault: &Vault,
    keys: &VoiceKeys<'_>,
    v: &VoiceCall<'_>,
    now: Timestamp,
) -> Result<VoiceReply, CallsError> {
    let party = party_of(db, vault, v.call.caller).await?;
    let judged = judged_count(db, keys, (party, v), now).await?;
    if let Some(line) = judged.bound {
        return read_back_bound(db, vault, (party, v.call.pool_number), line).await;
    }
    let turn = CallTurn {
        attestation: v.attestation,
        tries_call: v.try_no.saturating_sub(1),
        tries_today: judged.before,
        entered: v.entered,
    };
    let answer = answer_for(db, vault, (party, v.call.pool_number), turn, judged.fresh).await?;
    reply(db, vault, keys, (party, v), answer, now).await
}

/// How one turn was judged (SEC-017, CALL-025, CALL-005, D-2201).
struct Judged {
    /// The day's wrong-try count the turn is judged on.
    before: u8,
    /// Whether this is a new turn (alerts only then), not a repeated callback.
    fresh: bool,
    /// The line this try read back, as stored with the try (`calls.pin_call.readback_line`).
    bound: Option<Uuid>,
}

/// The day's count this turn is judged on, whether it is a new turn, and the line its read-back
/// named: the recorded count and line for a repeated (CallSid, try); else today's count, and for a
/// wrong PIN the count before this try after counting it once, with the read-back line chosen
/// before the row is written stored in that same row (SEC-017, CALL-025, CALL-005, D-2201).
async fn judged_count(
    db: &mut PgConnection,
    keys: &VoiceKeys<'_>,
    (party, v): (Party, &VoiceCall<'_>),
    now: Timestamp,
) -> Result<Judged, CallsError> {
    let digest = caller_digest(keys.id_secret, keys.version, v.call.caller);
    let key = TryKey {
        digest: &digest,
        version: i16::try_from(keys.version).unwrap_or(i16::MAX),
        number: v.call.pool_number,
        day: arizona_day(now),
    };
    let id = (v.call_sid, i16::from(v.try_no));
    if v.entered.is_some()
        && let Some(before) = models::call_try(db, id).await?
    {
        let bound = readback_line(db, id).await?;
        return Ok(judged(before, false, bound));
    }
    let today = count(models::tries_today(db, &key).await?);
    let fresh = Judged {
        before: today,
        fresh: true,
        bound: None,
    };
    let Some(_) = v.entered else {
        return Ok(fresh);
    };
    let turn = CallTurn {
        attestation: v.attestation,
        tries_call: v.try_no.saturating_sub(1),
        tries_today: today,
        entered: v.entered,
    };
    let step = step_for(db, (party, v.call.pool_number), turn).await?;
    let line = match step {
        Step::Connect(_) | Step::Refuse | Step::AskPin => return Ok(fresh),
        Step::ReadBack(line) => Some(line),
        Step::WrongPin | Step::HangUp => None,
    };
    count_once(db, &key, id, line).await
}

/// A [`Judged`] from a stored count.
fn judged(before: i16, fresh: bool, bound: Option<Uuid>) -> Judged {
    Judged {
        before: count(before),
        fresh,
        bound,
    }
}

/// Counts one wrong try and records it for (CallSid, try) in one transaction, with `line` when the
/// try reads that line back and the count still allows the read-back (the confirm's own test,
/// CALL-005, D-2201); a try already recorded rolls the count back and returns the recorded count
/// and line (SEC-017).
async fn count_once(
    db: &mut PgConnection,
    key: &TryKey<'_>,
    id: (&str, i16),
    line: Option<Uuid>,
) -> Result<Judged, CallsError> {
    let cap = i16::from(TRIES_PER_DAY);
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    let after = models::count_try(&mut tx, key, cap).await?;
    let before = after.map_or(cap, |n| n - 1);
    let line = line.filter(|_| after.is_some_and(|n| n < cap));
    let recorded = match line {
        Some(l) => record_readback_try(&mut tx, id, before, l).await?,
        None => models::record_call_try(&mut tx, id, before).await?,
    };
    if recorded {
        tx.commit().await.map_err(|_| CallsError::Database)?;
        return Ok(judged(before, true, line));
    }
    tx.rollback().await.map_err(|_| CallsError::Database)?;
    let stored = models::call_try(db, id).await?.unwrap_or(cap);
    let bound = readback_line(db, id).await?;
    Ok(judged(stored, false, bound))
}

/// Records read-back try `try_no` of call `sid`, judged on `before`, with the line read back, in
/// the try's own insert (no `UPDATE` is granted); `false` when that try was already recorded
/// (SEC-017, CALL-005, D-2201).
async fn record_readback_try(
    tx: &mut PgConnection,
    (sid, try_no): (&str, i16),
    before: i16,
    line: Uuid,
) -> Result<bool, CallsError> {
    let row = sqlx::query_scalar!(
        "INSERT INTO calls.pin_call (call_sid, try_no, today_before, readback_line) \
         VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING RETURNING today_before",
        sid,
        try_no,
        before,
        line
    )
    .fetch_optional(tx)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(row.is_some())
}

/// The line read back at try `try_no` of call `sid`; `None` when that try read nothing back or
/// was never recorded (CALL-005, D-2201).
async fn readback_line(
    db: &mut PgConnection,
    (sid, try_no): (&str, i16),
) -> Result<Option<Uuid>, CallsError> {
    let row = sqlx::query_scalar!(
        "SELECT readback_line FROM calls.pin_call WHERE call_sid = $1 AND try_no = $2",
        sid,
        try_no
    )
    .fetch_optional(db)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(row.flatten())
}

/// Line `line` when it is still open on `number` and still belongs to `business`; else `None`
/// (CALL-005, CALL-002, D-2201).
async fn still_bound(
    db: &mut PgConnection,
    (business, number): (Uuid, &str),
    line: Uuid,
) -> Result<Option<RoutedLine>, CallsError> {
    let lines = lines_on(db, number).await?;
    Ok(lines
        .into_iter()
        .map(|(l, _)| l)
        .find(|l| l.line == line && l.business == business))
}

/// The read-back of the bound `line` (a new turn or Twilio's resend of it): its homeowner's first
/// name and street while the line is still open on `number` and the caller's business's; else the
/// PIN prompt, never another line's words (CALL-005, D-026, D-2201).
async fn read_back_bound(
    db: &mut PgConnection,
    vault: &Vault,
    (party, number): (Party, &str),
    line: Uuid,
) -> Result<VoiceReply, CallsError> {
    let Party::Business(business) = party else {
        return Ok(VoiceReply::HangUp);
    };
    let Some(routed) = still_bound(db, (business, number), line).await? else {
        return Ok(VoiceReply::AskPin);
    };
    let words = read_for_readback(db, vault, routed.homeowner)
        .await
        .map_err(|_| CallsError::Database)?;
    Ok(VoiceReply::ReadBack(words))
}

/// A stored count as the rule's `u8`, never above the day's limit.
fn count(n: i16) -> u8 {
    u8::try_from(n.clamp(0, i16::from(TRIES_PER_DAY))).unwrap_or(TRIES_PER_DAY)
}

/// The routing step for `turn` on the stored PINs, reading nothing from the vault.
async fn step_for(
    db: &mut PgConnection,
    (party, number): (Party, &str),
    turn: CallTurn,
) -> Result<Step, CallsError> {
    let stored = lines_on(db, number).await?;
    let lines: Vec<_> = stored.iter().map(|(l, _)| *l).collect();
    let pin_of = |l: &crate::routing::RoutedLine| stored.iter().find(|(s, _)| s.line == l.line)?.1;
    Ok(route_with(party, &lines, turn, pin_of))
}

/// The webhook's reply for `answer`; a connect dials (CALL-002, D-1836). A read-back reaches here
/// only when no line was bound to the try (the pool changed between the step and the answer), so
/// nothing is read back and the caller is asked for the PIN (CALL-005, D-2201).
async fn reply(
    db: &mut PgConnection,
    vault: &Vault,
    keys: &VoiceKeys<'_>,
    (party, v): (Party, &VoiceCall<'_>),
    answer: Answer,
    now: Timestamp,
) -> Result<VoiceReply, CallsError> {
    Ok(match answer {
        Answer::ReadBack(..) => VoiceReply::AskPin,
        Answer::Step(Step::Connect(line)) => {
            connect(db, vault, keys, (party, v), line, now).await?
        }
        Answer::Step(Step::AskPin | Step::WrongPin) => VoiceReply::AskPin,
        Answer::Step(Step::HangUp | Step::Refuse | Step::ReadBack(_)) => VoiceReply::HangUp,
    })
}

/// Confirms the read-back given at try `v.try_no`: the line stored with that try (the one read
/// back) is connected when `pressed`, only when that try was recorded as the second of an
/// A-attested call and the line is still open on the call's pool number and still the caller's
/// business's (CALL-005, CALL-002, D-026, D-2201). A line closed or moved since, or a try that read
/// nothing back, never connects another line: the caller is asked for the PIN again or the call
/// ends. Nothing is counted.
///
/// # Errors
///
/// As [`voice_turn`].
pub async fn voice_confirm(
    db: &mut PgConnection,
    vault: &Vault,
    keys: &VoiceKeys<'_>,
    v: &VoiceCall<'_>,
    (pressed, now): (bool, Timestamp),
) -> Result<VoiceReply, CallsError> {
    let id = (v.call_sid, i16::from(v.try_no));
    let Some(before) = models::call_try(db, id).await? else {
        return Ok(VoiceReply::HangUp);
    };
    let party = party_of(db, vault, v.call.caller).await?;
    let Party::Business(business) = party else {
        return Ok(VoiceReply::HangUp);
    };
    let given = v.attestation == Attestation::A
        && v.try_no == READBACK_AFTER
        && before + 1 < i16::from(TRIES_PER_DAY);
    let Some(line) = readback_line(db, id).await?.filter(|_| given) else {
        return Ok(VoiceReply::HangUp);
    };
    match still_bound(db, (business, v.call.pool_number), line).await? {
        Some(l) if pressed => connect(db, vault, keys, (party, v), l.line, now).await,
        _ => Ok(VoiceReply::AskPin),
    }
}

/// Dials the homeowner of `line` from its pool number and stores the call's start once per
/// parent CallSid (CALL-002, FEE-022, D-1836). The homeowner herself calling is not dialled: no
/// business phone number is stored where `calls` can read it (D-2093).
async fn connect(
    db: &mut PgConnection,
    vault: &Vault,
    keys: &VoiceKeys<'_>,
    (party, v): (Party, &VoiceCall<'_>),
    line: Uuid,
    now: Timestamp,
) -> Result<VoiceReply, CallsError> {
    if matches!(party, Party::Homeowner(_)) {
        return Ok(VoiceReply::HangUp);
    }
    let lines = lines_on(db, v.call.pool_number).await?;
    let Some((routed, _)) = lines.iter().find(|(l, _)| l.line == line) else {
        return Ok(VoiceReply::HangUp);
    };
    let from = keys.pool.iter().find(|n| n.as_str() == v.call.pool_number);
    let Some(from) = from else {
        return Ok(VoiceReply::HangUp);
    };
    let access = (Reader::Calls, Purpose::BridgeCall);
    let her = read_contact(db, vault, access, routed.homeowner, &[Field::Phone])
        .await
        .map_err(|_| CallsError::Database)?;
    let Some(twiml) = her.phone.as_ref().and_then(|p| dial_twiml(from, p)) else {
        return Ok(VoiceReply::HangUp);
    };
    models::insert_call_start(db, v.call_sid, line, now).await?;
    Ok(VoiceReply::Dial(twiml))
}

#[cfg(test)]
mod tests {
    //! TD-CALL-13's forced-collision cases (CALL-003, D-1755) on the pure draw, and TD-CALL-20's
    //! failed-draw and rejection cases (CALL-003, CALL-017, D-2451).
    use super::{PIN_DRAWS, draw_pin, number_from};
    use crate::routing::JobPin;

    fn pin(n: u16) -> JobPin {
        JobPin::from_number(n).expect("pin")
    }

    /// TD-CALL-13: a forced collision is drawn again, and the next free draw is taken.
    #[test]
    fn td_call_13_a_forced_collision_retries() {
        let mut draws = [1234, 1234, 42].into_iter();
        assert_eq!(draw_pin(&[pin(1234)], || draws.next()), Some(pin(42)));
    }

    /// TD-CALL-13: every draw colliding gives up after the bounded tries; a failed source too.
    #[test]
    fn td_call_13_used_up_retries_give_none() {
        let mut calls = 0;
        let got = draw_pin(&[pin(7)], || {
            calls += 1;
            Some(7)
        });
        assert_eq!((got, calls), (None, u32::from(PIN_DRAWS)));
        assert_eq!(draw_pin(&[], || None), None);
        assert_eq!(draw_pin(&[], || Some(10_000)), None);
    }

    /// TD-CALL-20: a draw whose source fails costs that draw only; the next free PIN is taken.
    #[test]
    fn td_call_20_a_failed_draw_costs_one_draw() {
        let mut draws = [None, Some(42)].into_iter();
        assert_eq!(draw_pin(&[], || draws.next().flatten()), Some(pin(42)));
    }

    /// TD-CALL-20: values of 60,000 or more are drawn again; the first below is taken mod 10,000.
    /// Six rejected values come first, more than a 4-try cap would allow (D-2597).
    #[test]
    fn td_call_20_rejected_values_are_drawn_again() {
        let rejected = [65_535_u16, 60_000, 61_234, 63_000, 64_999, 60_001];
        let mut values = rejected.into_iter().chain([59_999]);
        let got = number_from(|bytes| {
            *bytes = values.next()?.to_be_bytes();
            Some(())
        });
        assert_eq!((got, values.next()), (Some(9_999), None));
    }

    /// TD-CALL-20: a failing byte source gives `None`, after one try.
    #[test]
    fn td_call_20_a_failing_byte_source_gives_none() {
        let mut calls = 0;
        let got = number_from(|_| {
            calls += 1;
            None
        });
        assert_eq!((got, calls), (None, 1));
    }

    /// TD-CALL-20: the rejection cap of 64 draws ends a draw that only ever gives rejected values;
    /// the literal 64 is pinned so a small cap cannot come back (D-2597).
    #[test]
    fn td_call_20_the_rejection_cap_ends_the_draw() {
        let mut calls = 0_u32;
        let got = number_from(|bytes| {
            calls += 1;
            *bytes = 60_000_u16.to_be_bytes();
            Some(())
        });
        assert_eq!((got, calls), (None, 64));
    }
}
