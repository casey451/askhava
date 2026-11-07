//! The SEC-038 step-up gate on every guarded business change (TD-SEC-17; SEC-038, SEC-048; T085,
//! D-480, D-492): a live 30-day session with no Verify code in the last 5 minutes is refused and
//! logged, a 4-minute-old code passes, 5:01 is refused. All IDs are fake.

use askhava_businesses::api::{Acting, add_owner, create, record_step_up};
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, BusinessPrincipal, CookieJar, Realm, SensitiveChange, StepUpError,
    business_principal, require_step_up, start_session,
};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const ROUTE: &str = "/b/settings";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

/// A business owner signed in with a 30-day business session.
async fn signed_in(db: &mut PgConnection) -> (Acting, BusinessPrincipal) {
    let business = create(db, false, at(0)).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(0xd1), at(0))
        .await
        .expect("owner");
    let login = (Realm::Business, member);
    let jar = start_session(db, CookieJar::new(), login, BUSINESS_SESSION_LIFE, at(0)).await;
    let jar = jar.expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(member)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let principal = business_principal(db, &jar, at(1)).await.expect("read");
    (Acting { business, member }, principal.expect("signed in"))
}

async fn refusals(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM portals.security_event WHERE kind = 'authz_denied' AND route = $1",
    )
    .bind(ROUTE)
    .fetch_one(db)
    .await
    .expect("events")
}

/// TD-SEC-17 (SEC-038, D-480): for each listed change, no code is refused and logged; a code
/// 4 minutes old passes; at 5:01 it is refused and logged again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_17_each_change_needs_a_code_from_the_last_five_minutes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (who, principal) = signed_in(&mut db).await;
    let mut logged = 0;
    for change in SensitiveChange::ALL {
        let none = require_step_up(&mut db, &principal, (change, ROUTE), at(2)).await;
        assert_eq!(none, Err(StepUpError::NeedsCode), "{change:?} with no code");
        logged += 1;
        assert_eq!(refusals(&mut db).await, logged, "{change:?} refusal logged");
    }
    record_step_up(&mut db, who, at(10)).await.expect("code");
    for change in SensitiveChange::ALL {
        let fresh = require_step_up(&mut db, &principal, (change, ROUTE), at(250)).await;
        assert_eq!(fresh, Ok(()), "{change:?} at 4 minutes");
        let late = require_step_up(&mut db, &principal, (change, ROUTE), at(311)).await;
        assert_eq!(late, Err(StepUpError::NeedsCode), "{change:?} at 5:01");
        logged += 1;
    }
    assert_eq!(refusals(&mut db).await, logged);
    let edge = require_step_up(
        &mut db,
        &principal,
        (SensitiveChange::SavedCard, ROUTE),
        at(310),
    );
    assert_eq!(edge.await, Ok(()), "exactly 5:00 is still fresh");
}

/// SEC-038: a step-up time after now is no fresh code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_038_a_future_code_is_not_fresh(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (who, principal) = signed_in(&mut db).await;
    record_step_up(&mut db, who, at(100)).await.expect("code");
    let change = (SensitiveChange::AutoConfirm, ROUTE);
    let early = require_step_up(&mut db, &principal, change, at(50)).await;
    assert_eq!(early, Err(StepUpError::NeedsCode));
}
