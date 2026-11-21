//! The aftermath notices wired to their moments, in the database (TD-PICK-67; PICK-026,
//! PICK-038; D-043, D-044, D-1575, D-1576, D-4361). Fake IDs and amounts only.
//!
//! A job's card goes to fake businesses, some quote, one is picked and its $0 fee decision is
//! recorded through `settle`; the loss notice and "This job is no longer active." are the
//! `messaging.business_notice` tasks queued for the others, and a final move's
//! `jobs.final_status` task is run through `picks`' handler.

use askhava_businesses::api::{add_owner, create};
use askhava_core::api::{Claimed, Guarded, MIGRATOR, WorkerId, claim};
use askhava_jobs::api::{FINAL_STATUS_KIND, JobStatus, current, job_number, move_to};
use askhava_ops::api::Actor;
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, INACTIVE_TEMPLATE, PendingEnd, Refusal, Settled, end_pending,
    refused_at_confirm, run_final_status, settle,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

/// Fake business with an owner whose opaque mailbox ID is random.
async fn business(db: &mut PgConnection) -> Uuid {
    let business = create(db, false, at(0)).await.expect("business");
    add_owner(db, business, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    business
}

/// Moves `job` along `arrow` at `minute`; it must apply.
async fn moved(db: &mut PgConnection, job: Uuid, arrow: (JobStatus, JobStatus), minute: i64) {
    let applied = move_to(db, job, arrow, at(minute), Actor::System).await;
    assert!(matches!(applied, Ok(Guarded::Applied(()))), "move");
}

/// A job in Awaiting confirm whose card went to `sent` fake businesses, each quoting a firm
/// fake amount; the first business's pick waits for Confirm. Returns job, businesses, pick.
async fn waiting(db: &mut PgConnection, sent: i64) -> (Uuid, Vec<Uuid>, Uuid) {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    moved(db, job, (JobStatus::Intake, JobStatus::Open), 1).await;
    moved(db, job, (JobStatus::Open, JobStatus::QuotesReady), 1).await;
    moved(
        db,
        job,
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
        1,
    )
    .await;
    let mut businesses = Vec::new();
    for n in 0..sent {
        let b = business(db).await;
        sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
            .bind(job).bind(b).bind(at(2).to_sqlx())
            .execute(&mut *db).await.expect("card");
        quote(db, (job, b), 10_000 + n * 1_000, 3 + n).await;
        businesses.push(b);
    }
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(businesses[0])
    .bind(at(10).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    (job, businesses, pick)
}

/// A fake firm quote of `cents` by `business` on `job`, received at `minute`.
async fn quote(db: &mut PgConnection, (job, business): (Uuid, Uuid), cents: i64, minute: i64) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', $3, false, '', 'Fake quote', false, true, $4)",
    )
    .bind(job)
    .bind(business)
    .bind(cents)
    .bind(at(minute).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

/// Records the $0 fee decision for `pick` (a free first win) and settles it, committed.
async fn settled(pool: &PgPool, (job, business, pick): (Uuid, Uuid, Uuid)) -> Settled {
    let mut db = pool.acquire().await.expect("db");
    sqlx::query(
        "INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, \
         terms_version, fixed_at) VALUES ($1, $2, $3, 'tiers', 'fee_free', 0, 1, now()) \
         ON CONFLICT DO NOTHING",
    )
    .bind(pick)
    .bind(business)
    .bind(job)
    .execute(&mut *db)
    .await
    .expect("fee");
    let key = format!("t681.free.{job}.{business}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    drop(ledger_append(&mut db, &entry).await);
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let cleared = cleared.expect("cleared");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let done = settle(&mut tx, &cleared).await.expect("settle");
    tx.commit().await.expect("commit");
    done
}

/// The queued business notices of `job`: (business, template, payload).
async fn notices(pool: &PgPool, job: Uuid) -> Vec<(Uuid, String, Value)> {
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 ORDER BY id",
    )
    .bind(BUSINESS_NOTICE_KIND)
    .bind(job.to_string())
    .fetch_all(pool)
    .await
    .expect("tasks");
    rows.into_iter()
        .map(|(p,)| {
            let b = p["business"].as_str().and_then(|b| Uuid::parse_str(b).ok());
            let template = p["template"].as_str().expect("template").to_owned();
            (b.expect("business"), template, p)
        })
        .collect()
}

/// The businesses told "This job is no longer active." on `job`, in queue order.
async fn told_inactive(pool: &PgPool, job: Uuid) -> Vec<Uuid> {
    let got = notices(pool, job).await.into_iter();
    got.filter(|n| n.1 == INACTIVE_TEMPLATE)
        .map(|n| n.0)
        .collect()
}

/// Every `jobs.final_status` task queued for `job`, claimed.
async fn final_tasks(pool: &PgPool, job: Uuid) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    let claimed = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    let mine = |t: &Claimed| t.kind == FINAL_STATUS_KIND && t.payload["job"] == job.to_string();
    claimed.expect("claim").into_iter().filter(mine).collect()
}

/// TD-PICK-67 (PICK-026, PICK-038, D-1575): the settle that records the pick `free` queues one
/// loss notice to each other quoter, with the job number and the quote count only, and none to
/// the connected business; a second settle queues nothing more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_67_the_settle_tells_each_other_quoter_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, b, pick) = waiting(&mut db, 4).await;
    let number = job_number(&mut db, job).await.expect("number");
    drop(db);
    assert_eq!(settled(&pool, (job, b[0], pick)).await, Settled::Moved(job));
    assert_eq!(settled(&pool, (job, b[0], pick)).await, Settled::Was(job));
    let got = notices(&pool, job).await;
    let told: Vec<Uuid> = got.iter().map(|n| n.0).collect();
    assert_eq!(
        told,
        [b[1], b[2], b[3]],
        "each other quoter once, the picked none"
    );
    for (_, template, payload) in &got {
        assert!(
            template.starts_with("pick_lost"),
            "a loss notice: {template}"
        );
        let values = payload["values"].as_object().expect("values");
        let keys: Vec<&String> = values.keys().collect();
        assert_eq!(
            keys,
            ["count", "job"],
            "the job number and the quote count only"
        );
        assert_eq!(values["job"], number);
        assert_eq!(values["count"], 4);
    }
}

