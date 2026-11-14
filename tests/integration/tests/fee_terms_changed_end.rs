//! A pick whose fee terms changed before its Confirm ends as `fee_terms_changed` (T467; PICK-050,
//! PICK-046, PICK-047, OPS-034, FEE-005, FEE-016, FEE-017; D-1871 (3), D-3722).
//!
//! Against PostgreSQL: a job walks to Quotes ready with three $190 quotes, quote 1 is picked on the
//! page (its fee fixed under tiers), and the owner switches the fee mode to flat.
//! `picks::api::end_for_fee_terms` then ends the waiting pick with no fee and its PaymentIntent
//! record, moves it to her backup or queues her PICK-046 text event, and changes nothing in every
//! other case; a "Confirm and pay" press racing it on another connection either charges with the
//! pick left waiting or is refused before any Stripe confirm. Every ID, name and number is fake.

use std::sync::Mutex;
use std::time::Duration;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, TEXT_KIND, current, move_to, record_job_facts, text_due};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    BusinessId, ChargeOutcome, FeeMode, IntentStripe, PickNow, Press, confirm_and_pay, pick_fee,
    switch_fee_mode,
};
use askhava_picks::api::{
    Backup, Browser, PagePick, bind_backup, cant_take, choose_backup, end_for_fee_terms,
    ended_businesses, page_pick,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER: Uuid = Uuid::from_u128(0x0a46_7001);
const BROWSER: Browser = [7; 32];
const PICKS: &str = "SELECT coalesce(string_agg(concat_ws('|', id, status, end_reason, ended_at, \
                     quote_number), ',' ORDER BY picked_at, id), '') FROM picks.pick";
const PAYMENTS: &str = "SELECT concat_ws(';', \
     coalesce((SELECT string_agg(concat_ws('|', pick_id, ended_at), ',' ORDER BY pick_id) \
        FROM payments.pick_intent), ''), \
     coalesce((SELECT string_agg(concat_ws('|', pick_id, kind, ended_at), ',' ORDER BY pick_id) \
        FROM payments.pay_timer), ''), \
     coalesce((SELECT string_agg(concat_ws('|', pick_id, try_no, status), ',' \
        ORDER BY pick_id, try_no) FROM payments.charge_attempt), ''), \
     (SELECT count(*) FROM payments.ledger_entry), (SELECT count(*) FROM payments.pick_fee))";
const TASKS: &str = "SELECT count(*) FROM core.task";

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_400_000 + minute * 60).expect("time")
}

fn quoter(n: u128) -> Uuid {
    Uuid::from_u128(0xb467_0100 + n)
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-467".to_owned())
}

/// A job at Quotes ready with three confirmed $190 quotes by quoters 1 to 3.
async fn quotes_ready(db: &mut PgConnection) -> Uuid {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(0), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    for n in 1..=3 {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(quoter(n))
        .bind(at(i64::try_from(n).expect("small")).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
    job
}

/// Quote 1 picked on the page under tiers (backup quote `backup` if named); `switch` then moves
/// the fee mode to flat. Returns the job and the pick.
async fn picked(pool: &PgPool, backup: Option<usize>, switch: bool) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db).await;
    let Ok(PagePick::Picked(pick)) =
        page_pick(&mut db, (job, false), 1, Actor::System, at(7)).await
    else {
        panic!("a page pick");
    };
    if backup.is_some() {
        choose_backup(&mut db, (job, 1), (backup, &BROWSER), at(7))
            .await
            .expect("asked");
        bind_backup(&mut db, pick, &BROWSER, at(7))
            .await
            .expect("bound");
    }
    if switch {
        switch_fee_mode(&mut db, OWNER, FeeMode::Flat)
            .await
            .expect("switched");
    }
    (job, pick)
}

