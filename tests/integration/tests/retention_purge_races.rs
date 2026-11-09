//! The contact purge against concurrent requests, holds and hold-producing tasks on PostgreSQL
//! (TD-PRIV-18; PRIV-007, PRIV-008, PRIV-010; second audit B03, D-1839).
//!
//! The purge decides and clears in one transaction under the retention lock order: the retention
//! advisory key (exclusive for the purge, shared for each hold placement and release), then the
//! homeowner row, then the vault contact row. Each barrier test holds one of those locks in a test
//! transaction, waits until `pg_locks` shows the purge (or a writer) waiting on it, then commits,
//! so the race the audit found (a request or hold committed between the old eligibility read and
//! the clear) is forced on every run. Both purge paths are run: a job's contact stage
//! (`run_purge`) and a request that never got a job (`run_request_purge`). The job text
//! (PRIV-008) and the ledger and audit records (PRIV-010) are never touched by the contact purge;
//! the last case checks the audit row count stays one per clear. Every handler runs as
//! `askhava_app`. Every name, phone, email and address is fake.

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{TextIn, receive_text, run_purge, run_request_purge};
use askhava_integration::set_app_role;
use askhava_privacy::api::{
    HoldSource, NewContact, PURGE_KIND, Vault, find_by_phone, place_hold, release_hold,
    store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{Acquire, PgConnection, PgPool};
use std::time::Duration;

const JOB: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_0274);
const JOB_B: Uuid = Uuid::from_u128(0x0000_7e57_0000_0000_0000_0000_0000_274b);
const HOMEOWNER: Uuid = Uuid::from_u128(0x274a);
const PHONE: &str = "(928) 555-0174";

/// The purge path under test.
#[derive(Debug, Clone, Copy)]
enum Path {
    /// `jobs`' contact stage for [`JOB`], handed on as `privacy.purge`.
    Job,
    /// `intake.request_purge` for a request that never got a job.
    Request(Uuid),
}

fn vault() -> Vault {
    let key = |byte: u8| SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    Vault::new(&key(0x32), &key(0x42), &url).expect("vault keys")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// A pool whose every connection runs as `askhava_app` (R9).
async fn app_pool(pool: &PgPool, size: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(size)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

async fn run(pool: &PgPool, sql: &'static str, id: Option<Uuid>) {
    sqlx::query(sql).bind(id).execute(pool).await.expect("seed");
}

/// Her request for `job` (none when `None`), opened and closed the given days ago.
async fn request(pool: &PgPool, job: Option<Uuid>, opened: i32, closed: i32) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id,
           consent_template, consent_sent_at, opened_at, closed_at)
         VALUES ($1, $2, 'en', gen_random_uuid(), 'consent_first',
           now() - make_interval(days => $3), now() - make_interval(days => $3),
           now() - make_interval(days => $4)) RETURNING id",
    )
    .bind(HOMEOWNER)
    .bind(job)
    .bind(opened)
    .bind(closed)
    .fetch_one(pool)
    .await
    .expect("request")
}

/// A homeowner with a fake contact, [`JOB`]'s request closed 181 days ago with its consent
/// record and an intake answer, and her other job [`JOB_B`] closed 200 days ago; returns the
/// contact and a jobless request that ended 200 days ago (the request path's target).
async fn seeded(pool: &PgPool) -> (Uuid, Uuid) {
    let new = NewContact {
        name: Some(SecretString::from("Wren Halloway".to_owned())),
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: Some(SecretString::from("wren@example.com".to_owned())),
        address: Some(SecretString::from("74 Fake Lane".to_owned())),
        onsite_contact: None,
    };
    let contact = store_contact(&mut app(pool).await, &vault(), &new)
        .await
        .expect("contact");
    sqlx::query("INSERT INTO intake.homeowner (id, contact_id, created_at) VALUES ($1, $2, now())")
        .bind(HOMEOWNER)
        .bind(contact)
        .execute(pool)
        .await
        .expect("homeowner");
    request(pool, Some(JOB), 200, 181).await;
    request(pool, Some(JOB_B), 230, 200).await;
    for sql in [
        "INSERT INTO intake.consent_record (homeowner_id, conversation_id, channel, inbox_id,
           consent_version, consented_at) SELECT homeowner_id, id, 'sms', first_inbox_id, 'v1',
           opened_at FROM intake.conversation WHERE job_id = $1",
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at)
           SELECT id, 'leak.where', 'kitchen', opened_at FROM intake.conversation WHERE job_id = $1",
    ] {
        run(pool, sql, Some(JOB)).await;
    }
    let jobless = request(pool, None, 220, 200).await;
    (contact, jobless)
}

