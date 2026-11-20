//! Queued business texts are sent on the business line, and a closed job's rows are expired
//! (BIZ-032, BIZ-036, BIZ-037, CON-019, TXT-035, QH-001, QH-003, QH-007, SEC-047; D-2042, D-4051,
//! D-4764, D-4913; T573).
//!
//! On fresh databases, with a fake carrier that records each call's numbers (as `outbox.rs`): a
//! notice row and an auto reply to a consented linked phone go once from the business number,
//! each with one `vault_read.server.business_text` audit row and no number in clear in any audit
//! row or outbox column; a phone without the CON-019 box, a recipient that is a business ID and
//! a line with no stored number expire unread; with the switch off, no business number or no
//! builder the rows wait unread; a 213 phone is held by its own zone's window; the clock is read
//! for each row; and `expire_job_rows` expires a closed job's earlier rows except the close
//! texts. All data is fake.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_businesses::api::{ConsentChannel, add_owner, create, may_text, record_consent};
use askhava_core::api::{Clock, ManualClock, WorkerId, claim};
use askhava_messaging::api::{
    CLOSE_TEXTS, EXPIRED_ACTION, LinePhones, NewText, OutboxSender, PhoneUnread, Relation,
    SendChecks, SendingNumber, enqueue, expire_job_rows, run_send_due, schedule_send_due,
    sender_add,
};
use askhava_privacy::api::{
    BusinessContactError, BusinessPurpose, Vault, read_staff_phone, store_staff_phone,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const SHOP: &str = "+19285550142";
const LA_SHOP: &str = "+12135550143";
const HOMEOWNER_LINE: &str = "+19285550100";
const BUSINESS_LINE: &str = "+19285550101";
/// Noon in Arizona and 11 a.m. in Los Angeles.
const NOON: &str = "2026-11-10T19:00:00Z";
const READS: &str = "SELECT count(*) FROM ops.audit_log WHERE action = $1";

/// A fake carrier that records each call's `(to, from)`.
#[derive(Clone, Default)]
struct FakeCarrier(Arc<Mutex<Vec<(String, String)>>>);

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        let mut calls = self.0.lock().expect("lock");
        calls.push((to.to_owned(), from.to_owned()));
        Ok(SentText {
            sid: format!("SM{:032}", calls.len()),
            status: "queued".to_owned(),
        })
    }
}

/// The composition's gate and reader: `businesses::api::may_text` and the vault's phone read
/// under `BusinessText`, as `server`'s `NoticeChecks` and `business_text_phone` are.
struct Side;

impl SendChecks for Side {
    async fn may_text(&self, conn: &mut PgConnection, business: Uuid, phone: Uuid) -> bool {
        may_text(conn, business, phone).await.unwrap_or(false)
    }

    async fn accept_by(&self, _: &mut PgConnection, _: Uuid, _: Timestamp) -> Option<Timestamp> {
        None
    }
}

impl LinePhones for Side {
    async fn phone(
        &self,
        conn: &mut PgConnection,
        line: Uuid,
    ) -> Result<SecretString, PhoneUnread> {
        read_staff_phone(conn, &vault(), BusinessPurpose::BusinessText, line)
            .await
            .map_err(|_| PhoneUnread::Never)
    }
}

/// A clock that moves 40 seconds on at each reading.
struct StepClock(Timestamp, AtomicI64);

impl Clock for StepClock {
    fn now(&self) -> Timestamp {
        self.0 + SignedDuration::from_secs(40 * self.1.fetch_add(1, Ordering::SeqCst))
    }
}

