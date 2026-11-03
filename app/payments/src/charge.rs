//! The Confirm-and-pay charge: one PaymentIntent per pick, one attempt per press (FEE-016,
//! FEE-017, FEE-037, FEE-041, FEE-047, D-153, D-204, D-365, D-490, D-491).
//!
//! [`open_intent`] creates the pick's one PaymentIntent on the first Confirm-page load under the
//! key derived from `pick-<id>-intent`, and stores it (FEE-047). [`confirm_and_pay`] is the only
//! code that charges: it runs for the business's press alone, never on its own (FEE-016), and in
//! this order: a repeated press answers from its own attempt; the FEE-017 check (the caller's
//! [`PickNow`] and `payments`' own record of the pick: another business's, paid or ended); the
//! charging pause (OPS-024, FEE-038); an undecided earlier attempt is settled by asking Stripe
//! first, and blocks a new try until Stripe says it failed (FEE-037); the monthly fee limit
//! (FEE-041), which ends the pick at once; only then is the PaymentIntent made, if the page has
//! not made it, and a new attempt `pick-<id>-try<n>` started under the pick's row lock (so two
//! presses never make two undecided tries) confirms it with its own key. A confirm Stripe
//! refuses with a 4xx (other than 402 and 429) charged nothing, so the intent is read again. No
//! transaction or pooled connection is held across a Stripe call (R11). The timers and events
//! each answer starts are `handlers.rs`'s; a success writes the pick's one FEE-042 evidence
//! record, [`write_evidence`], in the transaction that stores it (D-1474).

use std::future::Future;

use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_core::api::arizona;
use askhava_ops::api::{Actor, AuditEntry, audit};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::handlers::{
    CHECK_AFTER, EndReason, announce_end, charges_paused, end_pick, queue_check, record,
};
use crate::models::{
    BusinessId, ChargeError, ChargeOutcome, PickNow, Press, attempt_label, intent_label,
};
use crate::service::customer_of;
use crate::status::AttemptStatus;

/// The idempotency action of a pick's PaymentIntent create (FEE-047, SEC-018).
pub const INTENT_ACTION: &str = "payments.pick_intent";
/// The idempotency action of one charge attempt's confirm (FEE-016, SEC-018).
pub const ATTEMPT_ACTION: &str = "payments.charge_attempt";
/// The audit subject kind of a pick's charge attempts (JOB-005).
pub const ATTEMPT_SUBJECT: &str = "payments.charge_attempt";
/// How long after a confirm starts it may still be on its way to Stripe (the HTTP client gives
/// up at 15 seconds): an intent that still wants a card in that time is not a decline (FEE-037).
pub const IN_FLIGHT: SignedDuration = SignedDuration::from_secs(60);

