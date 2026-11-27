//! The consent races: a send against an opt-out, and a START against a later STOP (TD-CON-39,
//! TD-CON-40, TD-CON-41; CON-013, CON-015, CON-035; D-5382, D-5383, D-5388).
//!
//! Proves, against PostgreSQL, that an opt-out committing after `send_checked`'s list check but
//! before the claim (another connection holds the outbox row `FOR UPDATE`, the claim seen waiting
//! by `pg_blocking_pids`) stops the text: no carrier call, the row `queued` again and expired by
//! the sender's next pass; that one committed in the holder's own transaction, the latest a test
//! can place it, stops it too, a queued HELP reply included; that with no opt-out the row is sent
//! once and the listing's own confirmation still goes; that a START received before a later STOP
//! lifts nothing however late it runs, in either lock order, while one received after it lifts;
//! and that the inbound path's START with no inbox row fails closed with one stale-START alert.
//! Fake numbers and keys only.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_core::api::{Clock, Lease, ManualClock, Registry, WorkerId, claim};
use askhava_messaging::api::{
    InboundText, NewText, OutboxError, OutboxSender, Outcome, ProviderKeyword, Relation,
    RevokeChannel, SendingNumber, To, enqueue, on_inbound_text, on_received_text,
    register_send_due, schedule_send_due, send_checked, suppress, suppress_indexed,
};
use askhava_privacy::api::{IndexPurpose, NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::task::JoinHandle;

const FROM: &str = "+19285550100";
const PHONE: &str = "+19285550181";
const OTHER: &str = "+19285550182";

/// A fake carrier that records every call and accepts it.
#[derive(Clone, Default)]
struct FakeCarrier {
    sent: Arc<Mutex<Vec<String>>>,
}

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        sent.push(to.to_owned());
        Ok(SentText {
            sid: format!("SM{:032}", sent.len()),
            status: "queued".to_owned(),
        })
    }
}

