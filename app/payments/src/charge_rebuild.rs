//! Rebuilding a charge lost in a restore from Stripe's record (OPS-016, FEE-028, FEE-030,
//! FEE-042; D-3753, D-3844).
//!
//! T110's replay lists Stripe's succeeded charges (`list_charge_page`) and hands each to
//! [`rebuild_charge`], which books one Stripe already took and the restored database lost through
//! the success path's own `book_success`, under the pick's `charge_key`: one evidence record, one
//! `charge` ledger entry, its receipt task. It never calls Stripe. A listed charge carries no
//! idempotency key or `pick-<id>-try<n>` label, so it is matched by its `metadata[pick_id]` and the
//! PaymentIntent stored for that pick in `payments.pick_intent`, and booked on the pick's newest
//! `payments.charge_attempt` (D-3844). The release and the attempt's status are not rerun here: the
//! attempt's own check task converges on the same record (FEE-019).

use askhava_adapters::api::ListedCharge;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::charge::{book_success, is_charge_id};
use crate::models::ChargeError;

/// What [`rebuild_charge`] did with one listed charge (D-3753, TD-OPS-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rebuilt {
    /// Booked now: its evidence record, `charge` entry and receipt task were written.
    Booked,
    /// The pick already holds its evidence record; nothing changed.
    AlreadyBooked,
    /// No held pick, PaymentIntent and attempt match it; booked nowhere. Its ID (`ch_…` or
    /// `py_…`) is for T110 to refund and flag.
    Unmatched(String),
    /// Stripe shows it refunded, fully or in part; booked nowhere. Its ID is for T110 to list.
    Refunded(String),
}

/// OPS-016, FEE-019, FEE-021, FEE-028, FEE-030, FEE-042, D-3753, D-3844, D-4292: books one
/// succeeded `charge` as Stripe lists it, once, at `at`, on its own transaction (a savepoint
/// inside a caller's), when its `metadata[pick_id]` names a pick `payments` holds, its
/// PaymentIntent is the one stored for that pick and the pick has a `charge_attempt` row. It never
/// calls Stripe. A refund (any amount) answers [`Rebuilt::Refunded`]; a pick with its evidence
/// record answers [`Rebuilt::AlreadyBooked`]; anything else [`Rebuilt::Unmatched`].
///
/// Locks, in `record`'s order (D-4292): the pick's newest `charge_attempt` row `FOR UPDATE`, then
/// its `pick_intent` row `FOR UPDATE`. `record` (the webhook and the 30-minute check) locks the
/// attempt row `FOR UPDATE` and then `pick_intent` `FOR SHARE` through `hold_pick`;
/// `confirm_and_pay`'s `begin_attempt`, `end_pick`, `end_intent_unless_charging` and
/// `late_success` lock `pick_intent` and no existing attempt row, so no two paths wait on each
/// other in opposite order. A newer attempt made while this call waited is seen under the
/// `pick_intent` lock and the call starts over on it.
///
/// Booked is decided without `xmin`: the evidence record is read absent in a statement after the
/// `pick_intent` lock is held, and every writer of `payments.charge_evidence` takes that lock
/// first (`charge::write_evidence`, reached only through `book_success`, called by `record`'s
/// effects after `hold_pick` and by this function), so `book_success` wrote it here.
///
/// # Errors
/// [`ChargeError::Database`]; nothing was written.
pub async fn rebuild_charge(
    conn: &mut PgConnection,
    charge: &ListedCharge,
    at: Timestamp,
) -> Result<Rebuilt, ChargeError> {
    if charge.amount_refunded > 0 {
        return Ok(Rebuilt::Refunded(charge.id.clone()));
    }
    let pick = charge
        .pick_id
        .as_deref()
        .and_then(|p| Uuid::parse_str(p).ok());
    let (Some(pick), Some(intent), true) = (pick, &charge.payment_intent, is_charge_id(&charge.id))
    else {
        return Ok(Rebuilt::Unmatched(charge.id.clone()));
    };
    loop {
        let mut tx = db(conn.begin().await)?;
        let try_no = match held_try(&mut tx, pick, intent).await? {
            Held::Try(try_no) => try_no,
            Held::Moved => {
                db(tx.rollback().await)?;
                continue;
            }
            Held::Answer(answer) => {
                return Ok(answer.unwrap_or(Rebuilt::Unmatched(charge.id.clone())));
            }
        };
        book_success(&mut tx, (pick, try_no), &charge.id, at).await?;
        db(tx.commit().await)?;
        return Ok(Rebuilt::Booked);
    }
}

/// What [`held_try`] found under the pick's locks.
enum Held {
    /// The newest attempt, locked, with no evidence record: book on it.
    Try(i32),
    /// A newer attempt was made while this call waited: start over.
    Moved,
    /// `Some(AlreadyBooked)`, or `None` for unmatched (no such pick, another intent, no attempt).
    Answer(Option<Rebuilt>),
}

/// D-3844, D-4292: locks pick `pick`'s newest `charge_attempt` row, then its `pick_intent` row,
/// both `FOR UPDATE`, and then reads, in a fresh statement, whether its evidence record exists and
/// whether that attempt is still the newest; the stored PaymentIntent must be `intent`.
async fn held_try(tx: &mut PgConnection, pick: Uuid, intent: &str) -> Result<Held, ChargeError> {
    let newest = db(sqlx::query_scalar!(
        "SELECT try_no FROM payments.charge_attempt WHERE pick_id = $1
          ORDER BY try_no DESC LIMIT 1 FOR UPDATE",
        pick
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let stored = db(sqlx::query_scalar!(
        "SELECT stripe_intent_id FROM payments.pick_intent WHERE pick_id = $1 FOR UPDATE",
        pick
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let (Some(newest), Some(true)) = (newest, stored.map(|s| s == intent)) else {
        return Ok(Held::Answer(None));
    };
    let now = db(sqlx::query!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.charge_evidence WHERE pick_id = $1) AS "booked!",
             (SELECT max(try_no) FROM payments.charge_attempt WHERE pick_id = $1) AS try_no"#,
        pick
    )
    .fetch_one(&mut *tx)
    .await)?;
    Ok(if now.booked {
        Held::Answer(Some(Rebuilt::AlreadyBooked))
    } else if now.try_no == Some(newest) {
        Held::Try(newest)
    } else {
        Held::Moved
    })
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, ChargeError> {
    result.map_err(|_| ChargeError::Database)
}
