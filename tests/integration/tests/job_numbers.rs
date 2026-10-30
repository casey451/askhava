//! Job numbers on a fresh database: a number texted out is never reused, including after a restore
//! (JOB-024; TD-JOB-06, D-851).
//!
//! Proves that every created job gets its own number from 1001 up, kept after it is cancelled;
//! that concurrent creates never share one; that the app role can neither rewrite nor delete an
//! issued number nor insert one twice; and that after a simulated restore (the lost window's jobs
//! gone and the counter rolled back to the backup) the next job would reuse a texted number until
//! the replay raises the floor, after which every new number is higher than any texted one and a
//! lower floor never moves it back. All data is fake.

use askhava_integration::{is_permission_denied, set_app_role};
use askhava_jobs::api::{JobStatus, create, job_number, move_to, raise_number_floor};
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

/// Creates a job now and returns its ID and number.
async fn new_job(conn: &mut PgConnection) -> (Uuid, i64) {
    let id = create(conn, Timestamp::now(), false, Actor::System)
        .await
        .expect("create");
    (id, job_number(conn, id).await.expect("number"))
}

/// TD-JOB-06: numbers start at 1001, rise by one and stay with a cancelled job (JOB-024).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_06_each_job_keeps_its_own_number(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let (first, n1) = new_job(&mut conn).await;
    assert_eq!(n1, 1001);
    let step = (JobStatus::Intake, JobStatus::Cancelled);
    let cancelled = move_to(&mut conn, first, step, Timestamp::now(), Actor::System).await;
    assert!(cancelled.expect("cancel").applied().is_some(), "cancelled");
    assert_eq!(
        job_number(&mut conn, first).await,
        Ok(1001),
        "kept after cancel"
    );
    let (_, n2) = new_job(&mut conn).await;
    assert_eq!(n2, 1002);
}

/// TD-JOB-06: twenty concurrent creates get twenty different numbers (JOB-024).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_06_concurrent_creates_never_share_a_number(pool: PgPool) {
    let tasks: Vec<_> = (0..20)
        .map(|_| {
            let pool = pool.clone();
            tokio::spawn(async move {
                let mut conn = app_conn(&pool).await;
                new_job(&mut conn).await.1
            })
        })
        .collect();
    let mut numbers = Vec::new();
    for task in tasks {
        numbers.push(task.await.expect("join"));
    }
    numbers.sort_unstable();
    assert_eq!(numbers, (1001..=1020).collect::<Vec<i64>>());
}

/// TD-JOB-06: the app role cannot rewrite, delete or re-issue a number (JOB-024, D-092).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_06_the_app_role_cannot_rewrite_or_reissue_a_number(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    let (_, issued) = new_job(&mut conn).await;
    for sql in [
        "UPDATE jobs.job_number SET number = number + 100",
        "DELETE FROM jobs.job_number",
        "TRUNCATE jobs.job_number",
        "DELETE FROM jobs.job_number_next",
    ] {
        let outcome = sqlx::query(sql).execute(&mut *conn).await;
        assert!(is_permission_denied(&outcome), "{sql}: {outcome:?}");
    }
    sqlx::query("UPDATE jobs.job_number_next SET next = $1")
        .bind(issued)
        .execute(&mut *conn)
        .await
        .expect("lower the counter");
    let again = create(&mut conn, Timestamp::now(), false, Actor::System).await;
    assert!(again.is_err(), "an issued number is refused, never reused");
}

/// Simulates a restore from a backup taken before `lost`: their numbers and jobs are gone and
/// the counter is back at `next`, as the backup held it. Runs as the migration identity.
async fn restore_to_backup(pool: &PgPool, lost: &[Uuid], next: i64) {
    let mut tx = pool.begin().await.expect("begin");
    for sql in [
        "DELETE FROM jobs.job_number WHERE job_id = ANY($1)",
        "DELETE FROM jobs.job WHERE id = ANY($1)",
    ] {
        sqlx::query(sql)
            .bind(lost)
            .execute(&mut *tx)
            .await
            .expect("drop the lost window");
    }
    sqlx::query("UPDATE jobs.job_number_next SET next = $1")
        .bind(next)
        .execute(&mut *tx)
        .await
        .expect("roll the counter back");
    tx.commit().await.expect("commit");
}

/// The number the next job would get, without keeping it: the create is rolled back.
async fn would_get(conn: &mut PgConnection) -> i64 {
    let mut tx = conn.begin().await.expect("begin");
    let (_, number) = new_job(&mut tx).await;
    tx.rollback().await.expect("rollback");
    number
}

/// TD-JOB-06: after a restore, a number texted in the lost window is never issued again once the
/// replay raises the floor, and a lower floor never moves the counter back (JOB-024, OPS-015).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_06_a_texted_number_is_not_reused_after_a_restore(pool: PgPool) {
    let mut conn = app_conn(&pool).await;
    for _ in 0..3 {
        new_job(&mut conn).await;
    }
    let (a, texted_a) = new_job(&mut conn).await;
    let (b, texted_b) = new_job(&mut conn).await;
    assert_eq!((texted_a, texted_b), (1004, 1005));
    restore_to_backup(&pool, &[a, b], 1004).await;
    assert_eq!(
        would_get(&mut conn).await,
        1004,
        "the restore really rolled back"
    );
    raise_number_floor(&mut conn, texted_b)
        .await
        .expect("raise");
    assert_eq!(new_job(&mut conn).await.1, 1006);
    raise_number_floor(&mut conn, 1002)
        .await
        .expect("lower floor");
    assert_eq!(
        new_job(&mut conn).await.1,
        1007,
        "the counter never moves down"
    );
}