impl FakeCarrier {
    fn calls(&self) -> usize {
        self.sent.lock().expect("lock").len()
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

/// `secs` after 10 a.m. Arizona time on a July day, inside both quiet-hours windows (QH-001).
fn at(secs: i64) -> Timestamp {
    let ten = jiff::civil::date(2026, 7, 15)
        .at(10, 0, 0, 0)
        .in_tz("America/Phoenix")
        .expect("zone")
        .timestamp();
    ten + SignedDuration::from_secs(secs)
}

async fn contact(db: &mut PgConnection, phone: &str) -> Uuid {
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// Queues `template` for `her` about `job`.
async fn queue(db: &mut PgConnection, her: Uuid, template: &str, job: u128) -> Uuid {
    let new = NewText {
        template_id: template,
        subject: &[Uuid::from_u128(job)],
        recipient: her,
        relation: Relation::OpenRequest,
        body: "Ask Hava: fake body",
    };
    enqueue(db, &secret(), &new).await.expect("queued").0
}

async fn try_send(
    db: &mut PgConnection,
    carrier: &FakeCarrier,
    id: Uuid,
    secs: i64,
) -> Result<Option<SentText>, OutboxError> {
    let vault = vault();
    let to = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    send_checked(db, (&secret(), carrier, &vault), id, (to, FROM), at(secs)).await
}

async fn pid(conn: &mut PgConnection) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(conn)
        .await
        .expect("pid")
}

/// A bounded poll until backend `pid` is blocked by backend `holder`; fails, never hangs.
async fn blocked_by(pool: &PgPool, pid: i32, holder: i32) {
    for _ in 0..200 {
        let blocked: bool = sqlx::query_scalar("SELECT $2 = ANY (pg_blocking_pids($1))")
            .bind(pid)
            .bind(holder)
            .fetch_one(pool)
            .await
            .expect("pg_blocking_pids");
        if blocked {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("backend {pid} never waited on backend {holder}");
}

/// The advisory-lock wait predicate of `optout_number_lock.rs`: a bounded poll of `pg_locks`.
async fn waits_on_advisory(pool: &PgPool, pid: i32) {
    for _ in 0..200 {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks \
             WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
        )
        .bind(pid)
        .fetch_one(pool)
        .await
        .expect("pg_locks");
        if waiting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("backend {pid} never waited on an advisory lock");
}

async fn finish<T>(task: JoinHandle<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("finished in time")
        .expect("task")
}

fn digests(phone: &str) -> Vec<(u16, [u8; 32])> {
    vault().blind_index(IndexPurpose::HomeownerPhone, phone)
}

/// Where the race's opt-out of the number commits, if anywhere.
#[derive(Clone, Copy)]
enum OptOut {
    None,
    /// On its own connection while the row is held: before the claim.
    Before,
    /// In the holder's transaction, visible only as the holder releases the row.
    InHolder,
}

/// Sends row `id` to `phone` at 60 s while another connection holds the row `FOR UPDATE`; once
/// the claim is seen waiting on it, the portal opt-out commits as `opt_out` says.
async fn held_send(
    pool: &PgPool,
    (carrier, id, phone): (&FakeCarrier, Uuid, &str),
    opt_out: OptOut,
) -> Result<Option<SentText>, OutboxError> {
    let mut holder = pool.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM messaging.outbox WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holder)
        .await
        .expect("row lock");
    let holder_pid = pid(&mut holder).await;
    let mut conn = pool.acquire().await.expect("connection");
    let sender_pid = pid(&mut conn).await;
    let carrier_copy = carrier.clone();
    let send = tokio::spawn(async move { try_send(&mut conn, &carrier_copy, id, 60).await });
    blocked_by(pool, sender_pid, holder_pid).await;
    let portal = (RevokeChannel::Portal, "portal");
    match opt_out {
        OptOut::None => {}
        OptOut::Before => {
            let mut other = pool.acquire().await.expect("connection");
            let added = suppress(&mut other, &vault(), phone, portal, at(30)).await;
            assert_eq!(added, Ok(true), "listed before the claim");
        }
        OptOut::InHolder => {
            let added = suppress_indexed(&mut holder, &digests(phone), portal, at(30)).await;
            assert_eq!(added, Ok(true), "listed in the holder's transaction");
        }
    }
    holder.commit().await.expect("commit");
    finish(send).await
}

async fn row(pool: &PgPool, id: Uuid) -> (String, i32) {
    sqlx::query_as("SELECT status, attempts FROM messaging.outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row")
}

/// Runs the sender's `send_due` pass at 120 s, as the worker would.
async fn sender_pass(pool: &PgPool, carrier: &FakeCarrier) {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(at(120)));
    let number = SendingNumber::parse(FROM).ok();
    let sender = OutboxSender::new((secret(), vault()), carrier.clone(), (number, None), clock)
        .expect("sender");
    let mut registry = Registry::new();
    register_send_due(&mut registry, Arc::new(sender));
    let mut db = pool.acquire().await.expect("connection");
    schedule_send_due(&mut db, at(120)).await.expect("schedule");
    let owner = WorkerId::new(pool).await.expect("worker");
    let tasks = claim(pool, owner, at(120), SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    let manual = ManualClock::new(at(120));
    for task in tasks {
        registry
            .run(pool, &manual, owner, task, Lease::WORKER)
            .await
            .expect("run");
    }
}

/// TD-CON-39, CON-013, CON-035: an opt-out committed after the list check and before the claim
/// stops the text; the row is `queued` again with its attempt counted, and the next pass expires it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_39_an_opt_out_before_the_claim_stops_the_text(pool: PgPool) {
    let carrier = FakeCarrier::default();
    let mut db = pool.acquire().await.expect("connection");
    let her = contact(&mut db, PHONE).await;
    let id = queue(&mut db, her, "first_quote", 1).await;
    let sent = held_send(&pool, (&carrier, id, PHONE), OptOut::Before).await;
    assert_eq!(sent, Err(OutboxError::Suppressed));
    assert_eq!(carrier.calls(), 0, "no carrier call");
    assert_eq!(row(&pool, id).await, ("queued".to_owned(), 1));
    sender_pass(&pool, &carrier).await;
    assert_eq!(row(&pool, id).await.0, "expired");
    assert_eq!(carrier.calls(), 0, "the next pass sends nothing either");
}

/// TD-CON-39, CON-013, D-5280, D-5388: the opt-out committed in the transaction that holds the
/// row, so it is visible only as the claim proceeds to the re-read, stops a text and a HELP reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_39_an_opt_out_released_with_the_row_stops_the_text_and_the_help_reply(
    pool: PgPool,
) {
    let carrier = FakeCarrier::default();
    let mut db = pool.acquire().await.expect("connection");
    for (job, phone, template) in [(1, PHONE, "first_quote"), (2, OTHER, "help_reply")] {
        let her = contact(&mut db, phone).await;
        let id = queue(&mut db, her, template, job).await;
        let sent = held_send(&pool, (&carrier, id, phone), OptOut::InHolder).await;
        assert_eq!(sent, Err(OutboxError::Suppressed), "{template}");
        assert_eq!(row(&pool, id).await, ("queued".to_owned(), 1), "{template}");
    }
    assert_eq!(carrier.calls(), 0, "no carrier call");
}

/// TD-CON-39, CON-009: with no opt-out the held row is sent once; the current listing's own
/// confirmation within 5 minutes still goes past the re-read.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_39_without_an_opt_out_the_row_goes_once_and_the_confirmation_goes(pool: PgPool) {
    let carrier = FakeCarrier::default();
    let mut db = pool.acquire().await.expect("connection");
    let her = contact(&mut db, PHONE).await;
    let id = queue(&mut db, her, "first_quote", 1).await;
    let sent = held_send(&pool, (&carrier, id, PHONE), OptOut::None).await;
    assert!(sent.expect("sent").is_some());
    assert_eq!(row(&pool, id).await.0, "sent");
    assert_eq!(try_send(&mut db, &carrier, id, 61).await, Ok(None), "once");
    let stop = start_or_stop(her, (PHONE, "Opt out"), Uuid::from_u128(0x39));
    let outcome = on_inbound_text(&mut db, (&secret(), &vault()), &stop, at(30)).await;
    let Ok(Outcome::OptedOut {
        confirmation: Some(confirmation),
        ..
    }) = outcome
    else {
        panic!("the app confirms an opt-out the provider did not answer: {outcome:?}");
    };
    let confirmed = held_send(&pool, (&carrier, confirmation, PHONE), OptOut::None).await;
    assert!(confirmed.expect("confirmation sent").is_some());
    assert_eq!(carrier.calls(), 2, "the row and the confirmation");
}