async fn purge(pool: &PgPool, path: Path) -> Result<(), HandlerError> {
    let (kind, payload) = match path {
        Path::Job => (PURGE_KIND, json!({ "job": JOB, "stage": "contact" })),
        Path::Request(id) => ("intake.request_purge", json!({ "conversation": id })),
    };
    let task = Claimed {
        id: 1,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    };
    let pool = app_pool(pool, 2).await;
    match path {
        Path::Job => run_purge(pool, task).await,
        Path::Request(_) => run_request_purge(pool, task).await,
    }
}

/// Name, email and address cleared; phone kept with its index.
async fn cleared(pool: &PgPool, contact: Uuid) -> (bool, bool) {
    sqlx::query_as(
        "SELECT name IS NULL AND email IS NULL AND address IS NULL,
           phone IS NOT NULL AND EXISTS (SELECT 1 FROM privacy.contact_index
             WHERE contact_id = $1 AND purpose = 'homeowner-phone')
         FROM privacy.contact WHERE id = $1",
    )
    .bind(contact)
    .fetch_one(pool)
    .await
    .expect("contact row")
}

/// Waits until `n` lock requests of this test database are waiting (the barrier).
async fn wait_for_waiters(pool: &PgPool, n: i64) {
    for _ in 0..400 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid = l.pid
             WHERE NOT l.granted AND a.datname = current_database()",
        )
        .fetch_one(pool)
        .await
        .expect("locks");
        if waiting >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{n} lock waits never appeared: the purge did not wait for the lock order");
}

/// Her text, as the inbound route hands it to `intake` (fake words, inbox ID from `n`).
async fn text_in(db: &mut PgConnection, contact: Uuid, n: u128) -> Option<Uuid> {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x2740_0000 + n),
        contact,
        inbound: Inbound {
            words: "my water heater leaks again",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: Timestamp::now(),
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    receive_text(db, &secret, &msg, Settings::default())
        .await
        .expect("text")
        .conversation
}

/// Queues a hold-producing task with `key` (pending until marked).
async fn queue(pool: &PgPool, kind: &'static str, key: String) {
    let mut task = NewTask::new(kind, json!({ "job": JOB_B }), Timestamp::now());
    task.dedupe_key = Some(key);
    enqueue(&mut app(pool).await, &task).await.expect("task");
}

async fn mark_tasks(pool: &PgPool, column: &str) {
    let sql = if column == "dead_at" {
        "UPDATE core.task SET dead_at = now() WHERE $1::uuid IS NULL"
    } else {
        "UPDATE core.task SET done_at = now() WHERE $1::uuid IS NULL"
    };
    run(pool, sql, None).await;
}

async fn new_request_in_the_gap(pool: &PgPool, which: usize) {
    let (contact, jobless) = seeded(pool).await;
    let mut writer = app(pool).await;
    let mut tx = writer.begin().await.expect("begin");
    assert!(text_in(&mut tx, contact, 1).await.is_some());
    let path = [Path::Job, Path::Request(jobless)][which];
    let racing = tokio::spawn({
        let pool = pool.clone();
        async move { purge(&pool, path).await }
    });
    wait_for_waiters(pool, 1).await;
    tx.commit().await.expect("request commits");
    assert_eq!(racing.await.expect("join"), Ok(()));
    assert_eq!(cleared(pool, contact).await, (false, true));
}

/// TD-PRIV-18 (PRIV-007, D-1839): a new request whose transaction holds the homeowner row when
/// the job's contact purge starts makes the purge wait; once it commits, the purge sees it and
/// keeps the contact.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_a_request_committed_in_the_gap_keeps_the_contact(pool: PgPool) {
    new_request_in_the_gap(&pool, 0).await;
}