/// The pick's PaymentIntent record with one open card-fix timer, and each attempt status in order.
async fn intent(pool: &PgPool, (job, pick): (Uuid, Uuid), attempts: &[&str]) {
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, \
         stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 2500, $4, false, $5)",
    )
    .bind(pick)
    .bind(quoter(1))
    .bind(job)
    .bind(format!("pi_FakeT467{}", pick.simple()))
    .bind(at(8).to_sqlx())
    .execute(pool)
    .await
    .expect("intent");
    sqlx::query(
        "INSERT INTO payments.pay_timer (pick_id, kind, business_clock, due_at) \
         VALUES ($1, 'card-fix', true, $2)",
    )
    .bind(pick)
    .bind(at(600).to_sqlx())
    .execute(pool)
    .await
    .expect("timer");
    for (n, status) in (1_i32..).zip(attempts) {
        attempt(pool, pick, n, status).await;
    }
}

async fn attempt(db: impl sqlx::PgExecutor<'_>, pick: Uuid, n: i32, status: &str) {
    sqlx::query(
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(pick)
    .bind(n)
    .bind(Uuid::from_u128(
        pick.as_u128() ^ u128::from(n.unsigned_abs()),
    ))
    .bind(status)
    .bind(at(9).to_sqlx())
    .execute(db)
    .await
    .expect("attempt");
}

/// `end_for_fee_terms` on `pick` of `job` in its own committed transaction.
async fn end(pool: &PgPool, (job, pick): (Uuid, Uuid), paused: bool) -> Backup {
    let mut tx = pool.begin().await.expect("tx");
    let ended = end_for_fee_terms(&mut tx, (pick, job), paused, (Actor::System, at(10))).await;
    tx.commit().await.expect("commit");
    ended.expect("ended")
}

async fn text(pool: &PgPool, sql: &'static str) -> String {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect("read")
}

/// Every row the end could change, as one string.
async fn rows(pool: &PgPool) -> (String, String, i64) {
    let tasks: i64 = sqlx::query_scalar(TASKS)
        .fetch_one(pool)
        .await
        .expect("tasks");
    (text(pool, PICKS).await, text(pool, PAYMENTS).await, tasks)
}

/// The versions of the queued PICK-046 text events.
async fn lapsed(pool: &PgPool) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT (payload->>'version')::int FROM core.task \
         WHERE kind = $1 AND payload->>'template' = 'pick_lapsed'",
    )
    .bind(TEXT_KIND)
    .fetch_all(pool)
    .await
    .expect("events")
}

async fn ended_row(pool: &PgPool, pick: Uuid) -> (String, Option<String>, bool) {
    sqlx::query_as("SELECT status, end_reason, ended_at IS NOT NULL FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("pick")
}

fn fee_terms_changed() -> (String, Option<String>, bool) {
    (
        "cancelled".to_owned(),
        Some("fee_terms_changed".to_owned()),
        true,
    )
}

/// True once `n` sessions in this test's database wait on a lock, polling for up to 10 seconds.
async fn lock_waiters(pool: &PgPool, n: i64) -> bool {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("activity");
        if waiting >= n {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// D-1871 (3), FEE-005: after the switch the waiting pick ends `fee_terms_changed` with its
/// intent and every open timer ended, its fee row unchanged and no charge; with no backup the job
/// returns to Quotes ready and one current-version PICK-046 event is queued, which the job-text
/// step's version check sends; a repeat queues none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_switched_pick_ends_and_her_lapsed_text_is_queued(pool: PgPool) {
    let (job, pick) = picked(&pool, None, true).await;
    intent(&pool, (job, pick), &["declined"]).await;
    let mut db = pool.acquire().await.expect("db");
    let fee = pick_fee(&mut db, pick).await.expect("read").expect("fee");
    assert_eq!(fee.mode, FeeMode::Tiers);
    assert_eq!(end(&pool, (job, pick), false).await, Backup::NoBackup);
    assert_eq!(ended_row(&pool, pick).await, fee_terms_changed());
    let open: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM payments.pick_intent WHERE ended_at IS NULL) \
         + (SELECT count(*) FROM payments.pay_timer WHERE ended_at IS NULL)",
    )
    .fetch_one(&pool)
    .await
    .expect("open");
    assert_eq!(open, 0, "the intent and its timers are ended");
    assert_eq!(pick_fee(&mut db, pick).await.expect("read"), Some(fee));
    let after = text(&pool, PAYMENTS).await;
    assert!(
        after.contains("|1|declined") && after.ends_with(";0;1"),
        "{after}"
    );
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(row.status.code(), "quotes_ready");
    assert_eq!(lapsed(&pool).await, [row.version]);
    assert!(
        text_due(&mut db, job, row.version).await.expect("due"),
        "sent, not dropped"
    );
    assert_eq!(end(&pool, (job, pick), false).await, Backup::Done);
    assert_eq!(lapsed(&pool).await.len(), 1, "a repeat queues none");
}