/// An inbound text from `phone` with `text` ("START" is the provider's START) and inbox ID `inbox`.
fn start_or_stop<'a>(her: Uuid, (phone, text): (&'a str, &'a str), inbox: Uuid) -> InboundText<'a> {
    let provider = match text {
        "START" => ProviderKeyword::Start,
        "STOP" => ProviderKeyword::OptOut,
        _ => ProviderKeyword::None,
    };
    InboundText {
        inbox_id: inbox,
        contact: her,
        relation: Relation::OpenRequest,
        phone,
        text,
        provider,
        emergency: false,
    }
}

/// Records inbox row `n`, received at `secs`.
async fn inbox(db: &mut PgConnection, n: u128, secs: i64) -> Uuid {
    let id = Uuid::from_u128(n);
    sqlx::query("INSERT INTO messaging.inbox (id, message_sid, received_at) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(format!("SM{n:032}"))
        .bind(at(secs).to_sqlx())
        .execute(db)
        .await
        .expect("inbox row");
    id
}

/// Runs text `text` from `her` at `secs` with inbox row `id` through `on_inbound_text`.
async fn text(db: &mut PgConnection, her: Uuid, (text, id): (&str, Uuid), secs: i64) -> Outcome {
    let msg = start_or_stop(her, (PHONE, text), id);
    on_inbound_text(db, (&secret(), &vault()), &msg, at(secs))
        .await
        .expect("inbound")
}

async fn listed(pool: &PgPool) -> bool {
    let (versions, hashes): (Vec<i16>, Vec<Vec<u8>>) = digests(PHONE)
        .iter()
        .map(|(v, h)| (i16::try_from(*v).expect("version"), h.to_vec()))
        .unzip();
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM messaging.suppression s \
         JOIN UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON s.index_version = k.v AND s.digest = k.h)",
    )
    .bind(versions)
    .bind(hashes)
    .fetch_one(pool)
    .await
    .expect("listed")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

const STARTS: &str = "SELECT count(*) FROM messaging.optout_event WHERE action = 'start'";

