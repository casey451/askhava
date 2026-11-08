//! EMERGENCY card and pick emails, the "Emergency emails at any hour" switch with its test
//! emergency email, and the after-hours tick's conditions (TD-CARD-09, TD-BIZ-24, TD-INT-31:
//! CARD-021, BIZ-007, BIZ-037, BIZ-042, INT-029, INT-033, PICK-044; T150, D-344, D-346, D-362,
//! D-491, D-492). Every ID and mailbox is fake.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    AFTER_HOURS_OFF_KIND, Acting, Checks, EMERGENCY_TEST_KIND, EMERGENCY_YES_LIFE, Event,
    JobTypeChoice, Roles, SwitchError, VISIT_WITHIN, add_member, add_owner, apply, covered,
    covered_night_count, create, emergency_test_words, missed_in_a_row, queue_emergency_retests,
    recheck_after_hours, record_alert_answer, remove_member, request_welcome, send_emergency_test,
    set_after_hours, set_emergency_emails, set_job_types, set_towns, suspend_after_hours,
    switch_off_warns, switch_state, welcome_sent,
};
use askhava_ops::api::Actor;
use askhava_picks::api::{WindowRules, emergency_pick_plan, emergency_pick_subject};
use askhava_portals::api::{
    ALERT_QUESTION, DND_LINE, OFF_WARNING, SwitchStep, TEST_BUTTON, emergency_switch_page,
    post_step,
};
use askhava_quotes::api::{
    CardNotice, EMERGENCY_FIRST_LINE, emergency_card_subject, queue_card_notice,
    queue_emergency_card,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const NOTICE: &str = "messaging.business_notice";
const JOB: Uuid = Uuid::from_u128(0x150);
const NOTICE_WORDS: CardNotice<'static> = CardNotice {
    number: 1042,
    category: "HVAC",
    town: "Lake Havasu City",
    card: "AC not cooling. How soon: today.",
};

fn az(time: &str) -> Timestamp {
    format!("2026-11-02T{time}-07:00").parse().expect("time")
}

fn day(n: i64) -> SignedDuration {
    SignedDuration::from_hours(24 * n)
}

/// An active HVAC business in the town with the welcome sent: the owner (mailbox `seed`), a
/// "Cards and quotes" member (`seed + 1`) and a "Confirm and billing" member (`seed + 2`).
async fn business(db: &mut PgConnection, seed: u128) -> [Acting; 3] {
    let t = az("09:00:00");
    let b = create(db, false, t).await.expect("business");
    let owner = add_owner(db, b, Uuid::from_u128(seed), t).await;
    let owner = Acting {
        business: b,
        member: owner.expect("owner"),
    };
    let mut people = vec![owner];
    for (n, roles) in [(1, (true, false)), (2, (false, true))] {
        let roles = Roles::new(roles.0, roles.1).expect("roles");
        let m = add_member(db, owner, Uuid::from_u128(seed + n), roles, t).await;
        people.push(Acting {
            business: b,
            member: m.expect("member"),
        });
    }
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    request_welcome(db, b, t).await.expect("welcome");
    welcome_sent(db, b, t).await.expect("sent");
    serve_hvac(db, b).await;
    [people[0], people[1], people[2]]
}

async fn serve_hvac(db: &mut PgConnection, b: Uuid) {
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let job = JobTypeChoice::new("hvac_repair", "no cooling");
    let allowed: BTreeSet<_> = [job.clone()].into();
    set_job_types(db, b, &[job], &allowed, actor)
        .await
        .expect("job types");
}

/// `who` turns the switch on, sends the test and answers Yes at `at`.
async fn covers(db: &mut PgConnection, who: Acting, at: Timestamp) {
    set_emergency_emails(db, who, true, at).await.expect("on");
    send_emergency_test(db, who, at).await.expect("test");
    record_alert_answer(db, who, true, at).await.expect("yes");
}