/// PICK-047: with a named available backup the pick moves to it at once, the new pick has its own
/// fee under the terms in force, and no PICK-046 event is queued; a paused job queues none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_switched_pick_moves_to_her_backup_and_a_paused_job_gets_no_text(pool: PgPool) {
    let (job, pick) = picked(&pool, Some(3), true).await;
    let Backup::Moved(new) = end(&pool, (job, pick), false).await else {
        panic!("moved to the backup");
    };
    assert_eq!(ended_row(&pool, pick).await, fee_terms_changed());
    let mut db = pool.acquire().await.expect("db");
    let fee = pick_fee(&mut db, new)
        .await
        .expect("read")
        .expect("own fee");
    assert_eq!((fee.mode, fee.business.0), (FeeMode::Flat, quoter(3)));
    assert!(lapsed(&pool).await.is_empty());
    let (job, pick) = picked(&pool, None, false).await;
    switch_fee_mode(&mut db, OWNER, FeeMode::Tiers)
        .await
        .expect("switched back");
    assert_ne!(end(&pool, (job, pick), true).await, Backup::NoBackup);
    assert_eq!(ended_row(&pool, pick).await, fee_terms_changed());
    assert!(lapsed(&pool).await.is_empty(), "a paused job gets no text");
}

/// Switches the fee mode away from `mode`, which then holds the mode in force.
async fn flip(pool: &PgPool, mode: &mut FeeMode) {
    *mode = if *mode == FeeMode::Tiers {
        FeeMode::Flat
    } else {
        FeeMode::Tiers
    };
    let mut db = pool.acquire().await.expect("db");
    switch_fee_mode(&mut db, OWNER, *mode)
        .await
        .expect("switched");
}

/// D-1871 (3), (4): with the mode unchanged, an attempt that may still succeed, the job in
/// Payment pending, or no waiting pick, the answer is `Done` and nothing changes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_pick_whose_terms_stand_or_whose_charge_may_succeed_is_left(pool: PgPool) {
    let (job, pick) = picked(&pool, None, false).await;
    let before = rows(&pool).await;
    assert_eq!(
        end(&pool, (job, pick), false).await,
        Backup::Done,
        "same mode"
    );
    assert_eq!(rows(&pool).await, before);
    let mut mode = FeeMode::Tiers;
    for status in [
        "sent",
        "processing",
        "succeeded",
        "needs_action",
        "unreachable",
    ] {
        let (job, pick) = picked(&pool, None, false).await;
        intent(&pool, (job, pick), &["declined", status]).await;
        flip(&pool, &mut mode).await;
        let before = rows(&pool).await;
        assert_eq!(
            end(&pool, (job, pick), false).await,
            Backup::Done,
            "{status}"
        );
        assert_eq!(rows(&pool).await, before, "{status}");
    }
    let (job, pick) = picked(&pool, None, false).await;
    flip(&pool, &mut mode).await;
    let move_on = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    let mut db = pool.acquire().await.expect("db");
    let moved = move_to(&mut db, job, move_on, at(9), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "payment pending");
    let before = rows(&pool).await;
    assert_eq!(
        end(&pool, (job, pick), false).await,
        Backup::Done,
        "pending"
    );
    assert_eq!(rows(&pool).await, before);
    let unknown = Uuid::from_u128(0x0467_dead);
    assert_eq!(
        end(&pool, (job, unknown), false).await,
        Backup::Done,
        "no pick"
    );
    assert_eq!(rows(&pool).await, before);
}