/// The two Stripe calls a pick's one PaymentIntent needs beyond [`PaymentsAdapter`]: create it
/// unconfirmed, and confirm it with one attempt's key (FEE-047). The adapter methods that post
/// them are T132's (`app/adapters/src/payments.rs`, D-1424).
pub trait IntentStripe: PaymentsAdapter {
    /// Creates the PaymentIntent for `amount_cents` on `customer` with `metadata[pick_id]`,
    /// unconfirmed; returns `pi_…`.
    fn create_intent(
        &self,
        idempotency: &IdempotencyKey,
        customer: &str,
        amount_cents: u32,
        pick_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send;

    /// Confirms `intent` on-session with `payment_method` under one attempt's key.
    fn confirm_intent(
        &self,
        idempotency: &IdempotencyKey,
        intent: &str,
        payment_method: &str,
    ) -> impl Future<Output = Result<PaymentIntent, AdapterError>> + Send;
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, ChargeError> {
    result.map_err(|_| ChargeError::Database)
}

async fn intent_of(conn: &mut PgConnection, pick: Uuid) -> Result<Option<String>, ChargeError> {
    db(sqlx::query_scalar!(
        "SELECT stripe_intent_id FROM payments.pick_intent WHERE pick_id = $1",
        pick
    )
    .fetch_optional(conn)
    .await)
}

/// The pick's one PaymentIntent, created on the first call under the key derived from
/// `pick-<id>-intent`, so two tabs, a reload or a retry reach the same one (FEE-047, D-490).
///
/// # Errors
/// [`ChargeError::BadField`] for a $0 fee or an answer that is not `pi_…`;
/// [`ChargeError::NoCustomer`]; [`ChargeError::Stripe`] (nothing stored); `Database`.
pub async fn open_intent(
    pool: &PgPool,
    stripe: &impl IntentStripe,
    secret: &SecretString,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<String, ChargeError> {
    if press.amount_cents == 0 {
        return Err(ChargeError::BadField);
    }
    let mut conn = db(pool.acquire().await)?;
    if let Some(found) = intent_of(&mut conn, press.pick).await? {
        return Ok(found);
    }
    let customer = customer_of(&mut conn, press.business)
        .await
        .map_err(|_| ChargeError::Database)?
        .ok_or(ChargeError::NoCustomer)?;
    drop(conn);
    let key = IdempotencyKey::derive(secret, INTENT_ACTION, &[&intent_label(press.pick)]);
    let pick = press.pick.to_string();
    let created = stripe
        .create_intent(&key, customer.as_str(), press.amount_cents, &pick)
        .await
        .map_err(|_| ChargeError::Stripe)?;
    if !created.starts_with("pi_") {
        return Err(ChargeError::BadField);
    }
    let mut conn = db(pool.acquire().await)?;
    db(sqlx::query!(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (pick_id) DO NOTHING",
        press.pick,
        press.business.0,
        press.job,
        i64::from(press.amount_cents),
        created,
        press.emergency,
        at.to_sqlx() as _,
    )
    .execute(&mut *conn)
    .await)?;
    intent_of(&mut conn, press.pick)
        .await?
        .ok_or(ChargeError::Database)
}

/// One "Confirm and pay" press (FEE-016, FEE-017, FEE-037, FEE-041, FEE-047): see the module
/// docs for the order of checks. Every attempt confirms the pick's one PaymentIntent with the key
/// derived from `pick-<id>-try<n>`; nothing here retries a charge.
///
/// # Errors
/// [`ChargeError::BadField`] for a $0 fee or a card ID that is not `pm_…`; the errors of
/// [`open_intent`]; [`ChargeError::Database`].
pub async fn confirm_and_pay(
    pool: &PgPool,
    stripe: &impl IntentStripe,
    secret: &SecretString,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<ChargeOutcome, ChargeError> {
    if !press.payment_method.starts_with("pm_") || press.amount_cents == 0 {
        return Err(ChargeError::BadField);
    }
    let mut conn = db(pool.acquire().await)?;
    let repeated = press_status(&mut conn, press).await?;
    drop(conn);
    if let Some(status) = repeated {
        return repeat_press(pool, stripe, press.pick, status, at).await;
    }
    if let Some(stop) = before_attempt(pool, stripe, press, at).await? {
        return Ok(stop);
    }
    let intent = open_intent(pool, stripe, secret, press, at).await?;
    let mut conn = db(pool.acquire().await)?;
    let Some(try_no) = begin_attempt(&mut conn, press, at).await? else {
        return Ok(ChargeOutcome::Pending);
    };
    drop(conn);
    let label = attempt_label(press.pick, try_no);
    let key = IdempotencyKey::derive(secret, ATTEMPT_ACTION, &[&label]);
    let answer = stripe
        .confirm_intent(&key, &intent, press.payment_method)
        .await;
    let status = match answer_status(&answer) {
        Some(status) => status,
        None => {
            let found = stripe.fetch_payment_intent(&intent).await;
            AttemptStatus::after_refusal(found.as_ref().ok().map(|f| f.status.as_str()))
        }
    };
    let status = record(pool, press.pick, try_no, status, at).await?;
    report(pool, press.pick, status, &intent).await
}

/// A repeated press, reload or duplicate callback answers from its own attempt, asking Stripe
/// first while it is undecided, and never sends again (FEE-016).
async fn repeat_press(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    pick: Uuid,
    status: AttemptStatus,
    at: Timestamp,
) -> Result<ChargeOutcome, ChargeError> {
    let status = if status.undecided() {
        ask_stripe(pool, stripe, pick, at).await?.unwrap_or(status)
    } else {
        status
    };
    let mut conn = db(pool.acquire().await)?;
    let intent = intent_of(&mut conn, pick)
        .await?
        .ok_or(ChargeError::Database)?;
    drop(conn);
    report(pool, pick, status, &intent).await
}

/// The attempt status a confirm call's answer means, or `None` for a 4xx refusal other than 402
/// and 429, which Stripe answered without charging (the caller reads the intent again): a 402 is
/// a decline; a request that never left, or a 429, is unreachable; a timeout or a 5xx stays
/// `Sent` until Stripe is asked (FEE-018, FEE-037).
fn answer_status(answer: &Result<PaymentIntent, AdapterError>) -> Option<AttemptStatus> {
    match answer {
        Ok(intent) => Some(AttemptStatus::from_intent(&intent.status)),
        Err(AdapterError::Vendor { status: 402, .. }) => Some(AttemptStatus::Declined),
        Err(
            AdapterError::Vendor { status: 429, .. }
            | AdapterError::NotSent
            | AdapterError::BadConfig
            | AdapterError::InvalidId,
        ) => Some(AttemptStatus::Unreachable),
        Err(AdapterError::Vendor {
            status: 400..=499, ..
        }) => None,
        Err(
            AdapterError::OutcomeUnknown
            | AdapterError::Vendor { .. }
            | AdapterError::Decode
            | AdapterError::ObjectExists,
        ) => Some(AttemptStatus::Sent),
    }
}

/// The page's outcome for `status`, with the card-fix deadline when one runs (FEE-020).
async fn report(
    pool: &PgPool,
    pick: Uuid,
    status: AttemptStatus,
    intent: &str,
) -> Result<ChargeOutcome, ChargeError> {
    let mut conn = db(pool.acquire().await)?;
    let fix_by = db(sqlx::query_scalar!(
        "SELECT due_at FROM payments.pay_timer
         WHERE pick_id = $1 AND kind = 'card-fix' AND ended_at IS NULL",
        pick
    )
    .fetch_optional(&mut *conn)
    .await)?
    .flatten()
    .map(|due| due.to_jiff());
    Ok(status.outcome(fix_by, intent))
}

/// The status of this press's own attempt, if it made one (a repeated press, FEE-016).
async fn press_status(
    conn: &mut PgConnection,
    press: &Press<'_>,
) -> Result<Option<AttemptStatus>, ChargeError> {
    let code = db(sqlx::query_scalar!(
        "SELECT status FROM payments.charge_attempt WHERE pick_id = $1 AND press_id = $2",
        press.pick,
        press.press
    )
    .fetch_optional(conn)
    .await)?;
    Ok(code.as_deref().and_then(AttemptStatus::from_code))
}

/// A pick's latest attempt: its number, status and start.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Latest {
    /// The attempt number.
    pub(crate) try_no: i32,
    /// Its stored status.
    pub(crate) status: AttemptStatus,
    /// When its confirm call started.
    pub(crate) started: Timestamp,
}

/// The pick's latest attempt.
pub(crate) async fn latest(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<Latest>, ChargeError> {
    let row = db(sqlx::query!(
        "SELECT try_no, status, started_at FROM payments.charge_attempt WHERE pick_id = $1
         ORDER BY try_no DESC LIMIT 1",
        pick
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.and_then(|r| {
        AttemptStatus::from_code(&r.status).map(|status| Latest {
            try_no: r.try_no,
            status,
            started: r.started_at.to_jiff(),
        })
    }))
}

/// The checks before a new attempt, in order (FEE-017, OPS-024, FEE-038, FEE-037, FEE-041); `Some`
/// stops the press, and none of them calls Stripe but the FEE-037 question about an earlier try.
async fn before_attempt(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<Option<ChargeOutcome>, ChargeError> {
    match press.now {
        PickNow::MovedOn => return Ok(Some(ChargeOutcome::MovedOn)),
        PickNow::Payable => {}
    }
    let mut conn = db(pool.acquire().await)?;
    if let Some(stop) = pick_record(&mut conn, press).await? {
        return Ok(Some(stop));
    }
    if charges_paused(&mut conn).await? {
        return Ok(Some(ChargeOutcome::Paused));
    }
    drop(conn);
    if let Some(stop) = settle_earlier(pool, stripe, press.pick, at).await? {
        return Ok(Some(stop));
    }
    let mut conn = db(pool.acquire().await)?;
    if over_limit(&mut conn, press.business, press.amount_cents, at).await? {
        let mut tx = db(sqlx::Connection::begin(&mut *conn).await)?;
        if !end_pick(&mut tx, press.pick, EndReason::FeeLimit, at).await? {
            let parties = (press.business.0, press.job);
            announce_end(&mut tx, press.pick, parties, EndReason::FeeLimit, at).await?;
        }
        db(tx.commit().await)?;
        return Ok(Some(ChargeOutcome::OverLimit));
    }
    Ok(None)
}

/// `payments`' own record of the pick (FEE-017): another business's pick, a paid one or an
/// ended one stops the press; no record yet (no intent made) stops nothing.
async fn pick_record(
    conn: &mut PgConnection,
    press: &Press<'_>,
) -> Result<Option<ChargeOutcome>, ChargeError> {
    let row = db(sqlx::query!(
        r#"SELECT business_id, ended_at IS NOT NULL AS "ended!", EXISTS (SELECT 1
             FROM payments.charge_attempt a
             WHERE a.pick_id = i.pick_id AND a.status = 'succeeded') AS "paid!"
           FROM payments.pick_intent i WHERE pick_id = $1"#,
        press.pick
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.business_id != press.business.0 || (row.ended && !row.paid) {
        return Ok(Some(ChargeOutcome::MovedOn));
    }
    Ok(row.paid.then_some(ChargeOutcome::AlreadyPaid))
}

/// An undecided latest attempt is settled by asking Stripe first; it blocks a new try until
/// Stripe says it failed, and never while Stripe is unreachable (FEE-037, D-153).
async fn settle_earlier(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    pick: Uuid,
    at: Timestamp,
) -> Result<Option<ChargeOutcome>, ChargeError> {
    let mut conn = db(pool.acquire().await)?;
    let open = latest(&mut conn, pick).await?;
    drop(conn);
    if !open.is_some_and(|l| l.status.undecided()) {
        return Ok(None);
    }
    let asked = ask_stripe(pool, stripe, pick, at).await?;
    Ok(asked.map_or(Some(ChargeOutcome::Pending), AttemptStatus::blocks_new_try))
}

/// Sets, or with `None` removes, the business's monthly fee limit (FEE-041, D-204); the portal
/// and card-authorization field that calls it is T224's.
///
/// # Errors
/// [`ChargeError::BadField`] for a limit of 0; [`ChargeError::Database`].
pub async fn set_fee_limit(
    conn: &mut PgConnection,
    business: BusinessId,
    limit_cents: Option<u32>,
) -> Result<(), ChargeError> {
    if limit_cents == Some(0) {
        return Err(ChargeError::BadField);
    }
    db(sqlx::query!(
        "DELETE FROM payments.fee_limit WHERE business_id = $1",
        business.0
    )
    .execute(&mut *conn)
    .await)?;
    if let Some(cents) = limit_cents {
        db(sqlx::query!(
            "INSERT INTO payments.fee_limit (business_id, limit_cents) VALUES ($1, $2)",
            business.0,
            i64::from(cents)
        )
        .execute(&mut *conn)
        .await)?;
    }
    Ok(())
}

/// True when `amount_cents` would take the business's succeeded charges this Arizona calendar
/// month over its limit (FEE-041); exactly at the limit is still charged.
async fn over_limit(
    conn: &mut PgConnection,
    business: BusinessId,
    amount_cents: u32,
    at: Timestamp,
) -> Result<bool, ChargeError> {
    let limit = db(sqlx::query_scalar!(
        "SELECT limit_cents FROM payments.fee_limit WHERE business_id = $1",
        business.0
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(limit) = limit else {
        return Ok(false);
    };
    let month = at
        .to_zoned(arizona())
        .first_of_month()
        .and_then(|day| day.start_of_day())
        .map_err(|_| ChargeError::BadField)?
        .timestamp();
    let charged = db(sqlx::query_scalar!(
        r#"SELECT coalesce(sum(i.amount_cents), 0)::bigint AS "sum!" FROM payments.charge_attempt a
           JOIN payments.pick_intent i USING (pick_id)
           WHERE i.business_id = $1 AND a.status = 'succeeded' AND a.decided_at >= $2"#,
        business.0,
        month.to_sqlx() as _,
    )
    .fetch_one(&mut *conn)
    .await)?;
    Ok(charged + i64::from(amount_cents) > limit)
}

/// Starts the next attempt, try `n`, for this press, with its 30-minute check (FEE-019), under
/// the pick's row lock, and only while no earlier attempt is undecided or paid and the pick has
/// not ended, so the check and the insert are one step (FEE-016, FEE-017, FEE-037); `None` when
/// another press holds the pick (the page waits for it).
async fn begin_attempt(
    conn: &mut PgConnection,
    press: &Press<'_>,
    at: Timestamp,
) -> Result<Option<i32>, ChargeError> {
    let mut tx = db(sqlx::Connection::begin(&mut *conn).await)?;
    db(sqlx::query_scalar!(
        "SELECT pick_id FROM payments.pick_intent WHERE pick_id = $1 FOR UPDATE",
        press.pick
    )
    .fetch_one(&mut *tx)
    .await)?;
    let try_no = db(sqlx::query_scalar!(
        r#"INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
           SELECT $1, coalesce(max(a.try_no), 0) + 1, $2, 'sent', $3
           FROM payments.charge_attempt a WHERE a.pick_id = $1
           HAVING NOT coalesce(bool_or(a.status IN ('sent', 'processing', 'succeeded')), false)
             AND NOT EXISTS (SELECT 1 FROM payments.pick_intent i
                             WHERE i.pick_id = $1 AND i.ended_at IS NOT NULL)
           ON CONFLICT DO NOTHING RETURNING try_no"#,
        press.pick,
        press.press,
        at.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    if let Some(try_no) = try_no {
        let actor = Actor::Business(press.business.0);
        let entry = AuditEntry::status_change(actor, (ATTEMPT_SUBJECT, press.pick), "new", "sent");
        audit(&mut tx, &entry)
            .await
            .map_err(|_| ChargeError::Database)?;
        let check_at = at.saturating_add(CHECK_AFTER).unwrap_or(at);
        queue_check(&mut tx, press.pick, try_no, 0, check_at).await?;
    }
    db(tx.commit().await)?;
    Ok(try_no)
}

/// Asks Stripe for the pick's PaymentIntent and records what it says on the latest attempt
/// (FEE-019, FEE-037); `None` when Stripe could not be reached. Asking never charges.
///
/// # Errors
/// [`ChargeError::Database`].
pub(crate) async fn ask_stripe(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    pick: Uuid,
    at: Timestamp,
) -> Result<Option<AttemptStatus>, ChargeError> {
    let mut conn = db(pool.acquire().await)?;
    let (Some(intent), Some(last)) = (
        intent_of(&mut conn, pick).await?,
        latest(&mut conn, pick).await?,
    ) else {
        return Ok(None);
    };
    drop(conn);
    let Ok(found) = stripe.fetch_payment_intent(&intent).await else {
        return Ok(None);
    };
    let in_flight = at
        < last
            .started
            .saturating_add(IN_FLIGHT)
            .unwrap_or(last.started);
    let status = AttemptStatus::asked(last.status, &found.status, in_flight);
    if status.settles(last.status) {
        return record(pool, pick, last.try_no, status, at).await.map(Some);
    }
    Ok(Some(last.status))
}

/// Writes the pick's one FEE-042 evidence record for its succeeded attempt `try_no`, on the
/// caller's transaction, so it commits with the success (FEE-042, FEE-015, D-365, D-1474,
/// D-1478): IDs and timestamps only (the press, the attempt's key label `pick-<id>-try<n>`, the
/// Stripe customer and PaymentIntent, the version of the business's latest FEE-015 acceptance at
/// or before the press, or none when none is recorded, the press and success times), never card
/// data (FEE-014) or a homeowner field (PRIV-027). A second success for the same pick writes
/// nothing; the table is append-only for the app role.
///
/// # Errors
/// [`ChargeError::Database`], also when the pick, the attempt or the customer is missing and no
/// record exists, so a success never goes unrecorded silently.
pub(crate) async fn write_evidence(
    tx: &mut PgConnection,
    pick: Uuid,
    try_no: i32,
    at: Timestamp,
) -> Result<(), ChargeError> {
    let written = db(sqlx::query!(
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at)
         SELECT i.pick_id, i.business_id, i.job_id, a.press_id, a.try_no, $3, c.stripe_customer_id,
           i.stripe_intent_id,
           (SELECT z.version FROM payments.card_authorization z
             WHERE z.business_id = i.business_id AND z.accepted_at <= a.started_at
             ORDER BY z.accepted_at DESC, z.version DESC LIMIT 1),
           i.amount_cents, a.started_at, $4
         FROM payments.pick_intent i
         JOIN payments.charge_attempt a ON a.pick_id = i.pick_id AND a.try_no = $2
         JOIN payments.customer c ON c.business_id = i.business_id
         WHERE i.pick_id = $1
         ON CONFLICT (pick_id) DO NOTHING",
        pick,
        try_no,
        attempt_label(pick, try_no),
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await)?;
    if written.rows_affected() == 1 {
        return Ok(());
    }
    let kept = db(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.charge_evidence WHERE pick_id = $1) AS "kept!""#,
        pick
    )
    .fetch_one(&mut *tx)
    .await)?;
    if kept {
        Ok(())
    } else {
        Err(ChargeError::Database)
    }
}
