//! Late cards when a business becomes eligible, in the database (TD-CARD-17: CARD-018, CARD-024,
//! CARD-020; T677, D-4360, D-5075). Fake IDs, names and words only; the towns are public place
//! names; the clock is set.
//!
//! A house-cleaning job in Lake Havasu City is Open with its card sent. A business that becomes
//! eligible (activation, its own unpause, a rule unpause, a newly ticked job type) has one
//! `businesses.became_eligible` task queued; `quotes::api::late_cards` then sends it the card once,
//! sent at the set time, with its card notice due then (its 24 hours from the send). A job in
//! Quotes ready or a final status, a business already carded and a business outside the job's
//! reach get nothing, and a rerun sends nothing twice.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    BECAME_ELIGIBLE_KIND, Checks, Event, JobTypeChoice, PauseReason, add_owner, apply, create,
    pause_for_rule, pause_self, request_welcome, resume_self, save_business_name, set_job_types,
    set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Claimed, Guarded};
use askhava_integration::set_app_role;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, Place, build_card, card_digest, keep_card, late_cards,
    mark_card_sent, run_became_eligible,
};
use jiff::Timestamp;
use jiff::civil::date;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const OTHER_TOWN: &str = "kingman";
const CLEANING: &str = "house_cleaning";
const REGULAR: &str = "regular cleaning";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-9001,Sample Shine One LLC,,,2019-03-04\nCL-9002,Sample Shine Two LLC,,,2019-03-04\n";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

/// The set clock: the job opens at 9:00, the eligibility event comes at 10:00 Arizona time.
const OPENED: &str = "2026-11-02T16:00:00Z";
const NOW: &str = "2026-11-02T17:00:00Z";

/// The catalog and the fake city list, then the app role, as production runs.
async fn setup(pool: &PgPool) -> PgConnection {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    import_city_list(&mut db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    db
}

/// Moves `job` along `arrow` at [`OPENED`]; the move applies.
async fn step(db: &mut PgConnection, job: Uuid, arrow: (JobStatus, JobStatus)) {
    let moved = move_to(db, job, arrow, ts(OPENED), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))));
}

/// A house-cleaning job, moved from Intake to Open, with its card stored and sent at [`OPENED`].
async fn open_job(db: &mut PgConnection) -> Uuid {
    let at = ts(OPENED);
    let job = askhava_jobs::api::create(db, at, false, Actor::System)
        .await
        .expect("job");
    step(db, job, (JobStatus::Intake, JobStatus::Open)).await;
    let parts = CardParts {
        category: "House cleaning".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "Whole house cleaning every other week, three bedrooms".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let facts = CardFacts {
        category: CLEANING.to_owned(),
        candidates: vec![REGULAR.to_owned()],
        emergency: false,
    };
    let digest = card_digest(card.text());
    keep_card(db, (Uuid::new_v4(), job), &digest, &card, &facts, at)
        .await
        .expect("kept");
    assert!(mark_card_sent(db, job, &digest, at).await.expect("sent"));
    job
}

/// A business named on the fake city list (`n` 1 or 2), serving `town`, still Pending;
/// `ticked` ticks regular cleaning.
async fn pending(db: &mut PgConnection, n: usize, town: &str, ticked: bool) -> Uuid {
    let t = ts("2026-11-01T17:00:00Z");
    let b = create(db, false, t).await.expect("business");
    add_owner(db, b, Uuid::new_v4(), t).await.expect("owner");
    let name = format!("Sample Shine {} LLC", ["One", "Two"][n - 1]);
    save_business_name(db, b, (&name, None), None)
        .await
        .expect("name");
    set_towns(db, b, &[town], Actor::Business(b))
        .await
        .expect("towns");
    if ticked {
        tick(db, b).await;
    }
    b
}

async fn tick(db: &mut PgConnection, b: Uuid) {
    let choice = JobTypeChoice::new(CLEANING, REGULAR);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, Actor::Business(b))
        .await
        .expect("job types");
}

