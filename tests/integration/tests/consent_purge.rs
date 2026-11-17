//! TD-CON-30, the `messaging` half (CON-022, CON-036; D-2208, D-3155, D-3753, D-3967): the
//! five-year clock, the opt-out events' purge and the message-log deletes, on fresh databases.
//!
//! Proves that `messaging.purge_expired_optout_events`, on the database clock with strict
//! comparisons, keeps a row and a last message seeded 1,826 days ago and removes a row whose last
//! message and own time are 1,827 days and one minute ago; that it leaves an opt-out event a
//! suppression row references and the number's newest one; that a temporary `suppression` or
//! `outbox` table in the session changes nothing; that the app role cannot call
//! `messaging.last_message_at` nor change or delete an opt-out event itself; that a message in
//! between restarts the clock, inbound through its `messaging.inbox_contact` row (written once by
//! `on_inbound_text`) and outbound through its outbox row; and that the message-log deletes
//! remove only that number's rows. Rows are seeded in the past, since the functions read the
//! database clock. The sweep, the `intake` purge and the vault phone clear are T563's cases.
//! Every ID, digest, SID and phone is fake.

use askhava_integration::{is_permission_denied, set_app_role};
use askhava_messaging::api::{
    InboundText, ProviderKeyword, Relation, delete_message_log, due_numbers, on_inbound_text,
    purge_optout_events,
};
use askhava_privacy::api::Vault;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// 1,827 days and one minute: past the boundary, kept off it by the minute.
const PAST: &str = "1827 days 1 minute";
/// 1,826 days: inside the five years.
const INSIDE: &str = "1826 days";

fn digest(byte: u8) -> (u16, [u8; 32]) {
    (1, [byte; 32])
}

/// One outbox row to `contact`, sent `ago` before the database clock, with a delivery event and a
/// retry row tied to it; returns its ID.
async fn outbound(db: &mut PgConnection, contact: Uuid, ago: &str) -> Uuid {
    let sid = format!("SM{}", Uuid::new_v4().simple());
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, recipient_kind, \
         subject_ids, status, provider_sid, created_at, sent_at) \
         VALUES (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'fake.kind', $1, \
         'homeowner', '{}', 'sent', $2, now() - $3::interval, now() - $3::interval) RETURNING id",
    )
    .bind(contact)
    .bind(&sid)
    .bind(ago)
    .fetch_one(&mut *db)
    .await
    .expect("outbox row");
    sqlx::query(
        "INSERT INTO messaging.delivery_event (provider_sid, status, received_at) \
         VALUES ($1, 'delivered', now() - $2::interval)",
    )
    .bind(&sid)
    .bind(ago)
    .execute(&mut *db)
    .await
    .expect("delivery event");
    sqlx::query(
        "INSERT INTO messaging.text_retry (outbox_id, failures, first_failed_at) \
         VALUES ($1, 1, now() - $2::interval)",
    )
    .bind(id)
    .bind(ago)
    .execute(&mut *db)
    .await
    .expect("retry row");
    id
}

/// One inbound text from `contact`, received `ago`: its inbox row and its `inbox_contact` row.
async fn inbound(db: &mut PgConnection, contact: Uuid, ago: &str) -> Uuid {
    let sid = format!("SM{}", Uuid::new_v4().simple());
    sqlx::query_scalar(
        "WITH i AS (INSERT INTO messaging.inbox (message_sid, received_at) \
           VALUES ($1, now() - $3::interval) RETURNING id, received_at) \
         INSERT INTO messaging.inbox_contact (inbox_id, contact_id, received_at) \
         SELECT i.id, $2, i.received_at FROM i RETURNING inbox_id",
    )
    .bind(&sid)
    .bind(contact)
    .bind(ago)
    .fetch_one(db)
    .await
    .expect("inbound text")
}

/// One opt-out event under `digest`, effective `ago`.
async fn event(db: &mut PgConnection, (version, digest): (u16, [u8; 32]), ago: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO messaging.optout_event (action, channel, detected, index_version, digest, \
         effective_at) VALUES ('opt_out', 'text', 'word:stop', $1, $2, now() - $3::interval) \
         RETURNING id",
    )
    .bind(i16::try_from(version).expect("version"))
    .bind(digest.to_vec())
    .bind(ago)
    .fetch_one(db)
    .await
    .expect("opt-out event")
}

async fn suppress(db: &mut PgConnection, (version, digest): (u16, [u8; 32]), event: Uuid) {
    sqlx::query(
        "INSERT INTO messaging.suppression (index_version, digest, event_id, since) \
         VALUES ($1, $2, $3, now())",
    )
    .bind(i16::try_from(version).expect("version"))
    .bind(digest.to_vec())
    .bind(event)
    .execute(db)
    .await
    .expect("suppression row");
}

async fn events(db: &mut PgConnection) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM messaging.optout_event ORDER BY id")
        .fetch_all(db)
        .await
        .expect("events")
}

