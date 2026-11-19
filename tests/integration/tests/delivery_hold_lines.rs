//! T652: a delivery alert's hold is per line (TD-OPS-47; OPS-025, OPS-004; D-163, D-4310).
//!
//! At most one text-delivery alert is open at a time; after a line's own alert is acknowledged,
//! that line raises no new one until it is back at 97% of the last 200 texts. Another line's
//! acknowledged alert never holds it. Fake data only: made-up counts and minutes.

use askhava_ops::api::{Audience, DeliveryCounts, DeliveryOutcome, record_delivery_counts};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-10-30T16:00:00Z".parse().expect("instant");
    start + SignedDuration::from_mins(minute)
}

fn counts(audience: Audience, failed_of_50: u32, failed_of_200: u32) -> DeliveryCounts {
    DeliveryCounts::new(audience, failed_of_50, failed_of_200).expect("counts")
}

async fn report(pool: &PgPool, c: DeliveryCounts, minute: i64) -> DeliveryOutcome {
    let mut db = pool.acquire().await.expect("conn");
    record_delivery_counts(&mut db, c, at(minute))
        .await
        .expect("report")
}

/// Casey acknowledges alert `id` at `minute` (fake acknowledgement).
async fn casey_acknowledges(pool: &PgPool, id: Uuid, minute: i64) {
    sqlx::query("UPDATE ops.alert SET resolved_at = $2::timestamptz WHERE id = $1")
        .bind(id)
        .bind(at(minute).to_string())
        .execute(pool)
        .await
        .expect("casey");
}

async fn raised(pool: &PgPool, c: DeliveryCounts, minute: i64) -> Uuid {
    match report(pool, c, minute).await {
        DeliveryOutcome::Raised(id) => id,
        other => panic!("no alert at minute {minute}: {other:?}"),
    }
}

/// TD-OPS-47 (OPS-025, OPS-004, D-163, D-4310): the business line that drops while the
/// homeowner line's alert is open raises its own alert once that one is acknowledged; a line
/// whose own alert was acknowledged stays held until it reaches 97% of 200, and the other line's
/// acknowledgement never holds it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_47_a_delivery_hold_is_per_line(pool: PgPool) {
    let home = |f50, f200| counts(Audience::Homeowner, f50, f200);
    let business = |f50, f200| counts(Audience::Business, f50, f200);
    let first = raised(&pool, home(6, 6), 0).await;
    assert_eq!(
        report(&pool, business(9, 9), 1).await,
        DeliveryOutcome::Nothing,
        "one open at a time"
    );
    assert_eq!(
        report(&pool, home(0, 6), 2).await,
        DeliveryOutcome::Acknowledged(first)
    );
    let second = raised(&pool, business(9, 9), 3).await;
    casey_acknowledges(&pool, second, 4).await;
    assert_eq!(
        report(&pool, business(9, 9), 5).await,
        DeliveryOutcome::Held
    );
    let third = raised(&pool, home(6, 6), 6).await;
    assert_eq!(
        report(&pool, home(0, 6), 7).await,
        DeliveryOutcome::Acknowledged(third)
    );
    assert_eq!(
        report(&pool, business(0, 11), 8).await,
        DeliveryOutcome::Held
    );
    assert_eq!(
        report(&pool, business(0, 6), 9).await,
        DeliveryOutcome::Nothing
    );
    raised(&pool, business(0, 11), 10).await;
}
