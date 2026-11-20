//! TD-JOB-20 (PICK-050, BIZ-051, PICK-046, PICK-047, OPS-034, FEE-021; T669, D-4349): a pick
//! whose job is in Payment pending (moved there through `confirm_move`) always ends on "Can't
//! take it" (`cant_take`), on its business leaving (`ineligible`) and on a lapse
//! (`payment_failed`, never `expired`): its intent ends through `payments`' own pick end, a later
//! success is refunded once and nothing is released, the job returns to Quotes ready and her
//! backup is offered with her text. `end_for_fee_terms` still refuses Payment pending. A backup
//! pick made after that earlier Payment pending still lapses as `expired` with her PICK-046 text.
//! PostgreSQL through each module's API over a fake Stripe; fake data only.

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_businesses::api::add_owner;
use askhava_core::api::{Claimed, Guarded, MIGRATOR};
use askhava_jobs::api::{JobStatus, TEXT_KIND, current, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    BusinessId, IntentStripe, LATE_KIND, PickNow, Press, confirm_and_pay, ensure_customer,
    run_charge_check,
};
use askhava_picks::api::{
    Backup, ConfirmMove, PagePick, bind_backup, cant_take, choose_backup, confirm_move,
    end_for_fee_terms, end_for_leaving, is_released, lapse, page_pick, run_confirm_ends_at,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const ID_SECRET: &str = "test-only-id-secret-not-a-real-key";
const INTENT: &str = "pi_fake669paid";
/// 10:00 Arizona on a Monday.
const NOW: i64 = 1_793_638_800;

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_second(NOW).expect("time") + SignedDuration::from_mins(minutes)
}

/// How the test ends the pick.
#[derive(Clone, Copy)]
enum End {
    CantTake,
    Leaving,
}

/// Moves new job `job` Intake to Open to Quotes ready.
async fn quotes_ready(db: &mut PgConnection, job: Uuid) {
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(0), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
}

