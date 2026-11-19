//! TD-CON-30 (CON-022, CON-036; D-079, D-2208, D-3155, D-3753, D-3967, D-4387, D-4418): the
//! five-year clock, the opt-out events' purge and the message-log deletes (T321), and the `intake`
//! purge, the daily sweep and its vault phone clear (T563), on fresh databases.
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
//! database clock.
//!
//! T563's cases: `intake.purge_expired_consent` keeps 1,826 days, removes 1,827 days and a minute
//! and keeps a 6-year-old record of a number texted yesterday; through the sweep a number at 1,826
//! days keeps everything and one at 1,827 days not on suppression loses its rows and its vault
//! phone, with audit rows of IDs only; an opted-out number keeps its suppression row, the event it
//! points to and its phone, and is refused a text after the purge and after `rotate_batch`; a
//! message in between restarts the sweep's clock; the append-only tables refuse the app role's
//! UPDATE and DELETE; and D-4418's five cases (an opt-out before the step, the sweep across a
//! rotation, a v1-only contact kept then cleared, a held contact row, an opt-out after a clear).
//! Every ID, digest, SID and phone is fake.

use std::time::Duration;

use askhava_intake::api::{SweepStep, due_number, sweep_consent, sweep_number};
use askhava_integration::{is_permission_denied, set_app_role};
use askhava_messaging::api::{
    self as listing, InboundText, ProviderKeyword, Relation, RevokeChannel, SendWindow,
    delete_message_log, due_numbers, listing_key, on_inbound_text, purge_optout_events,
    send_window, still_unlisted,
};
use askhava_privacy::api::{IndexPurpose, NewContact, Vault, rotate_batch, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection as _, PgConnection, PgPool};

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

/// A `<version>:<base64>` key entry of 32 copies of `byte`: `tests/integration/tests/vault.rs`'s
/// `key(version, byte)` helper (TD-PRIV-09; each test file is its own crate).
fn key(version: u16, byte: u8) -> String {
    format!("{version}:{}", STANDARD.encode([byte; 32]))
}

fn vault_of(seal: &str, index: &str) -> Vault {
    let secret = |s: &str| SecretString::from(s.to_owned());
    let url = "postgres://askhava_app@db.invalid:5432/askhava";
    Vault::new(&secret(seal), &secret(index), &secret(url)).expect("vault keys")
}

/// Seal and index keys at v1 only.
fn v1() -> Vault {
    vault_of(&key(1, 0x11), &key(1, 0x21))
}

/// Seal v2 active beside v1, index keys v1 and v2 (`the_rotation_drill_moves_every_row_to_v2`).
fn both() -> Vault {
    vault_of(
        &format!("{},{}", key(2, 0x12), key(1, 0x11)),
        &format!("{},{}", key(1, 0x21), key(2, 0x22)),
    )
}

fn now() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

/// A vault contact with fake phone `phone`, indexed under `vault`'s versions.
async fn person(db: &mut PgConnection, vault: &Vault, phone: &str) -> Uuid {
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    store_contact(db, vault, &new).await.expect("store")
}

/// A homeowner for `contact` with one SMS consent record given `ago`; returns the record's ID.
async fn consent(db: &mut PgConnection, contact: Uuid, ago: &str) -> Uuid {
    sqlx::query_scalar(
        "WITH h AS (INSERT INTO intake.homeowner (contact_id, created_at) \
           VALUES ($1, now() - $2::interval) RETURNING id), \
         c AS (INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
           consent_template, consent_sent_at, opened_at) SELECT h.id, 'en', gen_random_uuid(), \
           'consent_en', now() - $2::interval, now() - $2::interval FROM h RETURNING id, homeowner_id) \
         INSERT INTO intake.consent_record (homeowner_id, conversation_id, channel, inbox_id, \
           consent_version, consented_at) SELECT c.homeowner_id, c.id, 'sms', gen_random_uuid(), \
           'con_031.v1', now() - $2::interval FROM c RETURNING id",
    )
    .bind(contact)
    .bind(ago)
    .fetch_one(db)
    .await
    .expect("consent record")
}

/// `contact`'s CON-022 rows: consent records, outbox rows and inbound contact rows.
async fn rows_of(db: &mut PgConnection, contact: Uuid) -> i64 {
    count(
        db,
        "SELECT (SELECT count(*) FROM intake.consent_record r JOIN intake.homeowner h \
           ON h.id = r.homeowner_id WHERE h.contact_id = $1) \
         + (SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1) \
         + (SELECT count(*) FROM messaging.inbox_contact WHERE contact_id = $1)",
        contact,
    )
    .await
}

