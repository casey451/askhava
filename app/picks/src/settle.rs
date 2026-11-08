//! The settle and the masked-line retry (PICK-022, PRIV-005, BIZ-014, CALL-001; D-1564, D-1688,
//! D-1739).
//!
//! [`settle`] is the only code that moves a pick to `paid` or `free`. It takes `payments`'
//! cleared [`Releasable`] by reference, never a bare basis, so no crate can mark a pick paid or
//! free without `payments`' clearance (a verified succeeded charge or a recorded $0 decision);
//! the release then consumes the same `Releasable` through the minter. It moves only an
//! `awaiting_confirm` pick that has not ended, under the pick's row lock, with one audit row in
//! the caller's transaction, and leaves every other status alone ([`Settled`]); a pick already
//! `paid` or `free` for this basis answers [`Settled::Was`], so a restart between the settle's
//! commit and the release's claim releases on the retry (D-1739).
//!
//! [`retry_line`] is the [`LINE_RETRY_KIND`] handler `release` queues when a shown release's
//! masked line fails to open (D-1688). It opens the line from the release row's bound contact
//! (T253) only when the job never had one (`calls::api::job_had_line`), so a line closed after
//! 30 days is never reopened and a job whose line is open is left alone, and only while the pick
//! is still `paid` or `free` (`status::release_shown_codes`, D-2017); a failure is the task's
//! error, so the worker's retries and dead letter apply, and the release's High alert stays.

use askhava_adapters::api::MaskedPool;
use askhava_calls::api::{Connected, assign_number, job_had_line};
use askhava_core::api::{Claimed, HandlerError};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use askhava_payments::api::Releasable;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models::PicksError;
use crate::status::{release_shown_codes, release_status, settle_action, settle_step};

/// The masked-line retry task a failed line queues, one per pick (CALL-001, D-1688).
pub const LINE_RETRY_KIND: &str = "picks.line_retry";

const RETRY_FAILED: HandlerError = HandlerError("masked line retry failed");

/// What the settle does with a pick stored at `code` for a clearing of `basis` (PICK-022,
/// D-1564, D-1739): move it from awaiting confirm, note that an earlier call moved it, or leave
/// any other status alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettleStep {
    /// Awaiting confirm: move it to `release_status`.
    Move,
    /// Already at `release_status` for this basis.
    Was,
    /// Any other status, or an unknown code: nothing.
    Leave,
}

/// What [`settle`] did with the pick (PICK-022, D-1564).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// Moved from awaiting confirm to `paid` or `free` now; the pick's job.
    Moved(Uuid),
    /// Already `paid` or `free` for the clearing's basis (an earlier call moved it); the job.
    Was(Uuid),
    /// Unknown, ended, or in any other status: left alone, and nothing is to be released.
    Left,
}

/// Moves the pick `cleared` names from `awaiting_confirm` to `paid` (a paid charge) or `free` (a
/// $0 decision), in the caller's transaction (PICK-022, PRIV-005, BIZ-014, D-1564, D-1739).
///
/// # Errors
/// [`PicksError::Database`] or [`PicksError::Audit`]; nothing is moved then.
pub async fn settle(tx: &mut PgConnection, cleared: &Releasable) -> Result<Settled, PicksError> {
    let (pick, want) = (cleared.pick(), release_status(cleared.basis()));
    let row = sqlx::query!(
        r#"SELECT job_id, status, ended_at IS NOT NULL AS "ended!"
           FROM picks.pick WHERE id = $1 FOR UPDATE"#,
        pick
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| PicksError::Database)?;
    let Some(row) = row else {
        return Ok(Settled::Left);
    };
    if row.ended {
        return Ok(Settled::Left);
    }
    match settle_step(&row.status, cleared.basis()) {
        SettleStep::Move => {}
        SettleStep::Was => return Ok(Settled::Was(row.job_id)),
        SettleStep::Leave => return Ok(Settled::Left),
    }
    sqlx::query!(
        "UPDATE picks.pick SET status = $2 WHERE id = $1",
        pick,
        want.code()
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| PicksError::Database)?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: settle_action(cleared.basis()),
        subject_kind: "picks.pick",
        subject_id: pick,
        change: Change::Action,
    };
    audit(tx, &entry).await.map_err(|_| PicksError::Audit)?;
    Ok(Settled::Moved(row.job_id))
}

