//! The daily license-map re-apply over recorded ROC lists and the way back from a
//! `no_covered_job_type` pause (TD-BIZ-48: BIZ-050, LIC-043; T372, D-2705, D-2711, D-2730).
//! Lists are recorded with `import_roc_list`, businesses bound to a number with `check_license`.
//! Fake data only: invented license numbers, no names, phones or addresses.

use askhava_businesses::api::{
    BusinessesError, Capability, CapabilityAnswers, JobTypeChoice, Known, LICENSE_CHANGE_KIND,
    LargestJob, NewComplaint, REAPPLY_KIND, apply_lists, business, checklist_offer, create,
    job_types, pause_self, pre_ticked, record_complaint, run_reapply, save_capability,
    save_job_types, schedule_reapply,
};
use askhava_catalog::api::{Catalog, parse, set_category_enabled};
use askhava_licensing::api::{
    ClassMap, PestAttestation, check_license, import_roc_list, record_pest_attestation,
    report_pest_license_wrong,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use jiff::civil::Date;
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The number every case's business holds.
const NUMBER: &str = "ROC900100";
/// A second business's number, unchanged across the lists.
const STEADY: &str = "ROC900200";
/// A third business's number, left off the newer list.
const DROPPED: &str = "ROC900300";
/// Casey, who alone switches a category (LIC-002).
const OWNER: Uuid = Uuid::from_u128(0x7372);

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("license map loads")
}

fn now() -> Timestamp {
    "2026-11-12T15:00:00Z".parse().expect("fake time")
}

fn day(d: i8) -> Date {
    Date::new(2026, 11, d).expect("fake date")
}

/// A list holding `rows` (`number`, `class`) Active, plus ten filler numbers so a shorter list is
/// never refused as truncated (D-905).
fn list(rows: &[(&str, &str)]) -> String {
    let mut text = "license_number,class,status,issued_on\n".to_owned();
    for n in 0..10 {
        text.push_str(&format!("ROC9990{n:02},R-11,Active,2015-01-01\n"));
    }
    for (number, class) in rows {
        text.push_str(&format!("{number},{class},Active,2015-01-01\n"));
    }
    text
}

async fn record(db: &mut PgConnection, d: i8, rows: &[(&str, &str)]) {
    import_roc_list(db, day(d), &list(rows))
        .await
        .expect("list recorded");
}

/// An active business bound to `number` on the latest list, with `ticked` stored.
async fn bound(db: &mut PgConnection, number: &str, ticked: Vec<JobTypeChoice>) -> Uuid {
    let id = create(db, false, now()).await.expect("business");
    check_license(db, id, number).await.expect("checked");
    let offer = checklist_offer(map(), &catalog(), &["R-11", "R-37"]);
    let answers = CapabilityAnswers {
        ticked,
        towns: vec!["lake_havasu_city".to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours: false,
    };
    save_capability(db, id, &offer, &answers, (Actor::System, now()))
        .await
        .expect("saved");
    sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("active");
    id
}

/// The pre-ticked job types `classes` give in `category`.
fn covered(classes: &[&str], category: &str) -> Vec<JobTypeChoice> {
    let offer = checklist_offer(map(), &catalog(), classes);
    let ticks = pre_ticked(&offer).into_iter();
    ticks.filter(|c| c.category == category).collect()
}

async fn store_catalog(db: &mut PgConnection) {
    askhava_catalog::api::load(db, &catalog())
        .await
        .expect("catalog stored");
}

async fn notices(db: &mut PgConnection) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(LICENSE_CHANGE_KIND)
        .fetch_all(db)
        .await
        .expect("tasks")
}

async fn standing(db: &mut PgConnection, id: Uuid) -> (String, Option<String>) {
    let b = business(db, id).await.expect("read");
    let reason = b.pause_reason.map(|r| r.code().to_owned());
    (b.status.code().to_owned(), reason)
}

fn paused(reason: &str) -> (String, Option<String>) {
    ("paused".to_owned(), Some(reason.to_owned()))
}