/// TD-PRIV-18 (PRIV-007, D-1839): the same race on the jobless-request purge path.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_a_request_committed_in_the_gap_keeps_a_jobless_requests_contact(pool: PgPool) {
    new_request_in_the_gap(&pool, 1).await;
}

/// TD-PRIV-18 (PRIV-007, D-1839): a dispute hold on her other job whose placement holds the shared
/// retention key makes each purge path wait and then keeps the contact; once released, the purge
/// clears it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_a_hold_placed_in_the_gap_keeps_the_contact(pool: PgPool) {
    let (contact, jobless) = seeded(&pool).await;
    let dispute = Uuid::from_u128(0xd274);
    for path in [Path::Job, Path::Request(jobless)] {
        let mut holder = app(&pool).await;
        let mut tx = holder.begin().await.expect("begin");
        let source = HoldSource::Dispute;
        let now = Timestamp::now();
        place_hold(&mut tx, source, dispute, JOB_B, now)
            .await
            .expect("hold");
        let racing = tokio::spawn({
            let pool = pool.clone();
            async move { purge(&pool, path).await }
        });
        wait_for_waiters(&pool, 1).await;
        tx.commit().await.expect("hold commits");
        assert_eq!(racing.await.expect("join"), Ok(()));
        assert_eq!(cleared(&pool, contact).await, (false, true), "{path:?}");
    }
    let mut conn = app(&pool).await;
    release_hold(
        &mut conn,
        HoldSource::Dispute,
        dispute,
        JOB_B,
        Timestamp::now(),
    )
    .await
    .expect("release");
    assert_eq!(purge(&pool, Path::Request(jobless)).await, Ok(()));
    assert_eq!(cleared(&pool, contact).await, (true, true));
}

/// TD-PRIV-18 (PRIV-007, D-1839): an unfinished hold-producing task keeps the contact on both
/// paths, pending and then failed for good: her other job's contact-stage `jobs.purge_due` or
/// `privacy.purge_recheck` (where the dispute holds are placed) and a complaint hold event. Once
/// every one is done the purge clears, and the job's own re-check (which handed the purge on) never
/// blocks it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_a_pending_or_failed_hold_task_keeps_the_contact(pool: PgPool) {
    let (contact, jobless) = seeded(&pool).await;
    let tasks = [
        ("jobs.purge_due", format!("jobs.purge_due:{JOB_B}:contact")),
        (
            "privacy.purge_recheck",
            format!("privacy.purge_recheck:{JOB_B}:first"),
        ),
        (
            "businesses.complaint_hold",
            "businesses.complaint_hold:00000000-0000-0000-0000-000000000c74:open".to_owned(),
        ),
    ];
    for (kind, key) in tasks {
        queue(&pool, kind, key).await;
        for state in ["pending", "dead_at"] {
            if state == "dead_at" {
                mark_tasks(&pool, "dead_at").await;
            }
            for path in [Path::Job, Path::Request(jobless)] {
                assert_eq!(purge(&pool, path).await, Ok(()));
                let kept = cleared(&pool, contact).await;
                assert_eq!(kept, (false, true), "{kind} {state} {path:?}");
            }
        }
        mark_tasks(&pool, "done_at").await;
    }
    queue(
        &pool,
        "privacy.purge_recheck",
        format!("privacy.purge_recheck:{JOB}:first"),
    )
    .await;
    assert_eq!(purge(&pool, Path::Job).await, Ok(()));
    assert_eq!(cleared(&pool, contact).await, (true, true));
}

async fn purge_wins_first(pool: &PgPool, which: usize) {
    let (contact, jobless) = seeded(pool).await;
    let path = [Path::Job, Path::Request(jobless)][which];
    let mut locker = pool.acquire().await.expect("connection");
    let mut tx = locker.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM privacy.contact WHERE id = $1 FOR UPDATE")
        .bind(contact)
        .execute(&mut *tx)
        .await
        .expect("contact row lock");
    let purging = tokio::spawn({
        let pool = pool.clone();
        async move { purge(&pool, path).await }
    });
    wait_for_waiters(pool, 1).await;
    let writing = tokio::spawn({
        let pool = pool.clone();
        async move { text_in(&mut app(&pool).await, contact, 2).await }
    });
    wait_for_waiters(pool, 2).await;
    tx.rollback().await.expect("release the contact row");
    assert_eq!(purging.await.expect("join"), Ok(()));
    let opened = writing.await.expect("join").expect("a new request");
    assert_eq!(cleared(pool, contact).await, (true, true));
    let found = find_by_phone(&mut app(pool).await, &vault(), PHONE).await;
    assert_eq!(found, Ok(Some(contact)));
    let open: (Uuid, Option<Uuid>) = sqlx::query_as(
        "SELECT homeowner_id, job_id FROM intake.conversation WHERE id = $1 AND closed_at IS NULL",
    )
    .bind(opened)
    .fetch_one(pool)
    .await
    .expect("open request");
    assert_eq!(open, (HOMEOWNER, None));
}

