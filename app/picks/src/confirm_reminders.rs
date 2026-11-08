//! Confirm by email alone: the reminders, the window's extensions and the lapse (PICK-019,
//! PICK-033, PICK-037, PICK-043 to PICK-046, BIZ-033, BIZ-039; D-344, D-345, D-380, D-491).
//!
//! No call is ever placed to a business (BIZ-039): every pick notice and reminder is an email
//! (and a text where the business chose text). [`reminders`] lists a non-emergency pick's
//! reminders: one every [`WindowRules::reminder_minutes`] business minutes from the notice's send
//! while the window is open, the final one marked last, each with the Hold line's new end while a
//! HOLD is still allowed; a time that falls at 8 p.m. waits for 8 a.m. (quiet hours).
//! [`emergency_reminders`] is PICK-044's single one. [`claim_reminder`] sends each once and
//! [`extend_once`] stores a HOLD or the bounce grace once per pick
//! (`migrations/0144_picks_confirm_reminder.sql`). [`lapse_plan`] decides the homeowner's
//! PICK-046 text after a lapse, and [`owner_told`] whether the owner gets the BIZ-033 email.
//! The wording is built here from the brand file, never a literal (D-1971).

use askhava_core::api::{add_business_minutes, arizona_time, brand, is_business_hours};
use askhava_messaging::api::Language;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::confirm::{EmergencyPlan, WindowRules, dollars};
use crate::models::PicksError;

/// The most reminders one window lists, a bound on a misconfigured setting.
const MOST_REMINDERS: u32 = 64;

/// One reminder email of a pick waiting for Confirm (PICK-037, D-380).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reminder {
    /// When it goes; it is due from this second on, never before.
    pub at: Timestamp,
    /// Whether it is the "Last reminder".
    pub last: bool,
    /// The end a HOLD would give, for the Hold line; none once a HOLD was used or isn't allowed.
    pub hold_to: Option<Timestamp>,
}

impl Reminder {
    /// PICK-037: whether the reminder is due at `now` (11:00:00 is, 10:59:59 is not).
    #[must_use]
    pub fn due(&self, now: Timestamp) -> bool {
        now >= self.at
    }
}

/// PICK-019: whether a Confirm at `now` is inside a window ending at `due` (13:59:59 is, 14:00:00
/// is lapsed).
#[must_use]
pub fn window_open(now: Timestamp, due: Timestamp) -> bool {
    now < due
}

/// QH-001: `at`, or the next 8 a.m. when it falls outside 8 a.m. to 8 p.m. Arizona time.
fn quiet(at: Timestamp) -> Timestamp {
    match is_business_hours(at) {
        true => at,
        false => add_business_minutes(at, 0),
    }
}

/// PICK-037, PICK-043, D-345, D-380: the reminders of a non-emergency window that started at
/// `start` and now ends at `due` (after any HOLD or grace), every `reminder_minutes` business
/// minutes from `start`, strictly before `due`; the final one is last. When `hold_offered`
/// ([`crate::confirm::hold_allowed`]), each carries the end a HOLD would give.
#[must_use]
pub fn reminders(
    start: Timestamp,
    due: Timestamp,
    hold_offered: bool,
    rules: WindowRules,
) -> Vec<Reminder> {
    let hold_to = hold_offered.then(|| add_business_minutes(due, rules.hold_minutes));
    let every = rules.reminder_minutes;
    let mut out: Vec<Reminder> = (1..=MOST_REMINDERS)
        .take_while(|_| every > 0)
        .map(|k| quiet(add_business_minutes(start, k.saturating_mul(every))))
        .take_while(|at| *at < due)
        .map(|at| Reminder {
            at,
            last: false,
            hold_to,
        })
        .collect();
    if let Some(final_one) = out.last_mut() {
        final_one.last = true;
    }
    out
}

/// PICK-044: an emergency pick's one reminder (none when quiet hours push it past the end), with
/// no Hold line.
#[must_use]
pub fn emergency_reminders(plan: &EmergencyPlan) -> Vec<Reminder> {
    plan.reminder_at
        .map(|at| Reminder {
            at,
            last: true,
            hold_to: None,
        })
        .into_iter()
        .collect()
}