/// TD-CON-40, CON-015, CON-035, D-5388: a START received before a later STOP, rerun afterwards,
/// lifts nothing; one received after it lifts; the inbound entry fails closed on a missing row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_40_a_start_older_than_the_last_stop_lifts_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let her = contact(&mut db, PHONE).await;
    let first = inbox(&mut db, 0x400, 0).await;
    text(&mut db, her, ("STOP", first), 0).await;
    let old_start = inbox(&mut db, 0x401, 100).await;
    let later_stop = inbox(&mut db, 0x402, 200).await;
    text(&mut db, her, ("STOP", later_stop), 200).await;
    let rerun = text(&mut db, her, ("START", old_start), 300).await;
    assert_eq!(rerun, Outcome::Suppressed { alert: None });
    assert!(listed(&pool).await, "the later STOP's listing stays");
    assert_eq!(count(&pool, STARTS).await, 0, "no start event");
    let missing = Uuid::from_u128(0x4ff);
    let msg = start_or_stop(her, (PHONE, "START"), missing);
    let stale = on_received_text(&mut db, (&secret(), &vault()), &msg, at(350)).await;
    assert_eq!(stale, Ok(Outcome::Suppressed { alert: None }));
    assert!(
        listed(&pool).await,
        "a START with no inbox row lifts nothing"
    );
    assert_eq!(count(&pool, STARTS).await, 0, "no start event");
    let alerts: Vec<(String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT severity, subject_kind, subject_id FROM ops.alert WHERE kind = 'messaging.stale_start'",
    )
    .fetch_all(&pool)
    .await
    .expect("alerts");
    let expected = (
        "normal".to_owned(),
        "messaging.inbox".to_owned(),
        Some(missing),
    );
    assert_eq!(
        alerts,
        vec![expected],
        "one alert holding the inbox ID only"
    );
    let new_start = inbox(&mut db, 0x403, 400).await;
    let lifted = on_received_text(
        &mut db,
        (&secret(), &vault()),
        &start_or_stop(her, (PHONE, "START"), new_start),
        at(400),
    )
    .await;
    assert_eq!(lifted, Ok(Outcome::Resubscribed), "CON-015");
    assert!(!listed(&pool).await);
    assert_eq!(count(&pool, STARTS).await, 1);
}

/// TD-CON-41, CON-009, CON-015: a START that commits first, then a STOP received after it: the
/// STOP creates the listing again and queues its confirmation; the next text is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_41_a_start_first_then_a_later_stop_ends_listed(pool: PgPool) {
    let (carrier, mut db) = (FakeCarrier::default(), pool.acquire().await.expect("db"));
    let her = contact(&mut db, PHONE).await;
    let first = inbox(&mut db, 0x410, 0).await;
    text(&mut db, her, ("STOP", first), 0).await;
    let start = inbox(&mut db, 0x411, 100).await;
    assert_eq!(
        text(&mut db, her, ("START", start), 100).await,
        Outcome::Resubscribed
    );
    let stop = inbox(&mut db, 0x412, 200).await;
    let Outcome::OptedOut {
        confirmation: Some(_),
        ..
    } = text(&mut db, her, ("Opt out", stop), 200).await
    else {
        panic!("the STOP that creates the listing queues the CON-009 confirmation");
    };
    assert!(listed(&pool).await);
    let next = queue(&mut db, her, "first_quote", 41).await;
    assert_eq!(
        try_send(&mut db, &carrier, next, 400).await,
        Err(OutboxError::Suppressed)
    );
    assert_eq!(carrier.calls(), 0);
}

/// TD-CON-41, CON-015, CON-035: the STOP's transaction holds the number's lock while an earlier
/// START waits on it; the STOP commits, and the START then lifts nothing: the STOP wins.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_41_a_start_waiting_on_a_later_stop_lifts_nothing(pool: PgPool) {
    let (carrier, mut db) = (FakeCarrier::default(), pool.acquire().await.expect("db"));
    let her = contact(&mut db, PHONE).await;
    let first = inbox(&mut db, 0x420, 0).await;
    text(&mut db, her, ("STOP", first), 0).await;
    let start = inbox(&mut db, 0x421, 100).await;
    let mut stop = pool.begin().await.expect("begin");
    let portal = (RevokeChannel::Portal, "portal");
    let renewed = suppress_indexed(&mut stop, &digests(PHONE), portal, at(200)).await;
    assert_eq!(renewed, Ok(false), "already listed: a repeat, logged");
    let mut conn = pool.acquire().await.expect("connection");
    let start_pid = pid(&mut conn).await;
    let waiting = tokio::spawn(async move { text(&mut conn, her, ("START", start), 250).await });
    waits_on_advisory(&pool, start_pid).await;
    stop.commit().await.expect("commit");
    assert_eq!(finish(waiting).await, Outcome::Suppressed { alert: None });
    assert!(listed(&pool).await, "the STOP wins");
    let next = queue(&mut db, her, "first_quote", 42).await;
    assert_eq!(
        try_send(&mut db, &carrier, next, 400).await,
        Err(OutboxError::Suppressed)
    );
    assert_eq!(count(&pool, STARTS).await, 0);
}
