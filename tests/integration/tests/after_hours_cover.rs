//! TD-BIZ-47 (T369; BIZ-007, INT-033; D-2702, D-2711, D-3106): the sign-up after-hours answer
//! ticks only while someone covers, and a suspension lasts until a new Yes. A capability save
//! with after-hours yes and nobody covering leaves the tick false and says it was held back; with
//! an owner who has the switch on, a sent test and a current Yes it is ticked; suspend, untick,
//! re-tick leaves the business suspended and out of `covered_night_count` until a new Yes, and the
//! switch page then shows it as suspended, never as taking jobs (D-3239); the emergency-switch
//! page's after-hours step ticks once the owner's Yes is in and is refused before it, listing the
//! steps still missing. With `EMERGENCY_SWITCH_HTML_DIR` set,
//! `td_biz_47_write_pages_for_axe` writes the page with the box for
//! `tests/e2e/emergency_switch_after_hours.spec.ts`. Every ID and mailbox is fake.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Acting, Capability, CapabilityAnswers, Checks, EMERGENCY_YES_LIFE, Event, JobTypeChoice,
    LargestJob, OfferRow, Roles, SwitchError, SwitchState, TradeOffer, add_member, add_owner,
    apply, covered_night_count, create, record_alert_answer, save_capability, send_emergency_test,
    set_after_hours, set_emergency_emails, set_job_types, set_towns, suspend_after_hours,
};
use askhava_ops::api::Actor;
use askhava_portals::api::{
    AfterHoursBox, SUSPENDED_LINE, SwitchStep, after_hours_line, emergency_switch_page_with_box,
    held_back_link, post_step,
};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const JOB_TYPE: (&str, &str) = ("hvac_repair", "no cooling");

fn az(time: &str) -> Timestamp {
    format!("2026-11-02T{time}-07:00").parse().expect("time")
}

fn offer() -> Vec<TradeOffer> {
    vec![TradeOffer {
        category: JOB_TYPE.0.to_owned(),
        name: "HVAC".to_owned(),
        rows: vec![OfferRow {
            choice: JobTypeChoice::new(JOB_TYPE.0, JOB_TYPE.1),
            licensed: true,
            recurring: false,
        }],
    }]
}

fn answers(after_hours: bool) -> CapabilityAnswers {
    CapabilityAnswers {
        ticked: vec![JobTypeChoice::new(JOB_TYPE.0, JOB_TYPE.1)],
        towns: vec![TOWN.to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours,
    }
}

/// An active business with an owner (mailbox `seed`) and a "Cards and quotes" member (`seed + 1`).
async fn business(db: &mut PgConnection, seed: u128) -> [Acting; 2] {
    let t = az("09:00:00");
    let b = create(db, false, t).await.expect("business");
    let owner = Acting {
        business: b,
        member: add_owner(db, b, Uuid::from_u128(seed), t)
            .await
            .expect("owner"),
    };
    let roles = Roles::new(true, false).expect("roles");
    let cards = add_member(db, owner, Uuid::from_u128(seed + 1), roles, t).await;
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    [
        owner,
        Acting {
            business: b,
            member: cards.expect("member"),
        },
    ]
}

async fn serve_hvac(db: &mut PgConnection, b: Uuid) {
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let job = JobTypeChoice::new(JOB_TYPE.0, JOB_TYPE.1);
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

async fn ticked(db: &mut PgConnection, b: Uuid) -> bool {
    sqlx::query_scalar("SELECT after_hours FROM businesses.business WHERE id = $1")
        .bind(b)
        .fetch_one(db)
        .await
        .expect("tick")
}

async fn night(db: &mut PgConnection, at: Timestamp) -> u32 {
    covered_night_count(db, (JOB_TYPE.0, TOWN), (at, EMERGENCY_YES_LIFE))
        .await
        .expect("count")
}

/// TD-BIZ-47 (BIZ-007, INT-033): a capability save with after-hours yes and nobody covering
/// leaves the tick false, says the answer was held back and links to the switch page.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_47_signup_yes_with_nobody_covering_is_held_back(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, _] = business(&mut db, 0x4700).await;
    let b = owner.business;
    let saved = save_capability(
        &mut db,
        b,
        &offer(),
        &answers(true),
        (Actor::System, az("10:00:00")),
    )
    .await
    .expect("saved");
    assert!(saved.after_hours_held);
    assert!(!ticked(&mut db, b).await);
    assert_eq!(night(&mut db, az("23:00:00")).await, 0);
    assert_eq!(
        held_back_link(saved.after_hours_held),
        Some(("/b/emergency-emails", "Emergency emails at any hour"))
    );
    let no = save_capability(
        &mut db,
        b,
        &offer(),
        &answers(false),
        (Actor::System, az("10:01:00")),
    )
    .await;
    assert!(!no.expect("saved").after_hours_held);
    assert_eq!(held_back_link(false), None);
}

/// TD-BIZ-47 (BIZ-007): with an owner who has the switch on, a sent test and a current Yes, the
/// sign-up's after-hours yes is ticked and the business counts at night.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_47_signup_yes_with_a_covering_owner_ticks(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, _] = business(&mut db, 0x4710).await;
    let b = owner.business;
    covers(&mut db, owner, az("10:00:00")).await;
    let saved = save_capability(
        &mut db,
        b,
        &offer(),
        &answers(true),
        (Actor::System, az("10:05:00")),
    )
    .await
    .expect("saved");
    assert!(!saved.after_hours_held);
    assert!(ticked(&mut db, b).await);
    assert_eq!(night(&mut db, az("23:00:00")).await, 1);
    let no = save_capability(
        &mut db,
        b,
        &offer(),
        &answers(false),
        (Actor::System, az("10:10:00")),
    )
    .await;
    assert!(!no.expect("saved").after_hours_held);
    assert!(!ticked(&mut db, b).await, "a later no unticks");
}