async fn tasks(db: &mut PgConnection, kind: &str) -> Vec<(Value, Timestamp)> {
    let rows: Vec<(Value, jiff_sqlx::Timestamp)> =
        sqlx::query_as("SELECT payload, run_at FROM core.task WHERE kind = $1 ORDER BY run_at")
            .bind(kind)
            .fetch_all(db)
            .await
            .expect("tasks");
    rows.into_iter().map(|(p, t)| (p, t.to_jiff())).collect()
}

async fn ticked(db: &mut PgConnection, b: Uuid) -> bool {
    sqlx::query_scalar("SELECT after_hours FROM businesses.business WHERE id = $1")
        .bind(b)
        .fetch_one(db)
        .await
        .expect("tick")
}

async fn night(db: &mut PgConnection, at: Timestamp) -> u32 {
    covered_night_count(db, ("hvac_repair", TOWN), (at, EMERGENCY_YES_LIFE))
        .await
        .expect("count")
}

fn to(payload: &Value) -> Vec<Uuid> {
    serde_json::from_value(payload["to"].clone()).expect("to")
}

/// An after-hours business whose owner covers it; the "Cards and quotes" member has no switch.
async fn after_hours(db: &mut PgConnection, seed: u128) -> [Acting; 3] {
    let people = business(db, seed).await;
    covers(db, people[0], az("10:00:00")).await;
    let b = people[0].business;
    set_after_hours(db, b, true, (az("10:00:00"), EMERGENCY_YES_LIFE))
        .await
        .expect("tick");
    people
}

/// TD-CARD-09 (CARD-021, CARD-020, BIZ-039): an emergency job at 14:00: every card email has the
/// EMERGENCY subject and first line and goes to every recipient at once; no call is queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_09_emergency_card_at_14_00_goes_to_everyone_at_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = business(&mut db, 0xa0).await;
    let b = owner.business;
    let sent = queue_emergency_card(&mut db, (JOB, b), &NOTICE_WORDS, az("14:00:00")).await;
    assert!(sent.expect("queue"));
    let queued = tasks(&mut db, NOTICE).await;
    assert_eq!(queued.len(), 1, "one notice, all at once");
    let (p, run_at) = &queued[0];
    assert_eq!(*run_at, az("14:00:00"));
    assert_eq!(to(p), [Uuid::from_u128(0xa0), Uuid::from_u128(0xa1)]);
    let subject = "Ask Hava: EMERGENCY HVAC job in Lake Havasu City. Quote as soon as you can.";
    assert_eq!(p["subject"], subject);
    assert_eq!(emergency_card_subject("HVAC", "Lake Havasu City"), subject);
    assert_eq!(
        p["first_line"],
        "EMERGENCY: the homeowner chose emergency. Quote as soon as you can."
    );
    assert_eq!(p["first_line"], EMERGENCY_FIRST_LINE);
    let calls: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind LIKE '%call%'")
        .fetch_one(&mut *db)
        .await
        .expect("calls");
    assert_eq!(calls, 0, "no call is placed (BIZ-039)");
}

/// TD-CARD-09 (CARD-021, QH-004, QH-005, BIZ-037): at 23:00 only people with the switch on at an
/// after-hours business get it at once; every other recipient gets it at 08:00 as a whole.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_09_at_23_00_only_switched_on_people_at_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = after_hours(&mut db, 0xb0).await;
    let plain = business(&mut db, 0xc0).await[0].business;
    for b in [owner.business, plain] {
        queue_emergency_card(&mut db, (JOB, b), &NOTICE_WORDS, az("23:00:00"))
            .await
            .expect("queue");
    }
    let queued = tasks(&mut db, NOTICE).await;
    let eight: Timestamp = "2026-11-03T08:00:00-07:00".parse().expect("eight");
    let now: Vec<_> = queued
        .iter()
        .filter(|(_, t)| *t == az("23:00:00"))
        .collect();
    assert_eq!(now.len(), 1);
    assert_eq!(to(&now[0].0), [Uuid::from_u128(0xb0)]);
    let held: Vec<_> = queued.iter().filter(|(_, t)| *t == eight).collect();
    let mut held_to: Vec<Uuid> = held.iter().flat_map(|(p, _)| to(p)).collect();
    held_to.sort();
    let want = [0xb1, 0xc0, 0xc1].map(Uuid::from_u128);
    assert_eq!(held_to, want, "the rest at 08:00");
    assert!(held.iter().all(|(p, _)| p["emergency"] == true));
}

