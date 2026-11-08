//! Confirm by email alone (TD-PICK-25, TD-PICK-26, TD-PICK-27, TD-PICK-31, TD-PICK-16; PICK-019,
//! PICK-033, PICK-037, PICK-043 to PICK-046, BIZ-033, BIZ-039). Fake IDs, names and numbers only.
//!
//! The 4-business-hour window and its edges, a reminder every business hour with the Hold line and
//! the last one, a HOLD moving both, quiet hours, the all-bounced grace once, the emergency's one
//! reminder, the lapse text and its timing, the BIZ-033 owner email, and each reminder and
//! extension stored once. No function here can place a call (BIZ-039).

use askhava_businesses::api::Roles;
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_picks::api::{
    ConfirmSettings, EmailOutcome, Extension, HoldAsk, Reminder, WindowRules, bounce_grace,
    claim_reminder, confirm_by, emergency_plan, emergency_reminders, extend_once, hold, lapse_plan,
    lapse_text, member_confirmed_line, owner_told, reminder_lines, reminders, window_open,
};
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// Arizona wall-clock time on November `day`, 2026 (UTC-7 all year).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

fn second_before(at: Timestamp) -> Timestamp {
    at - SignedDuration::from_secs(1)
}

fn times(list: &[Reminder]) -> Vec<(Timestamp, bool)> {
    list.iter().map(|r| (r.at, r.last)).collect()
}

fn ask() -> HoldAsk {
    HoldAsk {
        by: Roles::BOTH,
        emergency: false,
        held: false,
        away_delayed: false,
    }
}

/// TD-PICK-25, TD-PICK-31, TD-PICK-16 (PICK-019, PICK-037, PICK-043, D-345, D-380): a pick at
/// 10:00 ends at 14:00; reminders at 11, 12 and 13, the last one last, never a second early, each
/// with the Hold line; a HOLD at 12:30 moves the end to 16:00 and the last reminder to 15:00, with
/// no Hold line after it.
#[test]
fn td_pick_25_reminders_every_business_hour_and_a_hold() {
    let rules = WindowRules::default();
    let due = confirm_by(az(9, 10, 0), ConfirmSettings::default());
    assert_eq!(due, az(9, 14, 0));
    assert!(window_open(second_before(due), due) && !window_open(due, due));
    let list = reminders(az(9, 10, 0), due, true, rules);
    let want = [
        (az(9, 11, 0), false),
        (az(9, 12, 0), false),
        (az(9, 13, 0), true),
    ];
    assert_eq!(times(&list), want);
    assert!(list.iter().all(|r| r.hold_to == Some(az(9, 16, 0))));
    assert!(list[0].due(az(9, 11, 0)) && !list[0].due(second_before(az(9, 11, 0))));
    let held = hold(ask(), (az(9, 12, 30), due), rules).expect("hold");
    assert_eq!(held, az(9, 16, 0));
    let after = reminders(az(9, 10, 0), held, false, rules);
    assert_eq!(after.len(), 5);
    assert_eq!(times(&after)[4], (az(9, 15, 0), true));
    assert!(after.iter().all(|r| r.hold_to.is_none()));
}

/// TD-PICK-25, TD-PICK-16, TD-PICK-31 (PICK-019, QH-001): a pick at 19:30 counts 30 minutes today
/// and ends at 11:30 the next day; one at 20:30 counts from 08:00; a reminder that would fall at
/// 8 p.m. waits for 8 a.m.
#[test]
fn td_pick_25_evening_picks_and_quiet_hours() {
    let (rules, settings) = (WindowRules::default(), ConfirmSettings::default());
    let late = confirm_by(az(9, 19, 30), settings);
    assert_eq!(late, az(10, 11, 30));
    let list = reminders(az(9, 19, 30), late, true, rules);
    let want = [
        (az(10, 8, 30), false),
        (az(10, 9, 30), false),
        (az(10, 10, 30), true),
    ];
    assert_eq!(times(&list), want);
    let night = confirm_by(az(9, 20, 30), settings);
    assert_eq!(night, az(10, 12, 0));
    assert_eq!(
        reminders(az(9, 20, 30), night, true, rules)[0].at,
        az(10, 9, 0)
    );
    let seven = reminders(
        az(9, 19, 0),
        confirm_by(az(9, 19, 0), settings),
        true,
        rules,
    );
    assert_eq!(seven[0].at, az(10, 8, 0));
    let none = WindowRules {
        reminder_minutes: 0,
        ..rules
    };
    assert!(reminders(az(9, 10, 0), az(9, 14, 0), true, none).is_empty());
}