/// A consent record, an outbound and an inbound text for `contact`, all `ago`; returns their IDs.
async fn seed(db: &mut PgConnection, contact: Uuid, ago: &str) -> [Uuid; 3] {
    let record = consent(db, contact, ago).await;
    [
        record,
        outbound(db, contact, ago).await,
        inbound(db, contact, ago).await,
    ]
}

async fn has_phone(db: &mut PgConnection, contact: Uuid) -> bool {
    count(
        db,
        "SELECT count(*) FROM privacy.contact WHERE id = $1 AND phone IS NOT NULL",
        contact,
    )
    .await
        == 1
}

/// The sweep's step for `contact` with its key built under `vault` now.
async fn step(db: &mut PgConnection, vault: &Vault, contact: Uuid) -> SweepStep {
    let due = due_number(db, Some(vault), contact).await.expect("due");
    sweep_number(db, &due).await.expect("step")
}

async fn list(db: &mut PgConnection, vault: &Vault, phone: &str) -> bool {
    listing::suppress(db, vault, phone, (RevokeChannel::Portal, "portal"), now())
        .await
        .expect("suppress")
}

/// Whether a text to `contact` is refused because her number is listed.
async fn refused(db: &mut PgConnection, vault: &Vault, contact: Uuid) -> bool {
    send_window(db, vault, contact, now()).await == Ok(SendWindow::OptedOut)
}

/// The suppression rows held under `phone`'s digests.
async fn listed(db: &mut PgConnection, vault: &Vault, phone: &str) -> i64 {
    let digests: Vec<Vec<u8>> = vault
        .blind_index(IndexPurpose::HomeownerPhone, phone)
        .into_iter()
        .map(|(_, d)| d.to_vec())
        .collect();
    sqlx::query_scalar("SELECT count(*) FROM messaging.suppression WHERE digest = ANY ($1)")
        .bind(digests)
        .fetch_one(db)
        .await
        .expect("listed")
}

/// TD-CON-30 (T563, D-3753): `intake.purge_expired_consent` with the database clock keeps a record
/// and a last message 1,826 days old, removes one whose last message and own time are 1,827 days and
/// a minute old, and keeps a 6-year-old record of a number last messaged yesterday.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_the_intake_purge_reads_the_last_message(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let [inside, past, recent] = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    consent(&mut db, inside, INSIDE).await;
    outbound(&mut db, inside, INSIDE).await;
    let gone = consent(&mut db, past, PAST).await;
    outbound(&mut db, past, PAST).await;
    consent(&mut db, recent, "2191 days").await;
    inbound(&mut db, recent, "1 day").await;
    set_app_role(&mut db).await.expect("app role");
    for (contact, want) in [(inside, vec![]), (past, vec![gone]), (recent, vec![])] {
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT intake.purge_expired_consent($1)")
            .bind(contact)
            .fetch_all(&mut *db)
            .await
            .expect("purge");
        assert_eq!(ids, want);
    }
    assert_eq!(
        rows_of(&mut db, inside).await + rows_of(&mut db, recent).await,
        2 + 2
    );
}

/// TD-CON-30 (T563): through the sweep, a number last messaged 1,826 days ago keeps everything;
/// at 1,827 days a number not on suppression loses its CON-022 rows and its vault phone, with audit
/// rows of IDs only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_the_sweep_keeps_1826_days_and_clears_1827(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let vault = v1();
    let kept = person(&mut db, &vault, "+19285550141").await;
    let due = person(&mut db, &vault, "+19285550142").await;
    let kept_event = event(
        &mut db,
        vault.blind_index(IndexPurpose::HomeownerPhone, "+19285550141")[0],
        INSIDE,
    )
    .await;
    seed(&mut db, kept, INSIDE).await;
    let gone = seed(&mut db, due, PAST).await;
    set_app_role(&mut db).await.expect("app role");
    assert_eq!(step(&mut db, &vault, kept).await, SweepStep::Kept);
    assert_eq!(sweep_consent(&mut db, Some(&vault)).await, Ok(1));
    assert_eq!(
        (rows_of(&mut db, kept).await, has_phone(&mut db, kept).await),
        (3, true)
    );
    assert_eq!(events(&mut db).await, vec![kept_event]);
    assert_eq!(
        (rows_of(&mut db, due).await, has_phone(&mut db, due).await),
        (0, false)
    );
    let audit: Vec<(String, String, Uuid, Option<String>)> = sqlx::query_as(
        "SELECT action, subject_kind, subject_id, field FROM ops.audit_log \
         WHERE action IN ('consent_purged', 'consent_phone_cleared') ORDER BY id",
    )
    .fetch_all(&mut *db)
    .await
    .expect("audit");
    let purged = |kind: &str, id: Uuid| ("consent_purged".into(), kind.into(), id, None);
    assert_eq!(
        audit,
        vec![
            purged("intake.consent_record", gone[0]),
            purged("messaging.outbox", gone[1]),
            purged("messaging.inbox", gone[2]),
            (
                "consent_phone_cleared".into(),
                "privacy.contact".into(),
                due,
                Some("phone".into())
            ),
        ]
    );
}

