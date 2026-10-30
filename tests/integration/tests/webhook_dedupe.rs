//! Webhook dedupe and quote-page link tokens on a real database (SEC-017, SEC-037, SEC-039;
//! TD-SEC-16, TD-SEC-18, D-491).
//!
//! A probe module table built exactly as `askhava_core::api::Dedupe`'s pattern documents proves
//! that one event delivered 100 times, in turn or all at once, has one effect; that a Twilio
//! status sequence queued, sent, delivered, delivered has 3; and that two voice steps of one call
//! both apply. The quote-token cases prove a re-issued token replaces the old one, a revoked one
//! resolves to nothing (the page's 404), and the stored salt reproduces the stored hash (replay);
//! a revoke stamped before the issue (clock skew) still revokes, and a concurrent issue that
//! loses the one-live-token race answers `Busy`.
//! All data is fake.

use askhava_core::api::{Dedupe, EventKey, IdSecret, LinkKind, LinkSalt, LinkToken, link_token};
use askhava_jobs::api::{create, quote_token};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use std::time::Duration;

/// Creates the probe module's `webhook_event` and effect tables, as a module's migration would.
async fn probe_tables(pool: &PgPool) {
    for sql in [
        "CREATE TABLE core.probe_webhook_event (source text NOT NULL, event_key text NOT NULL, \
         received_at timestamptz NOT NULL, PRIMARY KEY (source, event_key))",
        "CREATE TABLE core.probe_effect (event_key text NOT NULL)",
    ] {
        sqlx::query(sql).execute(pool).await.unwrap();
    }
}

/// One delivery as a module handles it: the dedupe insert and, on the first, the effect, in one
/// transaction (SEC-017).
async fn deliver(pool: &PgPool, source: &str, key: &EventKey) -> Dedupe {
    let mut tx = pool.begin().await.unwrap();
    let row: Option<String> = sqlx::query_scalar(
        "INSERT INTO core.probe_webhook_event (source, event_key, received_at) \
         VALUES ($1, $2, now()) ON CONFLICT (source, event_key) DO NOTHING RETURNING event_key",
    )
    .bind(source)
    .bind(key.as_str())
    .fetch_optional(&mut *tx)
    .await
    .unwrap();
    let outcome = Dedupe::from_returning(row);
    if outcome.is_first() {
        sqlx::query("INSERT INTO core.probe_effect (event_key) VALUES ($1)")
            .bind(key.as_str())
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    outcome
}

async fn effects(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.probe_effect")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// SEC-017: the same event 100 times in turn, then 100 times at once, has exactly one effect.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn one_event_delivered_100_times_changes_nothing_after_the_first(pool: PgPool) {
    probe_tables(&pool).await;
    let key = EventKey::new("evt_fake_100").unwrap();
    assert_eq!(deliver(&pool, "stripe", &key).await, Dedupe::First);
    for _ in 1..100 {
        assert_eq!(deliver(&pool, "stripe", &key).await, Dedupe::Repeat);
    }
    let racing = EventKey::new("evt_fake_race").unwrap();
    let runs = (0..100).map(|_| {
        let (pool, key) = (pool.clone(), racing.clone());
        tokio::spawn(async move { deliver(&pool, "stripe", &key).await })
    });
    let mut firsts = 0;
    for run in runs.collect::<Vec<_>>() {
        firsts += usize::from(run.await.unwrap().is_first());
    }
    assert_eq!(firsts, 1);
    assert_eq!(effects(&pool).await, 2);
}

/// TD-SEC-18: Twilio statuses queued, sent, delivered, delivered give 3 effects; two voice steps
/// of one CallSid both apply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_18_twilio_pairs_dedupe_by_status_and_step(pool: PgPool) {
    probe_tables(&pool).await;
    let sid = "SM00000000000000000000000000000001";
    for status in ["queued", "sent", "delivered", "delivered"] {
        let _ = deliver(
            &pool,
            "twilio_status",
            &EventKey::pair(sid, status).unwrap(),
        )
        .await;
    }
    assert_eq!(effects(&pool).await, 3);
    let call = "CA00000000000000000000000000000001";
    for step in ["greeting", "pin"] {
        let key = EventKey::pair(call, step).unwrap();
        assert!(deliver(&pool, "twilio_voice", &key).await.is_first());
    }
    assert_eq!(effects(&pool).await, 5);
}