async fn count(db: &mut PgConnection, sql: &'static str, contact: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(contact)
        .fetch_one(db)
        .await
        .expect("count")
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

/// TD-CON-30 (D-3753): a row and a last message 1,826 days old stay; a row whose last message and
/// own time are 1,827 days and one minute old goes, and only its ID comes back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_the_purge_keeps_1826_days_and_removes_1827_days_and_a_minute(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (kept_number, gone_number) = (Uuid::new_v4(), Uuid::new_v4());
    outbound(&mut db, kept_number, INSIDE).await;
    let inside_old = event(&mut db, digest(0x11), PAST).await;
    let inside_newest = event(&mut db, digest(0x11), INSIDE).await;
    outbound(&mut db, gone_number, PAST).await;
    let gone = event(&mut db, digest(0x22), PAST).await;
    let young = event(&mut db, digest(0x22), INSIDE).await;
    let newest = event(&mut db, digest(0x22), "1 day").await;
    set_app_role(&mut db).await.expect("app role");
    let kept = purge_optout_events(&mut db, kept_number, &[digest(0x11)]).await;
    assert_eq!(
        kept,
        Ok(Vec::new()),
        "a number messaged 1,826 days ago keeps its events"
    );
    let purged = purge_optout_events(&mut db, gone_number, &[digest(0x22)]).await;
    assert_eq!(
        purged,
        Ok(vec![gone]),
        "only the event past 1,827 days goes"
    );
    let left = events(&mut db).await;
    assert_eq!(left, sorted(vec![inside_old, inside_newest, young, newest]));
    let edge = event_at_edge(&mut db).await;
    assert!(
        edge.is_empty(),
        "the row-age floor holds at 1,827 days and a minute: {edge:?}"
    );
}

/// A number whose last message and newest event are 1,827 days and one minute old: its one event
/// is its newest, so it stays; the row-age and newest-event floors together.
async fn event_at_edge(db: &mut PgConnection) -> Vec<Uuid> {
    sqlx::query("RESET ROLE")
        .execute(&mut *db)
        .await
        .expect("reset");
    let number = Uuid::new_v4();
    outbound(db, number, PAST).await;
    event(db, digest(0x33), PAST).await;
    set_app_role(db).await.expect("app role");
    purge_optout_events(db, number, &[digest(0x33)])
        .await
        .expect("purge")
}

/// TD-CON-30: the event a suppression row references and the number's newest one (under another
/// index version) stay; the app role may not delete or change an opt-out event itself.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_a_referenced_and_the_newest_event_stay(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let number = Uuid::new_v4();
    let (v1, v2) = (digest(0x44), (2, [0x45; 32]));
    outbound(&mut db, number, "2000 days").await;
    let referenced = event(&mut db, v1, "2003 days").await;
    suppress(&mut db, v1, referenced).await;
    let plain = event(&mut db, v1, "2002 days").await;
    let newest = event(&mut db, v2, "2001 days").await;
    set_app_role(&mut db).await.expect("app role");
    let purged = purge_optout_events(&mut db, number, &[v1, v2]).await;
    assert_eq!(purged, Ok(vec![plain]));
    assert_eq!(events(&mut db).await, sorted(vec![referenced, newest]));
    let delete = sqlx::query("DELETE FROM messaging.optout_event WHERE id = $1")
        .bind(referenced)
        .execute(&mut *db)
        .await;
    assert!(
        is_permission_denied(&delete),
        "the app role never deletes an event"
    );
    let update = sqlx::query("UPDATE messaging.optout_event SET detected = 'owner' WHERE id = $1")
        .bind(newest)
        .execute(&mut *db)
        .await;
    assert!(
        is_permission_denied(&update),
        "the app role never changes an event"
    );
}

/// TD-CON-30 (D-3753): temporary `suppression` and `outbox` tables in the session change nothing,
/// and the app role cannot call `messaging.last_message_at`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_a_temporary_table_changes_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let number = Uuid::new_v4();
    outbound(&mut db, number, "2000 days").await;
    let referenced = event(&mut db, digest(0x55), "2004 days").await;
    suppress(&mut db, digest(0x55), referenced).await;
    let shadowed = event(&mut db, digest(0x55), "2003 days").await;
    let newest = event(&mut db, digest(0x55), "2002 days").await;
    set_app_role(&mut db).await.expect("app role");
    for sql in [
        "CREATE TEMP TABLE suppression (index_version smallint, digest bytea, event_id uuid, since timestamptz)",
        "CREATE TEMP TABLE outbox (recipient_id uuid, created_at timestamptz, sent_at timestamptz)",
    ] {
        sqlx::query(sql)
            .execute(&mut *db)
            .await
            .expect("temp table");
    }
    sqlx::query("INSERT INTO pg_temp.suppression (event_id) VALUES ($1)")
        .bind(shadowed)
        .execute(&mut *db)
        .await
        .expect("temp suppression");
    sqlx::query("INSERT INTO pg_temp.outbox VALUES ($1, now(), now())")
        .bind(number)
        .execute(&mut *db)
        .await
        .expect("temp outbox");
    let purged = purge_optout_events(&mut db, number, &[digest(0x55)]).await;
    assert_eq!(purged, Ok(vec![shadowed]), "the real tables decide");
    assert_eq!(events(&mut db).await, sorted(vec![referenced, newest]));
    let clock =
        sqlx::query_scalar::<_, Option<String>>("SELECT messaging.last_message_at($1)::text")
            .bind(number)
            .fetch_one(&mut *db)
            .await;
    assert!(
        is_permission_denied(&clock),
        "only the definer functions read the clock"
    );
}