/// TD-CARD-09 (CARD-020): a non-emergency card keeps the normal subject (no EMERGENCY fields).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_09_a_regular_card_has_the_normal_subject(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = business(&mut db, 0xd0).await[0].business;
    queue_card_notice(&mut db, (JOB, b), &NOTICE_WORDS, (false, az("14:00:00")))
        .await
        .expect("queue");
    let queued = tasks(&mut db, NOTICE).await;
    assert!(queued[0].0.get("subject").is_none());
    assert!(queued[0].0.get("emergency").is_none());
}

/// TD-CARD-09, TD-BIZ-24 (BIZ-007, BIZ-042): the tick is refused until a person who can confirm
/// has the switch on and a Yes after the test email; a "Cards and quotes" member alone is not
/// enough; a Yes needs the test first.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_tick_needs_a_confirming_person_with_a_yes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [_, cards, billing] = business(&mut db, 0xe0).await;
    let (b, t) = (cards.business, (az("10:00:00"), EMERGENCY_YES_LIFE));
    let refused = set_after_hours(&mut db, b, true, t).await;
    assert_eq!(refused, Err(SwitchError::NoCover));
    covers(&mut db, cards, az("10:00:00")).await;
    let refused = set_after_hours(&mut db, b, true, t).await;
    assert_eq!(refused, Err(SwitchError::NoCover), "cards member alone");
    set_emergency_emails(&mut db, billing, true, az("10:00:00"))
        .await
        .expect("on");
    let early = record_alert_answer(&mut db, billing, true, az("10:00:00")).await;
    assert_eq!(early, Err(SwitchError::NoTest));
    send_emergency_test(&mut db, billing, az("10:01:00"))
        .await
        .expect("test");
    record_alert_answer(&mut db, billing, true, az("10:02:00"))
        .await
        .expect("yes");
    set_after_hours(&mut db, b, true, t).await.expect("tick");
    assert!(ticked(&mut db, b).await);
    assert_eq!(tasks(&mut db, EMERGENCY_TEST_KIND).await.len(), 2);
}

/// TD-CARD-09, TD-BIZ-24 (BIZ-007, INT-033): turning the last switch off warns, removes the tick,
/// emails the owner and drops the business from the night count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_last_switch_off_warns_unticks_and_emails_the_owner(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = after_hours(&mut db, 0xf0).await;
    assert_eq!(night(&mut db, az("23:00:00")).await, 1);
    assert!(switch_off_warns(&mut db, owner).await.expect("warns"));
    set_emergency_emails(&mut db, owner, false, az("23:00:00"))
        .await
        .expect("off");
    assert!(!ticked(&mut db, owner.business).await);
    assert_eq!(night(&mut db, az("23:00:01")).await, 0);
    let emails = tasks(&mut db, AFTER_HOURS_OFF_KIND).await;
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].0["member"], owner.member.to_string());
}

/// TD-BIZ-24 (BIZ-007): removing the last confirming person with the switch on removes the tick
/// and emails the owner; another person with the switch on keeps it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_removal_of_the_last_such_person_unticks(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, _, billing] = business(&mut db, 0x100).await;
    covers(&mut db, billing, az("10:00:00")).await;
    let b = owner.business;
    set_after_hours(&mut db, b, true, (az("10:00:00"), EMERGENCY_YES_LIFE))
        .await
        .expect("tick");
    assert!(
        !switch_off_warns(&mut db, owner)
            .await
            .expect("owner has none")
    );
    remove_member(&mut db, owner, billing.member, az("11:00:00"))
        .await
        .expect("removed");
    assert!(
        recheck_after_hours(&mut db, b, az("11:00:00"))
            .await
            .expect("recheck")
    );
    assert!(!ticked(&mut db, b).await);
    assert_eq!(tasks(&mut db, AFTER_HOURS_OFF_KIND).await.len(), 1);
}