/// D-1871: a waiting pick with no stored fee is never ended for its terms.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_pick_with_no_stored_fee_is_left(pool: PgPool) {
    let (job, pick) = picked(&pool, None, true).await;
    sqlx::query("DELETE FROM payments.pick_fee WHERE pick_id = $1")
        .bind(pick)
        .execute(&pool)
        .await
        .expect("fixture: no stored fee");
    let before = rows(&pool).await;
    assert_eq!(end(&pool, (job, pick), false).await, Backup::Done);
    assert_eq!(rows(&pool).await, before);
}

/// A pick with no PaymentIntent record ends with no payments write.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_pick_with_no_intent_ends_with_no_payments_write(pool: PgPool) {
    let (job, pick) = picked(&pool, None, true).await;
    let payments = text(&pool, PAYMENTS).await;
    assert_eq!(end(&pool, (job, pick), false).await, Backup::NoBackup);
    assert_eq!(ended_row(&pool, pick).await, fee_terms_changed());
    assert_eq!(text(&pool, PAYMENTS).await, payments);
}

/// A Stripe that records every call; each confirm succeeds.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
}

impl Recorder {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }

    fn log(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }
}

impl PaymentsAdapter for Recorder {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("a charge never creates a second, confirmed intent (FEE-047)")
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        self.log(format!("fetch {id}"));
        Err(AdapterError::NotSent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for Recorder {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        cents: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        self.log(format!("create {cents}"));
        Ok("pi_FakeT467".to_owned())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.log(format!("confirm {intent}"));
        Ok(PaymentIntent {
            id: intent.to_owned(),
            status: "succeeded".to_owned(),
            amount: 2500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT467".to_owned(),
            metadata: IntentMetadata {
                pick_id: "fake".to_owned(),
            },
            livemode: Some(false),
            latest_charge: Some("ch_FakeT467".to_owned()),
        })
    }
}

/// Quoter 1's press of the stored fee on `pick`.
fn press(job: Uuid, pick: Uuid, cents: u64) -> Press<'static> {
    Press {
        pick,
        business: BusinessId(quoter(1)),
        job,
        amount_cents: u32::try_from(cents).expect("cents"),
        payment_method: "pm_FakeT467",
        press: Uuid::from_u128(pick.as_u128() ^ 0x467),
        emergency: false,
        now: PickNow::Payable,
    }
}

/// FEE-016, FEE-017, D-3722: the end holds the intent lock while a press runs: the press waits,
/// then finds the intent ended and starts no attempt and no Stripe confirm; the pick has ended.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_press_after_the_end_is_refused_before_any_stripe_confirm(pool: PgPool) {
    let (job, pick) = picked(&pool, None, true).await;
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(quoter(1))
        .bind("cus_FakeT467")
        .execute(&pool)
        .await
        .expect("customer");
    intent(&pool, (job, pick), &[]).await;
    let mut db = pool.acquire().await.expect("db");
    let cents = pick_fee(&mut db, pick)
        .await
        .expect("read")
        .expect("fee")
        .cents;
    let mut tx = pool.begin().await.expect("tx");
    let ended = end_for_fee_terms(&mut tx, (pick, job), false, (Actor::System, at(10))).await;
    assert_eq!(ended.expect("ended"), Backup::NoBackup);
    let stripe = Recorder::default();
    let p = press(job, pick, cents);
    let key = secret();
    let (charged, ()) = tokio::join!(confirm_and_pay(&pool, &stripe, &key, &p, at(10)), async {
        assert!(
            lock_waiters(&pool, 1).await,
            "the press waits on the intent lock"
        );
        tx.commit().await.expect("commit");
    });
    assert_eq!(charged, Ok(ChargeOutcome::Pending));
    assert!(
        stripe.calls().iter().all(|c| !c.starts_with("confirm")),
        "{:?}",
        stripe.calls()
    );
    assert!(text(&pool, PAYMENTS).await.contains(";;0;"), "no attempt");
    assert_eq!(ended_row(&pool, pick).await, fee_terms_changed());
}

