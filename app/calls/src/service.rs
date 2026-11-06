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

/// The `ops.alert` kind raised when a job had to share a number (CALL-017, D-077).
pub const POOL_ALERT: &str = "calls.pool_exhausted";
/// The audit action of a number given to a job (JOB-005).
pub const ASSIGNED_ACTION: &str = "calls.number_assigned";
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

/// A job's masked line: the pool number both sides see, and whether it is shared (CALL-002).
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

    /// True when another open line of the same homeowner or business uses the number, so calls
    /// on it route by PIN (CALL-004, CALL-017).
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
/// (CALL-001, CALL-002, CALL-017, JOB-005; D-077, D-231).
///
/// # Errors
///
/// [`CallsError::LineOpen`] when the job's open line is another business's,
/// [`CallsError::NumberRetired`] when its open line's number left the pool,
/// [`CallsError::Ops`] or [`CallsError::Database`].
pub async fn assign_number(
    db: &mut PgConnection,
    pool: &MaskedPool,
    job: Connected,
    now: Timestamp,
) -> Result<Line, CallsError> {
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    models::lock_pool(&mut tx).await?;
    if let Some((id, number, business, shared)) = models::open_line_of(&mut tx, job.job).await? {
        if business != job.business {
            return Err(CallsError::LineOpen);
        }
        let number = in_pool(pool, &number).ok_or(CallsError::NumberRetired)?;
        return Ok(Line { id, number, shared });
    }
    let open = models::open_lines(&mut tx).await?;
    let numbers = pool.numbers();
    let (index, shared) =
        choose(numbers, &open, (job.homeowner, job.business)).ok_or(CallsError::Database)?;
    let number = numbers.get(index).cloned().ok_or(CallsError::Database)?;
    let ids = (job.job, job.homeowner, job.business);
    let stored = models::insert_line(&mut tx, ids, (number.as_str(), shared), now).await?;
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
    /// The number cannot be read as a US number: no call.
    NotDialable,
}

/// Whether `call` may dial the homeowner with vault contact `contact` at `now`: the one
/// suppression list is checked first, through `messaging`, whatever the call, except a code call
/// she just asked for on screen (CON-035, CALL-014, CALL-022; D-1681). Call it outside a
/// transaction: the vault refuses a read inside one.
///
/// # Errors
///
/// [`CallsError::Messaging`] when the phone or the list cannot be read.
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
    Ok(match window {
        SendWindow::OptedOut => CallGate::OptedOut,
        SendWindow::NotTextable => CallGate::NotDialable,
        SendWindow::Now | SendWindow::Hold { .. } => CallGate::Dial,
    })
}
