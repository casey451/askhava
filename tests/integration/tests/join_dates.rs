//! The free first win's join dates are read from `businesses` and `catalog` (FEE-010, LIC-002;
//! D-019, D-152, D-195, D-3779; T486).
//!
//! Against PostgreSQL with the shipped catalog stored as the app role: `join_facts` answers the
//! sign-up's submit time and the trades chosen at sign-up, never a category ticked later on the
//! checklist, and `None` with no sign-up row; a new category the file has on reads a switch-on
//! time and one it has off reads none; the owner's off-to-on switch records the time, a switch-off
//! keeps it, a later switch-on records the later time, a reload or a repeated switch-on changes
//! nothing; unknown and never-switched-on codes give nothing; the check refuses an enabled row with
//! no time; the migration's backfill gives on categories 1970 and leaves off ones null; and
//! TD-FEE-08's switch-on cases hold on the read dates through `in_free_win_window`. Fake data only.

use askhava_businesses::api::{create, join_facts};
use askhava_catalog::api::{Catalog, load, parse, set_category_enabled, trades_opened};
use askhava_integration::{set_app_role, split_statements};
use askhava_ops::api::Actor;
use askhava_payments::api::{JoinDates, in_free_win_window};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const OWNER: Uuid = Uuid::from_u128(0x0486);
const ON_AT_LOAD: &str = "plumbing_repair";
const OFF_AT_LOAD: &str = "junk_removal";

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn at(text: &str) -> Timestamp {
    text.parse().expect("fake time")
}

fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n * 24)
}

fn codes(list: &[&str]) -> Vec<String> {
    list.iter().map(|c| (*c).to_owned()).collect()
}

/// An app-role connection with the shipped catalog stored.
async fn app(pool: &sqlx::PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    load(&mut conn, &catalog()).await.expect("catalog stored");
    conn
}

/// A business whose sign-up form was submitted at `joined` with `trades` chosen.
async fn signed_up(conn: &mut PgConnection, joined: Timestamp, trades: &[&str]) -> Uuid {
    let id = create(conn, true, joined).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, gen_random_uuid(), $2, $3)",
    )
    .bind(id)
    .bind(joined.to_sqlx())
    .bind(codes(trades))
    .execute(&mut *conn)
    .await
    .expect("sign-up row");
    id
}

async fn opened(conn: &mut PgConnection, list: &[&str]) -> Vec<Timestamp> {
    trades_opened(conn, &codes(list)).await.expect("read")
}

async fn switch(conn: &mut PgConnection, code: &str, on: bool) -> bool {
    set_category_enabled(conn, code, on, Actor::Owner(OWNER))
        .await
        .expect("switched")
}

/// FEE-010, D-152, D-3779: the submit time and the sign-up trades; a checklist tick added later
/// is not a sign-up trade; no sign-up row gives `None`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn join_facts_reads_the_submit_time_and_signup_trades(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let joined = at("2026-12-01T15:30:00.123456Z");
    let id = signed_up(&mut conn, joined, &["pool_repair", "hvac_repair"]).await;
    sqlx::query(
        "INSERT INTO businesses.job_type_choice (business_id, category, job_type) \
         VALUES ($1, 'electrical', 'Outlet repair')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await
    .expect("later checklist tick");
    let facts = join_facts(&mut conn, id).await.expect("read").expect("row");
    assert_eq!(facts.joined(), joined);
    assert_eq!(facts.trades(), codes(&["pool_repair", "hvac_repair"]));
    let bare = create(&mut conn, false, joined).await.expect("business");
    assert_eq!(join_facts(&mut conn, bare).await.expect("read"), None);
}

/// LIC-002, LIC-003, D-3779: the file's on category reads a time at load, its off one none; an
/// unknown code gives nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_new_category_reads_a_switch_on_time_only_when_on(pool: sqlx::PgPool) {
    let before = Timestamp::now() - SignedDuration::from_mins(5);
    let mut conn = app(&pool).await;
    let after = Timestamp::now() + SignedDuration::from_mins(5);
    let read = opened(&mut conn, &[ON_AT_LOAD, OFF_AT_LOAD, "no_such_trade"]).await;
    assert_eq!(read.len(), 1);
    assert!(read[0] > before && read[0] < after);
    assert!(opened(&mut conn, &[OFF_AT_LOAD]).await.is_empty());
    assert!(opened(&mut conn, &["no_such_trade"]).await.is_empty());
    assert!(opened(&mut conn, &[]).await.is_empty());
}

