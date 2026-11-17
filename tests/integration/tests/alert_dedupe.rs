//! TD-OPS-34 (OPS-004, SEC-051; T320, D-2207, D-3144): a check that runs on a schedule raises its
//! alert once while its condition lasts. `raise_alert_once` (kind and subject) and
//! `raise_kind_once` (kind alone, no subject) each leave one open alert however often or however
//! concurrently they run, and a new one opens only after it is resolved; `check_webhook_silence`
//! run twice while webhooks stay silent leaves one open alert; `raise_alert` itself still opens
//! one alert per call, each event its own alert (TD-FEE-33's two fraud warnings, SEC-048's burst
//! alert). All data is fake.

use askhava_messaging::api::{WEBHOOKS_SILENT_ALERT, check_webhook_silence};
use askhava_ops::api::{NewAlert, Severity, raise_alert, raise_alert_once, raise_kind_once};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// A fake alert kind about a subject.
const SUBJECT_KIND: &str = "test.subject_once";
/// A fake subject-less alert kind.
const KIND_ONLY: &str = "test.kind_once";
/// A fake subject.
const SUBJECT: (&str, Uuid) = ("pick", Uuid::from_u128(0x320));

/// Open (unresolved) alerts of `kind`.
async fn open(db: &mut PgConnection, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL")
        .bind(kind)
        .fetch_one(db)
        .await
        .expect("count")
}

/// Resolves every open alert of `kind`, as Casey would.
async fn resolve(db: &mut PgConnection, kind: &str) {
    sqlx::query("UPDATE ops.alert SET resolved_at = now() WHERE kind = $1 AND resolved_at IS NULL")
        .bind(kind)
        .execute(db)
        .await
        .expect("resolve");
}

/// TD-OPS-34 (OPS-004): two `raise_alert_once` calls of one kind and subject leave one open
/// alert, the second returning none; after it is resolved a raise opens a new one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_34_raise_alert_once_opens_one_until_resolved(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let first = raise_alert_once(&mut db, SUBJECT_KIND, Severity::High, SUBJECT).await;
    assert!(matches!(first, Ok(Some(_))), "the first raise opens one");
    let second = raise_alert_once(&mut db, SUBJECT_KIND, Severity::High, SUBJECT).await;
    assert!(matches!(second, Ok(None)), "one is open: none raised");
    assert_eq!(open(&mut db, SUBJECT_KIND).await, 1);
    resolve(&mut db, SUBJECT_KIND).await;
    let third = raise_alert_once(&mut db, SUBJECT_KIND, Severity::High, SUBJECT).await;
    assert!(matches!(third, Ok(Some(_))), "after resolving, a new one");
    assert_eq!(open(&mut db, SUBJECT_KIND).await, 1);
}

/// TD-OPS-34 (OPS-004, SEC-051): two `raise_kind_once` calls of one subject-less kind leave one
/// open alert; after it is resolved a third opens one more. An alert of that kind about a subject
/// does not count as the open one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_34_raise_kind_once_opens_one_until_resolved(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let about = NewAlert {
        kind: KIND_ONLY,
        severity: Severity::Normal,
        subject: Some(SUBJECT),
    };
    raise_alert(&mut db, &about).await.expect("subject alert");
    let first = raise_kind_once(&mut db, KIND_ONLY, Severity::High).await;
    assert!(
        matches!(first, Ok(Some(_))),
        "a subject alert is not the open one"
    );
    let second = raise_kind_once(&mut db, KIND_ONLY, Severity::High).await;
    assert!(matches!(second, Ok(None)), "one is open: none raised");
    assert_eq!(
        open(&mut db, KIND_ONLY).await,
        2,
        "the subject alert and one"
    );
    resolve(&mut db, KIND_ONLY).await;
    let third = raise_kind_once(&mut db, KIND_ONLY, Severity::High).await;
    assert!(matches!(third, Ok(Some(_))), "after resolving, one more");
    assert_eq!(open(&mut db, KIND_ONLY).await, 1);
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(KIND_ONLY)
        .fetch_one(&mut *db)
        .await
        .expect("total");
    assert_eq!(total, 3);
}

/// TD-OPS-34 (OPS-004): two concurrent raises of either once function leave one open alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_34_concurrent_raises_leave_one(pool: PgPool) {
    let (mut a, mut b) = (
        pool.acquire().await.expect("a"),
        pool.acquire().await.expect("b"),
    );
    let (x, y) = tokio::join!(
        raise_alert_once(&mut a, SUBJECT_KIND, Severity::High, SUBJECT),
        raise_alert_once(&mut b, SUBJECT_KIND, Severity::High, SUBJECT),
    );
    assert_eq!(
        [x, y].iter().filter(|r| matches!(r, Ok(Some(_)))).count(),
        1
    );
    let (x, y) = tokio::join!(
        raise_kind_once(&mut a, KIND_ONLY, Severity::High),
        raise_kind_once(&mut b, KIND_ONLY, Severity::High),
    );
    assert_eq!(
        [x, y].iter().filter(|r| matches!(r, Ok(Some(_)))).count(),
        1
    );
    assert_eq!(open(&mut a, SUBJECT_KIND).await, 1);
    assert_eq!(open(&mut a, KIND_ONLY).await, 1);
}

/// TD-OPS-34 (SEC-051, OPS-004): `check_webhook_silence` run twice while webhooks stay silent
/// leaves one open alert, the second run raising none; once it is resolved a run opens a new one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_34_webhook_silence_alerts_once_while_silent(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now: Timestamp = "2026-11-03T18:00:00Z".parse().expect("time");
    let sent = now - SignedDuration::from_hours(3);
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at) \
         VALUES (repeat('d', 64), 'job_card', gen_random_uuid(), 'business', '{}', 'notice', \
         'sent', $1, 're_dedupe_d', $1, $1)",
    )
    .bind(jiff_sqlx::ToSqlx::to_sqlx(sent))
    .execute(&mut *db)
    .await
    .expect("a sent email");
    assert_eq!(check_webhook_silence(&mut db, now).await, Ok(true));
    let later = now + SignedDuration::from_hours(1);
    assert_eq!(
        check_webhook_silence(&mut db, later).await,
        Ok(false),
        "still silent, one already open"
    );
    assert_eq!(open(&mut db, WEBHOOKS_SILENT_ALERT).await, 1);
    resolve(&mut db, WEBHOOKS_SILENT_ALERT).await;
    assert_eq!(check_webhook_silence(&mut db, later).await, Ok(true));
    assert_eq!(open(&mut db, WEBHOOKS_SILENT_ALERT).await, 1);
}

/// TD-OPS-34 (OPS-004): `raise_alert` itself still opens one alert per call, each event its own
/// alert (TD-FEE-33's two fraud warnings on one pick, SEC-048's subject-less burst alert).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_34_raise_alert_opens_one_per_call(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    for subject in [Some(SUBJECT), None] {
        let alert = NewAlert {
            kind: SUBJECT_KIND,
            severity: Severity::High,
            subject,
        };
        raise_alert(&mut db, &alert).await.expect("first");
        raise_alert(&mut db, &alert).await.expect("second");
    }
    assert_eq!(open(&mut db, SUBJECT_KIND).await, 4);
}