async fn apply(db: &mut PgConnection) -> usize {
    apply_lists(db, (map(), &catalog()), now())
        .await
        .expect("applied")
}

/// An R-11 and R-37 business ticking only plumbing, whose R-37 then goes: paused for
/// `no_covered_job_type` with one email queued.
async fn lost_plumbing(db: &mut PgConnection) -> Uuid {
    store_catalog(db).await;
    record(db, 1, &[(NUMBER, "R-11"), (NUMBER, "R-37")]).await;
    let id = bound(db, NUMBER, covered(&["R-37"], "plumbing_repair")).await;
    record(db, 2, &[(NUMBER, "R-11")]).await;
    assert_eq!(apply(db).await, 2);
    assert_eq!(standing(db, id).await, paused("no_covered_job_type"));
    id
}

/// TD-BIZ-48 (BIZ-050, D-393): a removed R-37 takes the plumbing job types off, pauses the
/// business when nothing is left and queues one email; an unchanged record and a number missing
/// from the newer list are untouched; a re-run queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_a_removed_class_unticks_pauses_and_emails_once(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    store_catalog(&mut db).await;
    let rows = [
        (NUMBER, "R-11"),
        (NUMBER, "R-37"),
        (STEADY, "R-11"),
        (DROPPED, "R-37"),
    ];
    record(&mut db, 1, &rows).await;
    let plumbing = covered(&["R-37"], "plumbing_repair");
    assert!(!plumbing.is_empty());
    let id = bound(&mut db, NUMBER, plumbing.clone()).await;
    let steady = bound(&mut db, STEADY, covered(&["R-11"], "electrical")).await;
    let dropped = bound(&mut db, DROPPED, plumbing.clone()).await;
    let steady_ticks = job_types(&mut db, steady).await.expect("read");
    record(&mut db, 2, &[(NUMBER, "R-11"), (STEADY, "R-11")]).await;
    assert_eq!(apply(&mut db).await, 2);
    assert!(job_types(&mut db, id).await.expect("read").is_empty());
    assert_eq!(standing(&mut db, id).await, paused("no_covered_job_type"));
    let tasks = notices(&mut db).await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["business"], serde_json::json!(id));
    assert_eq!(tasks[0]["dropped_classes"], serde_json::json!(["R-37"]));
    assert_eq!(tasks[0]["paused"], serde_json::json!(true));
    assert_eq!(
        job_types(&mut db, steady).await.expect("read"),
        steady_ticks
    );
    let mut kept = job_types(&mut db, dropped).await.expect("read");
    let mut plumbing = plumbing;
    kept.sort();
    plumbing.sort();
    assert_eq!(kept, plumbing);
    assert_eq!(standing(&mut db, dropped).await.0, "active");
    assert_eq!(apply(&mut db).await, 0);
    assert_eq!(notices(&mut db).await.len(), 1);
}

/// TD-BIZ-48 (BIZ-050, LIC-043): an added class is offered by email and never ticked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_an_added_class_is_offered_never_ticked(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    store_catalog(&mut db).await;
    record(&mut db, 1, &[(NUMBER, "R-11")]).await;
    let id = bound(&mut db, NUMBER, covered(&["R-11"], "electrical")).await;
    let before = job_types(&mut db, id).await.expect("read");
    record(&mut db, 2, &[(NUMBER, "R-11"), (NUMBER, "R-37")]).await;
    apply(&mut db).await;
    assert_eq!(job_types(&mut db, id).await.expect("read"), before);
    assert_eq!(standing(&mut db, id).await.0, "active");
    let tasks = notices(&mut db).await;
    assert_eq!(tasks.len(), 1);
    let offered = tasks[0]["offered"].as_array().expect("offered").clone();
    assert!(offered.iter().any(|p| p[0] == "plumbing_repair"));
}