/// The [`LINE_RETRY_KIND`] handler: opens the released job's masked line from `masked` for the
/// release row's bound contact, only when the job never had one and the pick is still `paid` or
/// `free` (`status::shows_release`; a no-show or fraud-refunded pick gets nothing) (CALL-001,
/// CALL-010, D-1688, D-2017). Safe to run twice: once a line exists it does nothing.
///
/// # Errors
/// A [`HandlerError`] when the payload has no pick or the line still fails to open; the worker
/// retries it and dead-letters it after its attempts.
pub async fn retry_line(
    pool: PgPool,
    masked: MaskedPool,
    task: Claimed,
    at: Timestamp,
) -> Result<(), HandlerError> {
    let pick = task.payload["pick"]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or(HandlerError("line retry is missing a pick"))?;
    let mut conn = pool.acquire().await.map_err(|_| RETRY_FAILED)?;
    let bound = sqlx::query!(
        r#"SELECT r.job_id, r.business_id, r.contact_id, p.status = ANY($2) AS "shown!"
           FROM picks.release r JOIN picks.pick p ON p.id = r.pick_id WHERE r.pick_id = $1"#,
        pick,
        &release_shown_codes()
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| RETRY_FAILED)?;
    let Some(bound) = bound.filter(|b| b.shown) else {
        return Ok(());
    };
    if job_had_line(&mut conn, bound.job_id)
        .await
        .map_err(|_| RETRY_FAILED)?
    {
        return Ok(());
    }
    let job = Connected {
        job: bound.job_id,
        homeowner: bound.contact_id,
        business: bound.business_id,
    };
    assign_number(&mut conn, &masked, job, at)
        .await
        .map(|_| ())
        .map_err(|_| RETRY_FAILED)
}

#[cfg(test)]
mod tests {
    //! D-2017: `retry_line` opens a released pick's line only while the pick is paid or free.
    //! The pick and release rows are seeded in this crate's own schema; fake IDs and numbers.

    use askhava_adapters::api::{MaskedPool, TwilioConfig};
    use askhava_calls::api::job_had_line;
    use askhava_core::api::{Claimed, Settings};
    use askhava_ops::api::Actor;
    use jiff::Timestamp;
    use serde_json::json;
    use sqlx::types::Uuid;
    use sqlx::{PgConnection, PgPool};

    use super::{LINE_RETRY_KIND, retry_line};
    use crate::status::PickStatus;

    /// A masked pool of one fake number.
    fn masked() -> MaskedPool {
        let pairs: Vec<(String, String)> = [
            ("ASKHAVA_APP_ENV", "dev"),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
            (
                "ASKHAVA_TWILIO_ACCOUNT_SID",
                "AC00000000000000000000000000000000",
            ),
            ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
            (
                "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
                "VA00000000000000000000000000000001",
            ),
            ("ASKHAVA_MASKED_POOL_SIZE", "1"),
            ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let settings = Settings::from_pairs(pairs.clone()).expect("settings");
        TwilioConfig::from_settings(&settings, pairs)
            .expect("pool")
            .pool()
            .clone()
    }

    /// A released pick at `status`, its release row bound to a fake contact: `(pick, job)`.
    async fn released(db: &mut PgConnection, status: PickStatus) -> (Uuid, Uuid) {
        let business = askhava_businesses::api::create(db, false, Timestamp::now())
            .await
            .expect("business");
        let job = askhava_jobs::api::create(db, Timestamp::now(), false, Actor::System)
            .await
            .expect("job");
        let pick = sqlx::query_scalar!(
            "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
             VALUES ($1, $2, 1, 'page', $3, now()) RETURNING id",
            job,
            business,
            status.code(),
        )
        .fetch_one(&mut *db)
        .await
        .expect("pick");
        sqlx::query!(
            "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
             contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', now(), $4, false)",
            pick,
            job,
            business,
            Uuid::from_u128(0x0254_0b01),
        )
        .execute(&mut *db)
        .await
        .expect("release");
        (pick, job)
    }

    fn task(pick: Uuid) -> Claimed {
        Claimed {
            id: 1,
            kind: LINE_RETRY_KIND.to_owned(),
            payload: json!({"pick": pick}),
            attempts: 1,
        }
    }

    /// TD-PICK-50, D-2017 (CALL-001, PRIV-005): a released pick that is now a no-show or
    /// fraud-refunded gets no line from the retry; a paid one does.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_pick_50_the_line_retry_opens_only_for_a_paid_or_free_pick(pool: PgPool) {
        let mut db = pool.acquire().await.expect("db");
        for status in [PickStatus::NoShow, PickStatus::FraudRefunded] {
            let (pick, job) = released(&mut db, status).await;
            retry_line(pool.clone(), masked(), task(pick), Timestamp::now())
                .await
                .expect("nothing");
            let had = job_had_line(&mut db, job).await.expect("line");
            assert!(!had, "{status:?}");
        }
        let (pick, job) = released(&mut db, PickStatus::Paid).await;
        retry_line(pool.clone(), masked(), task(pick), Timestamp::now())
            .await
            .expect("opened");
        assert!(job_had_line(&mut db, job).await.expect("line"));
    }
}