/// What extended a pick's Confirm window (PICK-019, PICK-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extension {
    /// A HOLD by the link or the HOLD reply.
    Hold,
    /// The all-bounced grace.
    Bounce,
}

/// PICK-019, PICK-043: stores `kind` for `pick` with its new end, once per pick and kind; false
/// when it was stored before, so a second HOLD or grace changes nothing.
///
/// # Errors
/// [`PicksError::Database`].
pub async fn extend_once(
    db: &mut PgConnection,
    (pick, kind): (Uuid, Extension),
    new_due: Timestamp,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let kind = match kind {
        Extension::Hold => "hold",
        Extension::Bounce => "bounce",
    };
    let done = sqlx::query!(
        "INSERT INTO picks.window_extension (pick_id, kind, new_due, granted_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (pick_id, kind) DO NOTHING",
        pick,
        kind,
        new_due.to_sqlx() as _,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// PICK-037: claims `reminder` of `pick` for sending at `at`; false when it was claimed before,
/// so a rerun sends nothing twice.
///
/// # Errors
/// [`PicksError::Database`].
pub async fn claim_reminder(
    db: &mut PgConnection,
    pick: Uuid,
    reminder: Reminder,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let done = sqlx::query!(
        "INSERT INTO picks.confirm_reminder (pick_id, remind_at, last, claimed_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (pick_id, remind_at) DO NOTHING",
        pick,
        reminder.at.to_sqlx() as _,
        reminder.last,
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| PicksError::Database)?;
    Ok(done.rows_affected() == 1)
}

/// "3 p.m." or "3:30 p.m.", Arizona time; its final period ends a sentence too.
fn clock(at: Timestamp) -> String {
    let t = arizona_time(at);
    let (h, m) = (t.hour(), t.minute());
    let half = if h < 12 { "a.m." } else { "p.m." };
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    match m {
        0 => format!("{h12} {half}"),
        _ => format!("{h12}:{m:02} {half}"),
    }
}

/// The business minutes from `from` to `to` (0 when `to` is not later), by halving.
fn business_minutes(from: Timestamp, to: Timestamp) -> u32 {
    let (mut low, mut high) = (0_u32, 5 * 12 * 60);
    while low < high {
        let mid = (low + high) / 2;
        if add_business_minutes(from, mid) < to {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

/// "1 hour", "2 hours" or "30 minutes" (Spanish: "1 hora", "2 horas", "30 minutos").
fn time_left(minutes: u32, language: Language) -> String {
    let (hours, rest) = (minutes / 60, minutes % 60);
    match (rest, hours, language) {
        (0, 1, Language::En) => "1 hour".to_owned(),
        (0, 1, Language::Es) => "1 hora".to_owned(),
        (0, n, Language::En) => format!("{n} hours"),
        (0, n, Language::Es) => format!("{n} horas"),
        (_, _, Language::En) => format!("{minutes} minutes"),
        (_, _, Language::Es) => format!("{minutes} minutos"),
    }
}

fn short(language: Language) -> &'static str {
    match language {
        Language::En => brand().short_name(),
        Language::Es => brand().es().short_name(),
    }
}

/// PICK-037, D-380: the reminder's subject for job `job` ending at `due`, `left` business minutes
/// after it goes; then the Hold line while a HOLD is offered.
#[must_use]
pub fn reminder_lines(job: u32, due: Timestamp, r: Reminder, language: Language) -> Vec<String> {
    let (b, by) = (short(language), clock(due));
    let left = time_left(business_minutes(r.at, due), language);
    let subject = match (r.last, language) {
        (false, Language::En) => format!("{b}: Reminder: accept job {job} by {by}"),
        (false, Language::Es) => {
            format!("{b}: Recordatorio: acepte el trabajo {job} antes de las {by}")
        }
        (true, Language::En) => format!(
            "{b}: Last reminder: job {job} has {left} to accept. After {by} the homeowner can pick another quote."
        ),
        (true, Language::Es) => format!(
            "{b}: Último recordatorio: le queda {left} para aceptar el trabajo {job}. Después de las {by}, el cliente puede elegir otra cotización."
        ),
    };
    let hold = r.hold_to.map(|to| match language {
        Language::En => format!(
            "Need more time? Hold for 2 more hours: confirm by {}",
            clock(to)
        ),
        Language::Es => format!(
            "¿Necesita más tiempo? Pida 2 horas más: confirme antes de las {}",
            clock(to)
        ),
    });
    std::iter::once(subject).chain(hold).collect()
}

/// The homeowner's text after a pick lapsed (PICK-046).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LapsePlan {
    /// When it goes: at once on an emergency job (QH-004), else under quiet hours.
    pub send_at: Timestamp,
    /// The quote numbers she can still pick; the lapsed business's own never (PICK-035).
    pub others: Vec<u32>,
}

/// PICK-046, PICK-035, QH-004, D-345: the lapse at `lapsed` of `business`'s pick, with `quotes`
/// the job's pickable quotes as (number, business).
#[must_use]
pub fn lapse_plan(
    (lapsed, emergency): (Timestamp, bool),
    business: Uuid,
    quotes: &[(u32, Uuid)],
) -> LapsePlan {
    LapsePlan {
        send_at: if emergency { lapsed } else { quiet(lapsed) },
        others: quotes
            .iter()
            .filter(|(_, b)| *b != business)
            .map(|(n, _)| *n)
            .collect(),
    }
}

/// PICK-046, PICK-016, D-420: the lapse text naming `name`, with `lines` the TXT-031 lines of the
/// other quotes (none gives the "we'll text you" text) and `link` the quote page.
#[must_use]
pub fn lapse_text(name: &str, lines: &[String], link: &str, language: Language) -> String {
    let b = short(language);
    let list = lines.join("\n");
    match (lines.is_empty(), language) {
        (true, Language::En) => format!(
            "{b}: {name} couldn't take it this time. We'll text you when another quote comes in. If it can't wait, it's fine to call a company you know."
        ),
        (true, Language::Es) => format!(
            "{b}: {name} no pudo tomarlo esta vez. Le avisaremos por texto cuando llegue otra cotización. Si no puede esperar, puede llamar a una compañía que conozca."
        ),
        (false, Language::En) => format!(
            "{b}: {name} couldn't take it this time. You still have other quotes:\n{list}\nTo pick, reply the number and YES, for example 2 YES. Picking shares your first name and job address with that business once it accepts. Or see them at: {link}\nBy picking, you agree to the Terms: {}/terms",
            brand().domain()
        ),
        (false, Language::Es) => format!(
            "{b}: {name} no pudo tomarlo esta vez. Todavía tiene otras cotizaciones:\n{list}\nPara elegir, responda el número y SÍ, por ejemplo 2 SÍ. Al elegir, compartimos su nombre y la dirección del trabajo con esa compañía cuando acepte. O véalas en: {link}\nAl elegir, acepta los Términos: {}/terms",
            brand().domain()
        ),
    }
}

/// BIZ-033, D-258: whether the owner gets the "confirmed by a member" email: only when the phone
/// that confirmed is not the owner's own (phones are opaque IDs).
#[must_use]
pub fn owner_told(confirmed_by: Uuid, owner_phone: Option<Uuid>) -> bool {
    owner_phone != Some(confirmed_by)
}

/// BIZ-033: the owner's email line naming the job, the fee and the member who confirmed.
#[must_use]
pub fn member_confirmed_line(job: u32, fee_cents: i64, member: &str, language: Language) -> String {
    let (b, fee) = (short(language), dollars(fee_cents));
    match language {
        Language::En => format!(
            "{b}: {member} confirmed job {job} and paid the introduction fee by job size, {fee}. You can see the job in your portal."
        ),
        Language::Es => format!(
            "{b}: {member} confirmó el trabajo {job} y pagó la cuota de presentación según el tamaño del trabajo, {fee}. Puede ver el trabajo en su portal."
        ),
    }
}