/// TD-PICK-25, TD-PICK-26 (PICK-019, D-332, D-345): every email bounced or failed gives one more
/// business hour, once; a spam delivery, a wait, no email or an emergency job gives none.
#[test]
fn td_pick_26_the_bounce_grace_is_one_hour_once() {
    use EmailOutcome::{Bounced, Delivered, Failed, Spam, Waiting};
    let (rules, due) = (WindowRules::default(), az(9, 14, 0));
    let all = [Bounced, Failed, Bounced];
    assert_eq!(
        bounce_grace(&all, (false, false), due, rules),
        Some(az(9, 15, 0))
    );
    assert_eq!(bounce_grace(&all, (false, true), due, rules), None);
    assert_eq!(bounce_grace(&all, (true, false), due, rules), None);
    for other in [Spam, Delivered, Waiting] {
        assert_eq!(
            bounce_grace(&[Bounced, other], (false, false), due, rules),
            None
        );
    }
    assert_eq!(bounce_grace(&[], (false, false), due, rules), None);
}

/// TD-PICK-26, TD-PICK-25 (PICK-044, BIZ-039): an emergency pick at 14:00 has one reminder at
/// 14:30:00 with no Hold line; one at 23:00 of an after-hours business goes at once with 60 clock
/// minutes; at 19:40 without after-hours the reminder would wait past the end, so none goes.
#[test]
fn td_pick_26_the_emergency_reminder() {
    let rules = WindowRules::default();
    let day = emergency_reminders(&emergency_plan(az(9, 14, 0), false, rules));
    assert_eq!(times(&day), [(az(9, 14, 30), true)]);
    assert!(day[0].hold_to.is_none() && !day[0].due(second_before(az(9, 14, 30))));
    let night = emergency_plan(az(9, 23, 0), true, rules);
    assert_eq!(
        (night.send_at, night.confirm_by),
        (az(9, 23, 0), az(10, 0, 0))
    );
    assert_eq!(emergency_reminders(&night)[0].at, az(9, 23, 30));
    assert!(emergency_reminders(&emergency_plan(az(9, 19, 40), false, rules)).is_empty());
}

/// TD-PICK-31 (PICK-037, D-380): the reminder, last reminder and Hold line wording, both
/// languages.
#[test]
fn td_pick_31_reminder_wording() {
    let due = az(9, 14, 0);
    let first = Reminder {
        at: az(9, 11, 0),
        last: false,
        hold_to: Some(az(9, 16, 0)),
    };
    assert_eq!(
        reminder_lines(1042, due, first, Language::En),
        [
            "Ask Hava: Reminder: accept job 1042 by 2 p.m.",
            "Need more time? Hold for 2 more hours: confirm by 4 p.m."
        ]
    );
    let last = Reminder {
        at: az(9, 13, 0),
        last: true,
        hold_to: None,
    };
    assert_eq!(
        reminder_lines(1042, due, last, Language::En),
        [
            "Ask Hava: Last reminder: job 1042 has 1 hour to accept. After 2 p.m. the homeowner can pick another quote."
        ]
    );
    let half = Reminder {
        at: az(9, 12, 30),
        ..last
    };
    assert!(reminder_lines(1042, az(9, 14, 30), half, Language::En)[0].contains("has 2 hours"));
    assert!(reminder_lines(1042, az(9, 13, 30), half, Language::Es)[0].contains("1 hora"));
    let odd = Reminder {
        at: az(9, 13, 30),
        ..last
    };
    assert!(reminder_lines(1042, due, odd, Language::En)[0].contains("30 minutes"));
    let es = reminder_lines(1042, az(9, 16, 30), first, Language::Es);
    assert!(es[0].starts_with("Ask Hava: Recordatorio") && es[0].contains("4:30 p.m."));
    assert!(es[1].contains("4 p.m.") && es.len() == 2);
    let morning = Reminder {
        at: az(9, 9, 0),
        ..last
    };
    assert!(reminder_lines(1, az(9, 10, 0), morning, Language::Es)[0].contains("10 a.m."));
    assert!(reminder_lines(1, az(9, 12, 0), morning, Language::Es)[0].contains("3 horas"));
    assert!(reminder_lines(1, az(9, 9, 30), morning, Language::Es)[0].contains("30 minutos"));
}

