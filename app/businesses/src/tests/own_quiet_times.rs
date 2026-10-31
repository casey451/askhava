//! A business's self-pause and own quiet times (TD-QH-03 and T182's cases of TD-BIZ-02: BIZ-010,
//! D-028, D-085, D-968, D-969).
//!
//! The pure cases walk every quarter-hour window against every minute of the day; the database
//! cases run on a fresh database with fake IDs only.

use askhava_ops::api::{Actor, EXPORT_PAGE_MAX, export_audit};
use jiff::Timestamp;
use jiff::civil::{Time, time};
use sqlx::types::Uuid;

use crate::api::{
    BusinessesError, Checks, Event, OwnHours, PauseReason, Widens, apply, business, create,
    own_hours, pause_self, resume_self, set_own_hours,
};
use crate::rules::{APP_CLOSE_MINUTE, APP_OPEN_MINUTE, may_notify, may_resume_itself};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn hm(minute: i16) -> Time {
    let (h, m) = (minute / 60, minute % 60);
    time(
        i8::try_from(h).expect("hour"),
        i8::try_from(m).expect("minute"),
        0,
        0,
    )
}

/// TD-QH-03 (BIZ-010, D-085): own quiet times outside 8 a.m. to 8 p.m., empty, backwards or off
/// the whole minute are refused, so they can never widen the window.
#[test]
fn td_qh_03_own_quiet_times_that_widen_are_refused() {
    for (open, close) in [
        (time(7, 59, 0, 0), time(17, 0, 0, 0)),
        (time(9, 0, 0, 0), time(20, 1, 0, 0)),
        (time(0, 0, 0, 0), time(23, 59, 0, 0)),
        (time(12, 0, 0, 0), time(12, 0, 0, 0)),
        (time(15, 0, 0, 0), time(9, 0, 0, 0)),
        (time(9, 0, 30, 0), time(17, 0, 0, 0)),
    ] {
        assert_eq!(OwnHours::new(open, close), Err(Widens), "{open} {close}");
    }
    let all = OwnHours::new(time(8, 0, 0, 0), time(20, 0, 0, 0)).expect("app-wide");
    assert_eq!((all.open_minute(), all.close_minute()), (480, 1200));
    assert_eq!(OwnHours::from_minutes(479, 600), Err(Widens));
}

/// TD-QH-03 (BIZ-010, D-085, QH-001): for every quarter-hour own window and every minute of the
/// day, a notice may go only inside both the app-wide window and the own window; with no own
/// window, exactly 8:00 to 19:59.
#[test]
fn td_qh_03_own_quiet_times_only_narrow_the_window() {
    let app = APP_OPEN_MINUTE..APP_CLOSE_MINUTE;
    for minute in 0..24 * 60 {
        assert_eq!(may_notify(None, hm(minute)), app.contains(&minute));
    }
    for open in (0..24 * 4).map(|q| q * 15) {
        for close in (open + 15..=24 * 60).step_by(15) {
            let Ok(own) = OwnHours::from_minutes(open, close) else {
                assert!(open < APP_OPEN_MINUTE || close > APP_CLOSE_MINUTE);
                continue;
            };
            for minute in 0..24 * 60 {
                let want = app.contains(&minute) && (open..close).contains(&minute);
                assert_eq!(may_notify(Some(own), hm(minute)), want, "{open} {close}");
            }
        }
    }
}

/// TD-BIZ-02 (BIZ-010, BIZ-009, D-969): a business may resume itself only from its own pause.
#[test]
fn td_biz_02_only_a_self_pause_is_self_resumed() {
    for reason in PauseReason::ALL {
        let own = reason.code() == "self";
        assert!(may_resume_itself(Some(reason)) == own, "{}", reason.code());
    }
    assert!(!may_resume_itself(None));
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

async fn active(db: &mut Db) -> Uuid {
    let id = create(db, false, at(0)).await.expect("create");
    let checks = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, id, Event::Checked(checks), Actor::System, at(1))
        .await
        .expect("active");
    id
}

