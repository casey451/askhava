//! The homeowner's feedback words at the 3-year purge and on "Delete my data", on a fresh database
//! (TD-PRIV-22, T315; PRIV-009, PRIV-011, JOB-017; D-2205, D-3264).
//!
//! The text stage of a job closed 1,096 days ago blanks its feedback body once, sets
//! `body_purged_at`, writes one audit row of IDs and takes it off `feedback::shown`; while a
//! dispute or complaint hold stands the words stay and a daily `jobs.feedback_purge` asks again;
//! a confirmed "Delete my data" blanks her jobs' feedback in the vault delete's transaction; the
//! app role can change no other column and cannot bring a purged body back. The composition's
//! step (`server::feedback_step`) is `privacy::api::is_held` then `jobs::api::feedback_text_stage`,
//! run here the same way, since a test crate may not depend on `server` (D-736). Every handler
//! runs as `askhava_app`. Every name, phone and word is fake.

use askhava_core::api::Claimed;
use askhava_integration::{PERMISSION_DENIED, set_app_role};
use askhava_jobs::api::feedback::{shown, store};
use askhava_jobs::api::{
    FEEDBACK_PURGE_KIND, FeedbackStage, PURGE_DUE_KIND, feedback_text_stage, sweep_closed,
};
use askhava_portals::api::delete_her_data;
use askhava_privacy::api::{
    HoldSource, NewContact, PURGE_KIND, Vault, is_held, place_hold, release_hold, run_purge_due,
    store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const JOB: Uuid = Uuid::from_u128(0x7315_00a1);
const WORDS: &str = "Great work on the faucet. Zelda Canary, call me at 928-555-0142";

fn vault() -> Vault {
    let key = |byte: u8| SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    Vault::new(&key(0x35), &key(0x45), &url).expect("vault keys")
}

async fn app(pool: &PgPool) -> PgConnection {
    let mut conn = pool.acquire().await.expect("connection").detach();
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn app_pool(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

/// A Done job closed `days` ago with ticked feedback stored as the app role; returns its ID.
async fn done_with_feedback(pool: &PgPool, job: Uuid, days: i64) -> Uuid {
    let closed = Timestamp::now() - SignedDuration::from_hours(days * 24 + 1);
    sqlx::query(
        "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from) \
         VALUES ($1, 'done', $2, $2, $2)",
    )
    .bind(job)
    .bind(closed.to_sqlx())
    .execute(pool)
    .await
    .expect("job");
    let stored = store(&mut app(pool).await, job, (WORDS, true), closed).await;
    stored.expect("store").expect("stored once")
}

/// The feedback's (body, purged) for `job`.
async fn feedback(pool: &PgPool, job: Uuid) -> (String, bool) {
    sqlx::query_as("SELECT body, body_purged_at IS NOT NULL FROM jobs.feedback WHERE job_id = $1")
        .bind(job)
        .fetch_one(pool)
        .await
        .expect("feedback row")
}

/// The `feedback_purged` audit rows of feedback `id`: (actor kind, actor ID).
async fn purge_audits(pool: &PgPool, id: Uuid) -> Vec<(String, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT actor_kind, actor_id FROM ops.audit_log WHERE action = 'feedback_purged' \
         AND subject_kind = 'jobs.feedback' AND subject_id = $1",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .expect("audit rows")
}

/// The composition's feedback step on the text stage (`server::feedback_step`).
async fn text_step(pool: &PgPool, job: Uuid) -> FeedbackStage {
    let mut conn = app(pool).await;
    let held = is_held(&mut conn, job).await.expect("hold check");
    let stage = feedback_text_stage(&mut conn, (job, held), Timestamp::now()).await;
    stage.expect("feedback step")
}

/// TD-PRIV-22 (PRIV-009, JOB-017): 1,096 days after the close the text stage is due and handed
/// on; the feedback step blanks the body once with one audit row (IDs only, by the system) and
/// `shown` no longer returns it; a rerun writes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_22_the_text_stage_blanks_the_words_once(pool: PgPool) {
    let id = done_with_feedback(&pool, JOB, 1096).await;
    assert_eq!(shown(&mut app(&pool).await).await.expect("shown").len(), 1);
    sweep_closed(&mut app(&pool).await, Timestamp::now())
        .await
        .expect("sweep");
    let due: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND payload->>'stage' = 'text' \
         AND run_at <= now()",
    )
    .bind(PURGE_DUE_KIND)
    .fetch_one(&pool)
    .await
    .expect("due");
    assert_eq!(due, 1);
    let task = Claimed {
        id: 1,
        kind: PURGE_DUE_KIND.to_owned(),
        payload: json!({ "job": JOB, "stage": "text" }),
        attempts: 1,
    };
    run_purge_due(app_pool(&pool).await, task)
        .await
        .expect("text stage");
    let handed: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(PURGE_KIND)
        .fetch_one(&pool)
        .await
        .expect("handed on");
    assert_eq!(handed, 1);
    assert_eq!(text_step(&pool, JOB).await, FeedbackStage::Blanked);
    assert_eq!(feedback(&pool, JOB).await, (String::new(), true));
    assert!(
        shown(&mut app(&pool).await)
            .await
            .expect("shown")
            .is_empty()
    );
    assert_eq!(text_step(&pool, JOB).await, FeedbackStage::Nothing);
    assert_eq!(purge_audits(&pool, id).await, [("system".to_owned(), None)]);
}

/// TD-PRIV-22 (PRIV-007, PRIV-009): an open dispute or complaint hold keeps the words and queues
/// one daily ask-again; once released, the next step blanks them.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_22_a_hold_keeps_the_words_until_released(pool: PgPool) {
    let id = done_with_feedback(&pool, JOB, 1096).await;
    let now = Timestamp::now();
    for (n, source) in [HoldSource::Dispute, HoldSource::Complaint]
        .into_iter()
        .enumerate()
    {
        let hold = Uuid::from_u128(0xd0 + n as u128);
        place_hold(&mut app(&pool).await, source, hold, JOB, now)
            .await
            .expect("hold");
        for _ in 0..2 {
            assert_eq!(text_step(&pool, JOB).await, FeedbackStage::Held);
        }
        assert_eq!(feedback(&pool, JOB).await, (WORDS.to_owned(), false));
        let again: Vec<String> =
            sqlx::query_scalar("SELECT payload->>'job' FROM core.task WHERE kind = $1 AND run_at > now() + interval '23 hours'")
                .bind(FEEDBACK_PURGE_KIND)
                .fetch_all(&pool)
                .await
                .expect("ask again");
        assert_eq!(again, [JOB.to_string()]);
        release_hold(&mut app(&pool).await, source, hold, JOB, now)
            .await
            .expect("release");
    }
    assert_eq!(text_step(&pool, JOB).await, FeedbackStage::Blanked);
    assert_eq!(purge_audits(&pool, id).await.len(), 1);
}