/// TD-PICK-27 (PICK-046, PICK-035, QH-004, D-345, D-420): the lapse lists only the other
/// businesses' quotes, goes at once on an emergency and under quiet hours otherwise; the text ends
/// with the Terms line, and with no other quote it has no 911 line.
#[test]
fn td_pick_27_the_lapse_text() {
    let (lapsed, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let quotes = [(1, lapsed), (2, b), (3, c)];
    let plan = lapse_plan((az(9, 14, 0), false), lapsed, &quotes);
    assert_eq!((plan.send_at, plan.others), (az(9, 14, 0), vec![2, 3]));
    assert_eq!(
        lapse_plan((az(9, 20, 30), false), lapsed, &quotes).send_at,
        az(10, 8, 0)
    );
    assert_eq!(
        lapse_plan((az(10, 2, 0), true), lapsed, &quotes).send_at,
        az(10, 2, 0)
    );
    let lines = [
        "2. Fake Plumbing Co, $180".to_owned(),
        "3. Test Pipes LLC, $210".to_owned(),
    ];
    let text = lapse_text(
        "Sample Plumbing",
        &lines,
        "https://askhava.com/q/x",
        Language::En,
    );
    assert!(text.starts_with("Ask Hava: Sample Plumbing couldn't take it this time. You still have other quotes:\n2. Fake"));
    assert!(text.contains("reply the number and YES, for example 2 YES. Picking shares your first name and job address"));
    assert!(text.ends_with("By picking, you agree to the Terms: askhava.com/terms"));
    let none = lapse_text("Sample Plumbing", &[], "", Language::En);
    assert!(none.contains("We'll text you when another quote comes in.") && !none.contains("911"));
    assert!(
        lapse_text("Sample Plumbing", &lines, "x", Language::Es).ends_with("askhava.com/terms")
    );
    assert!(lapse_text("Sample Plumbing", &[], "", Language::Es).contains("otra cotización"));
}

/// BIZ-033 (D-258, D-491): the owner is told only when another phone confirmed, with the job, the
/// fee and the member.
#[test]
fn biz_033_the_owner_hears_of_a_members_confirm() {
    let (owner, member) = (Uuid::new_v4(), Uuid::new_v4());
    assert!(owner_told(member, Some(owner)) && owner_told(member, None));
    assert!(!owner_told(owner, Some(owner)));
    assert_eq!(
        member_confirmed_line(1042, 7_500, "Test Member", Language::En),
        "Ask Hava: Test Member confirmed job 1042 and paid the introduction fee by job size, $75. You can see the job in your portal."
    );
    assert!(member_confirmed_line(1042, 2_500, "Test Member", Language::Es).contains("$25"));
}

/// A job waiting in Awaiting confirm with one page pick of a fake business.
async fn picked(pool: &PgPool, at: Timestamp) -> Uuid {
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, false, Actor::System)
        .await
        .expect("job");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
    ] {
        let moved = move_to(&mut db, job, step, at, Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(Uuid::new_v4())
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick")
}

/// TD-PICK-16, TD-PICK-25 (PICK-037, PICK-043, PICK-019): each reminder is claimed once, a HOLD
/// and the grace are stored once each, and `askhava_app` can't delete either record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_16_each_reminder_and_extension_once(pool: PgPool) {
    let pick = picked(&pool, az(9, 10, 0)).await;
    let mut db = pool.acquire().await.expect("conn");
    let first = reminders(az(9, 10, 0), az(9, 14, 0), true, WindowRules::default())[0];
    assert!(
        claim_reminder(&mut db, pick, first, az(9, 11, 0))
            .await
            .expect("claim")
    );
    assert!(
        !claim_reminder(&mut db, pick, first, az(9, 11, 1))
            .await
            .expect("again")
    );
    for kind in [Extension::Hold, Extension::Bounce] {
        assert!(
            extend_once(&mut db, (pick, kind), az(9, 16, 0), az(9, 12, 30))
                .await
                .expect("x")
        );
        assert!(
            !extend_once(&mut db, (pick, kind), az(9, 18, 0), az(9, 13, 0))
                .await
                .expect("y")
        );
    }
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let gone = sqlx::query("DELETE FROM picks.confirm_reminder")
        .execute(&mut *db)
        .await;
    assert!(gone.is_err(), "reminders are append-only");
    let gone = sqlx::query("DELETE FROM picks.window_extension")
        .execute(&mut *db)
        .await;
    assert!(gone.is_err(), "extensions are append-only");
}
