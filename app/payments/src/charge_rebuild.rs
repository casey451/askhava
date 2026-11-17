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

/// OPS-016, FEE-028, FEE-030, FEE-042, D-3753, D-3844: books one succeeded `charge` as Stripe
/// lists it, once, at `at`, on its own transaction, when its `metadata[pick_id]` names a pick
/// `payments` holds, its PaymentIntent is the one stored for that pick and the pick has a
/// `charge_attempt` row; the pick's `pick_intent` row is locked first. It never calls Stripe. A
/// refund (any amount) answers [`Rebuilt::Refunded`]; a pick with its evidence record answers
/// [`Rebuilt::AlreadyBooked`]; anything else [`Rebuilt::Unmatched`]. Booked is reported only when
/// this transaction wrote the evidence record: a record another path wrote meanwhile (the
/// webhook does not take the pick's lock) means `book_success` wrote nothing, and answers
/// [`Rebuilt::AlreadyBooked`].
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
    let mut tx = db(conn.begin().await)?;
    let try_no = match held_try(&mut tx, pick, intent).await? {
        Ok(try_no) => try_no,
        Err(answer) => return Ok(answer.unwrap_or(Rebuilt::Unmatched(charge.id.clone()))),
    };
    book_success(&mut tx, (pick, try_no), &charge.id, at).await?;
    let ours = db(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.charge_evidence
             WHERE pick_id = $1 AND xmin = pg_current_xact_id()::xid) AS "ours!""#,
        pick
    )
    .fetch_one(&mut *tx)
    .await)?;
    db(tx.commit().await)?;
    Ok(if ours {
        Rebuilt::Booked
    } else {
        Rebuilt::AlreadyBooked
    })
}

/// D-3844: under the lock of pick `pick`'s `pick_intent` row, its newest attempt's `try_no` when
/// the stored PaymentIntent is `intent` and no evidence record exists; otherwise the answer:
/// `Some(AlreadyBooked)`, or `None` for unmatched (no such pick, another intent, no attempt).
async fn held_try(
    tx: &mut PgConnection,
    pick: Uuid,
    intent: &str,
) -> Result<Result<i32, Option<Rebuilt>>, ChargeError> {
    let held = db(sqlx::query!(
        r#"SELECT i.stripe_intent_id,
             EXISTS (SELECT 1 FROM payments.charge_evidence e WHERE e.pick_id = i.pick_id) AS "booked!",
             (SELECT max(a.try_no) FROM payments.charge_attempt a WHERE a.pick_id = i.pick_id) AS try_no
           FROM payments.pick_intent i WHERE i.pick_id = $1 FOR UPDATE OF i"#,
        pick
    )
    .fetch_optional(tx)
    .await)?;
    Ok(match held {
        Some(h) if h.stripe_intent_id != intent => Err(None),
        Some(h) if h.booked => Err(Some(Rebuilt::AlreadyBooked)),
        Some(h) => h.try_no.ok_or(None),
        None => Err(None),
    })
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, ChargeError> {
    result.map_err(|_| ChargeError::Database)
}
