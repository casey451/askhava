//! TD-PICK-68 (PICK-037, PICK-044, PICK-043, BIZ-037; T682, D-4361, D-5034): a pick waiting for
//! Confirm gets its reminders through `picks.confirm_reminder`, each once; a HOLD moves them and
//! drops the Hold line; an emergency pick gets its one, at night only to switched-on people; a
//! pressed, cancelled, ended or lapsed pick gets none. PostgreSQL; every ID and mailbox is fake.

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create};
use askhava_core::api::{Claimed, Guarded, IdSecret, brand};
use askhava_jobs::api::{JobStatus, job_number, move_to};
use askhava_messaging::api::{
    Line, Lines, MailboxKind, NoticeSender, NumberState, SendChecks, business_notice,
};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, CONFIRM_REMINDER_KIND, ConfirmSettings, HOLD_END_TEMPLATE, WindowRules,
    hold_pick, notice_sent, run_confirm_reminder_at,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Arizona wall-clock time on November 10, 2026 (UTC-7 all year).
fn az(hour: i8, minute: i8) -> Timestamp {
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::offset(-7));
    let day = date(2026, 11, 10).at(hour, minute, 0, 0);
    day.to_zoned(zone).expect("time").timestamp()
}

/// A business with an owner (mailbox `seed`), a "Confirm and billing" member (`seed + 1`) and a
/// "Cards and quotes" member (`seed + 2`); with `night`, ticked after-hours with only the billing
/// member's "Emergency emails at any hour" on.
async fn team(pool: &PgPool, seed: u128, night: bool) -> Uuid {
    let mut db = pool.acquire().await.expect("conn");
    let (t, mailbox) = (az(8, 0), |n| Uuid::from_u128(seed + n));
    let business = create(&mut db, false, t).await.expect("business");
    let owner = add_owner(&mut db, business, mailbox(0), t).await;
    let member = owner.expect("owner");
    let by = Acting { business, member };
    let bill = Roles::new(false, true).expect("roles");
    let bill = add_member(&mut db, by, mailbox(1), bill, t).await;
    let cards = Roles::new(true, false).expect("roles");
    let cards = add_member(&mut db, by, mailbox(2), cards, t).await;
    cards.expect("cards member");
    if night {
        let on = "WITH b AS (UPDATE businesses.business SET after_hours = true WHERE id = $1) \
                  INSERT INTO businesses.emergency_switch (member_id, business_id, switch_on) \
                  VALUES ($2, $1, true)";
        let done = sqlx::query(on).bind(business).bind(bill.expect("bill"));
        done.execute(&mut *db).await.expect("switch");
    }
    business
}

/// A job (an emergency when `emergency`) picked at `at` with `business`'s pick notice sent then.
async fn sent(pool: &PgPool, (business, emergency): (Uuid, bool), at: Timestamp) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut db, at, emergency, Actor::System).await;
    let job = job.expect("job");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
        (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
    ] {
        let moved = move_to(&mut db, job, step, at, Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'awaiting_confirm', $3) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    let due = notice_sent(&mut db, pick, at, ConfirmSettings::default()).await;
    assert_eq!(due.expect("sent").is_none(), emergency);
    (job, pick)
}

/// Every queued task of `kind` about `pick`, oldest first: its payload and run time.
async fn tasks(pool: &PgPool, kind: &str, pick: Uuid) -> Vec<(Value, Timestamp)> {
    let rows: Vec<(Value, jiff_sqlx::Timestamp)> = sqlx::query_as(
        "SELECT payload, run_at FROM core.task WHERE kind = $1 AND payload->>'pick' = $2 \
         ORDER BY id",
    )
    .bind(kind)
    .bind(pick.to_string())
    .fetch_all(pool)
    .await
    .expect("tasks");
    rows.into_iter().map(|(p, t)| (p, t.to_jiff())).collect()
}

/// Runs `pick`'s reminder task at `now` and returns the reminder notices queued so far and the
/// reminder runs' times.
async fn run(pool: &PgPool, pick: Uuid, now: Timestamp) -> (Vec<Value>, Vec<Timestamp>) {
    let runs = tasks(pool, CONFIRM_REMINDER_KIND, pick).await;
    let task = Claimed {
        id: 1,
        kind: String::new(),
        payload: runs[0].0.clone(),
        attempts: 1,
    };
    let ran = run_confirm_reminder_at(pool.clone(), WindowRules::default(), task, now).await;
    ran.expect("reminder run");
    let mut notices = tasks(pool, BUSINESS_NOTICE_KIND, pick).await;
    notices.retain(|(notice, _)| notice["template"] != HOLD_END_TEMPLATE);
    let runs = tasks(pool, CONFIRM_REMINDER_KIND, pick).await;
    for (notice, at) in &notices {
        assert_eq!(notice["emergency"], true, "carries its own subject");
        assert!(runs.iter().any(|(_, run)| run == at), "sent when it is due");
    }
    let notices = notices.into_iter().map(|(p, _)| p).collect();
    (notices, runs.into_iter().map(|(_, at)| at).collect())
}