/// TD-PRIV-18 (PRIV-007, D-1839): a job purge that holds the homeowner row first clears the old
/// contact; the request that waited on it then opens as today, on her kept phone's vault entry,
/// whose cleared details its questions ask for again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_a_purge_that_wins_the_lock_clears_and_the_request_then_opens(pool: PgPool) {
    purge_wins_first(&pool, 0).await;
}

/// TD-PRIV-18 (PRIV-007, D-1839): the same order on the jobless-request purge path.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_a_jobless_purge_that_wins_the_lock_clears_and_the_request_then_opens(
    pool: PgPool,
) {
    purge_wins_first(&pool, 1).await;
}

async fn one_of_twenty(pool: PgPool, contact: Uuid, jobless: Uuid, n: u32) -> Result<(), String> {
    let id = Uuid::from_u128(0xd000 + u128::from(n));
    let now = Timestamp::now();
    let mut conn = app(&pool).await;
    let outcome = match n % 4 {
        0 => purge(&pool, Path::Job).await.map_err(|e| e.0.to_owned()),
        1 => purge(&pool, Path::Request(jobless))
            .await
            .map_err(|e| e.0.to_owned()),
        2 => place_hold(&mut conn, HoldSource::Complaint, id, JOB_B, now)
            .await
            .map_err(|e| e.to_string()),
        _ => {
            text_in(&mut conn, contact, u128::from(n)).await;
            Ok(())
        }
    };
    if n % 8 == 2 {
        release_hold(&mut conn, HoldSource::Complaint, id, JOB_B, now)
            .await
            .map_err(|e| e.to_string())?;
    }
    outcome
}

async fn snapshot(pool: &PgPool, contact: Uuid) -> (bool, bool, i64, i64, i64) {
    let (gone, phone) = cleared(pool, contact).await;
    let (holds, requests, audits): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM privacy.retention_hold WHERE $1::uuid IS NOT NULL),
           (SELECT count(*) FROM intake.conversation),
           (SELECT count(*) FROM ops.audit_log WHERE action = 'contact_purged')",
    )
    .bind(contact)
    .fetch_one(pool)
    .await
    .expect("snapshot");
    (gone, phone, holds, requests, audits)
}

/// TD-PRIV-18 (PRIV-007, PRIV-008, PRIV-010, D-1839): twenty purges (both paths), hold placements
/// and releases and request writers at once all finish with no deadlock or error; the contact is
/// cleared at most once (one audit row), the job text and consent record stay, and running every
/// purge again changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_18_twenty_concurrent_purges_and_writers_never_deadlock(pool: PgPool) {
    let (contact, jobless) = seeded(&pool).await;
    let runs: Vec<_> = (0..20)
        .map(|n| tokio::spawn(one_of_twenty(pool.clone(), contact, jobless, n)))
        .collect();
    for handle in runs {
        assert_eq!(handle.await.expect("join"), Ok(()));
    }
    let after = snapshot(&pool, contact).await;
    assert!(after.4 <= 1, "the contact was cleared more than once");
    for path in [Path::Job, Path::Request(jobless)] {
        assert_eq!(purge(&pool, path).await, Ok(()));
    }
    assert_eq!(snapshot(&pool, contact).await, after);
    let kept: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM intake.intake_answer) + (SELECT count(*) FROM intake.consent_record WHERE conversation_id IN (SELECT id FROM intake.conversation WHERE job_id = $1))",
    )
    .bind(JOB)
    .fetch_one(&pool)
    .await
    .expect("kept rows");
    assert!(
        kept >= 2,
        "the contact purge touched the job text or consent"
    );
}
