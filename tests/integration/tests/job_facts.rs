//! A job's category and candidate job type codes as append-only narrowing rows (INT-018, FEE-005;
//! TD-JOB-17, D-2330, D-3157).
//!
//! Proves that the first `record_job_facts` writes a row, the same set again writes nothing, a
//! narrowing row is appended and read back, another category, a code not in the latest row or an
//! empty set is refused and writes nothing, the app role cannot change or delete a row, two
//! narrowings at once apply one after the other, a job with no rows reads none, and the emergency
//! path (every type of the category at release, narrowed after the trade question) reads back as
//! the settled type. All codes are fake.

use std::time::Duration;

use askhava_integration::{is_permission_denied, set_app_role};
use askhava_jobs::api::{FactsError, Recorded, create, job_facts, record_job_facts};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

/// A pooled connection already running as the app role.
async fn app_conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// Creates a job now (an emergency one when `emergency`) and returns its ID.
async fn new_job(conn: &mut PgConnection, emergency: bool) -> Uuid {
    create(conn, Timestamp::now(), emergency, Actor::System)
        .await
        .expect("create")
}

/// Owned codes from string literals.
fn codes(list: &[&str]) -> Vec<String> {
    list.iter().map(|c| (*c).to_owned()).collect()
}

/// How many fact rows job `job` has.
async fn rows(conn: &mut PgConnection, job: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs.job_facts WHERE job_id = $1")
        .bind(job)
        .fetch_one(conn)
        .await
        .expect("count")
}

/// TD-JOB-17: first row, same set again, a narrowing row read back (INT-018).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_17_first_row_then_idempotent_then_narrowed(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let job = new_job(&mut conn, false).await;
    let all = codes(&["fake_leak", "fake_drain", "fake_heater"]);
    let first = record_job_facts(&mut conn, job, "fake_plumbing", &all).await;
    assert_eq!(first, Ok(Recorded::Written));
    let again = codes(&["fake_heater", "fake_leak", "fake_drain", "fake_leak"]);
    let same = record_job_facts(&mut conn, job, "fake_plumbing", &again).await;
    assert_eq!(same, Ok(Recorded::Unchanged), "same set in any order");
    assert_eq!(rows(&mut conn, job).await, 1);
    let two = codes(&["fake_leak", "fake_drain"]);
    let narrowed = record_job_facts(&mut conn, job, "fake_plumbing", &two).await;
    assert_eq!(narrowed, Ok(Recorded::Written));
    let latest = job_facts(&mut conn, job).await.expect("read").expect("row");
    assert_eq!((latest.seq, latest.category.as_str()), (2, "fake_plumbing"));
    assert_eq!(latest.job_types, codes(&["fake_drain", "fake_leak"]));
}

/// TD-JOB-17: another category, a code not in the latest row, an empty set or blank codes are
/// refused and write nothing; an unknown job is not found (INT-018).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_17_anything_but_a_narrowing_is_refused(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let job = new_job(&mut conn, false).await;
    let two = codes(&["fake_leak", "fake_drain"]);
    let first = record_job_facts(&mut conn, job, "fake_plumbing", &two).await;
    assert_eq!(first, Ok(Recorded::Written));
    let cases = [
        (
            "fake_roofing",
            codes(&["fake_leak"]),
            FactsError::OtherCategory,
        ),
        (
            "fake_plumbing",
            codes(&["fake_leak", "fake_heater"]),
            FactsError::NotASubset,
        ),
        ("fake_plumbing", Vec::new(), FactsError::Empty),
        ("fake_plumbing", codes(&[" "]), FactsError::Empty),
        (" ", codes(&["fake_leak"]), FactsError::Empty),
    ];
    for (category, set, refusal) in cases {
        let outcome = record_job_facts(&mut conn, job, category, &set).await;
        assert_eq!(outcome, Err(refusal), "{category} {set:?}");
    }
    assert_eq!(rows(&mut conn, job).await, 1, "nothing written");
    let unknown = record_job_facts(&mut conn, Uuid::new_v4(), "fake_plumbing", &two).await;
    assert_eq!(unknown, Err(FactsError::NotFound));
}

/// TD-JOB-17: a job with no rows reads none, and the app role cannot change or delete a row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_17_no_rows_reads_none_and_rows_never_change(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let job = new_job(&mut conn, false).await;
    assert_eq!(job_facts(&mut conn, job).await, Ok(None));
    let set = codes(&["fake_leak"]);
    let first = record_job_facts(&mut conn, job, "fake_plumbing", &set).await;
    assert_eq!(first, Ok(Recorded::Written));
    for sql in [
        "UPDATE jobs.job_facts SET category = 'fake_roofing'",
        "DELETE FROM jobs.job_facts",
        "TRUNCATE jobs.job_facts",
    ] {
        let outcome = sqlx::query(sql).execute(&mut *conn).await;
        assert!(is_permission_denied(&outcome), "{sql}: {outcome:?}");
    }
    assert_eq!(rows(&mut conn, job).await, 1);
}

/// TD-JOB-17: a second narrowing waits on the first's row lock, then narrows its result, so both
/// rows are written in turn with no sequence clash (INT-018, D-2330).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_17_two_narrowings_at_once_apply_in_turn(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let job = new_job(&mut conn, false).await;
    let all = codes(&["fake_leak", "fake_drain", "fake_heater"]);
    let first = record_job_facts(&mut conn, job, "fake_plumbing", &all).await;
    assert_eq!(first, Ok(Recorded::Written));
    let mut tx = conn.begin().await.expect("begin");
    let two = codes(&["fake_leak", "fake_drain"]);
    let held = record_job_facts(&mut tx, job, "fake_plumbing", &two).await;
    assert_eq!(held, Ok(Recorded::Written));
    let racer = tokio::spawn({
        let pool = pool.clone();
        async move {
            let mut other = app_conn(&pool).await;
            let one = codes(&["fake_leak"]);
            record_job_facts(&mut other, job, "fake_plumbing", &one).await
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !racer.is_finished(),
        "the second narrowing waits on the lock"
    );
    tx.commit().await.expect("commit");
    assert_eq!(racer.await.expect("join"), Ok(Recorded::Written));
    let latest = job_facts(&mut conn, job).await.expect("read").expect("row");
    assert_eq!((latest.seq, latest.job_types), (3, codes(&["fake_leak"])));
}

/// TD-JOB-17: emergency path: every type of the category at release, narrowed after the trade
/// question, reads back as the settled type (INT-018, INT-029).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_17_emergency_row_narrowed_after_the_trade_question(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let job = new_job(&mut conn, true).await;
    let every = codes(&["fake_leak", "fake_drain", "fake_heater", "fake_burst"]);
    let release = record_job_facts(&mut conn, job, "fake_plumbing", &every).await;
    assert_eq!(release, Ok(Recorded::Written));
    let settled = codes(&["fake_burst"]);
    let answer = record_job_facts(&mut conn, job, "fake_plumbing", &settled).await;
    assert_eq!(answer, Ok(Recorded::Written));
    let latest = job_facts(&mut conn, job).await.expect("read").expect("row");
    assert_eq!(latest.category, "fake_plumbing");
    assert_eq!(latest.job_types, settled);
}