/// TD-PRIV-22 (PRIV-009): a held job with no feedback words left queues no ask-again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_22_a_held_job_without_words_queues_nothing(pool: PgPool) {
    let stage = feedback_text_stage(&mut app(&pool).await, (JOB, true), Timestamp::now()).await;
    assert_eq!(stage.expect("feedback step"), FeedbackStage::Nothing);
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(FEEDBACK_PURGE_KIND)
        .fetch_one(&pool)
        .await
        .expect("ask again");
    assert_eq!(queued, 0);
}

/// A homeowner with a fake vault contact and a closed request for each of `jobs`; returns the
/// contact.
async fn homeowner(pool: &PgPool, phone: &str, jobs: &[Uuid]) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Zelda Canary".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(&mut app(pool).await, &vault(), &new)
        .await
        .expect("contact");
    let home: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, now()) RETURNING id",
    )
    .bind(contact)
    .fetch_one(pool)
    .await
    .expect("homeowner");
    for job in jobs {
        sqlx::query(
            "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id,
               consent_template, consent_sent_at, opened_at, closed_at) VALUES ($1, $2, 'en',
               gen_random_uuid(), 'consent_first', now() - interval '9 days',
               now() - interval '9 days', now() - interval '8 days')",
        )
        .bind(home)
        .bind(job)
        .execute(pool)
        .await
        .expect("request");
    }
    contact
}

async fn name_cleared(pool: &PgPool, contact: Uuid) -> bool {
    sqlx::query_scalar("SELECT name IS NULL FROM privacy.contact WHERE id = $1")
        .bind(contact)
        .fetch_one(pool)
        .await
        .expect("contact row")
}