fn mailboxes(notice: &Value) -> Vec<Uuid> {
    let to = notice["to"].as_array().expect("to").iter();
    to.map(|v| v.as_str().expect("id").parse().expect("uuid"))
        .collect()
}

struct NoChecks;

impl SendChecks for NoChecks {
    async fn may_text(&self, _: &mut PgConnection, _: Uuid, _: Uuid) -> bool {
        false
    }

    async fn accept_by(&self, _: &mut PgConnection, _: Uuid, _: Timestamp) -> Option<Timestamp> {
        None
    }
}

/// Stores `notices` through `messaging`'s handler at `now` and returns the reminder emails' count.
async fn emails(pool: &PgPool, notices: &[Value], now: Timestamp) -> i64 {
    let secret = |s: &str| SecretString::from(s.to_owned());
    let links = IdSecret::new(1, secret("test-only-link-secret-not-a-real-key")).expect("links");
    let keys = (secret("test-only-email-secret-not-real"), links);
    let postal = "PO Box 0000, Lake Havasu City, AZ 86403";
    let sender = NoticeSender::new(keys, postal, MailboxKind::PoBox).expect("sender");
    let line = || Line {
        texting_on: false,
        number: None,
        state: NumberState::Working,
    };
    let lines = Lines {
        homeowner: line(),
        business: line(),
    };
    let mut db = pool.acquire().await.expect("conn");
    for (id, notice) in (1..).zip(notices) {
        let stored =
            business_notice(&mut db, (&sender, &NoChecks, &lines), (id, notice), now).await;
        stored.expect("stored");
    }
    sqlx::query_scalar("SELECT count(*) FROM messaging.email_outbox WHERE subject LIKE '%eminder%'")
        .fetch_one(&mut *db)
        .await
        .expect("count")
}

/// TD-PICK-68: a notice sent at 9 a.m. (window to 1 p.m.) gets reminders at 10, 11 and noon,
/// each once on a rerun, to the owner and the billing member; a HOLD at 10:30 moves the end to
/// 3 p.m., so they go on to 2 p.m., the last one marked, with no Hold line; each is its own email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_68_hourly_reminders_follow_the_window_and_a_hold(pool: PgPool) {
    let business = team(&pool, 0x6810, false).await;
    let (job, pick) = sent(&pool, (business, false), az(9, 0)).await;
    let first = tasks(&pool, CONFIRM_REMINDER_KIND, pick).await;
    assert_eq!(first.iter().map(|t| t.1).collect::<Vec<_>>(), [az(10, 0)]);
    let mut db = pool.acquire().await.expect("conn");
    let n = job_number(&mut db, job).await.expect("number");
    let short = brand().short_name();
    assert_eq!(run(&pool, pick, az(9, 59)).await.0.len(), 0, "not yet due");
    run(&pool, pick, az(10, 0)).await;
    let (notices, runs) = run(&pool, pick, az(10, 0)).await;
    assert_eq!(
        (notices.len(), runs.as_slice()),
        (1, &[az(10, 0), az(11, 0)][..])
    );
    let subject = format!("{short}: Reminder: accept job {n} by 1 p.m.");
    let hold = format!("{subject} Need more time? Hold for 2 more hours: confirm by 3 p.m.");
    assert_eq!(notices[0]["subject"], subject);
    assert_eq!(notices[0]["first_line"], hold);
    let both = [0x6810, 0x6811].map(Uuid::from_u128);
    assert_eq!(mailboxes(&notices[0]), both, "never the cards-only member");
    let rules = WindowRules::default();
    let held = hold_pick(&mut db, pick, Roles::BOTH, az(10, 30), rules).await;
    assert_eq!(held, Ok(Ok(az(15, 0))));
    for hour in [11, 12, 13, 14, 15, 15] {
        run(&pool, pick, az(hour, 0)).await;
    }
    let (notices, runs) = run(&pool, pick, az(16, 0)).await;
    assert_eq!(
        runs.last(),
        Some(&az(15, 0)),
        "no run after the window's end"
    );
    assert_eq!(
        notices.len(),
        5,
        "10, 11, noon, 1 and 2 p.m., none at or after 3 p.m."
    );
    let after_hold = format!("{short}: Reminder: accept job {n} by 3 p.m.");
    assert_eq!(
        notices[1]["first_line"], after_hold,
        "no Hold line once held"
    );
    assert_eq!(notices[3]["subject"], after_hold);
    let last = format!(
        "{short}: Last reminder: job {n} has 1 hour to accept. After 3 p.m. the homeowner can \
         pick another quote."
    );
    assert_eq!(notices[4]["first_line"], last);
    assert_eq!(
        emails(&pool, &notices, az(14, 0)).await,
        10,
        "five emails each"
    );
}