/// A business with its owner, a job in Quotes ready with its quote 1 and another business's
/// quote 2, her page pick of quote 1 with quote 2 bound as her backup when `backup`, and the
/// job moved to Payment pending through `confirm_move`: `(business, job, pick)`.
async fn pressed(pool: &PgPool, backup: bool) -> (Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, at(0)).await;
    let business = business.expect("business");
    add_owner(&mut db, business, Uuid::new_v4(), at(0))
        .await
        .expect("owner");
    let job = askhava_jobs::api::create(&mut db, at(0), false, Actor::System).await;
    let job = job.expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    quotes_ready(&mut db, job).await;
    for (n, who) in [(1, business), (2, Uuid::from_u128(0x669b2))] {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 32500, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(who)
        .bind(at(n).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
    let Ok(PagePick::Picked(pick)) =
        page_pick(&mut db, (job, false), 1, Actor::System, at(3)).await
    else {
        panic!("pick");
    };
    if backup {
        choose_backup(&mut db, (job, 1), (Some(2), &[6; 32]), at(4))
            .await
            .expect("asked");
        bind_backup(&mut db, pick, &[6; 32], at(4))
            .await
            .expect("bound");
    }
    let mut tx = pool.begin().await.expect("tx");
    let moved = confirm_move(&mut tx, (pick, job), at(5)).await;
    tx.commit().await.expect("commit");
    assert_eq!(moved.expect("confirm move"), ConfirmMove::Moved);
    (business, job, pick)
}

/// A fake Stripe confirming the intent as `confirms` and reporting it as succeeded.
struct FakeStripe {
    pick: Uuid,
    confirms: &'static str,
}

impl FakeStripe {
    fn intent(&self, id: &str, state: &str) -> PaymentIntent {
        PaymentIntent {
            id: id.to_owned(),
            status: state.to_owned(),
            amount: 2500,
            currency: "usd".to_owned(),
            customer: "cus_fake669".to_owned(),
            metadata: IntentMetadata {
                pick_id: self.pick.to_string(),
            },
            livemode: Some(false),
            latest_charge: Some(id.replacen("pi_", "ch_", 1)),
        }
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_fake669".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(self.intent(id, "succeeded"))
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        Ok(INTENT.to_owned())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        Ok(self.intent(intent, self.confirms))
    }
}

/// The $25 Confirm-and-pay press with Stripe confirming the intent as `confirms`.
async fn press(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid), confirms: &'static str) {
    let stripe = FakeStripe { pick, confirms };
    let secret = SecretString::from(ID_SECRET.to_owned());
    ensure_customer(pool, &stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake669",
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    confirm_and_pay(pool, &stripe, &secret, &press, at(6))
        .await
        .expect("pressed");
}

/// Runs a query with `$1` bound to `id` and answers its one value.
async fn one<T>(pool: &PgPool, sql: &'static str, id: Uuid) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
{
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// Pick `pick`'s status and end reason, and whether its PaymentIntent has ended.
async fn ends(pool: &PgPool, pick: Uuid) -> (String, Option<String>, bool) {
    let row: (String, Option<String>) =
        sqlx::query_as("SELECT status, end_reason FROM picks.pick WHERE id = $1")
            .bind(pick)
            .fetch_one(pool)
            .await
            .expect("pick");
    let sql =
        "SELECT count(*) FROM payments.pick_intent WHERE pick_id = $1 AND ended_at IS NOT NULL";
    (row.0, row.1, one::<i64>(pool, sql, pick).await == 1)
}

/// Job `job`'s status code now and whether it went Payment pending to Quotes ready.
async fn returned(pool: &PgPool, job: Uuid) -> (&'static str, bool) {
    let mut db = pool.acquire().await.expect("db");
    let status = current(&mut db, job).await.expect("job").status.code();
    let sql = "SELECT count(*) FROM jobs.status_history WHERE job_id = $1 \
               AND from_status = 'payment_pending' AND to_status = 'quotes_ready'";
    (status, one::<i64>(pool, sql, job).await == 1)
}

/// Her text events for `job` of `template`.
async fn her_texts(pool: &PgPool, job: Uuid, template: &str) -> i64 {
    let sql = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'job' = $1::text \
               AND payload->>'template' = $3";
    sqlx::query_scalar(sql)
        .bind(job)
        .bind(TEXT_KIND)
        .bind(template)
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// Ends the pressed pick `how` in one committed transaction.
async fn end(pool: &PgPool, how: End, (job, pick): (Uuid, Uuid)) -> Backup {
    let mut tx = pool.begin().await.expect("tx");
    let by = (Actor::System, at(10));
    let ended = match how {
        End::CantTake => cant_take(&mut tx, (pick, job), false, by).await,
        End::Leaving => end_for_leaving(&mut tx, (pick, job), false, by).await,
    };
    tx.commit().await.expect("commit");
    ended.expect("ended")
}

/// TD-JOB-20: with an idle intent the pick ends as `reason`, the intent closes, the job returns
/// to Quotes ready and her backup gets the pick with her PICK-047 text.
async fn idle_ends_to_backup(pool: PgPool, how: End, reason: &str) {
    let (business, job, pick) = pressed(&pool, true).await;
    press(&pool, (business, job, pick), "requires_payment_method").await;
    assert!(matches!(
        end(&pool, how, (job, pick)).await,
        Backup::Moved(_)
    ));
    let want = ("cancelled".to_owned(), Some(reason.to_owned()), true);
    assert_eq!(ends(&pool, pick).await, want);
    assert_eq!(returned(&pool, job).await, ("awaiting_confirm", true));
    assert_eq!(her_texts(&pool, job, "pick_moved").await, 1);
}

/// TD-JOB-20: with an attempt `processing` the pick and its intent still end, and the attempt's
/// later success is refunded once with nothing released (FEE-021).
async fn processing_ends_and_refunds(pool: PgPool, how: End, reason: &str) {
    let (business, job, pick) = pressed(&pool, false).await;
    press(&pool, (business, job, pick), "processing").await;
    assert_eq!(end(&pool, how, (job, pick)).await, Backup::NoBackup);
    let want = ("cancelled".to_owned(), Some(reason.to_owned()), true);
    assert_eq!(ends(&pool, pick).await, want);
    assert_eq!(returned(&pool, job).await, ("quotes_ready", true));
    assert_eq!(her_texts(&pool, job, "pick_lapsed").await, 1);
    let stripe = FakeStripe {
        pick,
        confirms: "succeeded",
    };
    let payload = json!({"pick": pick, "try": 1, "round": 0});
    for _ in 0..2 {
        run_charge_check(&pool, &stripe, &payload, at(20))
            .await
            .expect("check");
    }
    let sql = "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'pick' = $1::text";
    let refunds: i64 = sqlx::query_scalar(sql)
        .bind(pick)
        .bind(LATE_KIND)
        .fetch_one(&pool)
        .await
        .expect(sql);
    assert_eq!(refunds, 1, "refunded once");
    let mut db = pool.acquire().await.expect("db");
    assert!(!is_released(&mut db, pick).await.expect("release row"));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_cant_take_on_payment_pending_moves_to_her_backup(pool: PgPool) {
    idle_ends_to_backup(pool, End::CantTake, "cant_take").await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_cant_take_with_a_processing_attempt_refunds_its_success(pool: PgPool) {
    processing_ends_and_refunds(pool, End::CantTake, "cant_take").await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_leaving_on_payment_pending_moves_to_her_backup(pool: PgPool) {
    idle_ends_to_backup(pool, End::Leaving, "ineligible").await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_leaving_with_a_processing_attempt_refunds_its_success(pool: PgPool) {
    processing_ends_and_refunds(pool, End::Leaving, "ineligible").await;
}

/// TD-JOB-20 (D-3722): `end_for_fee_terms` still answers `Backup::Done` on Payment pending and
/// ends nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_fee_terms_end_still_refuses_payment_pending(pool: PgPool) {
    let (_, job, pick) = pressed(&pool, false).await;
    let mut tx = pool.begin().await.expect("tx");
    let ended = end_for_fee_terms(&mut tx, (pick, job), false, (Actor::System, at(10))).await;
    tx.commit().await.expect("commit");
    assert_eq!(ended.expect("answer"), Backup::Done);
    assert_eq!(ends(&pool, pick).await.0, "awaiting_confirm");
    assert_eq!(returned(&pool, job).await, ("payment_pending", false));
}

/// TD-JOB-20 (OPS-034): `lapse` on a pick whose job entered Payment pending records
/// `payment_failed`, never `expired`, and returns the job to Quotes ready.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_lapse_after_the_press_is_payment_failed_not_expired(pool: PgPool) {
    let (_, job, pick) = pressed(&pool, false).await;
    let mut tx = pool.begin().await.expect("tx");
    let lapsed = lapse(&mut tx, pick, false, (Actor::System, at(90))).await;
    tx.commit().await.expect("commit");
    assert_eq!(lapsed.expect("lapse"), Backup::NoBackup);
    assert_eq!(ends(&pool, pick).await.0, "payment_failed");
    assert_eq!(returned(&pool, job).await, ("quotes_ready", true));
}

/// Her text events of `template` for ended pick `pick` (deduped by the pick).
async fn pick_texts(pool: &PgPool, pick: Uuid, template: &str) -> i64 {
    let sql = "SELECT count(*) FROM core.task WHERE kind = $1 AND dedupe_key = $2";
    sqlx::query_scalar(sql)
        .bind(TEXT_KIND)
        .bind(format!("{TEXT_KIND}.{pick}.{template}"))
        .fetch_one(pool)
        .await
        .expect(sql)
}

/// TD-JOB-20 (OPS-034, PICK-019, PICK-046): pick A ends on Payment pending into backup B; B was
/// made after the job's stay in Payment pending, so B's window end lapses it as `expired` (a
/// counted lapse, never `payment_failed`), with no backup left and her one PICK-046 event.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_job_20_a_backup_picked_after_payment_pending_still_lapses_as_expired(pool: PgPool) {
    let (business, job, first) = pressed(&pool, true).await;
    press(&pool, (business, job, first), "requires_payment_method").await;
    let Backup::Moved(backup) = end(&pool, End::CantTake, (job, first)).await else {
        panic!("moved to her backup");
    };
    let mut db = pool.acquire().await.expect("db");
    let from = current(&mut db, job).await.expect("job").timer_from;
    let task = json!({"pick": backup, "from": from.as_second(), "due": at(90).as_second()});
    let claimed = Claimed {
        id: 1,
        kind: String::new(),
        payload: task,
        attempts: 1,
    };
    run_confirm_ends_at(pool.clone(), claimed, at(90))
        .await
        .expect("window end");
    assert_eq!(ends(&pool, backup).await.0, "expired");
    assert_eq!(
        current(&mut db, job).await.expect("job").status.code(),
        "quotes_ready"
    );
    assert_eq!(pick_texts(&pool, backup, "pick_lapsed").await, 1);
    assert_eq!(
        pick_texts(&pool, backup, "pick_moved").await,
        0,
        "no backup left"
    );
}