fn at(s: &str) -> Timestamp {
    s.parse().expect("timestamp")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

/// A business with an owner and one linked phone at `number` (directory digest and sealed
/// number), its CON-019 text box ticked when `consented`. Returns the phone's line ID.
async fn linked(db: &mut PgConnection, number: &str, consented: bool) -> Uuid {
    let t = at(NOON);
    let b = create(db, false, t).await.expect("business");
    let owner = add_owner(db, b, Uuid::new_v4(), t).await.expect("owner");
    let line: Uuid = sqlx::query_scalar(
        "INSERT INTO businesses.staff_phone (business_id, linked_by, linked_at) \
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(b)
    .bind(owner)
    .fetch_one(&mut *db)
    .await
    .expect("phone");
    sender_add(db, &vault(), (b, line), number)
        .await
        .expect("directory");
    store_staff_phone(db, &vault(), line, number)
        .await
        .expect("number");
    if consented {
        record_consent(db, (b, owner, Some(line)), &[ConsentChannel::Text], true, t)
            .await
            .expect("box");
    }
    line
}

/// Switches the business line on or off (TXT-035), as the owner portal's stored row does.
async fn switch(db: &mut PgConnection, on: bool) {
    sqlx::query(
        "INSERT INTO messaging.mode_state (audience, texting_on, line_state, launch_channel, \
         actor_id, changed_at) VALUES ('business', $1, 'working', 'sms', $2, now())",
    )
    .bind(on)
    .bind(Uuid::from_u128(0x0a))
    .execute(db)
    .await
    .expect("switch");
}

/// Queues `template` about `subject` for `recipient`, a business row unless `homeowner`.
async fn queue(db: &mut PgConnection, template: &str, ids: (Uuid, Uuid), homeowner: bool) -> Uuid {
    let text = NewText {
        template_id: template,
        subject: &[ids.1],
        recipient: ids.0,
        relation: if homeowner {
            Relation::OpenRequest
        } else {
            Relation::Business
        },
        body: "fake body",
    };
    enqueue(db, &secret(), &text).await.expect("queued").0
}

/// Runs the outbox sender once under `clock`, with the business side and `business` number
/// when `built`. Returns the carrier's `(to, from)` calls.
async fn run(
    pool: &PgPool,
    clock: Arc<dyn Clock>,
    (built, business): (bool, Option<&str>),
) -> Vec<(String, String)> {
    let carrier = FakeCarrier::default();
    let numbers = (SendingNumber::parse(HOMEOWNER_LINE).ok(), None);
    let plain = OutboxSender::new((secret(), vault()), carrier.clone(), numbers, clock.clone())
        .expect("sender");
    let mut db = pool.acquire().await.expect("connection");
    let now = at(NOON);
    schedule_send_due(&mut db, now).await.expect("schedule");
    drop(db);
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let mut tasks = claim(pool, owner, now + lease, lease, 10)
        .await
        .expect("claim");
    let task = tasks.pop().expect("one run");
    let ran = if built {
        let number = business.and_then(|n| SendingNumber::parse(n).ok());
        let sender = plain.with_business((Side, Side), number);
        run_send_due(pool.clone(), Arc::new(sender), task).await
    } else {
        run_send_due(pool.clone(), Arc::new(plain), task).await
    };
    ran.expect("run");
    sqlx::query("DELETE FROM core.task")
        .execute(pool)
        .await
        .expect("clear");
    carrier.0.lock().expect("lock").clone()
}

async fn run_at(pool: &PgPool, when: &str, side: (bool, Option<&str>)) -> Vec<(String, String)> {
    run(pool, Arc::new(ManualClock::new(at(when))), side).await
}

async fn status(pool: &PgPool, id: Uuid) -> (String, Option<Timestamp>) {
    let sql = "SELECT status, extract(epoch FROM not_before)::int8 FROM messaging.outbox \
               WHERE id = $1";
    let row: (String, Option<i64>) = sqlx::query_as(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row");
    let held = row.1.map(|t| Timestamp::from_second(t).expect("time"));
    (row.0, held)
}

async fn count(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// D-4051, D-4913 (TXT-035, BIZ-032, BIZ-037, SEC-047): a notice row and an auto reply to a
/// consented linked phone go once from the business number, each read audited once, and the
/// number is in clear in no audit row or outbox column; `Notice` still opens no phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_consented_phones_rows_are_sent_once_on_the_business_line(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let line = linked(&mut db, SHOP, true).await;
    switch(&mut db, true).await;
    let card = queue(
        &mut db,
        "job_card_notice",
        (line, Uuid::from_u128(1)),
        false,
    )
    .await;
    let reply = queue(
        &mut db,
        "business_auto_reply",
        (line, Uuid::from_u128(2)),
        false,
    )
    .await;
    let side = (true, Some(BUSINESS_LINE));
    let calls = run_at(&pool, NOON, side).await;
    let call = (SHOP.to_owned(), BUSINESS_LINE.to_owned());
    assert_eq!(calls, vec![call.clone(), call]);
    for id in [card, reply] {
        assert_eq!(status(&pool, id).await.0, "sent");
    }
    assert_eq!(run_at(&pool, NOON, side).await, vec![], "never twice");
    assert_eq!(
        count(&pool, READS, "vault_read.server.business_text").await,
        2
    );
    for clear in [
        "SELECT count(*) FROM ops.audit_log t WHERE to_jsonb(t)::text LIKE $1",
        "SELECT count(*) FROM messaging.outbox t WHERE to_jsonb(t)::text LIKE $1",
    ] {
        assert_eq!(count(&pool, clear, "%9285550142%").await, 0, "{clear}");
    }
    let refused = read_staff_phone(&mut db, &vault(), BusinessPurpose::Notice, line).await;
    assert_eq!(refused.err(), Some(BusinessContactError::PhoneRefused));
    let alerts = "SELECT count(*) FROM ops.alert WHERE severity = 'high' AND subject_kind = $1";
    assert_eq!(count(&pool, alerts, "vault_read.server.notice").await, 1);
}

/// CON-019, D-2042, D-4913: a phone without the box, a recipient that is a business ID (queued
/// before this change) and a line whose number is gone expire with one audit row each; the
/// refused rows read nothing and nothing reaches the carrier.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refused_unlinked_or_unreadable_phone_expires_the_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let unticked = linked(&mut db, SHOP, false).await;
    let gone = linked(&mut db, LA_SHOP, true).await;
    sqlx::query("DELETE FROM privacy.staff_phone WHERE line_id = $1")
        .bind(gone)
        .execute(&mut *db)
        .await
        .expect("number gone");
    switch(&mut db, true).await;
    let job = Uuid::from_u128(3);
    let mut rows = Vec::new();
    for recipient in [unticked, Uuid::from_u128(0xb1), gone] {
        rows.push(queue(&mut db, "job_card_notice", (recipient, job), false).await);
    }
    assert_eq!(
        run_at(&pool, NOON, (true, Some(BUSINESS_LINE))).await,
        vec![]
    );
    for id in rows {
        assert_eq!(status(&pool, id).await.0, "expired");
    }
    assert_eq!(count(&pool, READS, EXPIRED_ACTION).await, 3);
    assert_eq!(
        count(&pool, READS, "vault_read.server.business_text").await,
        0
    );
}

/// TXT-035, D-4913: with the switch off, with no business number, or without the builder, a
/// business row waits unread; switched on with a number it then goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn business_rows_wait_while_the_line_is_closed(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let line = linked(&mut db, SHOP, true).await;
    let id = queue(
        &mut db,
        "job_card_notice",
        (line, Uuid::from_u128(4)),
        false,
    )
    .await;
    assert_eq!(
        run_at(&pool, NOON, (true, Some(BUSINESS_LINE))).await,
        vec![]
    );
    switch(&mut db, true).await;
    for side in [(true, None), (false, None)] {
        assert_eq!(run_at(&pool, NOON, side).await, vec![]);
    }
    assert_eq!(status(&pool, id).await, ("queued".to_owned(), None));
    assert_eq!(
        count(&pool, READS, "vault_read.server.business_text").await,
        0
    );
    assert_eq!(
        run_at(&pool, NOON, (true, Some(BUSINESS_LINE))).await.len(),
        1
    );
}

/// QH-001, QH-003, D-4764 (5): a 213 business phone is held by Arizona's window unread, then by
/// Los Angeles' own (the one read that sends nothing), and goes at 8 a.m. there.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_213_business_phone_gets_its_own_zones_quiet_hours(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let line = linked(&mut db, LA_SHOP, true).await;
    switch(&mut db, true).await;
    let id = queue(
        &mut db,
        "job_card_notice",
        (line, Uuid::from_u128(5)),
        false,
    )
    .await;
    let side = (true, Some(BUSINESS_LINE));
    assert_eq!(run_at(&pool, "2026-11-10T04:30:00Z", side).await, vec![]);
    let arizona_8am = at("2026-11-10T15:00:00Z");
    assert_eq!(
        status(&pool, id).await,
        ("queued".to_owned(), Some(arizona_8am))
    );
    assert_eq!(
        count(&pool, READS, "vault_read.server.business_text").await,
        0
    );
    assert_eq!(run_at(&pool, "2026-11-10T15:30:00Z", side).await, vec![]);
    let la_8am = at("2026-11-10T16:00:00Z");
    assert_eq!(status(&pool, id).await, ("queued".to_owned(), Some(la_8am)));
    assert_eq!(
        count(&pool, READS, "vault_read.server.business_text").await,
        1
    );
    assert_eq!(run_at(&pool, "2026-11-10T16:00:00Z", side).await.len(), 1);
    assert_eq!(status(&pool, id).await.0, "sent");
}

