//! The owner's re-send goes only while the pick still shows its release (T739; TD-PICK-74;
//! PICK-022, SEC-006, PRIV-005; D-4727, D-5060).
//!
//! Driven through `picks::api::resend_release`: a released pick that is paid or free re-sends one
//! `messaging.business_notice` task and one audit row on the owner's press; the same released pick
//! later marked no-show, fraud-refunded or payment-failed (every released status
//! `status::shows_release` refuses) answers `NotReleased`, queues no notice and writes no audit
//! row. PostgreSQL through `#[sqlx::test]`; fake IDs only, and no contact detail is seeded.

use askhava_ops::api::Actor;
use askhava_picks::api::rules::{PickStatus, shows_release};
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, RELEASE_RESEND_ACTION, ResendPress, Resent, resend_release,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const NOTICES: &str = "SELECT count(*) FROM core.task WHERE kind = $1";
const AUDITS: &str = "SELECT count(*) FROM ops.audit_log WHERE action = $1";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_010).expect("time") + SignedDuration::from_secs(secs)
}

/// A fake business with an owner whose mailbox gets pick-and-release notices; the business ID.
async fn business(db: &mut PgConnection) -> Uuid {
    let id = askhava_businesses::api::create(db, false, at(0))
        .await
        .expect("business");
    sqlx::query(
        "INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes, \
         confirm_and_billing, added_at) VALUES ($1, gen_random_uuid(), true, true, true, $2)",
    )
    .bind(id)
    .bind(at(0).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("owner member");
    id
}

/// A new job with a pick of a new business at `status`, released with `basis`; the job and pick.
async fn released(db: &mut PgConnection, status: PickStatus, basis: &str) -> (Uuid, Uuid) {
    let business = business(db).await;
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(status.code())
    .bind(at(0).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    sqlx::query(
        "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
         contact_id, mail_chosen) VALUES ($1, $2, $3, $4, $5, gen_random_uuid(), false)",
    )
    .bind(pick)
    .bind(job)
    .bind(business)
    .bind(basis)
    .bind(at(10).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("release");
    (job, pick)
}

async fn count(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg.to_owned())
        .fetch_one(pool)
        .await
        .expect("count")
}

/// One fresh press of the owner's button on `job`.
async fn press(pool: &PgPool, job: Uuid) -> Resent {
    let mut db = pool.acquire().await.expect("conn");
    let press = ResendPress {
        press: Uuid::new_v4(),
        owner: Uuid::new_v4(),
    };
    resend_release(&mut db, job, press, at(60))
        .await
        .expect("answered")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_74_a_paid_and_a_free_released_pick_each_resend_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let cases = [(PickStatus::Paid, "paid"), (PickStatus::Free, "no_fee")];
    for (sent, (status, basis)) in (1_i64..).zip(cases) {
        assert!(shows_release(status));
        let (job, pick) = released(&mut db, status, basis).await;
        assert_eq!(press(&pool, job).await, Resent::Queued, "{}", status.code());
        assert_eq!(count(&pool, NOTICES, BUSINESS_NOTICE_KIND).await, sent);
        assert_eq!(count(&pool, AUDITS, RELEASE_RESEND_ACTION).await, sent);
        let subject: Uuid = sqlx::query_scalar(
            "SELECT subject_id FROM ops.audit_log WHERE action = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(RELEASE_RESEND_ACTION)
        .fetch_one(&mut *db)
        .await
        .expect("audit row");
        assert_eq!(subject, pick, "the audit row names the released pick");
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_74_a_released_pick_that_no_longer_shows_its_release_resends_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let ended = [
        PickStatus::NoShow,
        PickStatus::FraudRefunded,
        PickStatus::PaymentFailed,
    ];
    for status in ended {
        assert!(!shows_release(status));
        let (job, pick) = released(&mut db, PickStatus::Paid, "paid").await;
        sqlx::query("UPDATE picks.pick SET status = $2 WHERE id = $1")
            .bind(pick)
            .bind(status.code())
            .execute(&mut *db)
            .await
            .expect("later status");
        let answer = press(&pool, job).await;
        assert_eq!(answer, Resent::NotReleased, "{}", status.code());
        assert_eq!(count(&pool, NOTICES, BUSINESS_NOTICE_KIND).await, 0);
        assert_eq!(count(&pool, AUDITS, RELEASE_RESEND_ACTION).await, 0);
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_74_every_status_that_hides_the_release_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    for status in PickStatus::ALL {
        if shows_release(status) {
            continue;
        }
        let (job, _) = released(&mut db, status, "paid").await;
        let answer = press(&pool, job).await;
        assert_eq!(answer, Resent::NotReleased, "{}", status.code());
    }
    assert_eq!(count(&pool, NOTICES, BUSINESS_NOTICE_KIND).await, 0);
    assert_eq!(count(&pool, AUDITS, RELEASE_RESEND_ACTION).await, 0);
}