/// Activates `b` at `at` and sends its welcome email then (CON-020).
async fn activate(db: &mut PgConnection, b: Uuid, at: Timestamp) {
    checked(db, b, at).await;
    welcome(db, b, at).await;
}

async fn welcome(db: &mut PgConnection, b: Uuid, at: Timestamp) {
    request_welcome(db, b, at).await.expect("welcome");
    welcome_sent(db, b, at).await.expect("sent");
}

async fn checked(db: &mut PgConnection, b: Uuid, at: Timestamp) {
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, at)
        .await
        .expect("active");
}

/// Deletes every queued `businesses.became_eligible` task, so a case counts only its own event.
async fn clear_tasks(db: &mut PgConnection) {
    sqlx::query("DELETE FROM core.task WHERE kind = $1")
        .bind(BECAME_ELIGIBLE_KIND)
        .execute(db)
        .await
        .expect("clear");
}

/// The `businesses.became_eligible` tasks queued for `b`.
async fn eligible_tasks(db: &mut PgConnection, b: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND payload = jsonb_build_object('business', $2::text)",
    )
    .bind(BECAME_ELIGIBLE_KIND)
    .bind(b)
    .fetch_one(db)
    .await
    .expect("tasks")
}

/// (`b`'s cards of `job` sent at [`NOW`], `b`'s card notices of `job` due at [`NOW`], all of
/// `b`'s cards and notices of `job`).
async fn got(db: &mut PgConnection, job: Uuid, b: Uuid) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM quotes.job_card WHERE job_id = $1 AND business_id = $2 \
           AND sent_at = $3), \
         (SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice' \
           AND payload->>'job' = $1::text AND payload->>'business' = $2::text AND run_at = $3), \
         (SELECT count(*) FROM quotes.job_card WHERE job_id = $1 AND business_id = $2) + \
         (SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice' \
           AND payload->>'job' = $1::text AND payload->>'business' = $2::text)",
    )
    .bind(job)
    .bind(b)
    .bind(jiff_sqlx::Timestamp::from(ts(NOW)))
    .fetch_one(db)
    .await
    .expect("counts")
}

/// Runs the late cards for `b` at [`NOW`] twice; the card and its notice went once.
async fn sends_once(db: &mut PgConnection, job: Uuid, b: Uuid) {
    let first = late_cards(db, b, ts(NOW)).await.expect("late cards");
    assert_eq!(first, vec![job]);
    assert_eq!(
        got(db, job, b).await,
        (1, 1, 2),
        "card and notice at the set time"
    );
    let again = late_cards(db, b, ts(NOW)).await.expect("rerun");
    assert!(again.is_empty(), "a rerun sends nothing");
    assert_eq!(got(db, job, b).await, (1, 1, 2), "nothing twice");
}

/// TD-CARD-17 (CARD-018, CARD-020): activation sends the Open job's card once with its notice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_17_activation_sends_the_card_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    let job = open_job(&mut db).await;
    let b = pending(&mut db, 1, TOWN, true).await;
    assert_eq!(
        eligible_tasks(&mut db, b).await,
        0,
        "pending is not eligible"
    );
    let at = ts("2026-11-02T16:59:00Z");
    checked(&mut db, b, at).await;
    assert_eq!(eligible_tasks(&mut db, b).await, 1, "one eligibility task");
    let held = late_cards(&mut db, b, at).await;
    assert_eq!(held, Ok(Vec::new()), "no card before the welcome email");
    assert_eq!(eligible_tasks(&mut db, b).await, 2, "looked at again later");
    welcome(&mut db, b, ts(NOW)).await;
    sends_once(&mut db, job, b).await;
}