/// TD-BIZ-47 (BIZ-007, INT-033, D-2008): suspend, untick, re-tick leaves the business suspended
/// and out of the night count until a new Yes, after which it counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_47_untick_and_retick_keeps_a_suspension_until_a_new_yes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, _] = business(&mut db, 0x4720).await;
    let b = owner.business;
    serve_hvac(&mut db, b).await;
    covers(&mut db, owner, az("10:00:00")).await;
    let t = (az("10:00:00"), EMERGENCY_YES_LIFE);
    set_after_hours(&mut db, b, true, t).await.expect("tick");
    assert_eq!(night(&mut db, az("23:00:00")).await, 1);
    assert!(
        suspend_after_hours(&mut db, b, az("23:30:00"))
            .await
            .expect("suspend")
    );
    set_after_hours(&mut db, b, false, t).await.expect("untick");
    set_after_hours(&mut db, b, true, t).await.expect("re-tick");
    assert!(ticked(&mut db, b).await);
    assert_eq!(night(&mut db, az("23:40:00")).await, 0, "still suspended");
    record_alert_answer(&mut db, owner, true, az("23:45:00"))
        .await
        .expect("new yes");
    assert_eq!(night(&mut db, az("23:50:00")).await, 1);
}

/// TD-BIZ-47 (BIZ-007): the switch page's after-hours step is refused before the owner's Yes,
/// listing the steps still missing, and ticks once it is in; a "Cards and quotes" member gets no
/// box and is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_47_switch_page_ticks_only_after_the_owners_yes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, cards] = business(&mut db, 0x4730).await;
    let (b, t) = (owner.business, az("10:00:00"));
    let first = post_step(&mut db, owner, (SwitchStep::Switch(true), "tok"), t).await;
    assert!(first.expect("on").contains(&after_hours_line()));
    let tick = (SwitchStep::AfterHours(true), "tok");
    let refused = post_step(&mut db, owner, tick, t).await.expect("page");
    assert!(refused.contains(r#"id="after-hours-refused""#));
    assert!(refused.contains("Send a test emergency email."));
    assert!(!refused.contains("Turn on emergency emails at any hour."));
    assert!(!ticked(&mut db, b).await);
    post_step(&mut db, owner, (SwitchStep::SendTest, "tok"), t)
        .await
        .expect("test");
    post_step(&mut db, owner, (SwitchStep::Answer(true), "tok"), t)
        .await
        .expect("yes");
    let on = post_step(&mut db, owner, tick, t).await.expect("page");
    assert!(!on.contains(r#"id="after-hours-refused""#));
    assert!(on.contains(r#"value="after_hours_off""#));
    assert!(ticked(&mut db, b).await);
    let other = post_step(&mut db, cards, (SwitchStep::Switch(true), "tok"), t).await;
    assert!(!other.expect("page").contains(r#"id="after-hours-title""#));
    let denied = post_step(&mut db, cards, (SwitchStep::AfterHours(false), "tok"), t).await;
    assert_eq!(denied, Err(SwitchError::NotAllowed));
    assert!(ticked(&mut db, b).await);
    let off = post_step(&mut db, owner, (SwitchStep::AfterHours(false), "tok"), t).await;
    assert!(off.expect("page").contains(r#"value="after_hours_on""#));
    assert!(!ticked(&mut db, b).await);
    assert_eq!(
        SwitchStep::from_form("after_hours_on"),
        Some(SwitchStep::AfterHours(true))
    );
    assert_eq!(
        SwitchStep::from_form("after_hours_off"),
        Some(SwitchStep::AfterHours(false))
    );
}

/// TD-BIZ-47 (BIZ-007, INT-033, D-3239): a suspended business that is still ticked sees the
/// suspended line on the switch page, never that it is taking emergency jobs at night; a new Yes
/// ends the suspension and the page says it is taking them again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_47_switch_page_says_a_suspended_business_is_not_taking_jobs(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let [owner, _] = business(&mut db, 0x4740).await;
    let (b, t) = (owner.business, az("10:00:00"));
    covers(&mut db, owner, t).await;
    let tick = (SwitchStep::AfterHours(true), "tok");
    let on = post_step(&mut db, owner, tick, t).await.expect("page");
    assert!(on.contains(r#"id="after-hours-on""#));
    assert!(!on.contains(SUSPENDED_LINE));
    assert!(
        suspend_after_hours(&mut db, b, az("23:30:00"))
            .await
            .expect("suspend")
    );
    let page = post_step(&mut db, owner, (SwitchStep::Switch(true), "tok"), t).await;
    let page = page.expect("page");
    assert!(ticked(&mut db, b).await);
    assert!(page.contains(r#"id="after-hours-suspended""#));
    assert!(page.contains(SUSPENDED_LINE));
    assert!(!page.contains(r#"id="after-hours-on""#));
    assert!(!page.contains("You're taking emergency jobs"));
    let yes = (SwitchStep::Answer(true), "tok");
    let again = post_step(&mut db, owner, yes, az("23:45:00")).await;
    let again = again.expect("page");
    assert!(again.contains(r#"id="after-hours-on""#));
    assert!(!again.contains(SUSPENDED_LINE));
}

/// The e2e axe scan's pages: written only when `EMERGENCY_SWITCH_HTML_DIR` is set.
#[test]
fn td_biz_47_write_pages_for_axe() {
    let Some(dir) = std::env::var_os("EMERGENCY_SWITCH_HTML_DIR") else {
        return;
    };
    let fresh = SwitchState {
        on: true,
        test_sent: false,
        yes_current: false,
    };
    let done = SwitchState {
        on: true,
        test_sent: true,
        yes_current: true,
    };
    let pages = [
        ("unticked", done, AfterHoursBox::default()),
        (
            "ticked",
            done,
            AfterHoursBox {
                ticked: true,
                refused: false,
                suspended: false,
            },
        ),
        (
            "suspended",
            done,
            AfterHoursBox {
                ticked: true,
                refused: false,
                suspended: true,
            },
        ),
        (
            "refused",
            fresh,
            AfterHoursBox {
                ticked: false,
                refused: true,
                suspended: false,
            },
        ),
    ];
    for (name, state, after_hours) in pages {
        let html =
            emergency_switch_page_with_box(state, (false, false), Some(after_hours), "fake-csrf")
                .expect("page renders");
        let path = std::path::Path::new(&dir).join(format!("{name}.html"));
        std::fs::write(path, html).expect("page written");
    }
}