/// FEE-010, D-195, D-3779: switching on records the time, off keeps it, on again records a later
/// one; a reload and a repeated switch-on change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_owner_switch_records_the_latest_switch_on(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    assert!(switch(&mut conn, OFF_AT_LOAD, true).await);
    let first = opened(&mut conn, &[OFF_AT_LOAD]).await;
    assert_eq!(first.len(), 1);
    assert!(!switch(&mut conn, OFF_AT_LOAD, true).await);
    load(&mut conn, &catalog()).await.expect("reload");
    assert_eq!(opened(&mut conn, &[OFF_AT_LOAD]).await, first);
    assert!(switch(&mut conn, OFF_AT_LOAD, false).await);
    assert_eq!(opened(&mut conn, &[OFF_AT_LOAD]).await, first);
    sqlx::query("UPDATE catalog.category SET switched_on_at = $1 WHERE code = $2")
        .bind(at("2020-01-01T00:00:00Z").to_sqlx())
        .bind(OFF_AT_LOAD)
        .execute(&mut *conn)
        .await
        .expect("age the stored time");
    assert!(switch(&mut conn, OFF_AT_LOAD, true).await);
    let again = opened(&mut conn, &[OFF_AT_LOAD]).await;
    assert_eq!(again.len(), 1);
    assert!(again[0] >= first[0]);
}

/// FEE-010, D-3779: an enabled category with no switch-on time is refused by the check.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_enabled_row_with_no_time_is_refused(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let refused = sqlx::query("UPDATE catalog.category SET switched_on_at = NULL WHERE code = $1")
        .bind(ON_AT_LOAD)
        .execute(&mut *conn)
        .await;
    let Err(sqlx::Error::Database(err)) = refused else {
        panic!("the check must refuse it");
    };
    assert_eq!(err.constraint(), Some("category_switched_on_check"));
}

/// D-019, D-3779: replaying the migration's backfill and check on a catalog stored before the
/// column existed gives on categories 1970 and leaves off ones null, and the check validates.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_backfill_marks_on_categories_on_before_launch(pool: sqlx::PgPool) {
    drop(app(&pool).await);
    let mut admin = pool.acquire().await.expect("connection");
    let sql = include_str!("../../../migrations/0190_catalog_switched_on.sql");
    let mut steps = split_statements(sql).into_iter();
    assert!(
        steps
            .next()
            .expect("ADD COLUMN")
            .text
            .contains("ADD COLUMN")
    );
    let undo = [
        "ALTER TABLE catalog.category DROP CONSTRAINT category_switched_on_check",
        "UPDATE catalog.category SET switched_on_at = NULL",
    ];
    for sql in undo {
        sqlx::raw_sql(sql)
            .execute(&mut *admin)
            .await
            .expect("as before the migration");
    }
    // The SQL is this repository's own migration file, compiled in; nothing comes from input.
    for step in steps {
        sqlx::raw_sql(sqlx::AssertSqlSafe(step.text))
            .execute(&mut *admin)
            .await
            .expect("migration step");
    }
    let epoch = at("1970-01-01T00:00:00Z");
    assert_eq!(
        opened(&mut admin, &[ON_AT_LOAD, OFF_AT_LOAD]).await,
        [epoch]
    );
    let on: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog.category WHERE enabled AND switched_on_at <> $1",
    )
    .bind(epoch.to_sqlx())
    .fetch_one(&mut *admin)
    .await
    .expect("count");
    assert_eq!(on, 0);
}

/// TD-FEE-08 (FEE-010, D-195): on the read dates, a business that joined on day 200 for a trade
/// switched on at day 250 is inside; one joining exactly 90 days after the switch-on is outside,
/// one a second earlier inside.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_08_switch_on_cases_hold_on_the_read_dates(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    assert!(switch(&mut conn, OFF_AT_LOAD, true).await);
    let opened_at = opened(&mut conn, &[OFF_AT_LOAD]).await[0];
    let launch = opened_at - days(250);
    let cases = [
        (launch + days(200), true),
        (opened_at + days(90), false),
        (opened_at + days(90) - SignedDuration::from_secs(1), true),
    ];
    for (joined, inside) in cases {
        let id = signed_up(&mut conn, joined, &[OFF_AT_LOAD]).await;
        let facts = join_facts(&mut conn, id).await.expect("read").expect("row");
        let dates = JoinDates {
            joined: facts.joined(),
            launch,
            trades_opened: trades_opened(&mut conn, facts.trades())
                .await
                .expect("read"),
        };
        assert_eq!(in_free_win_window(&dates), inside, "joined {joined}");
    }
}
