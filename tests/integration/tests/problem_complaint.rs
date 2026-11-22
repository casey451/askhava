//! TD-BIZ-58 (T721, D-4423; JOB-016, BIZ-012, BIZ-011, BIZ-029, OPS-005): Casey counts a check-in
//! PROBLEM report as a complaint, once per job, on PostgreSQL with fake data only.
//!
//! Each PROBLEM goes through `jobs::api::answer_check_in` on a Connected job, its scrubbed words kept
//! by `privacy::api::keep_problem_words`; the count goes through
//! `portals::api::count_problem_as_complaint` with an owner session proof. The proof comes from
//! the composition root's `session_minter`, so `app/server/src/lib.rs` is compiled in place as
//! `pick_ended.rs` does (D-2853's exception, logged for this file in D-5377); nothing here builds a
//! release mint or a vendor client. Every word is fictional.

use askhava_businesses::api::{BOX_CLOSED_KIND, Counted, PROBLEM_COUNTED_ACTION, has_safety_word};
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{Answer, Inbound, JobStatus, answer_check_in, create, job_number, move_to};
use askhava_ops::api::{Actor, NewAlert, Severity, raise_alert};
use askhava_portals::api::{CountRefused, count_problem_as_complaint, problem_reports};
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, OwnerSessionProof, cut_problem_words, keep_problem_words, scrub,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate
/// for its `session_minter` only.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the session minter is called here")]
mod server;
use server::*;

/// Casey's fake owner record ID.
const OWNER: Uuid = Uuid::from_u128(0x721);
const WORDS: &str = "the price went way up after the work and the sink still drips";
const SAFE_WORDS: &str = "the worker threatened me when I asked about the bill";

fn days(d: i64) -> SignedDuration {
    SignedDuration::from_hours(d * 24)
}

fn proof() -> OwnerSessionProof {
    session_minter().owner(OWNER)
}

/// An active fake business.
async fn business(db: &mut PgConnection, now: Timestamp) -> Uuid {
    let id = askhava_businesses::api::create(db, false, now - days(60)).await;
    let id = id.expect("business");
    sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("active");
    id
}

