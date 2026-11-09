//! Giving a connected job its masked number, and the list check before an automated call
//! (CALL-001, CALL-002, CALL-014, CALL-017, CALL-021, CON-035; D-077, D-160, D-231).
//!
//! [`assign_number`] takes a number only from the [`MaskedPool`] settings hold: the app never buys
//! one, and the voice adapter dials out only from a [`PoolNumber`] (CALL-017, CALL-021). Under one
//! advisory lock it prefers a number no other open line of the same homeowner or business uses,
//! the least used first; with none free it shares the least-crowded one (calls then route by PIN,
//! CALL-004) and raises the one number-pool alert unless one is open (D-231, D-1680). Each side
//! is shown only the pool number, never the other's (CALL-002). [`automated_call_gate`] answers
//! whether an automated call may dial a homeowner: never after an opt-out (CON-035, CALL-014).

use askhava_adapters::api::{MaskedPool, PoolNumber};
use askhava_messaging::api::{SendWindow, send_window};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, CallsError, OpenLine};
use crate::pins::allocate_pin;

/// The `ops.alert` kind raised when a job had to share a number (CALL-017, D-077).
pub const POOL_ALERT: &str = "calls.pool_exhausted";
/// The audit action of a number given to a job (JOB-005).
pub const ASSIGNED_ACTION: &str = "calls.number_assigned";
/// The `ops.alert` kind raised when a safety call cannot dial her number (CALL-019, D-1682).
pub const SAFETY_NOT_DIALABLE_ALERT: &str = "calls.safety_call_not_dialable";
/// The alert subject kind of a homeowner's vault contact.
const CONTACT_KIND: &str = "privacy.contact";
/// The audit and alert subject kind of a line.
const LINE_KIND: &str = "calls.number_pool";

/// A job the business connected on: the IDs the pool rule weighs (CALL-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Connected {
    /// The job.
    pub job: Uuid,
    /// The job's homeowner record.
    pub homeowner: Uuid,
    /// The connected business.
    pub business: Uuid,
}

/// A job's masked line: the pool number both sides see, and whether it was shared when assigned
/// (CALL-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    id: Uuid,
    number: PoolNumber,
    shared: bool,
}

impl Line {
    /// The line's `calls.number_pool` row ID.
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The masked number, the only number either side sees (CALL-002).
    #[must_use]
    pub fn number(&self) -> &PoolNumber {
        &self.number
    }

    /// True when, at assignment, another open line of the same homeowner or business already used
    /// the number (CALL-017). Stored once and never updated: a line assigned unshared becomes
    /// shared in fact when a later line takes its number, so PIN routing (CALL-004, T077) must
    /// derive sharing from the open lines on the number, never from this flag (D-1680).
    #[must_use]
    pub fn shared(&self) -> bool {
        self.shared
    }
}

/// The number index `open` leaves best for a line of `homeowner` and `business`, and whether it is
/// shared (CALL-017): fewest open lines of the same homeowner or business first, then fewest open
/// lines at all, then settings order. `None` only for an empty pool, which settings refuse.
pub(crate) fn choose(
    pool: &[PoolNumber],
    open: &[OpenLine],
    (homeowner, business): (Uuid, Uuid),
) -> Option<(usize, bool)> {
    let weigh = |number: &PoolNumber| {
        let on = open.iter().filter(|line| line.number == number.as_str());
        let mine = on
            .clone()
            .filter(|line| line.homeowner == homeowner || line.business == business)
            .count();
        (mine, on.count())
    };
    pool.iter()
        .enumerate()
        .min_by_key(|(index, number)| (weigh(number), *index))
        .map(|(index, number)| (index, weigh(number).0 > 0))
}

/// Gives `job` its masked number at `now`, or returns its open line again (safe to call twice).
/// Writes the line, its audit row and, for a shared number with no pool alert open, one High
/// `calls.pool_exhausted` alert, in one transaction (a savepoint inside the caller's)
/// (CALL-001, CALL-002, CALL-017, JOB-005; D-077, D-231). The line's PIN is drawn at random in the
/// same transaction, unique among the number's open lines; when [`crate::pins::PIN_DRAWS`] draws
/// all collide, the pool alert is raised and no line is stored (CALL-003, D-1755).
///
/// # Errors
///
/// [`CallsError::LineOpen`] when the job's open line is another business's or homeowner's,
/// [`CallsError::PoolEmpty`] when the pool holds no number (settings refuse one),
/// [`CallsError::NumberRetired`] when its open line's number left the pool,
/// [`CallsError::NoFreePin`] when no free PIN was drawn,
/// [`CallsError::Ops`] or [`CallsError::Database`].
pub async fn assign_number(
    db: &mut PgConnection,
    pool: &MaskedPool,
    job: Connected,
    now: Timestamp,
) -> Result<Line, CallsError> {
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    models::lock_pool(&mut tx).await?;
    if let Some(line) = models::open_line_of(&mut tx, job.job).await? {
        if line.business != job.business || line.homeowner != job.homeowner {
            return Err(CallsError::LineOpen);
        }
        let number = in_pool(pool, &line.number).ok_or(CallsError::NumberRetired)?;
        return Ok(Line {
            id: line.id,
            number,
            shared: line.shared,
        });
    }
    let open = models::open_lines(&mut tx).await?;
    let numbers = pool.numbers();
    let (index, shared) =
        choose(numbers, &open, (job.homeowner, job.business)).ok_or(CallsError::PoolEmpty)?;
    let number = numbers.get(index).cloned().ok_or(CallsError::PoolEmpty)?;
    let Some(pin) = allocate_pin(&mut tx, number.as_str()).await? else {
        return refuse_line(tx).await;
    };
    let ids = (job.job, job.homeowner, job.business);
    let pin = pin.digits();
    let stored = models::insert_line(&mut tx, ids, (number.as_str(), shared, &pin), now).await?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: ASSIGNED_ACTION,
        subject_kind: LINE_KIND,
        subject_id: stored.id,
        change: Change::Action,
    };
    audit(&mut tx, &entry).await.map_err(|_| CallsError::Ops)?;
    if shared && !models::alert_open(&mut tx).await? {
        alert_pool(&mut tx, stored, now).await?;
    }
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(Line {
        id: stored.id,
        number,
        shared,
    })
}