fn secret() -> IdSecret {
    IdSecret::new(
        1,
        SecretString::from("fake-id-secret-for-tests-00000001".to_owned()),
    )
    .unwrap()
}

/// TD-SEC-16: a re-issued token replaces the old one (404), a revoked one resolves to nothing,
/// and replay from the stored salt reproduces the stored hash.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_16_reissue_and_revoke_and_replay(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    let now = Timestamp::now();
    let job = create(&mut db, now, false, Actor::System).await.unwrap();
    let first = quote_token::issue(&mut db, &secret(), job, now)
        .await
        .unwrap();
    assert_eq!(
        quote_token::resolve(&mut db, &first).await.unwrap(),
        Some(job)
    );
    let second = quote_token::issue(&mut db, &secret(), job, now)
        .await
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(quote_token::resolve(&mut db, &first).await.unwrap(), None);
    assert_eq!(
        quote_token::resolve(&mut db, &second).await.unwrap(),
        Some(job)
    );
    let rows: Vec<(Vec<u8>, Vec<u8>)> =
        sqlx::query_as("SELECT salt, token_hash FROM jobs.quote_token WHERE job_id = $1")
            .bind(job)
            .fetch_all(&mut *db)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    for (salt, hash) in rows {
        let salt = LinkSalt::from_bytes(&salt).unwrap();
        let replayed = link_token(&secret(), LinkKind::QuotePage, job, &salt);
        assert_eq!(replayed.hash().as_slice(), hash.as_slice());
    }
    assert!(quote_token::revoke(&mut db, job, now).await.unwrap());
    assert!(!quote_token::revoke(&mut db, job, now).await.unwrap());
    assert_eq!(quote_token::resolve(&mut db, &second).await.unwrap(), None);
    let unknown = LinkToken::parse(&"0".repeat(64)).unwrap();
    assert_eq!(quote_token::resolve(&mut db, &unknown).await.unwrap(), None);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND action LIKE 'quote_token_%'",
    )
    .bind(job)
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(audits, 3);
}

/// SEC-037: a revoke whose clock reads earlier than the issue's (skew between instances) still
/// revokes, stamped at the issue time, so a leaked link can always be shut off.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_revoke_stamped_before_the_issue_still_revokes(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    let now = Timestamp::now();
    let job = create(&mut db, now, false, Actor::System).await.unwrap();
    let token = quote_token::issue(&mut db, &secret(), job, now)
        .await
        .unwrap();
    let skewed = now - jiff::SignedDuration::from_secs(90);
    assert!(quote_token::revoke(&mut db, job, skewed).await.unwrap());
    assert_eq!(quote_token::resolve(&mut db, &token).await.unwrap(), None);
    let same: bool =
        sqlx::query_scalar("SELECT revoked_at = issued_at FROM jobs.quote_token WHERE job_id = $1")
            .bind(job)
            .fetch_one(&mut *db)
            .await
            .unwrap();
    assert!(same);
}

/// True once a session in this test's database waits on a lock, polling for up to 10 seconds.
async fn waits_on_a_lock(pool: &PgPool) -> bool {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting > 0 {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// SEC-037: two issues for one job at once; the one that inserts second waits on the
/// one-live-token index and, when the first commits, answers `Busy` and writes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_concurrent_issue_that_loses_the_race_answers_busy(pool: PgPool) {
    let mut db = pool.acquire().await.unwrap();
    let now = Timestamp::now();
    let job = create(&mut db, now, false, Actor::System).await.unwrap();
    let mut first = pool.begin().await.unwrap();
    let won = quote_token::issue(&mut first, &secret(), job, now)
        .await
        .unwrap();
    let racer = pool.clone();
    let second = tokio::spawn(async move {
        let mut db = racer.acquire().await.unwrap();
        quote_token::issue(&mut db, &secret(), job, now).await
    });
    assert!(waits_on_a_lock(&pool).await);
    first.commit().await.unwrap();
    assert_eq!(
        second.await.unwrap().unwrap_err(),
        quote_token::TokenError::Busy
    );
    assert_eq!(
        quote_token::resolve(&mut db, &won).await.unwrap(),
        Some(job)
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs.quote_token WHERE job_id = $1")
        .bind(job)
        .fetch_one(&mut *db)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}