/// A job walked to Connected 5 days before `now`.
async fn connected(db: &mut PgConnection, now: Timestamp) -> Uuid {
    let at = now - days(5);
    let job = create(db, at, false, Actor::System).await.expect("job");
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    for w in path.windows(2) {
        let moved = move_to(db, job, (w[0], w[1]), at, Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    job
}

/// A pick of `business` on `job` in `status`, picked at `at`.
async fn pick(db: &mut PgConnection, (job, business): (Uuid, Uuid), status: &str, at: Timestamp) {
    let sql = "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, \
               picked_at) VALUES ($1, $2, 1, 'page', $3, $4)";
    sqlx::query(sql)
        .bind(job)
        .bind(business)
        .bind(status)
        .bind(at.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("pick");
}

/// `words` as the inbound route scrubs them.
fn scrubbed(words: &str) -> String {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(words, &known, &mut NameMemory::default())
        .as_str()
        .to_owned()
}

/// Keeps `words` for a new inbox row on `job`; returns the inbox row.
async fn keep(db: &mut PgConnection, job: Uuid, words: &str) -> Uuid {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(words, &known, &mut NameMemory::default());
    let inbox = Uuid::new_v4();
    let kept = keep_problem_words(db, inbox, job, &scrubbed, has_safety_word(words)).await;
    assert!(kept.expect("kept"));
    inbox
}

/// The alert naming `inbox`.
async fn alert_of(db: &mut PgConnection, inbox: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM ops.alert WHERE subject_id = $1")
        .bind(inbox)
        .fetch_one(&mut *db)
        .await
        .expect("alert")
}

/// A day-3 PROBLEM with `words` on Connected `job` at `now`, through `answer_check_in`; its alert.
async fn problem(db: &mut PgConnection, job: Uuid, words: &str, now: Timestamp) -> Uuid {
    let inbox = keep(db, job, words).await;
    let how = (Answer::Problem, CheckIn::Day3, Contact::Email);
    let inbound = Inbound {
        inbox,
        safety: has_safety_word(words),
    };
    let answered = answer_check_in(db, job, how, inbound, (now, Actor::System)).await;
    assert!(answered.expect("answer").applied().is_some());
    alert_of(db, inbox).await
}

/// A second PROBLEM alert on `job` (another kept text), raised as `answer_check_in` raises one.
async fn another_problem(db: &mut PgConnection, job: Uuid) -> Uuid {
    let inbox = keep(db, job, WORDS).await;
    let new = NewAlert {
        kind: askhava_jobs::api::PROBLEM_ALERT,
        severity: Severity::Normal,
        subject: Some(("messaging.inbox", inbox)),
    };
    raise_alert(db, &new).await.expect("alert")
}

async fn count(db: &mut PgConnection, alert: Uuid, kind: &str, now: Timestamp) -> Counted {
    let counted = count_problem_as_complaint(db, &proof(), (alert, kind), (OWNER, now)).await;
    counted.expect("counted")
}

async fn refused(db: &mut PgConnection, alert: Uuid, kind: &str, now: Timestamp) -> CountRefused {
    let counted = count_problem_as_complaint(db, &proof(), (alert, kind), (OWNER, now)).await;
    counted.expect_err("refused")
}

/// `(kind, safety)` of each complaint against `business`.
async fn complaints(pool: &PgPool, business: Uuid) -> Vec<(String, bool)> {
    let sql = "SELECT kind, safety FROM businesses.complaint WHERE business_id = $1 \
               ORDER BY received_at";
    sqlx::query_as(sql)
        .bind(business)
        .fetch_all(pool)
        .await
        .expect("complaints")
}

async fn scalar(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn written(pool: &PgPool) -> (i64, i64) {
    let rows = scalar(pool, "SELECT count(*) FROM businesses.complaint").await;
    let sql = "SELECT count(*) FROM ops.audit_log WHERE action = 'businesses.problem_counted'";
    (rows, scalar(pool, sql).await)
}

async fn status(pool: &PgPool, business: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM businesses.business WHERE id = $1")
        .bind(business)
        .fetch_one(pool)
        .await
        .expect("status")
}

/// A Connected job with a paid pick of `business` and a day-3 PROBLEM of `words`; its alert.
async fn reported(db: &mut PgConnection, business: Uuid, words: &str, now: Timestamp) -> Uuid {
    let job = connected(db, now).await;
    pick(db, (job, business), "paid", now - days(5)).await;
    problem(db, job, words, now).await
}

/// TD-BIZ-58 (JOB-016, BIZ-012): three PROBLEM reports within 90 days are no complaint and leave
/// the business active; counted, the third pauses it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_problems_count_only_when_casey_counts_them(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let firm = business(&mut db, now).await;
    let mut alerts = Vec::new();
    for _ in 0..3 {
        alerts.push(reported(&mut db, firm, WORDS, now).await);
    }
    assert_eq!(written(&pool).await, (0, 0));
    assert_eq!(status(&pool, firm).await, "active");
    let mut paused = Vec::new();
    for a in alerts {
        let Counted::Recorded(r) = count(&mut db, a, "work_or_price", now).await else {
            panic!("recorded");
        };
        paused.push(r.paused);
    }
    assert_eq!(paused, [false, false, true]);
    assert_eq!(status(&pool, firm).await, "paused");
    assert_eq!(written(&pool).await, (3, 3));
}

/// TD-BIZ-58 (JOB-016, BIZ-011, BIZ-029, SEC-006): one complaint of the kind, its reply-box task
/// and one owner audit row; again, or another PROBLEM alert of the same job, is `Already`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_one_complaint_per_job(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let firm = business(&mut db, now).await;
    let job = connected(&mut db, now).await;
    pick(&mut db, (job, firm), "paid", now - days(5)).await;
    let alert = problem(&mut db, job, WORDS, now).await;
    let Counted::Recorded(r) = count(&mut db, alert, "work_or_price", now).await else {
        panic!("recorded");
    };
    assert_eq!(
        complaints(&pool, firm).await,
        [("work_or_price".to_owned(), false)]
    );
    let sql = "SELECT count(*) FROM core.task WHERE kind = $1 AND payload->>'complaint' = $2";
    let tasks: i64 = sqlx::query_scalar(sql)
        .bind(BOX_CLOSED_KIND)
        .bind(r.id.to_string())
        .fetch_one(&pool)
        .await
        .expect("task");
    assert_eq!(tasks, 1);
    let sql = "SELECT actor_kind, actor_id, subject_id FROM ops.audit_log WHERE action = $1";
    let audit: Vec<(String, Option<Uuid>, Uuid)> = sqlx::query_as(sql)
        .bind(PROBLEM_COUNTED_ACTION)
        .fetch_all(&pool)
        .await
        .expect("audit");
    assert_eq!(audit, [("owner".to_owned(), Some(OWNER), r.id)]);
    assert_eq!(
        count(&mut db, alert, "conduct", now).await,
        Counted::Already
    );
    let second = another_problem(&mut db, job).await;
    assert_eq!(count(&mut db, second, "other", now).await, Counted::Already);
    assert_eq!(written(&pool).await, (1, 1));
}

/// TD-BIZ-58 (JOB-016, D-4423): after the pick became `no_show`, the count goes to its business;
/// a newer fraud-refunded pick of another business is never the business of record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_a_no_show_pick_names_its_business(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let (firm, other) = (business(&mut db, now).await, business(&mut db, now).await);
    let job = connected(&mut db, now).await;
    pick(&mut db, (job, firm), "paid", now - days(5)).await;
    let alert = problem(&mut db, job, WORDS, now).await;
    sqlx::query("UPDATE picks.pick SET status = 'no_show', ended_at = $2 WHERE job_id = $1")
        .bind(job)
        .bind(now.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("no-show");
    pick(&mut db, (job, other), "fraud_refunded", now - days(4)).await;
    let counted = count(&mut db, alert, "no_show", now).await;
    assert!(matches!(counted, Counted::Recorded(_)));
    assert_eq!(
        complaints(&pool, firm).await,
        [("no_show".to_owned(), false)]
    );
    assert!(complaints(&pool, other).await.is_empty());
}

/// TD-BIZ-58 (JOB-016): two counts of one alert at once record one complaint.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_two_counts_at_once_record_one(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let firm = business(&mut db, now).await;
    let alert = reported(&mut db, firm, WORDS, now).await;
    let (mut a, mut b) = (
        pool.acquire().await.expect("a"),
        pool.acquire().await.expect("b"),
    );
    let (x, y) = tokio::join!(
        count(&mut a, alert, "conduct", now),
        count(&mut b, alert, "conduct", now)
    );
    let recorded = [x, y]
        .iter()
        .filter(|c| matches!(c, Counted::Recorded(_)))
        .count();
    assert_eq!(recorded, 1);
    assert_eq!(written(&pool).await, (1, 1));
}

/// TD-BIZ-58 (OPS-005): a safety PROBLEM is counted the same way and its complaint carries the
/// safety mark.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_a_safety_problem_is_counted_with_its_mark(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let firm = business(&mut db, now).await;
    let alert = reported(&mut db, firm, SAFE_WORDS, now).await;
    let kind: String = sqlx::query_scalar("SELECT kind FROM ops.alert WHERE id = $1")
        .bind(alert)
        .fetch_one(&pool)
        .await
        .expect("kind");
    assert_eq!(kind, askhava_jobs::api::PROBLEM_SAFETY_ALERT);
    let Counted::Recorded(r) = count(&mut db, alert, "safety", now).await else {
        panic!("recorded");
    };
    assert!(r.safety);
    assert_eq!(complaints(&pool, firm).await, [("safety".to_owned(), true)]);
}