/// TD-CON-30 (D-3155): a message in between restarts the clock, an inbound one through its
/// `inbox_contact` row and an outbound one through its outbox row; nothing of either number goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_a_message_in_between_restarts_the_clock(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (texted_us, texted_her) = (Uuid::new_v4(), Uuid::new_v4());
    outbound(&mut db, texted_us, "2000 days").await;
    inbound(&mut db, texted_us, "10 days").await;
    inbound(&mut db, texted_her, "2000 days").await;
    outbound(&mut db, texted_her, "10 days").await;
    event(&mut db, digest(0x66), "2002 days").await;
    event(&mut db, digest(0x66), "2001 days").await;
    event(&mut db, digest(0x67), "2002 days").await;
    event(&mut db, digest(0x67), "2001 days").await;
    set_app_role(&mut db).await.expect("app role");
    assert_eq!(due_numbers(&mut db, 100).await, Ok(Vec::new()));
    for (number, byte) in [(texted_us, 0x66), (texted_her, 0x67)] {
        let purged = purge_optout_events(&mut db, number, &[digest(byte)]).await;
        assert_eq!(purged, Ok(Vec::new()));
        let log = delete_message_log(&mut db, number)
            .await
            .expect("log delete");
        assert!(log.outbox.is_empty() && log.inbox.is_empty(), "{log:?}");
    }
    assert_eq!(events(&mut db).await.len(), 4);
}

/// TD-CON-30 (CON-022): the due numbers are those past 1,827 days, and the message-log deletes
/// remove only that number's outbox, retry, delivery, inbox and `inbox_contact` rows.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_the_message_log_deletes_remove_only_that_numbers_rows(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (gone, other, inside) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let sent = outbound(&mut db, gone, PAST).await;
    let received = inbound(&mut db, gone, "1900 days").await;
    outbound(&mut db, other, "2000 days").await;
    inbound(&mut db, other, "2000 days").await;
    outbound(&mut db, inside, INSIDE).await;
    set_app_role(&mut db).await.expect("app role");
    let due = sorted(vec![gone, other]);
    assert_eq!(due_numbers(&mut db, 100).await, Ok(due));
    let log = delete_message_log(&mut db, gone).await.expect("log delete");
    assert_eq!((log.outbox, log.inbox), (vec![sent], vec![received]));
    let not_due = delete_message_log(&mut db, inside).await.expect("not due");
    assert!(not_due.outbox.is_empty() && not_due.inbox.is_empty());
    for (number, rows) in [(gone, 0), (other, 1), (inside, 1)] {
        let outbox = "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1";
        assert_eq!(count(&mut db, outbox, number).await, rows);
        let delivery = "SELECT count(*) FROM messaging.delivery_event e JOIN messaging.outbox o \
                        ON o.provider_sid = e.provider_sid WHERE o.recipient_id = $1";
        assert_eq!(count(&mut db, delivery, number).await, rows);
    }
    let left = "SELECT (SELECT count(*) FROM messaging.inbox_contact WHERE contact_id = $1) \
                + (SELECT count(*) FROM messaging.inbox) + (SELECT count(*) FROM messaging.text_retry) \
                + (SELECT count(*) FROM messaging.delivery_event)";
    assert_eq!(
        count(&mut db, left, other).await,
        1 + 1 + 2 + 2,
        "the other numbers' rows stay"
    );
    assert_eq!(due_numbers(&mut db, 100).await, Ok(vec![other]));
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x32), &key(0x21), &url).expect("vault")
}

/// TD-CON-30 (D-3155): `on_inbound_text` records the sender's contact for each inbound text once,
/// so a resent delivery adds nothing, and that row is the clock's inbound half.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_an_inbound_text_records_its_contact_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let her = Uuid::new_v4();
    let inbox_id = Uuid::new_v4();
    let at: Timestamp = "2026-03-02T17:00:00Z".parse().expect("time");
    let msg = InboundText {
        inbox_id,
        contact: her,
        relation: Relation::OpenRequest,
        phone: "+19285550132",
        text: "thanks, see you then",
        provider: ProviderKeyword::None,
        emergency: false,
    };
    let secret = SecretString::from("fake-id-secret-for-tests".to_owned());
    for _ in 0..2 {
        on_inbound_text(&mut db, (&secret, &vault()), &msg, at)
            .await
            .expect("inbound");
    }
    let rows: Vec<(Uuid, Uuid, bool)> = sqlx::query_as(
        "SELECT inbox_id, contact_id, received_at = $1::timestamptz FROM messaging.inbox_contact",
    )
    .bind(at.to_string())
    .fetch_all(&mut *db)
    .await
    .expect("rows");
    assert_eq!(rows, vec![(inbox_id, her, true)]);
}
