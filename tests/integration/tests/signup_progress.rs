//! The read-only sign-up progress view (BIZ-002, BIZ-016, BIZ-040; T485, D-3779).
//!
//! `businesses::api::signup_progress` reads the stored identity step (none before any send),
//! whether the identity code was completed and whether the sign-up's own mailbox is verified; it
//! refuses a business with no sign-up row and changes no row. State is seeded in SQL; every ID is
//! fake.

use askhava_businesses::api::{
    BusinessesError, SignupProgress, Step, add_owner, create, signup_progress,
};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const MAILBOX: Uuid = Uuid::from_u128(0xe1);

fn now() -> Timestamp {
    "2026-11-07T17:00:00Z".parse().expect("time")
}

async fn signed_up(db: &mut PgConnection) -> Uuid {
    let id = create(db, false, now()).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at) \
         VALUES ($1, $2, now())",
    )
    .bind(id)
    .bind(MAILBOX)
    .execute(db)
    .await
    .expect("sign-up row");
    id
}

async fn set(db: &mut PgConnection, business: Uuid, step: &str, done: bool) {
    sqlx::query(
        "UPDATE businesses.signup SET identity_step = $2, code_done = $3 WHERE business_id = $1",
    )
    .bind(business)
    .bind(step)
    .bind(done)
    .execute(db)
    .await
    .expect("seed step");
}

/// One mailbox check of `mailbox`, by `member` (none for the sign-up's own), verified or open.
async fn check(db: &mut PgConnection, business: Uuid, member: Option<Uuid>, verified: bool) {
    sqlx::query(
        "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, sent_at, \
         window_at, verified_at) VALUES ($1, $2, $3, NOT $4, now(), now(), \
         CASE WHEN $4 THEN now() END)",
    )
    .bind(business)
    .bind(member)
    .bind(MAILBOX)
    .bind(verified)
    .execute(db)
    .await
    .expect("seed check");
}

async fn read(db: &mut PgConnection, business: Uuid) -> SignupProgress {
    signup_progress(db, business).await.expect("progress")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_fresh_signup_has_no_step_and_nothing_done(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = signed_up(&mut db).await;
    let p = read(&mut db, business).await;
    assert_eq!(
        (p.step(), p.code_done(), p.email_verified()),
        (None, false, false)
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_stored_step_reads_as_its_step(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = signed_up(&mut db).await;
    for step in [Step::Text, Step::Email, Step::Letter] {
        set(&mut db, business, step.code(), false).await;
        assert_eq!(read(&mut db, business).await.step(), Some(step));
    }
    set(&mut db, business, "letter", true).await;
    assert!(read(&mut db, business).await.code_done());
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn only_a_verified_signup_mailbox_check_counts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let open = signed_up(&mut db).await;
    check(&mut db, open, None, false).await;
    assert!(!read(&mut db, open).await.email_verified());

    let verified = signed_up(&mut db).await;
    check(&mut db, verified, None, true).await;
    assert!(read(&mut db, verified).await.email_verified());

    let member_only = signed_up(&mut db).await;
    let owner = add_owner(&mut db, member_only, MAILBOX, now())
        .await
        .expect("owner");
    check(&mut db, member_only, Some(owner), true).await;
    assert!(!read(&mut db, member_only).await.email_verified());
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_business_with_no_signup_is_unknown(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = create(&mut db, false, now()).await.expect("business");
    for id in [business, Uuid::from_u128(0xdead)] {
        assert_eq!(
            signup_progress(&mut db, id).await,
            Err(BusinessesError::UnknownBusiness)
        );
    }
}

async fn rows(db: &mut PgConnection, business: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT (SELECT row_to_json(s)::text FROM businesses.signup s WHERE business_id = $1) \
         || (SELECT coalesce(json_agg(c ORDER BY id)::text, '') FROM businesses.mailbox_check c \
         WHERE business_id = $1)",
    )
    .bind(business)
    .fetch_one(db)
    .await
    .expect("rows")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn two_reads_change_no_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = signed_up(&mut db).await;
    set(&mut db, business, "text", false).await;
    check(&mut db, business, None, false).await;
    let before = rows(&mut db, business).await;
    read(&mut db, business).await;
    read(&mut db, business).await;
    assert_eq!(rows(&mut db, business).await, before);
    assert_eq!(
        format!("{:?}", read(&mut db, business).await),
        "SignupProgress { step: Some(Text), code_done: false, email_verified: false }"
    );
}