/// TD-PRIV-22 (PRIV-011, JOB-017): "Delete my data" blanks her jobs' feedback in the vault
/// delete's transaction, audited as her; a failed vault delete leaves both; another homeowner's
/// feedback stays.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_22_delete_my_data_blanks_her_feedback_with_the_vault(pool: PgPool) {
    let (job_b, theirs) = (Uuid::from_u128(0x7315_00b1), Uuid::from_u128(0x7315_00e1));
    let mut ids = Vec::new();
    for job in [JOB, job_b, theirs] {
        ids.push(done_with_feedback(&pool, job, 9).await);
    }
    let her = homeowner(&pool, "(928) 555-0175", &[JOB, job_b]).await;
    homeowner(&pool, "(928) 555-0176", &[theirs]).await;
    let mut owner = pool.acquire().await.expect("owner").detach();
    sqlx::query("CREATE FUNCTION public.t315_refuse() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fake vault failure'; END $$")
        .execute(&mut owner)
        .await
        .expect("refuse");
    sqlx::query("CREATE TRIGGER t315_refuse BEFORE UPDATE ON privacy.contact FOR EACH ROW EXECUTE FUNCTION public.t315_refuse()")
        .execute(&mut owner)
        .await
        .expect("trigger");
    let mut db = app(&pool).await;
    let mut tx = db.begin().await.expect("begin");
    assert!(delete_her_data(&mut tx, her).await.is_err());
    tx.rollback().await.expect("rollback");
    assert_eq!(feedback(&pool, JOB).await, (WORDS.to_owned(), false));
    assert!(!name_cleared(&pool, her).await);
    sqlx::query("DROP TRIGGER t315_refuse ON privacy.contact")
        .execute(&mut owner)
        .await
        .expect("drop");
    let mut tx = db.begin().await.expect("begin");
    delete_her_data(&mut tx, her).await.expect("delete");
    tx.commit().await.expect("commit");
    for job in [JOB, job_b] {
        assert_eq!(feedback(&pool, job).await, (String::new(), true));
    }
    assert!(name_cleared(&pool, her).await);
    assert_eq!(
        purge_audits(&pool, ids[0]).await,
        [("homeowner".to_owned(), Some(her))]
    );
    assert_eq!(feedback(&pool, theirs).await, (WORDS.to_owned(), false));
    assert_eq!(shown(&mut db).await.expect("shown").len(), 1);
}

async fn refused(db: &mut PgConnection, sql: &'static str) -> Option<String> {
    let err = sqlx::query(sql).execute(db).await.expect_err(sql);
    let code = err.as_database_error().and_then(|e| e.code());
    code.map(|c| c.into_owned())
}

/// TD-PRIV-22 (JOB-017, PRIV-009): the app role may change no column but the body, its purge
/// time and the marks, never deletes, never changes the words but by blanking them, and never
/// brings a purged body back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_22_the_app_role_cannot_bring_the_words_back(pool: PgPool) {
    done_with_feedback(&pool, JOB, 1096).await;
    let mut db = app(&pool).await;
    for sql in [
        "UPDATE jobs.feedback SET share = false",
        "UPDATE jobs.feedback SET received_at = now()",
        "UPDATE jobs.feedback SET job_id = gen_random_uuid()",
        "UPDATE jobs.feedback SET id = gen_random_uuid()",
        "DELETE FROM jobs.feedback",
    ] {
        assert_eq!(
            refused(&mut db, sql).await.as_deref(),
            Some(PERMISSION_DENIED),
            "{sql}"
        );
    }
    for sql in [
        "UPDATE jobs.feedback SET body = 'Other fake words'",
        "UPDATE jobs.feedback SET body_purged_at = now()",
        "UPDATE jobs.feedback SET body = ''",
    ] {
        assert!(refused(&mut db, sql).await.is_some(), "{sql}");
    }
    assert_eq!(text_step(&pool, JOB).await, FeedbackStage::Blanked);
    for sql in [
        "UPDATE jobs.feedback SET body = 'Back again', body_purged_at = NULL",
        "UPDATE jobs.feedback SET body = 'Back again'",
        "UPDATE jobs.feedback SET body_purged_at = now() + interval '1 day'",
        "UPDATE jobs.feedback SET body_purged_at = NULL",
    ] {
        assert!(refused(&mut db, sql).await.is_some(), "{sql}");
    }
    assert_eq!(feedback(&pool, JOB).await, (String::new(), true));
}
