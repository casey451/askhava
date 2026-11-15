//! The Confirm route's $0 free-first-win decision and its one release through the sink (FEE-010,
//! FEE-012, PICK-022; D-152, D-195, D-2302, D-3100, D-3779, D-3813; T494).
//!
//! Against PostgreSQL as the app role with the shipped catalog and a recording sink: a $0
//! decision records one $0 "free first win" entry with the waived amount and hands one release
//! for the pick's own business and job to the sink; a press repeated after a crash between the
//! waiver and the release finds the waiver, writes nothing new and releases once; a business with
//! no sign-up row, or no launch setting, gets no free first win; a trade switched on after launch
//! opens its own window. Fake data only.

use std::sync::Mutex;

use askhava_businesses::api::create;
use askhava_catalog::api::{Catalog, load, parse, set_category_enabled};
use askhava_integration::set_app_role;
use askhava_ops::api::Actor;
use askhava_payments::api::{
    Appended, BusinessId, FeeJob, FeeMode, FeeQuote, PeriodJob, PickedFee, RecurringCap,
    Releasable, ReleaseError, ReleaseSink, Service, VisitTier, Waiver, fee_at_pick,
    fix_fee_at_pick,
};
use askhava_portals::api::{
    ConfirmFee, ConfirmedPick, decide_at_confirm, join_dates, release_no_fee,
};
use askhava_privacy::api::ReleaseBasis;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

const OWNER: Uuid = Uuid::from_u128(0x0494);
const ON_AT_LOAD: &str = "plumbing_repair";
const OFF_AT_LOAD: &str = "junk_removal";

/// A sink that records every cleared pick it is handed.
#[derive(Default)]
struct Recorder(Mutex<Vec<(Uuid, ReleaseBasis)>>);

impl ReleaseSink for Recorder {
    async fn release(&self, cleared: Releasable) -> Result<(), ReleaseError> {
        let mut seen = self.0.lock().expect("lock");
        seen.push((cleared.pick(), cleared.basis()));
        Ok(())
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t494".to_owned())
}

fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n * 24)
}

/// The $75 introduction fee by job size of a firm $1,000 quote.
fn picked() -> PickedFee {
    let period = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    fee_at_pick(FeeQuote::Fixed(100_000), None, period, FeeMode::Tiers, None).expect("fee")
}

/// An app-role connection with the shipped catalog stored.
async fn app(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    let catalog: Catalog =
        parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog");
    load(&mut conn, &catalog).await.expect("catalog stored");
    conn
}

/// A business; with `signup`, its sign-up form submitted then with `trades` chosen.
async fn business(
    conn: &mut PgConnection,
    signup: Option<Timestamp>,
    trades: &[&str],
) -> BusinessId {
    let id = create(conn, true, Timestamp::now())
        .await
        .expect("business");
    if let Some(joined) = signup {
        let codes: Vec<String> = trades.iter().map(|c| (*c).to_owned()).collect();
        sqlx::query(
            "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
             VALUES ($1, gen_random_uuid(), $2, $3)",
        )
        .bind(id)
        .bind(joined.to_sqlx())
        .bind(codes)
        .execute(&mut *conn)
        .await
        .expect("sign-up row");
    }
    BusinessId(id)
}

/// Fixes the fee at a new fake pick of `business` on a new fake job.
async fn pick_of(conn: &mut PgConnection, business: BusinessId, fee: &PickedFee) -> (Uuid, Uuid) {
    let (pick, job) = (Uuid::new_v4(), Uuid::new_v4());
    let at = Timestamp::now();
    let written = fix_fee_at_pick(conn, pick, business, job, fee, at).await;
    assert_eq!(written, Ok(true));
    (pick, job)
}

fn confirmed(pick: (Uuid, Uuid), business: BusinessId, fee: &PickedFee) -> ConfirmedPick<'_> {
    ConfirmedPick {
        pick: pick.0,
        business,
        job: pick.1,
        picked: fee,
        same_customer_jobs: &[],
    }
}

/// The decision in its own committed transaction, as the route's Confirm transaction.
async fn decide(
    conn: &mut PgConnection,
    press: ConfirmedPick<'_>,
    launch: Option<Timestamp>,
) -> ConfirmFee {
    let mut tx = conn.begin().await.expect("begin");
    let fee = decide_at_confirm(&mut tx, &secret(), press, launch).await;
    tx.commit().await.expect("commit");
    fee.expect("decision")
}

async fn ledger(conn: &mut PgConnection) -> Vec<(String, i64, Option<String>)> {
    sqlx::query_as("SELECT kind, amount_cents, stripe_id FROM payments.ledger_entry ORDER BY seq")
        .fetch_all(&mut *conn)
        .await
        .expect("ledger")
}