/// TD-CON-30 (T563, D-079): an opted-out number keeps its suppression row, the opt-out event it
/// points to and its locked phone, loses its other rows, and is still refused a text after the
/// purge and after an index-key rotation; a message in between restarts the sweep's clock.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_an_opted_out_number_keeps_its_phone_through_rotation(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let old = v1();
    let her = person(&mut db, &old, "+19285550143").await;
    let late = person(&mut db, &old, "+19285550144").await;
    seed(&mut db, her, PAST).await;
    seed(&mut db, late, PAST).await;
    outbound(&mut db, late, "10 days").await;
    set_app_role(&mut db).await.expect("app role");
    assert!(list(&mut db, &old, "+19285550143").await);
    let pointed: Uuid = sqlx::query_scalar("SELECT event_id FROM messaging.suppression")
        .fetch_one(&mut *db)
        .await
        .expect("listing");
    assert_eq!(step(&mut db, &old, her).await, SweepStep::Kept);
    assert_eq!(step(&mut db, &old, late).await, SweepStep::Kept);
    assert_eq!(
        (rows_of(&mut db, her).await, has_phone(&mut db, her).await),
        (0, true)
    );
    assert_eq!(
        (rows_of(&mut db, late).await, has_phone(&mut db, late).await),
        (4, true)
    );
    assert_eq!(events(&mut db).await, vec![pointed]);
    assert!(refused(&mut db, &old, her).await, "refused after the purge");
    let new = both();
    while rotate_batch(&mut db, &new, 10).await.expect("rotate") > 0 {}
    assert!(
        refused(&mut db, &new, her).await,
        "refused after the rotation"
    );
    assert_eq!(step(&mut db, &new, her).await, SweepStep::Kept);
    assert!(has_phone(&mut db, her).await);
}

/// TD-CON-30 (T563, D-3753): the app role still cannot UPDATE or DELETE a consent record or an
/// opt-out event, a referenced one included.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_the_append_only_tables_stay_append_only(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let her = Uuid::new_v4();
    consent(&mut db, her, PAST).await;
    let id = event(&mut db, digest(0x51), PAST).await;
    suppress(&mut db, digest(0x51), id).await;
    set_app_role(&mut db).await.expect("app role");
    for sql in [
        "UPDATE intake.consent_record SET consented_at = now()",
        "DELETE FROM intake.consent_record",
        "UPDATE messaging.optout_event SET effective_at = now()",
        "DELETE FROM messaging.optout_event",
    ] {
        let outcome = sqlx::query(sql).execute(&mut *db).await;
        assert!(is_permission_denied(&outcome), "{sql} refused");
    }
}

/// TD-CON-30 (T563, D-4418): with the listing key built, a `suppress` committed before the
/// number's transaction keeps its phone and suppression row; a `suppress` after the sweep cleared
/// a phone lists the number, which is then refused a text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_an_opt_out_before_the_step_keeps_the_phone(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let vault = v1();
    let her = person(&mut db, &vault, "+19285550145").await;
    let cleared = person(&mut db, &vault, "+19285550146").await;
    seed(&mut db, her, PAST).await;
    seed(&mut db, cleared, PAST).await;
    set_app_role(&mut db).await.expect("app role");
    let due = due_number(&mut db, Some(&vault), her).await.expect("due");
    assert!(list(&mut db, &vault, "+19285550145").await);
    assert_eq!(sweep_number(&mut db, &due).await, Ok(SweepStep::Kept));
    assert!(has_phone(&mut db, her).await);
    assert_eq!(listed(&mut db, &vault, "+19285550145").await, 1);
    assert_eq!(step(&mut db, &vault, cleared).await, SweepStep::Cleared);
    assert!(list(&mut db, &vault, "+19285550146").await, "newly listed");
    assert_eq!(listed(&mut db, &vault, "+19285550146").await, 1);
    let key = listing_key(
        &vault,
        &vault.blind_index(IndexPurpose::HomeownerPhone, "+19285550146"),
    )
    .expect("key");
    let mut tx = db.begin().await.expect("begin");
    assert_eq!(
        still_unlisted(&mut tx, &key).await,
        Ok(false),
        "refused a text"
    );
    tx.rollback().await.expect("rollback");
    assert!(
        send_window(&mut db, &vault, cleared, now()).await.is_err(),
        "no phone to text"
    );
}