/// No free PIN on the chosen number: raises the pool alert (no line to name), commits it and
/// refuses the line (CALL-003, CALL-017, D-1755).
async fn refuse_line(mut tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<Line, CallsError> {
    let alert = NewAlert {
        kind: POOL_ALERT,
        severity: Severity::High,
        subject: None,
    };
    raise_alert(&mut tx, &alert)
        .await
        .map_err(|_| CallsError::Ops)?;
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Err(CallsError::NoFreePin)
}

/// The pool's own number equal to `stored`, if it is still in the pool.
fn in_pool(pool: &MaskedPool, stored: &str) -> Option<PoolNumber> {
    pool.numbers()
        .iter()
        .find(|number| number.as_str() == stored)
        .cloned()
}

/// Raises the number-pool alert about the line `stored` and records it (CALL-017, D-077, D-231).
async fn alert_pool(
    tx: &mut PgConnection,
    stored: models::Stored,
    now: Timestamp,
) -> Result<(), CallsError> {
    let alert = NewAlert {
        kind: POOL_ALERT,
        severity: Severity::High,
        subject: Some((LINE_KIND, stored.id)),
    };
    let id = raise_alert(tx, &alert).await.map_err(|_| CallsError::Ops)?;
    models::insert_alert(tx, id, stored.seq, now).await
}

/// An automated call to a homeowner, as opposed to a masked call she or the business places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomatedCall {
    /// The safety voice call (CALL-019, CALL-022).
    Safety,
    /// The voice code call (CALL-014); `asked_on_screen` when she just tapped for it on that
    /// screen, the one case an opted-out number may get it.
    Code {
        /// She asked for this call on the screen she is on.
        asked_on_screen: bool,
    },
}

/// Whether an automated call may dial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallGate {
    /// The call may go.
    Dial,
    /// The number is on the suppression list: no call (CON-035).
    OptedOut,
    /// `messaging` would not text the number: not a `+1` number, or its area code is not in the
    /// quiet-hours table (QH-003). No call; a safety call answered this raises
    /// [`SAFETY_NOT_DIALABLE_ALERT`] (D-1681, D-1682).
    NotDialable,
}

/// Whether `call` may dial the homeowner with vault contact `contact` at `now`: the one
/// suppression list is checked first, through `messaging`, whatever the call, except a code call
/// she just asked for on screen (CON-035, CALL-014, CALL-022; D-1681). The check reuses
/// `messaging`'s text-send window, so the vault audits the phone read as a `Send` read, and a
/// number `messaging` would not text answers [`CallGate::NotDialable`]; for a safety call that
/// also raises one High [`SAFETY_NOT_DIALABLE_ALERT`] about the contact, so it is never silent
/// (D-1682). Call it outside a transaction: the vault refuses a read inside one.
///
/// # Errors
///
/// [`CallsError::Messaging`] when the phone or the list cannot be read, [`CallsError::Ops`] when
/// the alert cannot be written.
pub async fn automated_call_gate(
    db: &mut PgConnection,
    vault: &Vault,
    (contact, call): (Uuid, AutomatedCall),
    now: Timestamp,
) -> Result<CallGate, CallsError> {
    if let AutomatedCall::Code {
        asked_on_screen: true,
    } = call
    {
        return Ok(CallGate::Dial);
    }
    let window = send_window(db, vault, contact, now)
        .await
        .map_err(|_| CallsError::Messaging)?;
    let gate = match window {
        SendWindow::OptedOut => CallGate::OptedOut,
        SendWindow::NotTextable => CallGate::NotDialable,
        SendWindow::Now | SendWindow::Hold { .. } => CallGate::Dial,
    };
    if gate == CallGate::NotDialable && call == AutomatedCall::Safety {
        let alert = NewAlert {
            kind: SAFETY_NOT_DIALABLE_ALERT,
            severity: Severity::High,
            subject: Some((CONTACT_KIND, contact)),
        };
        raise_alert(db, &alert).await.map_err(|_| CallsError::Ops)?;
    }
    Ok(gate)
}