/// FEE-010, FEE-012, PICK-022, D-3813: a $0 decision records the free first win with the waived
/// amount and hands one release of the pick's own pair to the sink; a press repeated after a
/// crash between the waiver and the release finds the waiver, writes nothing and releases once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_zero_decision_records_one_waiver_and_releases_once(pool: PgPool) {
    let mut conn = app(&pool).await;
    let launch = Timestamp::now() - days(10);
    let biz = business(&mut conn, Some(launch + days(5)), &[ON_AT_LOAD]).await;
    let fee = picked();
    let pick = pick_of(&mut conn, biz, &fee).await;
    let press = confirmed(pick, biz, &fee);
    let ConfirmFee::NoFee(Waiver::FreeFirstWin {
        entry: Appended::New(_),
        waived_cents,
    }) = decide(&mut conn, press, Some(launch)).await
    else {
        panic!("expected a new free first win");
    };
    assert_eq!(waived_cents, 7_500);
    let sink = Recorder::default();
    let again = decide(&mut conn, press, Some(launch)).await;
    assert!(matches!(
        again,
        ConfirmFee::NoFee(Waiver::FreeFirstWin {
            entry: Appended::Already(_),
            waived_cents: 7_500,
        })
    ));
    release_no_fee(&mut conn, &sink, press)
        .await
        .expect("release");
    let released = sink.0.lock().expect("lock").clone();
    assert_eq!(released, [(pick.0, ReleaseBasis::NoFeeDue)]);
    assert_eq!(
        ledger(&mut conn).await,
        [("free-first-win".to_owned(), 0, None)]
    );
    let waived: Vec<i64> = sqlx::query_scalar("SELECT waived_cents FROM payments.free_win")
        .fetch_all(&mut *conn)
        .await
        .expect("free win");
    assert_eq!(waived, [7_500]);
}

/// FEE-010, D-3779, D-3813: no sign-up row joins at `Timestamp::MAX` and no launch setting opens
/// no launch window: both are charged, nothing is written and no release is handed over.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_signup_row_or_no_launch_gives_no_free_first_win(pool: PgPool) {
    let mut conn = app(&pool).await;
    let launch = Timestamp::now() - days(10);
    let fee = picked();
    let bare = business(&mut conn, None, &[]).await;
    let dates = join_dates(&mut conn, bare, Some(launch))
        .await
        .expect("dates");
    assert_eq!(dates.joined, Timestamp::MAX);
    let press_bare = pick_of(&mut conn, bare, &fee).await;
    let got = decide(&mut conn, confirmed(press_bare, bare, &fee), Some(launch)).await;
    assert_eq!(got, ConfirmFee::Charge);
    let joined = business(&mut conn, Some(launch + days(5)), &[OFF_AT_LOAD]).await;
    let press_unlaunched = pick_of(&mut conn, joined, &fee).await;
    let got = decide(&mut conn, confirmed(press_unlaunched, joined, &fee), None).await;
    assert_eq!(got, ConfirmFee::Charge);
    assert_eq!(ledger(&mut conn).await, []);
    let sink = Recorder::default();
    let refused = release_no_fee(&mut conn, &sink, confirmed(press_bare, bare, &fee)).await;
    assert!(refused.is_err());
    assert!(sink.0.lock().expect("lock").is_empty());
}

/// FEE-010, D-195, D-3779: a business that joined long after launch gets no win until a trade it
/// signed up for is switched on; that switch-on opens its own window.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_trade_switched_on_after_launch_gets_its_own_window(pool: PgPool) {
    let mut conn = app(&pool).await;
    let launch = Timestamp::now() - days(400);
    let biz = business(&mut conn, Some(Timestamp::now() + days(1)), &[OFF_AT_LOAD]).await;
    let fee = picked();
    let press = pick_of(&mut conn, biz, &fee).await;
    let before = decide(&mut conn, confirmed(press, biz, &fee), Some(launch)).await;
    assert_eq!(before, ConfirmFee::Charge);
    let on = set_category_enabled(&mut conn, OFF_AT_LOAD, true, Actor::Owner(OWNER)).await;
    assert_eq!(on.ok(), Some(true));
    let after = decide(&mut conn, confirmed(press, biz, &fee), Some(launch)).await;
    assert!(matches!(
        after,
        ConfirmFee::NoFee(Waiver::FreeFirstWin {
            entry: Appended::New(_),
            waived_cents: 7_500,
        })
    ));
    let sink = Recorder::default();
    release_no_fee(&mut conn, &sink, confirmed(press, biz, &fee))
        .await
        .expect("release");
    assert_eq!(sink.0.lock().expect("lock").len(), 1);
}
