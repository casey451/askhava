//! OPS-046's first-six-weeks email figures and the daily deferral check (TD-OPS-21's OPS-046 cases;
//! D-490, D-3883, T531). Every row is planted: fake provider IDs, random recipient IDs, no address.
//! Each figure comes from planted outbox and event rows; 5% deferrals raise nothing and 6% one
//! alert; the daily task raises once a day, the clock only ever moving forward.

use std::sync::Arc;

use askhava_core::api::{Clock, Lease, ManualClock, Outcome, Registry, WorkerId, claim};
use askhava_messaging::api::{
    DEFERRAL_ALERT, DEFERRAL_CHECK_KIND, EmailDay, WINDOW_DAYS, check_deferrals, email_days,
    register_deferral_check, schedule_deferral_check,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::{PgConnection, PgPool};

const DAY: SignedDuration = SignedDuration::from_hours(24);

/// Arizona midnight (07:00 UTC) of the fake first sending day, plus `days` days and `hours` hours.
fn at(days: i64, hours: i64) -> Timestamp {
    let start: Timestamp = "2026-09-01T07:00:00Z".parse().expect("time");
    start + DAY * i32::try_from(days).expect("days") + SignedDuration::from_hours(hours)
}

/// Plants `count` sent emails at `when`, provider IDs `fake-<tag>-<n>`.
async fn sends(db: &mut PgConnection, tag: &str, count: i32, when: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id,
           recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at)
         SELECT md5($1 || n) || md5(n || $1), 'fake_template', gen_random_uuid(), 'business',
           '{}', 'notice', 'sent', $3, 'fake-' || $1 || '-' || n, $3, $3
         FROM generate_series(1, $2) AS n",
    )
    .bind(tag)
    .bind(count)
    .bind(when.to_sqlx())
    .execute(db)
    .await
    .expect("sends");
}

/// Plants one provider event of `kind` for provider ID `fake-<tag>-<n>` at `when`.
async fn event(db: &mut PgConnection, tag: &str, n: i32, kind: &str, when: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at)
         VALUES ('fake-' || $1 || '-' || $2, $3, $4, $4)",
    )
    .bind(tag)
    .bind(n.to_string())
    .bind(kind)
    .bind(when.to_sqlx())
    .execute(db)
    .await
    .expect("event");
}

/// Plants `count` deferral events on `tag`'s emails 1 to `count` at `when`.
async fn deferrals(db: &mut PgConnection, tag: &str, count: i32, when: Timestamp) {
    for n in 1..=count {
        event(db, tag, n, "email.delivery_delayed", when).await;
    }
}

async fn alert_count(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(DEFERRAL_ALERT)
        .fetch_one(db)
        .await
        .expect("alerts")
}

fn day(days: &[EmailDay], index: usize) -> (i64, i64, i64, i64) {
    let d = days[index];
    (d.sends, d.deferrals, d.bounces, d.complaints)
}

/// TD-OPS-21 (OPS-046): each daily figure comes from its planted rows, a provider retrying one
/// email counts once, a quiet day is zeros, and the window ends after 6 weeks.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_21_each_daily_figure_comes_from_planted_rows(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    assert!(
        email_days(&mut db, at(1, 0))
            .await
            .expect("none")
            .is_empty()
    );
    sends(&mut db, "a", 4, at(0, 1)).await;
    deferrals(&mut db, "a", 1, at(0, 2)).await;
    event(&mut db, "a", 1, "email.delivery_delayed", at(0, 3)).await;
    event(&mut db, "a", 2, "email.bounced", at(0, 2)).await;
    event(&mut db, "a", 3, "email.complained", at(0, 4)).await;
    event(&mut db, "a", 4, "email.delivered", at(0, 2)).await;
    sends(&mut db, "b", 2, at(2, 23)).await;
    event(&mut db, "b", 1, "email.bounced", at(3, 1)).await;
    sends(&mut db, "late", 9, at(42, 1)).await;
    let days = email_days(&mut db, at(2, 5)).await.expect("days");
    assert_eq!(days.len(), 3, "the first send's day up to today");
    assert_eq!(day(&days, 0), (4, 1, 1, 1));
    assert_eq!(day(&days, 1), (0, 0, 0, 0), "a quiet day is zeros");
    assert_eq!(day(&days, 2), (2, 0, 0, 0));
    assert_eq!(days[0].day.to_string(), "2026-09-01");
    let days = email_days(&mut db, at(60, 0)).await.expect("days");
    assert_eq!(days.len(), usize::try_from(WINDOW_DAYS).expect("len"));
    assert_eq!(
        day(&days, 3),
        (0, 0, 1, 0),
        "an event counts on its own day"
    );
    assert_eq!(days[41].day.to_string(), "2026-10-12");
    assert_eq!(day(&days, 41), (0, 0, 0, 0), "day 43's sends are outside");
}

