//! TD-CALL-28 (CALL-001, CALL-009, JOB-015, JOB-003; T667, D-4349): the masked-line retry opens a
//! reconnected job's second line and never reopens the earlier connection's, and the release's
//! claim restarts the Connected wait. PostgreSQL through `picks`, `calls` and `jobs`' APIs; the
//! pick and release rows a failed release leaves are planted with one SQL statement each. Fake
//! data and numbers only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{CloseReason, Connected, assign_number, close_line};
use askhava_core::api::{Claimed, MIGRATOR, Settings, WorkerId, claim, finish};
use askhava_jobs::api::{JobStatus, TIMER_KIND, create, current, job_number, move_to, run_timer};
use askhava_ops::api::Actor;
use askhava_picks::api::{LINE_RETRY_KIND, check_ins_at_release, retry_line};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// A fixed fake instant.
const NOW: i64 = 1_793_638_800;
/// The fake vault contact every release here is bound to.
const CONTACT: Uuid = Uuid::from_u128(0x0667_0c01);

fn hours(n: i64) -> Timestamp {
    Timestamp::from_second(NOW).expect("time") + SignedDuration::from_hours(n)
}

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

/// A fake business and a job of it moved along the arrows to Connected at hour 0.
async fn connected(db: &mut PgConnection) -> (Uuid, Uuid) {
    let business = askhava_businesses::api::create(db, false, hours(0)).await;
    let job = create(db, hours(0), false, Actor::System)
        .await
        .expect("job");
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    for arrow in path.windows(2) {
        let moved = move_to(db, job, (arrow[0], arrow[1]), hours(0), Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{arrow:?}");
    }
    (job, business.expect("business"))
}

/// Plants a pick of `job` and `business` stored at `status` and its release row at `released`,
/// bound to [`CONTACT`]: what a release whose line failed to open leaves.
async fn released(db: &mut PgConnection, ids: (Uuid, Uuid), status: &str, at: Timestamp) -> Uuid {
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id",
    )
    .bind(ids.0)
    .bind(ids.1)
    .bind(status)
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    sqlx::query(
        "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
         contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', $4, $5, false)",
    )
    .bind(pick)
    .bind(ids.0)
    .bind(ids.1)
    .bind(at.to_sqlx())
    .bind(CONTACT)
    .execute(db)
    .await
    .expect("release");
    pick
}

/// Runs the line retry of `pick` at `at`.
async fn retry(pool: &PgPool, pick: Uuid, at: Timestamp) -> bool {
    let task = Claimed {
        id: 1,
        kind: LINE_RETRY_KIND.to_owned(),
        payload: json!({"pick": pick}),
        attempts: 1,
    };
    retry_line(pool.clone(), masked(), task, at).await.is_ok()
}

/// Each line of `job`, oldest first: when it was given and when it closed.
async fn lines(db: &mut PgConnection, job: Uuid) -> Vec<(Timestamp, Option<Timestamp>)> {
    type Row = (jiff_sqlx::Timestamp, Option<jiff_sqlx::Timestamp>);
    let sql = "SELECT assigned_at, closed_at FROM calls.number_pool WHERE job_id = $1 ORDER BY seq";
    let rows: Vec<Row> = sqlx::query_as(sql)
        .bind(job)
        .fetch_all(db)
        .await
        .expect("lines");
    rows.into_iter()
        .map(|(given, closed)| (given.to_jiff(), closed.map(|c| c.to_jiff())))
        .collect()
}

/// The first connection of `job`: a no-show pick released at hour 0 with its line open.
async fn first_connection(db: &mut PgConnection, (job, business): (Uuid, Uuid)) -> Uuid {
    let pick = released(db, (job, business), "no_show", hours(0)).await;
    let line = Connected {
        job,
        homeowner: CONTACT,
        business,
    };
    assign_number(db, &masked(), line, hours(0))
        .await
        .expect("first line");
    pick
}

/// The no-show path's close of `job`'s open line at hour `at`.
async fn no_show_close(db: &mut PgConnection, job: Uuid, at: i64) {
    let number = job_number(db, job).await.expect("number");
    let closed = close_line(db, (job, number), CloseReason::NoShow, hours(at)).await;
    assert!(closed.expect("close").is_some(), "the open line closed");
}

/// TD-CALL-28 (CALL-001, CALL-009, JOB-015): a job connected, returned with no call in 72 hours
/// and connected again with the same business gets its failed second line from the retry; a
/// retry for a pick whose own line exists opens nothing; the earlier line stays closed.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_28_a_reconnected_jobs_failed_line_is_retried_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, business) = connected(&mut db).await;
    let first = first_connection(&mut db, (job, business)).await;
    no_show_close(&mut db, job, 72).await;
    let second = released(&mut db, (job, business), "paid", hours(80)).await;
    assert!(retry(&pool, second, hours(81)).await, "opened");
    let after = lines(&mut db, job).await;
    let want = vec![(hours(0), Some(hours(72))), (hours(81), None)];
    assert_eq!(
        after, want,
        "the second line opened, the first stays closed"
    );
    assert!(retry(&pool, second, hours(82)).await, "nothing");
    assert!(
        retry(&pool, first, hours(82)).await,
        "the ended pick gets nothing"
    );
    assert_eq!(lines(&mut db, job).await, want, "its own line exists");
    no_show_close(&mut db, job, 90).await;
    assert!(retry(&pool, second, hours(91)).await, "nothing again");
    let closed = vec![(hours(0), Some(hours(72))), (hours(81), Some(hours(90)))];
    assert_eq!(
        lines(&mut db, job).await,
        closed,
        "no closed line is reopened"
    );
}

/// TD-CALL-28 (CALL-001, CALL-009): while the earlier connection's line is still open the retry
/// fails, so the worker keeps it queued, and opens nothing; once that line closes it opens.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_28_the_retry_fails_while_the_earlier_line_is_open(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, business) = connected(&mut db).await;
    first_connection(&mut db, (job, business)).await;
    let second = released(&mut db, (job, business), "paid", hours(80)).await;
    assert!(!retry(&pool, second, hours(81)).await, "the retry fails");
    assert_eq!(lines(&mut db, job).await, vec![(hours(0), None)]);
    no_show_close(&mut db, job, 82).await;
    assert!(retry(&pool, second, hours(83)).await, "opened");
    let want = vec![(hours(0), Some(hours(82))), (hours(83), None)];
    assert_eq!(lines(&mut db, job).await, want);
}

/// Claims and finishes every task due at `now`, running the job timers among them.
async fn fire(pool: &PgPool, now: Timestamp) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_mins(5);
    for task in claim(pool, owner, now, lease, 100).await.expect("claim") {
        finish(pool, owner, &task, now).await.expect("finish");
        if task.kind == TIMER_KIND {
            run_timer(pool.clone(), task).await.expect("timer");
        }
    }
}

/// TD-CALL-28 (JOB-015, JOB-003, D-807): a release that waited 2 days for the address restarts
/// the Connected wait from the release, so the job is still Connected 21 days after the settle
/// and closes 21 days after the release; an earlier instant never shortens the wait.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_28_a_late_release_restarts_the_connected_wait(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, business) = connected(&mut db).await;
    for at in [hours(48), hours(24)] {
        let queued = check_ins_at_release(&mut db, (job, business), at).await;
        assert!(queued.expect("check-ins"), "a Connected job");
    }
    fire(&pool, hours(21 * 24)).await;
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status.code(), "connected", "21 days after the settle");
    fire(&pool, hours(23 * 24)).await;
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status.code(), "closed", "21 days after the release");
}