/// TD-BIZ-02 (BIZ-010): the business pauses and resumes itself; a pause for another reason is
/// never self-resumed; an unknown business is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_02_business_pauses_and_resumes_itself(mut db: Db) {
    let id = active(&mut db).await;
    let paused = pause_self(&mut db, id, at(2)).await.expect("pause");
    let b = business(&mut db, id).await.expect("read");
    let reason = b.pause_reason.map(PauseReason::code);
    assert_eq!((paused.code(), reason), ("paused", Some("self")));
    let resumed = resume_self(&mut db, id, at(3)).await.expect("resume");
    assert_eq!(resumed.code(), "active");
    assert_eq!(
        resume_self(&mut db, id, at(4)).await,
        Err(BusinessesError::Refused)
    );
    let license = Event::Pause(PauseReason::License);
    apply(&mut db, id, license, Actor::System, at(5))
        .await
        .expect("pause");
    assert_eq!(
        resume_self(&mut db, id, at(6)).await,
        Err(BusinessesError::Refused)
    );
    let b = business(&mut db, id).await.expect("read");
    let reason = b.pause_reason.map(PauseReason::code);
    assert_eq!(reason, Some("license"));
    let missing = resume_self(&mut db, Uuid::from_u128(0xBAD), at(7)).await;
    assert_eq!(missing, Err(BusinessesError::UnknownBusiness));
}

/// TD-BIZ-02 (BIZ-010, D-085): own quiet times are stored, read back and cleared, each change
/// with one audit row naming the field.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_02_own_quiet_times_are_stored(mut db: Db) {
    let id = active(&mut db).await;
    assert_eq!(own_hours(&mut db, id).await, Ok(None));
    let hours = OwnHours::new(time(9, 0, 0, 0), time(17, 30, 0, 0)).expect("hours");
    let actor = Actor::Business(id);
    set_own_hours(&mut db, id, Some(hours), actor)
        .await
        .expect("set");
    assert_eq!(own_hours(&mut db, id).await, Ok(Some(hours)));
    set_own_hours(&mut db, id, None, actor)
        .await
        .expect("clear");
    assert_eq!(own_hours(&mut db, id).await, Ok(None));
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    let fields = rows
        .iter()
        .filter(|r| r.subject_id == id && r.action == "businesses.field_changed")
        .count();
    assert_eq!(fields, 2);
    let missing = set_own_hours(&mut db, Uuid::from_u128(0xBAD), None, actor).await;
    assert_eq!(missing, Err(BusinessesError::UnknownBusiness));
}

/// TD-QH-03 (BIZ-010, D-085): the table itself refuses a window wider than 8 a.m. to 8 p.m. or
/// half set, even from the app role.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_03_the_table_refuses_a_wider_window(mut db: Db) {
    let id = active(&mut db).await;
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let wider: [(Option<i16>, Option<i16>); 3] = [
        (Some(420), Some(1000)),
        (Some(480), Some(1260)),
        (Some(600), None),
    ];
    for (open, close) in wider {
        let r = sqlx::query!(
            "UPDATE businesses.business SET own_open_minute = $2, own_close_minute = $3 \
             WHERE id = $1",
            id,
            open,
            close
        )
        .execute(&mut *db)
        .await;
        assert!(r.is_err(), "{open:?} {close:?}");
    }
}

/// TD-BIZ-02 (BIZ-010, D-028): there is no weekly card limit: no column of the business record
/// caps cards per week.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_02_no_weekly_card_limit(mut db: Db) {
    let n = sqlx::query_scalar!(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'businesses' \
         AND (column_name LIKE '%week%' OR column_name LIKE '%max_card%' \
         OR column_name LIKE '%card_limit%')"
    )
    .fetch_one(&mut *db)
    .await
    .expect("columns");
    assert_eq!(n, Some(0));
}
