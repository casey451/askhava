//! TD-FEE-08 (FEE-010, FEE-011, FEE-012; D-019, D-151, D-152, D-195): the free first win goes
//! only to a business whose sign-up form was submitted strictly inside 90 days of launch or of a
//! category of theirs switching on, once per entity key (each of the five kinds), checked when
//! the win is used, and is a $0 ledger entry with the waived amount beside it.
//!
//! The activation date is not an input at all (D-152), so "activated on day 95" cannot change the
//! answer. All IDs, keys and amounts are fake.

use askhava_payments::api::{
    Appended, BusinessId, EntityKey, EntityKind, FeeJob, FeeMode, FeeQuote, FreeWinError,
    JoinDates, PeriodJob, PickedFee, RecurringCap, Service, VisitTier, Waiver, WinCheck,
    fee_at_pick, in_free_win_window, record_entity_keys, waive_at_confirm,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const LAUNCH_SECONDS: i64 = 1_790_000_000;

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t061".to_owned())
}

/// `days` days, `hours` hours and `minutes` minutes after launch.
fn after_launch(days: i64, hours: i64, minutes: i64) -> Timestamp {
    let span = SignedDuration::from_mins((days * 24 + hours) * 60 + minutes);
    Timestamp::from_second(LAUNCH_SECONDS)
        .expect("time")
        .checked_add(span)
        .expect("time")
}

fn dates(joined: Timestamp, opened: &[Timestamp]) -> JoinDates {
    JoinDates {
        joined,
        launch: after_launch(0, 0, 0),
        trades_opened: opened.to_vec(),
    }
}

fn picked(cents: u64, fee_free: bool) -> PickedFee {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    fee_at_pick(FeeQuote::Fixed(cents), None, job, FeeMode::Tiers, None).expect("fee")
}

fn biz(n: u128) -> BusinessId {
    BusinessId(Uuid::from_u128(0xb061_0000 + n))
}

fn job(n: u128) -> Uuid {
    Uuid::from_u128(0x0061_0000 + n)
}

async fn keys(db: &mut PgConnection, business: BusinessId, list: &[(EntityKind, &str)]) {
    let made: Vec<EntityKey> = list
        .iter()
        .map(|(kind, value)| EntityKey::new(&secret(), *kind, value).expect("key"))
        .collect();
    record_entity_keys(db, business, &made).await.expect("keys");
}

async fn waive(
    db: &mut PgConnection,
    business: BusinessId,
    on: Uuid,
    joined: &JoinDates,
) -> Waiver {
    let fee = picked(40_000, false);
    let check = WinCheck {
        business,
        job: on,
        picked: &fee,
        dates: joined,
        same_customer_jobs: &[],
    };
    waive_at_confirm(db, &secret(), check).await.expect("waive")
}

/// TD-FEE-08 (FEE-010, D-152, D-195): the window edges at launch and at a late switch-on.
#[test]
fn td_fee_08_window_edges() {
    assert!(in_free_win_window(&dates(after_launch(89, 23, 59), &[])));
    assert!(!in_free_win_window(&dates(after_launch(90, 0, 0), &[])));
    let opened = after_launch(250, 0, 0);
    assert!(in_free_win_window(&dates(
        after_launch(200, 0, 0),
        &[opened]
    )));
    assert!(in_free_win_window(&dates(
        after_launch(339, 23, 59),
        &[opened]
    )));
    assert!(!in_free_win_window(&dates(
        after_launch(340, 0, 0),
        &[opened]
    )));
    assert!(!in_free_win_window(&dates(after_launch(200, 0, 0), &[])));
}

/// TD-FEE-08 (FEE-011): keys are normalized and an empty one is refused.
#[test]
fn td_fee_08_keys_are_normalized_digests() {
    let a = EntityKey::new(&secret(), EntityKind::OwnerName, " Fake  OWNER Name").expect("a");
    let b = EntityKey::new(&secret(), EntityKind::OwnerName, "fake owner name").expect("b");
    let c = EntityKey::new(&secret(), EntityKind::CityEntity, "fake owner name").expect("c");
    assert_eq!(a, b);
    assert_ne!(a.digest(), c.digest());
    assert!(!a.digest().contains("fake"));
    assert_eq!(
        EntityKey::new(&secret(), EntityKind::Roc, "  "),
        Err(FreeWinError::EmptyKey)
    );
}

/// TD-FEE-08 (FEE-010, FEE-012, W24): a business inside the window gets one $0 free first win
/// with the waived amount; a repeated Confirm writes nothing new; its next job is charged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_08_one_free_win_recorded_at_zero(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let joined = dates(after_launch(89, 23, 59), &[]);
    keys(&mut db, biz(1), &[(EntityKind::Roc, "ROC 900001")]).await;
    let Waiver::FreeFirstWin {
        entry,
        waived_cents,
    } = waive(&mut db, biz(1), job(1), &joined).await
    else {
        panic!("expected a free first win");
    };
    assert!(matches!(entry, Appended::New(_)));
    assert_eq!(waived_cents, 2_500);
    let again = waive(&mut db, biz(1), job(1), &joined).await;
    assert!(matches!(
        again,
        Waiver::FreeFirstWin {
            entry: Appended::Already(_),
            ..
        }
    ));
    assert_eq!(
        waive(&mut db, biz(1), job(2), &joined).await,
        Waiver::NotWaived
    );
    let rows: Vec<(String, i64, Option<String>)> =
        sqlx::query_as("SELECT kind, amount_cents, stripe_id FROM payments.ledger_entry")
            .fetch_all(&mut *db)
            .await
            .expect("ledger");
    assert_eq!(rows, [("free-first-win".to_owned(), 0, None)]);
    let waived: i64 = sqlx::query_scalar("SELECT waived_cents FROM payments.free_win")
        .fetch_one(&mut *db)
        .await
        .expect("free win");
    assert_eq!(waived, 2_500);
}