/// TD-BIZ-24 (BIZ-042, BIZ-007, INT-033, D-362): a Yes on day 0 counts at day 89 23:59:59 and not
/// at day 90, when the re-test email goes and the tick waits; a new Yes restores the count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_a_yes_lasts_90_days_then_the_retest(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = after_hours(&mut db, 0x110).await;
    let yes = az("10:00:00");
    let last = yes + day(90) - SignedDuration::from_secs(1);
    let life = EMERGENCY_YES_LIFE;
    assert!(
        covered(&mut db, owner.business, (last, life))
            .await
            .expect("89")
    );
    assert_eq!(night(&mut db, last).await, 1);
    assert!(
        !covered(&mut db, owner.business, (yes + day(90), life))
            .await
            .expect("90")
    );
    assert_eq!(night(&mut db, yes + day(90)).await, 0);
    let n = queue_emergency_retests(&mut db, life, last)
        .await
        .expect("early");
    assert_eq!(n, 0);
    let n = queue_emergency_retests(&mut db, life, yes + day(90))
        .await
        .expect("due");
    assert_eq!(n, 1);
    assert_eq!(
        tasks(&mut db, EMERGENCY_TEST_KIND).await.len(),
        2,
        "re-test email"
    );
    assert!(ticked(&mut db, owner.business).await, "the tick waits");
    let state = switch_state(&mut db, owner, (yes + day(90), life)).await;
    assert!(!state.expect("state").yes_current);
    record_alert_answer(&mut db, owner, true, yes + day(91))
        .await
        .expect("new yes");
    assert_eq!(night(&mut db, yes + day(91)).await, 1);
}

/// TD-BIZ-24 (BIZ-007, D-362): two night EMERGENCY cards in a row with no own-link visit within
/// 60:00 suspend the tick, email the owner and leave the count; a visit at 59:59 on either card
/// keeps it; a new Yes restores it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_two_unvisited_night_cards_suspend_until_a_new_yes(pool: PgPool) {
    let edge = VISIT_WITHIN - SignedDuration::from_secs(1);
    assert!(missed_in_a_row(&[None, None], 2));
    assert!(missed_in_a_row(&[Some(edge), None, Some(VISIT_WITHIN)], 2));
    assert!(!missed_in_a_row(&[Some(edge), None], 2));
    assert!(!missed_in_a_row(&[None, Some(edge)], 2));
    assert!(!missed_in_a_row(&[None], 2));
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = after_hours(&mut db, 0x120).await;
    let b = owner.business;
    assert!(
        suspend_after_hours(&mut db, b, az("23:30:00"))
            .await
            .expect("suspend")
    );
    assert!(
        !suspend_after_hours(&mut db, b, az("23:31:00"))
            .await
            .expect("once")
    );
    assert_eq!(night(&mut db, az("23:30:00")).await, 0);
    assert_eq!(tasks(&mut db, AFTER_HOURS_OFF_KIND).await.len(), 1);
    send_emergency_test(&mut db, owner, az("23:40:00"))
        .await
        .expect("re-test");
    record_alert_answer(&mut db, owner, true, az("23:41:00"))
        .await
        .expect("yes");
    assert_eq!(night(&mut db, az("23:42:00")).await, 1);
}

/// TD-BIZ-24 (INT-033, D-346): a business ticked after-hours with no person who can confirm with
/// emergency emails on is not in the night count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_a_tick_with_no_switched_on_person_is_not_counted(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let b = business(&mut db, 0x130).await[0].business;
    sqlx::query("UPDATE businesses.business SET after_hours = true WHERE id = $1")
        .bind(b)
        .execute(&mut *db)
        .await
        .expect("tick by hand");
    assert_eq!(night(&mut db, az("23:00:00")).await, 0);
}