/// TD-BIZ-58 (JOB-016, TXT-020): another alert kind, an unknown alert, a kind outside the five,
/// words cut after 30 days and a job with no settled pick are refused and write nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_refusals_write_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let firm = business(&mut db, now).await;
    let alert = reported(&mut db, firm, WORDS, now).await;
    let new = NewAlert {
        kind: "jobs.something_else",
        severity: Severity::Normal,
        subject: Some(("messaging.inbox", Uuid::new_v4())),
    };
    let other = raise_alert(&mut db, &new).await.expect("other alert");
    assert_eq!(
        refused(&mut db, other, "other", now).await,
        CountRefused::NotProblem
    );
    let unknown = Uuid::new_v4();
    assert_eq!(
        refused(&mut db, unknown, "other", now).await,
        CountRefused::NotProblem
    );
    assert_eq!(
        refused(&mut db, alert, "rude", now).await,
        CountRefused::BadKind
    );
    let unpicked = connected(&mut db, now).await;
    let lone = problem(&mut db, unpicked, WORDS, now).await;
    assert_eq!(
        refused(&mut db, lone, "other", now).await,
        CountRefused::NoBusiness
    );
    sqlx::query("UPDATE privacy.message_body SET created_at = $1")
        .bind((now - days(31)).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("age");
    cut_problem_words(&mut db, now).await.expect("cut");
    assert_eq!(
        refused(&mut db, alert, "other", now).await,
        CountRefused::WordsGone
    );
    assert_eq!(written(&pool).await, (0, 0));
}

/// TD-BIZ-58 (JOB-016, PRIV-006): the last 30 days' PROBLEM alerts, newest first, with job number,
/// kept words, safety mark and counted mark; one raised earlier and resolved inside is not listed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_58_problem_reports_list_the_last_30_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let firm = business(&mut db, now).await;
    let old = reported(&mut db, firm, WORDS, now).await;
    sqlx::query("UPDATE ops.alert SET raised_at = $2, resolved_at = $3 WHERE id = $1")
        .bind(old)
        .bind((now - days(40)).to_sqlx())
        .bind((now - days(5)).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("old");
    let plain = reported(&mut db, firm, WORDS, now).await;
    let safe = reported(&mut db, firm, SAFE_WORDS, now).await;
    count(&mut db, plain, "work_or_price", now).await;
    let later = now + SignedDuration::from_secs(1);
    let listed = problem_reports(&mut db, &proof(), later)
        .await
        .expect("list");
    let alerts: Vec<Uuid> = listed.iter().map(|r| r.alert).collect();
    assert_eq!(alerts, [safe, plain]);
    let marks: Vec<(bool, bool)> = listed.iter().map(|r| (r.safety, r.counted)).collect();
    assert_eq!(marks, [(true, false), (false, true)]);
    assert_eq!(listed[1].words, scrubbed(WORDS));
    for r in &listed {
        assert!(!format!("{r:?}").contains(&r.words));
        assert!(r.number > 0);
    }
    let sql = "SELECT m.job_id FROM privacy.message_body m \
               JOIN ops.alert a ON a.subject_id = m.inbox_id WHERE a.id = $1";
    let job: Uuid = sqlx::query_scalar(sql)
        .bind(plain)
        .fetch_one(&pool)
        .await
        .expect("job");
    assert_eq!(
        listed[1].number,
        job_number(&mut db, job).await.expect("number")
    );
}