/// TD-CON-30 (T563, D-4418): with v1 and v2 configured after `rotate_batch`, a due number not on
/// suppression loses its phone and rows and one listed while only v1 existed keeps its phone; a
/// contact holding only its v1 digest keeps its phone that run and loses it once rotated.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_the_sweep_across_an_index_key_rotation(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (old, new) = (v1(), both());
    let free = person(&mut db, &old, "+19285550147").await;
    let listed_v1 = person(&mut db, &old, "+19285550148").await;
    let waiting = person(&mut db, &old, "+19285550149").await;
    for contact in [free, listed_v1, waiting] {
        seed(&mut db, contact, PAST).await;
    }
    set_app_role(&mut db).await.expect("app role");
    assert!(list(&mut db, &old, "+19285550148").await);
    assert_eq!(step(&mut db, &new, waiting).await, SweepStep::Kept);
    assert_eq!(
        (
            rows_of(&mut db, waiting).await,
            has_phone(&mut db, waiting).await
        ),
        (0, true)
    );
    while rotate_batch(&mut db, &new, 10).await.expect("rotate") > 0 {}
    assert_eq!(sweep_consent(&mut db, Some(&new)).await, Ok(2));
    assert!(!has_phone(&mut db, free).await && !has_phone(&mut db, waiting).await);
    assert_eq!(
        (
            rows_of(&mut db, free).await,
            has_phone(&mut db, listed_v1).await
        ),
        (0, true)
    );
    assert!(refused(&mut db, &new, listed_v1).await);
}

/// TD-CON-30 (T563, D-4387): a rotation that commits between `due_number` and the step changes
/// the contact's digests, so the step keeps its phone and deletes its rows; the contact waits in
/// `intake.consent_phone_wait` and the next run clears the phone (D-079, CON-022).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_digests_changed_under_the_step_wait_for_the_next_run(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (old, new) = (v1(), both());
    let her = person(&mut db, &old, "+19285550151").await;
    seed(&mut db, her, PAST).await;
    set_app_role(&mut db).await.expect("app role");
    let due = due_number(&mut db, Some(&old), her).await.expect("due");
    while rotate_batch(&mut db, &new, 10).await.expect("rotate") > 0 {}
    assert_eq!(sweep_number(&mut db, &due).await, Ok(SweepStep::Kept));
    assert_eq!(
        (rows_of(&mut db, her).await, has_phone(&mut db, her).await),
        (0, true)
    );
    assert_eq!(sweep_consent(&mut db, Some(&new)).await, Ok(1));
    assert!(!has_phone(&mut db, her).await);
}

/// TD-CON-30 (T563, D-4418): while another transaction holds the number's `privacy.contact` row,
/// its sweep step returns without waiting and clears and deletes nothing; a later run clears it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_30_a_held_contact_row_is_left_for_the_next_run(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let vault = v1();
    let her = person(&mut db, &vault, "+19285550150").await;
    seed(&mut db, her, PAST).await;
    set_app_role(&mut db).await.expect("app role");
    let mut holder = pool.acquire().await.expect("holder");
    let mut held = holder.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM privacy.contact WHERE id = $1 FOR UPDATE")
        .bind(her)
        .execute(&mut *held)
        .await
        .expect("hold the row");
    let waited = tokio::time::timeout(Duration::from_secs(10), step(&mut db, &vault, her)).await;
    assert_eq!(waited.expect("returned without waiting"), SweepStep::Busy);
    assert_eq!(
        (rows_of(&mut db, her).await, has_phone(&mut db, her).await),
        (3, true)
    );
    held.commit().await.expect("release");
    assert_eq!(sweep_consent(&mut db, Some(&vault)).await, Ok(1));
    assert_eq!(
        (rows_of(&mut db, her).await, has_phone(&mut db, her).await),
        (0, false)
    );
}
