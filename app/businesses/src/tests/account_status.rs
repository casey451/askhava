//! Account statuses, ticks, towns and the job-type checklist (TD-BIZ-02: BIZ-004, BIZ-005,
//! BIZ-006, BIZ-008, BIZ-009; the after-hours tick's column and `night_count`, BIZ-007, D-491).
//!
//! The pure cases walk every status against every event; the database cases run on a fresh
//! database with fake IDs only. The pause rules, own quiet times and the weekly-limit case of
//! TD-BIZ-02 are T182's; the after-hours conditions are TD-BIZ-24's (T150).

use std::collections::BTreeSet;

use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::types::Uuid;

use crate::api::{
    AccountStatus, BusinessesError, Checks, Event, Insurance, JobTypeChoice, PauseReason, Ticks,
    apply, business, create, job_types, night_count, set_job_types, set_ticks, set_towns, towns,
};
use crate::rules::{checklist, towns as town_list};
use crate::status::next;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const ALL_DONE: Checks = Checks {
    code_done: true,
    match_passed: true,
    flagged: false,
};
const NO_CODE: Checks = Checks {
    code_done: false,
    match_passed: true,
    flagged: false,
};
const NO_MATCH: Checks = Checks {
    code_done: true,
    match_passed: false,
    flagged: false,
};
const FLAGGED: Checks = Checks {
    code_done: true,
    match_passed: true,
    flagged: true,
};

fn events() -> Vec<Event> {
    let mut out = vec![Event::CategoryOpened, Event::Resume, Event::Remove];
    for c in [ALL_DONE, NO_CODE, NO_MATCH, FLAGGED] {
        out.extend([Event::Checked(c), Event::OwnerApproved(c)]);
    }
    out.extend(PauseReason::ALL.map(Event::Pause));
    out
}

/// The oracle: the one status each allowed (from, event) reaches, written from BIZ-009, BIZ-003,
/// BIZ-016 and BIZ-021 without the code under test.
fn oracle(from: &str, event: Event) -> Option<&'static str> {
    let live = ["pending", "active", "paused", "waitlist", "needs_review"];
    match event {
        Event::Checked(c) if from == "pending" && c.flagged => Some("needs_review"),
        Event::Checked(c) if from == "pending" && c.code_done && c.match_passed => Some("active"),
        Event::OwnerApproved(c) if from == "needs_review" && c.code_done && c.match_passed => {
            Some("active")
        }
        Event::CategoryOpened if from == "waitlist" => Some("pending"),
        Event::Pause(_) if from == "active" => Some("paused"),
        Event::Resume if from == "paused" => Some("active"),
        Event::Remove if live.contains(&from) => Some("removed"),
        _ => None,
    }
}

/// TD-BIZ-02 (BIZ-009, BIZ-003, BIZ-016): each of the six statuses is reachable only by its rules;
/// every other move is refused, and removed is final.
#[test]
fn td_biz_02_each_status_reachable_only_by_its_rules() {
    let codes: Vec<&str> = AccountStatus::ALL.iter().map(|s| s.code()).collect();
    assert_eq!(
        codes,
        [
            "pending",
            "active",
            "paused",
            "waitlist",
            "needs_review",
            "removed"
        ]
    );
    for from in AccountStatus::ALL {
        for event in events() {
            let got = next(from, event).ok().map(AccountStatus::code);
            assert_eq!(got, oracle(from.code(), event), "{} {event:?}", from.code());
        }
    }
}

/// TD-BIZ-02 (BIZ-003): Casey's approval never activates without the code and the match.
#[test]
fn td_biz_02_owner_approval_alone_never_activates() {
    let needs_review = AccountStatus::from_code("needs_review").expect("status");
    for c in [NO_CODE, NO_MATCH] {
        assert!(next(needs_review, Event::OwnerApproved(c)).is_err());
    }
    let pending = AccountStatus::from_code("pending").expect("status");
    assert!(next(pending, Event::OwnerApproved(ALL_DONE)).is_err());
}