/// TD-BIZ-48 (BIZ-050, LIC-002, T368): a category switched off is never offered.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_a_switched_off_category_is_not_offered(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    store_catalog(&mut db).await;
    record(&mut db, 1, &[(NUMBER, "R-11")]).await;
    bound(&mut db, NUMBER, covered(&["R-11"], "electrical")).await;
    set_category_enabled(&mut db, "plumbing_repair", false, Actor::Owner(OWNER))
        .await
        .expect("switched off");
    record(&mut db, 2, &[(NUMBER, "R-11"), (NUMBER, "R-37")]).await;
    apply(&mut db).await;
    for task in notices(&mut db).await {
        let offered = task["offered"].as_array().expect("offered").clone();
        assert!(offered.iter().all(|p| p[0] != "plumbing_repair"));
    }
}

/// TD-BIZ-48 (BIZ-050, D-2705): two lists recorded on days the task did not run are applied in
/// order, so a class removed on the first and not restored is removed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_lists_missed_are_applied_in_order(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    store_catalog(&mut db).await;
    record(&mut db, 1, &[(NUMBER, "R-11"), (NUMBER, "R-37")]).await;
    let id = bound(&mut db, NUMBER, covered(&["R-37"], "plumbing_repair")).await;
    assert_eq!(apply(&mut db).await, 1);
    record(&mut db, 2, &[(NUMBER, "R-11")]).await;
    record(&mut db, 3, &[(NUMBER, "R-11"), (STEADY, "R-11")]).await;
    assert_eq!(apply(&mut db).await, 2);
    assert!(job_types(&mut db, id).await.expect("read").is_empty());
    assert_eq!(standing(&mut db, id).await, paused("no_covered_job_type"));
}

/// TD-BIZ-48 (D-2711, D-2730): through `save_job_types`, a covered tick lifts the pause (status
/// audit row); a tick outside the rebuilt offer is refused and lifts nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_a_covered_tick_lifts_and_an_uncovered_one_is_refused(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = lost_plumbing(&mut db).await;
    let maps = (map(), &catalog());
    let at = (Actor::Business(id), now());
    let plumbing = covered(&["R-37"], "plumbing_repair");
    let refused = save_job_types(&mut db, id, Known::default(), maps, &plumbing, at).await;
    assert_eq!(refused, Err(BusinessesError::NotCovered));
    assert!(job_types(&mut db, id).await.expect("read").is_empty());
    assert_eq!(standing(&mut db, id).await, paused("no_covered_job_type"));
    let electrical = covered(&["R-11"], "electrical");
    let saved = save_job_types(&mut db, id, Known::default(), maps, &electrical, at).await;
    assert_eq!(saved.expect("saved").code(), "active");
    assert_eq!(standing(&mut db, id).await, ("active".to_owned(), None));
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 \
         AND from_status = 'paused' AND to_status = 'active'",
    )
    .bind(id)
    .fetch_one(&mut *db)
    .await
    .expect("audit");
    assert_eq!(rows, 1);
}

/// TD-BIZ-48 (BIZ-012, D-2730): a business that crossed the complaint limit while paused stays
/// paused for complaints.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_complaints_while_paused_keep_it_paused(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = lost_plumbing(&mut db).await;
    for n in 1..=3u128 {
        let complaint = NewComplaint {
            business: id,
            job: Uuid::from_u128(n),
            kind: "no_show",
            words: "did not arrive",
        };
        record_complaint(&mut db, &complaint, now())
            .await
            .expect("recorded");
    }
    let electrical = covered(&["R-11"], "electrical");
    let at = (Actor::Business(id), now());
    let maps = (map(), &catalog());
    let saved = save_job_types(&mut db, id, Known::default(), maps, &electrical, at).await;
    assert_eq!(saved.expect("saved").code(), "paused");
    assert_eq!(standing(&mut db, id).await, paused("complaints"));
}