/// TD-PICK-67 (PICK-026, D-044, D-1576): a job moved to Done, Closed or Cancelled after the
/// settle queues one `jobs.final_status` task, whose handler tells each quoter (one quoting after
/// the pick too) but the connected one "This job is no longer active." once, run twice.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_67_a_final_move_tells_each_quoter_but_the_connected_once(pool: PgPool) {
    for end in [JobStatus::Done, JobStatus::Closed, JobStatus::Cancelled] {
        let mut db = pool.acquire().await.expect("db");
        let (job, mut b, pick) = waiting(&mut db, 3).await;
        drop(db);
        assert_eq!(settled(&pool, (job, b[0], pick)).await, Settled::Moved(job));
        let mut db = pool.acquire().await.expect("db");
        let late = business(&mut db).await;
        quote(&mut db, (job, late), 9_000, 40).await;
        b.push(late);
        assert!(told_inactive(&pool, job).await.is_empty(), "not final yet");
        moved(&mut db, job, (JobStatus::Connected, end), 50).await;
        drop(db);
        let tasks = final_tasks(&pool, job).await;
        assert_eq!(tasks.len(), 1, "one final-status task");
        assert_eq!(tasks[0].payload["status"], end.code());
        for _ in 0..2 {
            run_final_status(pool.clone(), tasks[0].clone())
                .await
                .expect("handled");
        }
        assert_eq!(told_inactive(&pool, job).await, [b[1], b[2], b[3]]);
    }
}

/// TD-PICK-67 (PICK-026, D-044): Closed early is final too: its move queues the one task.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_67_closed_early_queues_the_final_task(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = askhava_jobs::api::create(&mut db, at(0), false, Actor::System)
        .await
        .expect("job");
    moved(&mut db, job, (JobStatus::Intake, JobStatus::ClosedEarly), 1).await;
    drop(db);
    let tasks = final_tasks(&pool, job).await;
    assert_eq!(tasks.len(), 1, "one final-status task");
    run_final_status(pool.clone(), tasks[0].clone())
        .await
        .expect("handled");
    assert!(told_inactive(&pool, job).await.is_empty(), "nobody quoted");
}

/// TD-PICK-67 (PICK-026, D-043, D-044): a quote refused at Confirm (a license refusal) gets "This
/// job is no longer active." at once and never again at the final move; another quoter gets it
/// once at the final move.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_67_a_quote_refused_at_confirm_hears_at_once_and_never_again(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, b, pick) = waiting(&mut db, 2).await;
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let by = (Actor::System, at(20));
    let refused = refused_at_confirm(&mut tx, (pick, Refusal::License), false, by).await;
    tx.commit().await.expect("commit");
    refused.expect("refused");
    assert_eq!(told_inactive(&pool, job).await, [b[0]], "at once");
    let mut db = pool.acquire().await.expect("db");
    let now = current(&mut db, job).await.expect("job").status;
    moved(&mut db, job, (now, JobStatus::Cancelled), 30).await;
    drop(db);
    let tasks = final_tasks(&pool, job).await;
    run_final_status(pool.clone(), tasks[0].clone())
        .await
        .expect("handled");
    assert_eq!(told_inactive(&pool, job).await, [b[0], b[1]], "never twice");
}

/// TD-PICK-67 (PICK-026, D-043): a waiting pick ended `ineligible` in Payment pending tells its
/// business "This job is no longer active." at once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_67_an_ineligible_end_hears_at_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, b, pick) = waiting(&mut db, 2).await;
    let arrow = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    moved(&mut db, job, arrow, 15).await;
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let by = (Actor::System, at(20));
    let ended = end_pending(&mut tx, (pick, job), PendingEnd::Ineligible, by).await;
    tx.commit().await.expect("commit");
    ended.expect("ended");
    assert_eq!(told_inactive(&pool, job).await, [b[0]]);
}