fn choice(category: &str, job_type: &str) -> JobTypeChoice {
    JobTypeChoice::new(category, job_type)
}

/// TD-BIZ-02 (BIZ-004, LIC-043): the checklist holds only job types within the license; repeats
/// collapse; a malformed code is refused; an empty list is allowed.
#[test]
fn td_biz_02_checklist_stays_within_the_license() {
    let allowed: BTreeSet<_> = [
        choice("plumbing_repair", "faucet repair"),
        choice("plumbing_repair", "toilet repair"),
    ]
    .into();
    let ticked = [
        choice("plumbing_repair", "toilet repair"),
        choice("plumbing_repair", "toilet repair"),
    ];
    assert_eq!(
        checklist(&ticked, &allowed),
        Ok(vec![choice("plumbing_repair", "toilet repair")])
    );
    let outside = [choice("electrical", "new outlet")];
    assert_eq!(
        checklist(&outside, &allowed),
        Err(BusinessesError::NotCovered)
    );
    for bad in [
        choice("Plumbing", "faucet repair"),
        choice("plumbing_repair", " "),
    ] {
        assert_eq!(checklist(&[bad], &allowed), Err(BusinessesError::BadCode));
    }
    assert_eq!(checklist(&[], &allowed), Ok(vec![]));
    assert_eq!(
        town_list(&["oatman", "lake_havasu_city", "oatman"]),
        Ok(vec!["lake_havasu_city".to_owned(), "oatman".to_owned()])
    );
    assert_eq!(town_list(&["Lake Havasu"]), Err(BusinessesError::BadCode));
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

async fn active_business(db: &mut Db, after_hours: bool, town: &str, category: &str) -> Uuid {
    let id = create(db, false, at(0)).await.expect("create");
    let actor = Actor::Business(id);
    let to = apply(db, id, Event::Checked(ALL_DONE), Actor::System, at(1))
        .await
        .expect("check");
    assert_eq!(to.code(), "active");
    let ticks = Ticks {
        overflow_on: false,
        speaks_spanish: false,
        insurance: None,
        after_hours,
    };
    set_ticks(db, id, ticks, actor).await.expect("ticks");
    set_towns(db, id, &[town], actor).await.expect("towns");
    let allowed: BTreeSet<_> = [choice(category, "no heat")].into();
    set_job_types(db, id, &[choice(category, "no heat")], &allowed, actor)
        .await
        .expect("jobs");
    id
}

/// TD-BIZ-02 (BIZ-005, BIZ-006, BIZ-008, BIZ-009): the ticks and the insurance answer are stored
/// as given; insurance No never pauses (D-061); a pause keeps its reason and a resume clears it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_02_ticks_insurance_and_pause_are_stored(mut db: Db) {
    let id = active_business(&mut db, false, "lake_havasu_city", "hvac_repair").await;
    let ticks = Ticks {
        overflow_on: true,
        speaks_spanish: true,
        insurance: Some(Insurance::No),
        after_hours: false,
    };
    set_ticks(&mut db, id, ticks, Actor::Business(id))
        .await
        .expect("ticks");
    let b = business(&mut db, id).await.expect("read");
    assert!(b.overflow_on && b.speaks_spanish && !b.after_hours);
    assert_eq!(
        (b.insurance, b.status.code()),
        (Some(Insurance::No), "active")
    );
    assert_eq!(
        towns(&mut db, id).await.expect("towns"),
        ["lake_havasu_city"]
    );
    assert_eq!(
        job_types(&mut db, id).await.expect("jobs"),
        [choice("hvac_repair", "no heat")]
    );
    let reason = PauseReason::SelfPaused;
    apply(
        &mut db,
        id,
        Event::Pause(reason),
        Actor::Business(id),
        at(5),
    )
    .await
    .expect("pause");
    let b = business(&mut db, id).await.expect("read");
    assert_eq!(
        (b.status.code(), b.pause_reason.map(|r| r.code())),
        ("paused", Some("self"))
    );
    let again = apply(
        &mut db,
        id,
        Event::Pause(PauseReason::Owner),
        Actor::System,
        at(6),
    )
    .await;
    assert_eq!(again, Err(BusinessesError::Refused));
    apply(&mut db, id, Event::Resume, Actor::Business(id), at(7))
        .await
        .expect("resume");
    let b = business(&mut db, id).await.expect("read");
    assert_eq!((b.status.code(), b.pause_reason), ("active", None));
    let outside = set_job_types(
        &mut db,
        id,
        &[choice("electrical", "new outlet")],
        &BTreeSet::new(),
        Actor::Business(id),
    )
    .await;
    assert_eq!(outside, Err(BusinessesError::NotCovered));
}