/// TD-BIZ-24 (BIZ-042, D-362): the screen's line, button and question; a No shows the iPhone
/// (Settings, Focus) and Android (Do Not Disturb) steps with their help pages; the test email's
/// subject; the warning before the last switch goes off.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_24_the_test_emergency_email_screen(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = business(&mut db, 0x140).await;
    let t = az("21:00:00");
    let on = post_step(&mut db, owner, (SwitchStep::Switch(true), "tok"), t).await;
    let on = on.expect("on");
    assert!(on.contains(DND_LINE) && on.contains(TEST_BUTTON));
    assert!(!on.contains(ALERT_QUESTION), "asked after the test");
    let sent = post_step(&mut db, owner, (SwitchStep::SendTest, "tok"), t).await;
    assert!(sent.expect("sent").contains(ALERT_QUESTION));
    let no = post_step(&mut db, owner, (SwitchStep::Answer(false), "tok"), t).await;
    let no = no.expect("no");
    assert!(no.contains("iPhone: In Settings, open Focus."));
    assert!(no.contains("Android: In your phone"));
    assert!(no.contains("Settings, open Do Not Disturb and add Gmail"));
    assert!(no.contains("https://support.apple.com/en-us/108274"));
    let yes = post_step(&mut db, owner, (SwitchStep::Answer(true), "tok"), t).await;
    assert!(!yes.expect("yes").contains("open Focus"));
    set_after_hours(&mut db, owner.business, true, (t, EMERGENCY_YES_LIFE))
        .await
        .expect("tick");
    let page = emergency_switch_page(
        switch_state(&mut db, owner, (t, EMERGENCY_YES_LIFE))
            .await
            .expect("state"),
        (true, false),
        "tok",
    );
    assert!(
        page.expect("page")
            .contains("email the owner to let them know")
    );
    assert!(OFF_WARNING.contains("stops getting emergency jobs at night"));
    let [subject, _] = emergency_test_words(false);
    assert_eq!(
        subject,
        "Ask Hava: EMERGENCY sample job card (not a real job)"
    );
}

/// TD-CARD-09 (PICK-044, PICK-037, BIZ-037, D-346): an emergency pick at 23:00 of an after-hours
/// business goes at once only to its switched-on confirming people, marked EMERGENCY, with one
/// reminder at 23:30; the homeowner reads that her request goes by email marked EMERGENCY.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_09_emergency_pick_at_23_00(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = after_hours(&mut db, 0x150).await;
    let day_people = [0x150, 0x152].map(Uuid::from_u128);
    let rules = WindowRules::default();
    let pick = emergency_pick_plan(
        &mut db,
        owner.business,
        (&day_people, az("23:00:00")),
        rules,
    )
    .await
    .expect("plan");
    assert_eq!(pick.to, [Uuid::from_u128(0x150)]);
    assert_eq!(pick.plan.send_at, az("23:00:00"));
    assert_eq!(pick.plan.reminder_at, Some(az("23:30:00")));
    assert_eq!(
        emergency_pick_subject(1042, "11 p.m. Arizona time"),
        "Ask Hava: EMERGENCY: a homeowner picked your quote for job 1042. Accept by 11 p.m. Arizona time."
    );
    let words = include_str!("../../../app/messaging/templates/en.json");
    assert!(words.contains("businesses get your request by email marked EMERGENCY"));
    let day = emergency_pick_plan(
        &mut db,
        owner.business,
        (&day_people, az("14:00:00")),
        rules,
    );
    assert_eq!(day.await.expect("day").to, day_people);
}

/// TD-INT-31 (INT-029, INT-030, D-492): "AC died, 92 inside" at 22:40 with emergency chosen and the
/// ZIP in: the card goes at once to the HVAC business that takes after-hours requests.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_31_after_hours_cards_go_as_soon_as_the_zip_is_in(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, ..] = after_hours(&mut db, 0x160).await;
    let at = az("22:40:00");
    queue_emergency_card(&mut db, (JOB, owner.business), &NOTICE_WORDS, at)
        .await
        .expect("queue");
    let queued = tasks(&mut db, NOTICE).await;
    let now: Vec<_> = queued.iter().filter(|(_, t)| *t == at).collect();
    assert_eq!(now.len(), 1);
    assert_eq!(to(&now[0].0), [Uuid::from_u128(0x160)]);
}