/// QH-001, D-4764 (4): the clock is read for each row, so in a batch that starts at 7:59:20 p.m.
/// the row reached after 8 p.m. is held to 8 a.m., never sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_row_is_checked_against_its_own_send_time(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let line = linked(&mut db, SHOP, true).await;
    switch(&mut db, true).await;
    let first = queue(
        &mut db,
        "job_card_notice",
        (line, Uuid::from_u128(6)),
        false,
    )
    .await;
    let second = queue(
        &mut db,
        "job_card_notice",
        (line, Uuid::from_u128(7)),
        false,
    )
    .await;
    let clock = StepClock(at("2026-11-11T02:58:40Z"), AtomicI64::new(0));
    let calls = run(&pool, Arc::new(clock), (true, Some(BUSINESS_LINE))).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(status(&pool, first).await.0, "sent");
    let next_8am = at("2026-11-11T15:00:00Z");
    assert_eq!(
        status(&pool, second).await,
        ("queued".to_owned(), Some(next_8am))
    );
}

/// QH-007, D-4764 (3): a closed job's queued and held rows created before the close expire, each
/// with one audit row; every close text survives, and so do another job's row and a row created
/// after the close time.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_closed_jobs_rows_expire_except_the_close_texts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (job, other, her) = (
        Uuid::from_u128(8),
        Uuid::from_u128(9),
        Uuid::from_u128(0xc0),
    );
    let line = linked(&mut db, SHOP, true).await;
    let queued = queue(&mut db, "first_quote", (her, job), true).await;
    let held = queue(&mut db, "job_card_notice", (line, job), false).await;
    sqlx::query(
        "UPDATE messaging.outbox SET not_before = now() + interval '9 hours' WHERE id = $1",
    )
    .bind(held)
    .execute(&mut *db)
    .await
    .expect("held");
    let elsewhere = queue(&mut db, "first_quote", (her, other), true).await;
    let mut kept = vec![elsewhere];
    for pattern in CLOSE_TEXTS {
        kept.push(queue(&mut db, &pattern.replace('%', ""), (her, job), true).await);
    }
    let before = Timestamp::now() - SignedDuration::from_hours(1);
    assert_eq!(
        expire_job_rows(&mut db, job, before).await,
        Ok(0),
        "created later"
    );
    let closed_at = Timestamp::now() + SignedDuration::from_hours(1);
    assert_eq!(expire_job_rows(&mut db, job, closed_at).await, Ok(2));
    assert_eq!(expire_job_rows(&mut db, job, closed_at).await, Ok(0));
    for id in [queued, held] {
        assert_eq!(status(&pool, id).await.0, "expired");
    }
    for id in kept {
        assert_eq!(status(&pool, id).await.0, "queued");
    }
    assert_eq!(count(&pool, READS, EXPIRED_ACTION).await, 2);
}