/// TD-BIZ-02 (BIZ-009, BIZ-003, BIZ-021): waitlist opens to pending; a flagged sign-up waits in
/// needs review until approval with the code and the match; removed is final.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_02_waitlist_review_and_removed(mut db: Db) {
    let id = create(&mut db, true, at(0)).await.expect("create");
    assert_eq!(
        business(&mut db, id).await.expect("read").status.code(),
        "waitlist"
    );
    let owner = Actor::Owner(Uuid::from_u128(0xCA));
    assert_eq!(
        apply(&mut db, id, Event::Checked(ALL_DONE), owner, at(1)).await,
        Err(BusinessesError::Refused)
    );
    apply(&mut db, id, Event::CategoryOpened, Actor::System, at(2))
        .await
        .expect("open");
    let to = apply(&mut db, id, Event::Checked(FLAGGED), Actor::System, at(3))
        .await
        .expect("flag");
    assert_eq!(to.code(), "needs_review");
    assert_eq!(
        apply(&mut db, id, Event::OwnerApproved(NO_CODE), owner, at(4)).await,
        Err(BusinessesError::Refused)
    );
    let to = apply(&mut db, id, Event::OwnerApproved(ALL_DONE), owner, at(5))
        .await
        .expect("ok");
    assert_eq!(to.code(), "active");
    apply(&mut db, id, Event::Remove, owner, at(6))
        .await
        .expect("remove");
    assert_eq!(
        apply(&mut db, id, Event::Resume, owner, at(7)).await,
        Err(BusinessesError::Refused)
    );
    let missing = apply(&mut db, Uuid::from_u128(0xBAD), Event::Remove, owner, at(8)).await;
    assert_eq!(missing, Err(BusinessesError::UnknownBusiness));
}

/// BIZ-007, D-491: `night_count` counts the active businesses with the after-hours tick in the
/// trade and town; untick, other town, other trade and paused businesses do not count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn night_count_counts_active_after_hours_businesses(mut db: Db) {
    active_business(&mut db, true, "lake_havasu_city", "hvac_repair").await;
    active_business(&mut db, false, "lake_havasu_city", "hvac_repair").await;
    active_business(&mut db, true, "parker", "hvac_repair").await;
    active_business(&mut db, true, "lake_havasu_city", "plumbing_repair").await;
    let paused = active_business(&mut db, true, "lake_havasu_city", "hvac_repair").await;
    let n = night_count(&mut db, "hvac_repair", "lake_havasu_city")
        .await
        .expect("count");
    assert_eq!(n, 2);
    apply(
        &mut db,
        paused,
        Event::Pause(PauseReason::License),
        Actor::System,
        at(9),
    )
    .await
    .expect("pause");
    let n = night_count(&mut db, "hvac_repair", "lake_havasu_city")
        .await
        .expect("count");
    assert_eq!(n, 1);
}

/// BIZ-009: the app role cannot delete a business row; removal is a status (postgres brain P5).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_app_role_cannot_delete_a_business(mut db: Db) {
    create(&mut db, false, at(0)).await.expect("create");
    sqlx::query!("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let deleted = sqlx::query!("DELETE FROM businesses.business")
        .execute(&mut *db)
        .await;
    assert!(deleted.is_err());
}