/// FEE-016, D-1871 (4), D-3722: a press holds the intent lock first and commits its `sent` attempt
/// only once the end waits on that lock: the end's separate statement after the lock sees it, so
/// the end answers `Done` and the pick stays waiting; a pick already charged is left too.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_press_first_charges_and_the_end_leaves_the_pick(pool: PgPool) {
    let (job, pick) = picked(&pool, None, true).await;
    intent(&pool, (job, pick), &["declined"]).await;
    let mut holder = pool.begin().await.expect("press");
    sqlx::query("SELECT 1 FROM payments.pick_intent WHERE pick_id = $1 FOR UPDATE")
        .bind(pick)
        .execute(&mut *holder)
        .await
        .expect("intent lock");
    let (ended, ()) = tokio::join!(end(&pool, (job, pick), false), async {
        assert!(
            lock_waiters(&pool, 1).await,
            "the end waits on the intent lock"
        );
        attempt(&mut *holder, pick, 2, "sent").await;
        holder.commit().await.expect("press commit");
    });
    assert_eq!(ended, Backup::Done);
    let waiting = ("awaiting_confirm".to_owned(), None, false);
    assert_eq!(ended_row(&pool, pick).await, waiting);
    let open: bool =
        sqlx::query_scalar("SELECT ended_at IS NULL FROM payments.pick_intent WHERE pick_id = $1")
            .bind(pick)
            .fetch_one(&pool)
            .await
            .expect("intent");
    assert!(open, "the intent stays open for the charge");
    assert!(lapsed(&pool).await.is_empty());
}

/// PICK-050, PICK-046: "Can't take it" still writes `cant_take` and with no backup now queues one
/// current-version `pick_lapsed` event.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn cant_take_with_no_backup_queues_her_lapsed_text(pool: PgPool) {
    let (job, pick) = picked(&pool, None, false).await;
    let mut tx = pool.begin().await.expect("tx");
    let by = (Actor::Business(quoter(1)), at(8));
    let done = cant_take(&mut tx, (pick, job), false, by).await;
    tx.commit().await.expect("commit");
    assert_eq!(done.expect("ended"), Backup::NoBackup);
    let row = ended_row(&pool, pick).await;
    assert_eq!(
        row,
        ("cancelled".to_owned(), Some("cant_take".to_owned()), true)
    );
    let mut db = pool.acquire().await.expect("db");
    let job_row = current(&mut db, job).await.expect("job");
    assert_eq!(lapsed(&pool).await, [job_row.version]);
}

/// PICK-046, PICK-035, D-3571: `ended_businesses` names a `cant_take` and a `fee_terms_changed`
/// business and never a `replaced` or `paused` one; the constraint refuses any other reason.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ended_businesses_names_cant_take_and_fee_terms_changed(pool: PgPool) {
    let job = Uuid::from_u128(0x0467_0900);
    let reasons = ["cant_take", "fee_terms_changed", "replaced", "paused"];
    for (n, reason) in (1_u128..).zip(reasons) {
        insert_ended(&pool, job, n, reason)
            .await
            .expect("an allowed reason");
    }
    let mut db = pool.acquire().await.expect("db");
    let mut named = ended_businesses(&mut db, job).await.expect("read");
    named.sort();
    assert_eq!(named, [quoter(1), quoter(2)]);
    assert!(
        insert_ended(&pool, job, 5, "lapsed").await.is_err(),
        "any other reason"
    );
}

async fn insert_ended(pool: &PgPool, job: Uuid, n: u128, reason: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at, \
         ended_at, end_reason) VALUES ($1, $2, $3, 'page', 'cancelled', $4, $4, $5)",
    )
    .bind(job)
    .bind(quoter(n))
    .bind(i32::try_from(n).expect("small"))
    .bind(at(1).to_sqlx())
    .bind(reason)
    .execute(pool)
    .await
    .map(drop)
}