/// TD-BIZ-48 (LIC-047, D-2730): a business whose pest license was reported wrong stays paused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_a_pest_license_reported_wrong_keeps_it_paused(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = lost_plumbing(&mut db).await;
    let pest = PestAttestation::new(true, "PM-4242", true, false).expect("attestation");
    record_pest_attestation(&mut db, id, &pest, Actor::Business(id))
        .await
        .expect("recorded");
    report_pest_license_wrong(&mut db, id, now())
        .await
        .expect("reported");
    let electrical = covered(&["R-11"], "electrical");
    let at = (Actor::Business(id), now());
    let maps = (map(), &catalog());
    let saved = save_job_types(&mut db, id, Known::default(), maps, &electrical, at).await;
    assert_eq!(saved.expect("saved").code(), "paused");
    assert_eq!(standing(&mut db, id).await, paused("license"));
}

/// TD-BIZ-48 (D-969, D-970, D-2730): a business self-paused before the license pause returns to
/// its own pause, never active.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_a_self_pause_underneath_comes_back(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    store_catalog(&mut db).await;
    record(&mut db, 1, &[(NUMBER, "R-11"), (NUMBER, "R-37")]).await;
    let id = bound(&mut db, NUMBER, covered(&["R-37"], "plumbing_repair")).await;
    pause_self(&mut db, id, now()).await.expect("self-paused");
    record(&mut db, 2, &[(NUMBER, "R-11")]).await;
    apply(&mut db).await;
    assert_eq!(standing(&mut db, id).await, paused("no_covered_job_type"));
    let electrical = covered(&["R-11"], "electrical");
    let at = (Actor::Business(id), now());
    let maps = (map(), &catalog());
    let saved = save_job_types(&mut db, id, Known::default(), maps, &electrical, at).await;
    assert_eq!(saved.expect("saved").code(), "paused");
    assert_eq!(standing(&mut db, id).await, paused("self"));
}

/// TD-BIZ-48 (D-2711): a paused business whose capability save holds a covered tick is resumed in
/// that transaction; one whose save holds no covered tick stays paused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_a_capability_save_resumes_only_with_a_covered_tick(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = lost_plumbing(&mut db).await;
    let offer = checklist_offer(map(), &catalog(), &["R-11"]);
    let windows = JobTypeChoice::new("window_cleaning", "inside and outside windows");
    let mut answers = CapabilityAnswers {
        ticked: vec![windows],
        towns: vec!["lake_havasu_city".to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours: false,
    };
    let at = (Actor::Business(id), now());
    save_capability(&mut db, id, &offer, &answers, at)
        .await
        .expect("saved");
    assert_eq!(standing(&mut db, id).await, paused("no_covered_job_type"));
    answers.ticked = covered(&["R-11"], "electrical");
    save_capability(&mut db, id, &offer, &answers, at)
        .await
        .expect("saved");
    assert_eq!(standing(&mut db, id).await, ("active".to_owned(), None));
}

/// TD-BIZ-48 (BIZ-050, JOB-006): the run is queued once a day at 7 a.m. Arizona time, after the
/// lists; a run queues tomorrow's first, then applies the lists.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_48_the_run_is_queued_once_a_day(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let early: Timestamp = "2026-11-12T13:00:00Z".parse().expect("fake time");
    schedule_reapply(&mut db, early).await.expect("queued");
    schedule_reapply(&mut db, early)
        .await
        .expect("queued again");
    record(&mut db, 1, &[(NUMBER, "R-11")]).await;
    run_reapply(&pool, now()).await.expect("ran");
    let due: Vec<(String, String)> = sqlx::query_as(
        "SELECT to_char(run_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), dedupe_key \
         FROM core.task WHERE kind = $1 ORDER BY run_at",
    )
    .bind(REAPPLY_KIND)
    .fetch_all(&mut *db)
    .await
    .expect("tasks");
    let at: Vec<&str> = due.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(at, ["2026-11-12T14:00:00Z", "2026-11-13T14:00:00Z"]);
    assert_eq!(due[1].1, format!("{REAPPLY_KIND}.2026-11-13"));
    assert_eq!(apply(&mut db).await, 0);
}