/// TD-PICK-68: an emergency pick's one reminder goes 30 minutes after the send, at night too and
/// never held to 8 a.m.: by day to everyone who gets picks, at night only to the switched-on
/// billing member, and to nobody at a business with nobody switched on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_68_the_emergency_reminder_goes_once_by_the_night_routing(pool: PgPool) {
    let covered = team(&pool, 0x6820, true).await;
    let (_, by_day) = sent(&pool, (covered, true), az(9, 0)).await;
    run(&pool, by_day, az(9, 30)).await;
    let (notices, runs) = run(&pool, by_day, az(9, 30)).await;
    assert_eq!((notices.len(), runs[0]), (1, az(9, 30)));
    assert_eq!(notices[0]["template"], "pick_notice_emergency");
    let both = [0x6820, 0x6821].map(Uuid::from_u128);
    assert_eq!(mailboxes(&notices[0]), both);
    let (_, at_night) = sent(&pool, (covered, true), az(19, 40)).await;
    let (notices, runs) = run(&pool, at_night, az(20, 10)).await;
    assert_eq!((notices.len(), runs[0]), (1, az(20, 10)));
    assert_eq!(mailboxes(&notices[0]), [Uuid::from_u128(0x6821)]);
    let subject = notices[0]["subject"].as_str().expect("subject");
    assert!(subject.ends_with("by 8:40 p.m."), "{subject}");
    assert_eq!(emails(&pool, &notices, az(20, 10)).await, 1, "not held");
    let nobody = team(&pool, 0x6830, false).await;
    let (_, waits) = sent(&pool, (nobody, true), az(19, 40)).await;
    assert!(run(&pool, waits, az(20, 10)).await.0.is_empty());
    let (_, late) = sent(&pool, (nobody, true), az(19, 20)).await;
    let (notices, runs) = run(&pool, late, az(20, 5)).await;
    assert_eq!(runs[0], az(19, 50), "due by day, run late at night");
    assert!(notices.is_empty(), "nobody is switched on");
    let claimed: i64 = sqlx::query_scalar("SELECT count(*) FROM picks.confirm_reminder")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(claimed, 3, "the late one was claimed and went to nobody");
}

/// TD-PICK-68: a pick whose business pressed Confirm, a lapsed or ended pick and a pick whose job
/// was cancelled after an earlier reminder get no reminder and no further run; a non-emergency
/// run at night, or in the last 15 minutes before 8 p.m. (QH-005, D-5035), sends nothing and
/// waits for 8 a.m., when the reminder goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_68_a_pressed_ended_or_cancelled_pick_gets_none(pool: PgPool) {
    let business = team(&pool, 0x6840, false).await;
    let mut db = pool.acquire().await.expect("conn");
    let (pressed_job, pressed) = sent(&pool, (business, false), az(9, 0)).await;
    let (_, lapsed) = sent(&pool, (business, false), az(9, 0)).await;
    let (cancelled_job, cancelled) = sent(&pool, (business, false), az(9, 0)).await;
    assert_eq!(run(&pool, cancelled, az(10, 0)).await.0.len(), 1);
    for (job, to) in [
        (pressed_job, JobStatus::PaymentPending),
        (cancelled_job, JobStatus::Cancelled),
    ] {
        let step = (JobStatus::AwaitingConfirm, to);
        let moved = move_to(&mut db, job, step, az(10, 30), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    let expire = sqlx::query("UPDATE picks.pick SET status = 'expired' WHERE id = $1");
    expire.bind(lapsed).execute(&pool).await.expect("lapsed");
    for (pick, before) in [(pressed, 0), (lapsed, 0), (cancelled, 1)] {
        let (notices, runs) = run(&pool, pick, az(11, 0)).await;
        assert_eq!(notices.len(), before, "nothing new");
        assert!(runs.iter().all(|at| *at <= az(11, 0)), "no further run");
    }
    let (_, evening) = sent(&pool, (business, false), az(18, 30)).await;
    let (notices, runs) = run(&pool, evening, az(21, 0)).await;
    assert!(notices.is_empty(), "quiet hours");
    let eight = az(8, 0) + jiff::SignedDuration::from_hours(24);
    assert_eq!(runs, [az(19, 30), eight]);
    let (_, dusk) = sent(&pool, (business, false), az(18, 45)).await;
    let (notices, runs) = run(&pool, dusk, az(19, 45)).await;
    assert!(notices.is_empty(), "too close to 8 p.m. for a send");
    assert_eq!(runs, [az(19, 45), eight], "waits for 8 a.m.");
    assert_eq!(run(&pool, dusk, eight).await.0.len(), 1, "sent at 8 a.m.");
    let (_, in_time) = sent(&pool, (business, false), az(18, 44)).await;
    assert_eq!(run(&pool, in_time, az(19, 44)).await.0.len(), 1);
}