/// TD-FEE-08 (FEE-010, D-152): a form submitted at exactly 90 days gets none; a fee-free job
/// type is never waived here, so it never uses the win.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_08_outside_the_window_or_fee_free_is_not_waived(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let late = dates(after_launch(90, 0, 0), &[]);
    assert_eq!(
        waive(&mut db, biz(1), job(1), &late).await,
        Waiver::NotWaived
    );
    let early = dates(after_launch(1, 0, 0), &[]);
    let free = picked(40_000, true);
    let check = WinCheck {
        business: biz(2),
        job: job(2),
        picked: &free,
        dates: &early,
        same_customer_jobs: &[],
    };
    let got = waive_at_confirm(&mut db, &secret(), check).await;
    assert_eq!(got, Ok(Waiver::NotWaived));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM payments.ledger_entry")
        .fetch_one(&mut *db)
        .await
        .expect("ledger");
    assert_eq!(count, 0);
}

/// TD-FEE-08 (FEE-011, D-151): a second account sharing any one of the five entity keys with a
/// business that used its win is charged; one sharing none still gets its own.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_08_one_win_per_entity_key(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let joined = dates(after_launch(10, 0, 0), &[]);
    let kinds = [
        EntityKind::Roc,
        EntityKind::CityEntity,
        EntityKind::CardFingerprint,
        EntityKind::IdentityContact,
        EntityKind::OwnerName,
    ];
    for (n, kind) in (0_u128..).zip(kinds) {
        let (first, second) = (biz(10 + n * 2), biz(11 + n * 2));
        let shared = format!("fake shared key {n}");
        let own = format!("fake own key {n}");
        keys(&mut db, first, &[(kind, &shared), (EntityKind::Roc, &own)]).await;
        keys(&mut db, second, &[(kind, &shared)]).await;
        let won = waive(&mut db, first, job(10 + n), &joined).await;
        assert!(matches!(won, Waiver::FreeFirstWin { .. }), "{kind:?}");
        let refused = waive(&mut db, second, job(20 + n), &joined).await;
        assert_eq!(refused, Waiver::NotWaived, "{kind:?}");
    }
    keys(&mut db, biz(99), &[(EntityKind::Roc, "fake unrelated key")]).await;
    let own = waive(&mut db, biz(99), job(99), &joined).await;
    assert!(matches!(own, Waiver::FreeFirstWin { .. }));
}

/// TD-FEE-08 (FEE-011): checked when used: two Confirms sharing a key at the same moment give one
/// win; the second writes nothing and reports `Race`, and on its retry is not waived.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_08_simultaneous_wins_on_a_shared_key(pool: PgPool) {
    let joined = dates(after_launch(10, 0, 0), &[]);
    let mut setup = pool.acquire().await.expect("connection");
    keys(
        &mut setup,
        biz(1),
        &[(EntityKind::CardFingerprint, "fakefp1")],
    )
    .await;
    keys(
        &mut setup,
        biz(2),
        &[(EntityKind::CardFingerprint, "fakefp1")],
    )
    .await;
    let mut first = pool.acquire().await.expect("connection");
    let mut tx = first.begin().await.expect("transaction");
    let won = waive(&mut tx, biz(1), job(1), &joined).await;
    assert!(matches!(won, Waiver::FreeFirstWin { .. }));
    let other = pool.clone();
    let second = tokio::spawn(async move {
        let mut db = other.acquire().await.expect("connection");
        let fee = picked(40_000, false);
        let check = WinCheck {
            business: biz(2),
            job: job(2),
            picked: &fee,
            dates: &joined,
            same_customer_jobs: &[],
        };
        waive_at_confirm(&mut db, &secret(), check).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    tx.commit().await.expect("commit");
    assert_eq!(second.await.expect("task"), Err(FreeWinError::Race));
    let joined = dates(after_launch(10, 0, 0), &[]);
    assert_eq!(
        waive(&mut setup, biz(2), job(2), &joined).await,
        Waiver::NotWaived
    );
    let wins: i64 = sqlx::query_scalar("SELECT count(*) FROM payments.ledger_entry")
        .fetch_one(&mut *setup)
        .await
        .expect("ledger");
    assert_eq!(wins, 1);
}

/// TD-FEE-08 (FEE-012, D-092): the app role may never change or remove a key, win or claim.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_08_free_win_tables_are_append_only(pool: PgPool) {
    for sql in [
        "UPDATE payments.entity_key SET kind = 'roc'",
        "DELETE FROM payments.entity_key",
        "UPDATE payments.free_win SET waived_cents = 1",
        "DELETE FROM payments.free_win",
        "TRUNCATE payments.free_win_claim",
    ] {
        let mut tx = pool.begin().await.expect("transaction");
        sqlx::query("SET LOCAL ROLE askhava_app")
            .execute(&mut *tx)
            .await
            .expect("role");
        let err = sqlx::query(sql).execute(&mut *tx).await.expect_err(sql);
        let code = err
            .as_database_error()
            .and_then(|e| e.code().map(|c| c.into_owned()));
        assert_eq!(code.as_deref(), Some("42501"), "{sql}");
    }
}
