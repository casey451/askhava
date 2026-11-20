//! The daily deliverability re-test run (TD-BIZ-55: BIZ-042; D-362, D-380, D-462, D-4361):
//! `businesses.deliverability_retests` re-tests a sample-pick Yes and an emergency-alert Yes once
//! they are 90 days old, one email per person and Yes, and a rerun queues nothing twice.
//! PostgreSQL; every ID and mailbox is fake.

use askhava_businesses::api::{
    Acting, EMERGENCY_TEST_KIND, RETEST_KIND, Roles, SAMPLE_PICK_KIND, add_member, add_owner,
    create, record_alert_answer, record_pick_alert, run_retests, send_emergency_test,
    set_emergency_emails,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_BOX: Uuid = Uuid::from_u128(0xf1);
const BILLING_BOX: Uuid = Uuid::from_u128(0xf2);
const DAY: i64 = 24 * 3600;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

/// The owner and a "Confirm and billing" member of one business.
async fn team(db: &mut PgConnection) -> (Acting, Acting) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, OWNER_BOX, at(0)).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let roles = Roles::new(false, true).expect("roles");
    let member = add_member(db, owner, BILLING_BOX, roles, at(0)).await;
    let member = Acting {
        business,
        member: member.expect("member"),
    };
    (owner, member)
}

/// `who` answers Yes to the sample pick and, after the test email, to the emergency alert at `when`.
async fn says_yes(db: &mut PgConnection, who: Acting, when: Timestamp) {
    record_pick_alert(db, who, true, when)
        .await
        .expect("pick yes");
    set_emergency_emails(db, who, true, when).await.expect("on");
    send_emergency_test(db, who, when).await.expect("test");
    record_alert_answer(db, who, true, when)
        .await
        .expect("alert yes");
}

/// How many `kind` emails were queued for `who` at or after `since`.
async fn emails(db: &mut PgConnection, kind: &str, who: Acting, since: Timestamp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND payload->>'member' = $2 \
         AND run_at >= $3",
    )
    .bind(kind)
    .bind(who.member.to_string())
    .bind(since.to_sqlx())
    .fetch_one(db)
    .await
    .expect("count")
}

/// TD-BIZ-55 (BIZ-042, D-4361): on the daily run a 90-day-old sample-pick Yes and emergency-alert
/// Yes each get one re-test email and an 89-day-old Yes none; a rerun queues nothing twice, and
/// each run queues the next day's run.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_55_a_ninety_day_old_yes_gets_one_retest_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (old, young) = team(&mut db).await;
    says_yes(&mut db, old, at(10)).await;
    says_yes(&mut db, young, at(10 + DAY)).await;
    let run = at(10 + 90 * DAY);
    run_retests(&pool, run).await.expect("run");
    for kind in [SAMPLE_PICK_KIND, EMERGENCY_TEST_KIND] {
        assert_eq!(emails(&mut db, kind, old, run).await, 1, "{kind}: 90 days");
        assert_eq!(
            emails(&mut db, kind, young, run).await,
            0,
            "{kind}: 89 days"
        );
    }
    run_retests(&pool, run).await.expect("same rerun");
    run_retests(&pool, run + SignedDuration::from_hours(1))
        .await
        .expect("later rerun");
    for kind in [SAMPLE_PICK_KIND, EMERGENCY_TEST_KIND] {
        assert_eq!(emails(&mut db, kind, old, run).await, 1, "{kind}: rerun");
        assert_eq!(emails(&mut db, kind, young, run).await, 0, "{kind}: rerun");
    }
    let next: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(RETEST_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("next run");
    assert_eq!(next, 1, "one next-day run, however often it reran");
}
