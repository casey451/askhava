//! The home page's texting number reader against PostgreSQL (T725; WEB-017, CON-005, D-4709):
//! before and after the Gate C launch date, an unset date, the switch off, a paused number, an
//! unset number and the on case. Fake numbers (555-01xx) and fake owner IDs only.

use askhava_messaging::api::{Audience, SendingNumber, record_suspension, switch_off};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;

use crate::api::homeowner_texting_number;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

/// A fake configured homeowner number.
const FAKE: &str = "+19285550101";

/// The fake Gate C instant.
fn launch() -> Timestamp {
    "2027-03-01T15:00:00Z".parse().expect("instant")
}

/// The fake number, parsed as `server` parses `ASKHAVA_HOMEOWNER_NUMBER`.
fn number() -> Option<SendingNumber> {
    Some(SendingNumber::parse(FAKE).expect("fake number"))
}

/// The reader with the fake number, `launch` and `now`.
async fn read(db: &mut Db, launch: Option<Timestamp>, now: Timestamp) -> Option<String> {
    homeowner_texting_number(db, (number(), launch), now)
        .await
        .expect("read")
}

/// WEB-017, CON-005: the number shows from the launch instant on, never before it, and never
/// without a launch date or a configured number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn web_017_the_number_shows_only_from_the_launch_date(mut db: Db) {
    let at = launch();
    let before = at - SignedDuration::from_secs(1);
    let after = at + SignedDuration::from_hours(24);
    assert_eq!(read(&mut db, Some(at), after).await.as_deref(), Some(FAKE));
    assert_eq!(read(&mut db, Some(at), at).await.as_deref(), Some(FAKE));
    assert_eq!(read(&mut db, Some(at), before).await, None);
    assert_eq!(read(&mut db, None, after).await, None);
    let unset = homeowner_texting_number(&mut db, (None, Some(at)), after).await;
    assert_eq!(unset.expect("read"), None);
}

/// WEB-017, CON-005, TXT-021: after the launch date, the homeowner switch off shows no number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_005_the_switch_off_shows_no_number(mut db: Db) {
    let now = launch() + SignedDuration::from_hours(1);
    switch_off(&mut db, Audience::Homeowner, (Uuid::from_u128(0x0725), now))
        .await
        .expect("switch off");
    assert_eq!(read(&mut db, Some(launch()), now).await, None);
}

/// WEB-017, CON-005, TXT-022: after the launch date, a paused (suspended) number shows none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_005_a_paused_number_shows_no_number(mut db: Db) {
    let now = launch() + SignedDuration::from_hours(1);
    record_suspension(
        &mut db,
        Audience::Homeowner,
        now,
        (Uuid::from_u128(0x0725), now),
    )
    .await
    .expect("suspend");
    assert_eq!(read(&mut db, Some(launch()), now).await, None);
}
