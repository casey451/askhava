//! The public business name read for the LIST reply and the who's-calling text (TD-BIZ-43;
//! PICK-041, BIZ-022; D-336, D-2206, D-2943). Fake names only.
//!
//! As the app role, names are stored through T165's `save_business_name`: a business with a trade
//! name gives the trade name, one without gives its business name, in the order asked. A stored
//! name holding a phone number, a link or a direction to another channel (inserted by the fixture
//! directly, as a row stored before a stricter screen) is left out and raises one Normal (Low)
//! alert naming the business ID only, however often it is read. A business with no stored name
//! is left out with one alert; an unknown business is left out with none.

use askhava_businesses::api::{PUBLIC_NAME_ALERT, create, public_names, save_business_name};
use askhava_integration::set_app_role;
use jiff::Timestamp;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn now() -> Timestamp {
    "2026-11-11T17:00:00Z".parse().expect("fake time")
}

async fn app(pool: &sqlx::PgPool) -> Db {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// The open alerts of [`PUBLIC_NAME_ALERT`]: severity, subject kind and subject ID.
async fn alerts(pool: &sqlx::PgPool) -> Vec<(String, String, Uuid)> {
    sqlx::query_as(
        "SELECT severity, subject_kind, subject_id FROM ops.alert \
         WHERE kind = $1 AND resolved_at IS NULL ORDER BY raised_at, subject_id",
    )
    .bind(PUBLIC_NAME_ALERT)
    .fetch_all(pool)
    .await
    .expect("alerts")
}

fn alert_for(id: Uuid) -> (String, String, Uuid) {
    ("normal".to_owned(), "businesses.business".to_owned(), id)
}

/// TD-BIZ-43 (PICK-041, BIZ-022, D-2943): the trade name when given, else the business name, in
/// the order asked; no stored name and an unknown business are left out, only the first alerting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_43_trade_name_else_business_name(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let traded = create(&mut conn, false, now()).await.expect("business");
    let plain = create(&mut conn, false, now()).await.expect("business");
    let unnamed = create(&mut conn, false, now()).await.expect("business");
    let names = ("Example Fixit LLC", Some("Sample Odd Jobs"));
    save_business_name(&mut conn, traded, names, None)
        .await
        .expect("saved");
    save_business_name(&mut conn, plain, ("Sample Valley Plumbing", None), None)
        .await
        .expect("saved");
    let unknown = Uuid::from_u128(0x5a3e);
    let asked = [plain, unnamed, unknown, traded, plain];
    let got = public_names(&mut conn, &asked).await.expect("read");
    let want = vec![
        (plain, "Sample Valley Plumbing".to_owned()),
        (traded, "Sample Odd Jobs".to_owned()),
    ];
    assert_eq!(got, want);
    assert_eq!(alerts(&pool).await, vec![alert_for(unnamed)]);
    assert_eq!(public_names(&mut conn, &[]).await, Ok(Vec::new()));
}

/// TD-BIZ-43 (BIZ-022, PICK-041, D-336): a stored name holding a phone number, a link or a
/// channel direction is left out with one alert holding the business ID only, also when read
/// again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_43_a_name_failing_the_screen_is_left_out(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let stored: [(&str, Option<&str>); 4] = [
        ("Sample Plumbing 928 555 0199", None),
        ("Sample Plumbing", Some("sampleplumbing.com")),
        ("Find us on Facebook Sample Plumbing", None),
        ("Example Fixit LLC", None),
    ];
    let mut ids = Vec::new();
    for (name, trade) in stored {
        let id = create(&mut conn, false, now()).await.expect("business");
        sqlx::query(
            "INSERT INTO businesses.business_name (business_id, business_name, trade_name, \
             changed_at) VALUES ($1, $2, $3, now())",
        )
        .bind(id)
        .bind(name)
        .bind(trade)
        .execute(&pool)
        .await
        .expect("fixture row");
        ids.push(id);
    }
    for _ in 0..2 {
        let got = public_names(&mut conn, &ids).await.expect("read");
        assert_eq!(got, vec![(ids[3], "Example Fixit LLC".to_owned())]);
    }
    let mut got = alerts(&pool).await;
    got.sort_by_key(|a| a.2);
    let mut want: Vec<_> = ids[..3].iter().copied().map(alert_for).collect();
    want.sort_by_key(|a| a.2);
    assert_eq!(got, want);
}