/// TD-OPS-21 (OPS-046): 5 deferrals in 100 sends raise nothing; a sixth raises one High alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_21_five_percent_raises_nothing_and_six_one_alert(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    sends(&mut db, "a", 100, at(0, 1)).await;
    deferrals(&mut db, "a", 5, at(0, 2)).await;
    assert_eq!(
        check_deferrals(&mut db, at(1, 6)).await.expect("check"),
        None
    );
    assert_eq!(alert_count(&mut db).await, 0);
    event(&mut db, "a", 6, "email.delivery_delayed", at(0, 3)).await;
    assert_eq!(
        check_deferrals(&mut db, at(0, 20)).await.expect("today"),
        None
    );
    assert!(
        check_deferrals(&mut db, at(1, 6))
            .await
            .expect("check")
            .is_some()
    );
    let severity: String = sqlx::query_scalar("SELECT severity FROM ops.alert WHERE kind = $1")
        .bind(DEFERRAL_ALERT)
        .fetch_one(&mut *db)
        .await
        .expect("severity");
    assert_eq!(severity, "high");
}

/// Claims and runs every task due on `clock`; returns how many ran.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) -> usize {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, clock.now(), Lease::WORKER.length, 10)
        .await
        .expect("claim");
    let ran = tasks.len();
    for task in tasks {
        let outcome = registry.run(pool, clock, owner, task, Lease::WORKER).await;
        assert!(matches!(outcome, Ok(Outcome::Done)), "{outcome:?}");
    }
    ran
}

/// TD-OPS-21 (OPS-046, D-3883): the daily task queues its next run and alerts once a day; the
/// clock only moves forward, and the earlier alert's Arizona day is moved back a day to stand
/// for a new database day (`raise_alert_once_a_day` reads the database clock).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_21_the_daily_check_alerts_once_a_day(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(at(0, 0)));
    let mut registry = Registry::new();
    register_deferral_check(&mut registry, Arc::clone(&clock) as Arc<dyn Clock>);
    let mut db = pool.acquire().await.expect("conn");
    sends(&mut db, "a", 50, at(0, 1)).await;
    deferrals(&mut db, "a", 3, at(0, 2)).await;
    schedule_deferral_check(&mut db, clock.now())
        .await
        .expect("first");
    clock.advance(DAY + SignedDuration::from_hours(1));
    assert_eq!(run_due(&pool, &registry, &clock).await, 1);
    assert_eq!(alert_count(&mut db).await, 1, "6% of day 0");
    sends(&mut db, "b", 50, at(1, 2)).await;
    deferrals(&mut db, "b", 4, at(1, 3)).await;
    clock.advance(DAY);
    assert_eq!(run_due(&pool, &registry, &clock).await, 1);
    assert_eq!(alert_count(&mut db).await, 1, "one alert a day");
    sqlx::query("UPDATE ops.alert SET raised_at = raised_at - interval '1 day' WHERE kind = $1")
        .bind(DEFERRAL_ALERT)
        .execute(&mut *db)
        .await
        .expect("an earlier day");
    sends(&mut db, "c", 50, at(2, 2)).await;
    deferrals(&mut db, "c", 3, at(2, 3)).await;
    clock.advance(DAY);
    assert_eq!(run_due(&pool, &registry, &clock).await, 1);
    assert_eq!(alert_count(&mut db).await, 2, "a new day alerts again");
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND done_at IS NULL AND dead_at IS NULL",
    )
    .bind(DEFERRAL_CHECK_KIND)
    .fetch_one(&mut *db)
    .await
    .expect("next run");
    assert_eq!(queued, 1, "the next day's check is queued");
}
