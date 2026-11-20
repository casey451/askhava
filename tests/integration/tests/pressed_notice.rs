//! TD-JOB-21 (OPS-034, PICK-035, PICK-046; T670, D-4349, D-4932): `notice_sent` for a waiting
//! pick whose job was cancelled before the pick notice went. A pick whose business had pressed
//! Confirm (the job entered Payment pending after the pick was made) is recorded as
//! `payment_failed`, never `expired`, so it is not counted as a lapsed pick; an unpressed pick
//! is still recorded as `expired`, also when an earlier pick of the same job was pressed before
//! this one was made (the `since` cutoff, D-4932). PostgreSQL through each module's API; fake data only.

use askhava_businesses::api::add_owner;
use askhava_core::api::{Guarded, MIGRATOR};
use askhava_jobs::api::{JobStatus, current, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    ConfirmMove, ConfirmSettings, PagePick, PendingEnd, PicksError, confirm_move, end_pending,
    notice_sent, page_pick,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// 10:00 Arizona on a Monday.
const NOW: i64 = 1_793_638_800;

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_second(NOW).expect("time") + SignedDuration::from_mins(minutes)
}

/// Moves `job` along `arrow` at minute `minute`.
async fn step(db: &mut PgConnection, job: Uuid, arrow: (JobStatus, JobStatus), minute: i64) {
    let moved = move_to(db, job, arrow, at(minute), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
}

/// A business with its owner and a job in Quotes ready with its one quote: the job.
async fn quoted(db: &mut PgConnection) -> Uuid {
    let business = askhava_businesses::api::create(db, false, at(0)).await;
    let business = business.expect("business");
    add_owner(db, business, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System).await;
    let job = job.expect("job");
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    step(db, job, (JobStatus::Intake, JobStatus::Open), 0).await;
    step(db, job, (JobStatus::Open, JobStatus::QuotesReady), 0).await;
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 32500, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(business)
    .bind(at(1).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
    job
}

/// Her page pick of the job's one quote at minute `minute`.
async fn picked(db: &mut PgConnection, job: Uuid, minute: i64) -> Uuid {
    let Ok(PagePick::Picked(pick)) =
        page_pick(db, (job, false), 1, Actor::System, at(minute)).await
    else {
        panic!("pick");
    };
    pick
}

/// [`quoted`] and her page pick of it at minute 3; when `pressed` the job then moves to Payment
/// pending through `confirm_move`. The job is cancelled after that: `(job, pick)`.
async fn cancelled(pool: &PgPool, pressed: bool) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let job = quoted(&mut db).await;
    let pick = picked(&mut db, job, 3).await;
    let mut from = JobStatus::AwaitingConfirm;
    if pressed {
        let moved = confirm_move(&mut db, (pick, job), at(5)).await;
        assert_eq!(moved.expect("confirm move"), ConfirmMove::Moved);
        from = JobStatus::PaymentPending;
    }
    step(&mut db, job, (from, JobStatus::Cancelled), 6).await;
    (job, pick)
}

/// Tells `notice_sent` the notice of `pick` went at minute 7 and answers the pick's stored
/// status and whether it has an end time, and the job's status code.
async fn after_send(pool: &PgPool, (job, pick): (Uuid, Uuid)) -> (String, bool, &'static str) {
    let mut db = pool.acquire().await.expect("db");
    let sent = notice_sent(&mut db, pick, at(7), ConfirmSettings::default()).await;
    assert_eq!(sent, Err(PicksError::MovedOn), "no window for a dead pick");
    let row: (String, bool) =
        sqlx::query_as("SELECT status, ended_at IS NOT NULL FROM picks.pick WHERE id = $1")
            .bind(pick)
            .fetch_one(pool)
            .await
            .expect("pick");
    let status = current(&mut db, job).await.expect("job").status.code();
    (row.0, row.1, status)
}

/// TD-JOB-21 (OPS-034, PICK-035): a pressed pick whose job was then cancelled is recorded as
/// `payment_failed`, never `expired`, and the job stays cancelled.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_21_a_pressed_pick_on_a_cancelled_job_is_payment_failed(pool: PgPool) {
    let ids = cancelled(&pool, true).await;
    let ended = after_send(&pool, ids).await;
    assert_eq!(ended, ("payment_failed".to_owned(), true, "cancelled"));
}

/// TD-JOB-21 (PICK-035, PICK-046): an unpressed pick on a cancelled job is still recorded as
/// `expired`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_21_an_unpressed_pick_on_a_cancelled_job_is_expired(pool: PgPool) {
    let ids = cancelled(&pool, false).await;
    let ended = after_send(&pool, ids).await;
    assert_eq!(ended, ("expired".to_owned(), true, "cancelled"));
}

/// TD-JOB-21 (OPS-034, PICK-046, D-4932): the job's Payment pending entry counts only after the
/// waiting pick was made. A first pick is pressed at minute 4 and its payment fails (the job
/// returns to Quotes ready); a second pick made at minute 5 is never pressed, so it is `expired`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_21_an_earlier_picks_press_does_not_count_for_a_later_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quoted(&mut db).await;
    let first = picked(&mut db, job, 3).await;
    let moved = confirm_move(&mut db, (first, job), at(4)).await;
    assert_eq!(moved.expect("confirm move"), ConfirmMove::Moved);
    let by = (Actor::System, at(4));
    let ended = end_pending(&mut db, (first, job), PendingEnd::PaymentFailed, by).await;
    ended.expect("first pick ends");
    let back = current(&mut db, job).await.expect("job").status;
    assert_eq!(back, JobStatus::QuotesReady, "back to Quotes ready");
    let second = picked(&mut db, job, 5).await;
    assert_ne!(first, second, "a new pick");
    step(
        &mut db,
        job,
        (JobStatus::AwaitingConfirm, JobStatus::Cancelled),
        6,
    )
    .await;
    drop(db);
    let ended = after_send(&pool, (job, second)).await;
    assert_eq!(ended, ("expired".to_owned(), true, "cancelled"));
}