/// TD-CARD-17 (CARD-018): the business's own unpause sends the card, through the handler too.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_17_a_self_unpause_sends_the_card_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    let b = pending(&mut db, 1, TOWN, true).await;
    activate(&mut db, b, ts("2026-11-01T18:00:00Z")).await;
    pause_self(&mut db, b, ts("2026-11-01T19:00:00Z"))
        .await
        .expect("paused");
    let job = open_job(&mut db).await;
    assert_eq!(got(&mut db, job, b).await.2, 0, "paused, nothing");
    clear_tasks(&mut db).await;
    resume_self(&mut db, b, ts(NOW)).await.expect("resumed");
    assert_eq!(eligible_tasks(&mut db, b).await, 1, "one eligibility task");
    let task = Claimed {
        id: 677,
        kind: BECAME_ELIGIBLE_KIND.to_owned(),
        payload: serde_json::json!({ "business": b }),
        attempts: 1,
    };
    let task_again = task.clone();
    run_became_eligible(pool.clone(), task, ts(NOW))
        .await
        .expect("handler");
    assert_eq!(
        got(&mut db, job, b).await,
        (1, 1, 2),
        "card and notice at the set time"
    );
    run_became_eligible(pool.clone(), task_again, ts(NOW))
        .await
        .expect("rerun");
    assert_eq!(got(&mut db, job, b).await, (1, 1, 2), "nothing twice");
}

/// TD-CARD-17 (CARD-018): a rule's pause lifted sends the card once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_17_a_rule_unpause_sends_the_card_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    let b = pending(&mut db, 1, TOWN, true).await;
    activate(&mut db, b, ts("2026-11-01T18:00:00Z")).await;
    let at = ts("2026-11-01T19:00:00Z");
    pause_for_rule(&mut db, b, PauseReason::Owner, Actor::System, at)
        .await
        .expect("paused");
    let job = open_job(&mut db).await;
    clear_tasks(&mut db).await;
    apply(&mut db, b, Event::Resume, Actor::System, ts(NOW))
        .await
        .expect("resumed");
    assert_eq!(eligible_tasks(&mut db, b).await, 1, "one eligibility task");
    sends_once(&mut db, job, b).await;
}

/// TD-CARD-17 (CARD-018): a newly ticked matching job type sends the card once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_17_a_new_job_type_tick_sends_the_card_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    let b = pending(&mut db, 1, TOWN, false).await;
    activate(&mut db, b, ts("2026-11-01T18:00:00Z")).await;
    let job = open_job(&mut db).await;
    clear_tasks(&mut db).await;
    tick(&mut db, b).await;
    assert_eq!(eligible_tasks(&mut db, b).await, 1, "one eligibility task");
    sends_once(&mut db, job, b).await;
}

/// TD-CARD-17 (CARD-018, CARD-024): a job in Quotes ready or a final status, a business already
/// carded and a business outside the job's reach get nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_17_other_statuses_holders_and_out_of_reach_get_nothing(pool: PgPool) {
    let mut db = setup(&pool).await;
    let ready = open_job(&mut db).await;
    step(&mut db, ready, (JobStatus::Open, JobStatus::QuotesReady)).await;
    let gone = open_job(&mut db).await;
    step(&mut db, gone, (JobStatus::Open, JobStatus::Cancelled)).await;
    let far = pending(&mut db, 2, OTHER_TOWN, true).await;
    activate(&mut db, far, ts(NOW)).await;
    let open = open_job(&mut db).await;
    let b = pending(&mut db, 1, TOWN, true).await;
    activate(&mut db, b, ts(NOW)).await;
    assert_eq!(late_cards(&mut db, b, ts(NOW)).await, Ok(vec![open]));
    assert_eq!(late_cards(&mut db, far, ts(NOW)).await, Ok(Vec::new()));
    for job in [ready, gone] {
        assert_eq!(got(&mut db, job, b).await.2, 0, "no card in another status");
    }
    assert_eq!(got(&mut db, open, far).await.2, 0, "outside the reach");
    let later = ts("2026-11-02T18:00:00Z");
    assert_eq!(late_cards(&mut db, b, later).await, Ok(Vec::new()));
    assert_eq!(got(&mut db, open, b).await, (1, 1, 2), "already carded");
}
