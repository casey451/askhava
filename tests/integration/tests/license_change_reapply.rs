//! A daily license list changes a business's class (TD-BIZ-30: BIZ-050, LIC-043; D-393). Adding
//! R-37 to an R-11 business offers the plumbing job types by email and ticks none; removing R-11
//! takes the electrical job types off at once with an email saying which and why, and pauses the
//! business when nothing is left; a no-license or agency-licensed job type stays and keeps it
//! active; the email is queued once per class change and list date. Fake data only.

use askhava_businesses::api::{
    BusinessesError, Capability, CapabilityAnswers, JobTypeChoice, LICENSE_CHANGE_KIND, LargestJob,
    Reapplied, business, checklist_offer, create, job_types, license_change_words, pause_self,
    pre_ticked, reapply_license, save_capability,
};
use askhava_catalog::api::{Catalog, parse};
use askhava_licensing::api::ClassMap;
use askhava_ops::api::Actor;
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("license map loads")
}

fn now() -> Timestamp {
    "2026-11-10T17:00:00Z".parse().expect("fake time")
}

/// The daily list's date and the time it is applied.
fn when() -> (Date, Timestamp) {
    ("2026-11-10".parse().expect("fake date"), now())
}

const WINDOWS: (&str, &str) = ("window_cleaning", "inside and outside windows");

/// An active R-11 business with every electrical job type ticked, plus `extra` if given.
async fn r11_business(db: &mut PgConnection, extra: Option<(&str, &str)>) -> Uuid {
    let id = create(db, false, now()).await.expect("business");
    let offer = checklist_offer(map(), &catalog(), &["R-11"]);
    let mut ticked = pre_ticked(&offer);
    ticked.extend(extra.map(|(c, t)| JobTypeChoice::new(c, t)));
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

/// Ticks `job` on business `id` directly, as an attested agency license would (LIC-043).
async fn tick(db: &mut PgConnection, id: Uuid, (category, job_type): (&str, &str)) {
    sqlx::query(
        "INSERT INTO businesses.job_type_choice (business_id, category, job_type) \
         VALUES ($1, $2, $3)",
    )
    .bind(id)
    .bind(category)
    .bind(job_type)
    .execute(db)
    .await
    .expect("ticked");
}

async fn notices(db: &mut PgConnection) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(LICENSE_CHANGE_KIND)
        .fetch_all(db)
        .await
        .expect("tasks")
}

/// TD-BIZ-30 (BIZ-050): a list adding R-37 emails an offer of plumbing job types and ticks none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_added_class_offers_and_ticks_nothing(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = r11_business(&mut db, None).await;
    let before = job_types(&mut db, id).await.expect("read");
    let maps = (map(), &catalog());
    let classes: (&[&str], &[&str]) = (&["R-11"], &["R-11", "R-37"]);
    let change = reapply_license(&mut db, id, maps, classes, when())
        .await
        .expect("re-applied");
    assert!(change.removed.is_empty() && !change.paused);
    assert!(!change.offered.is_empty());
    assert!(
        change
            .offered
            .iter()
            .all(|c| c.category == "plumbing_repair")
    );
    assert_eq!(job_types(&mut db, id).await.expect("read"), before);
    let tasks = notices(&mut db).await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["removed"], serde_json::json!([]));
    assert!(
        tasks[0]["offered"]
            .as_array()
            .is_some_and(|o| !o.is_empty())
    );
    let status = business(&mut db, id).await.expect("read").status;
    assert_eq!(status.code(), "active");
}

/// TD-BIZ-30 (BIZ-050, D-393): a list removing R-11 removes the electrical job types at once,
/// emails which and why, and pauses the business with the reason when nothing is left.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_removed_class_unticks_emails_and_pauses(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = r11_business(&mut db, None).await;
    let before = job_types(&mut db, id).await.expect("read");
    let classes: (&[&str], &[&str]) = (&["R-11"], &[]);
    let change = reapply_license(&mut db, id, (map(), &catalog()), classes, when())
        .await
        .expect("re-applied");
    assert_eq!(change.removed, before);
    assert!(change.offered.is_empty() && change.paused);
    assert!(job_types(&mut db, id).await.expect("read").is_empty());
    let b = business(&mut db, id).await.expect("read");
    assert_eq!(b.status.code(), "paused");
    let reason = b.pause_reason.map(|r| r.code());
    assert_eq!(reason, Some("no_covered_job_type"));
    let tasks = notices(&mut db).await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["dropped_classes"], serde_json::json!(["R-11"]));
    assert_eq!(tasks[0]["paused"], serde_json::json!(true));
    assert_eq!(tasks[0]["business"], serde_json::json!(id));
    let again = reapply_license(&mut db, id, (map(), &catalog()), classes, when()).await;
    assert_eq!(again.expect("re-run"), Reapplied::default());
    assert_eq!(notices(&mut db).await.len(), 1);
}

/// TD-BIZ-30 (BIZ-050, LIC-043): a no-license job type is kept, so the business stays active;
/// a business that is not taking cards is not paused by the removal.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_no_license_job_type_keeps_the_business_active(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = r11_business(&mut db, Some(WINDOWS)).await;
    let classes: (&[&str], &[&str]) = (&["R-11"], &[]);
    let change = reapply_license(&mut db, id, (map(), &catalog()), classes, when())
        .await
        .expect("re-applied");
    assert!(!change.removed.is_empty() && !change.paused);
    let kept = job_types(&mut db, id).await.expect("read");
    assert_eq!(kept, vec![JobTypeChoice::new(WINDOWS.0, WINDOWS.1)]);
    let status = business(&mut db, id).await.expect("read").status;
    assert_eq!(status.code(), "active");
    let pending = r11_business(&mut db, None).await;
    sqlx::query("UPDATE businesses.business SET status = 'pending' WHERE id = $1")
        .bind(pending)
        .execute(&mut *db)
        .await
        .expect("pending");
    let change = reapply_license(&mut db, pending, (map(), &catalog()), classes, when()).await;
    assert!(!change.expect("re-applied").paused);
    let status = business(&mut db, pending).await.expect("read").status;
    assert_eq!(status.code(), "pending");
}

/// TD-BIZ-30 (BIZ-050, BIZ-018): the email names the removed job types and why, the pause, and
/// the offered ones without ticking them, in English and Spanish.
#[test]
fn td_biz_30_email_says_which_and_why_in_both_languages() {
    let change = Reapplied {
        removed: vec![JobTypeChoice::new("electrical", "panel upgrade")],
        offered: vec![JobTypeChoice::new("plumbing_repair", "leak repair")],
        paused: true,
    };
    let [subject, body] = license_change_words(false, &change, &["R-11"]);
    assert!(subject.contains("job types"));
    assert!(body.contains("no longer shows R-11") && body.contains("panel upgrade"));
    assert!(body.contains("paused your account") && body.contains("leak repair"));
    assert!(body.contains("We didn't add these for you"));
    let [subject, body] = license_change_words(true, &change, &[]);
    assert!(subject.contains("tipos de trabajo"));
    assert!(body.contains("otra clase") && body.contains("panel upgrade"));
    assert!(body.contains("pausamos su cuenta") && body.contains("No los agregamos"));
    let offer_only = Reapplied {
        offered: change.offered.clone(),
        ..Reapplied::default()
    };
    let [_, body] = license_change_words(false, &offer_only, &[]);
    assert!(!body.contains("paused") && !body.contains("anymore"));
}

/// TD-BIZ-30 (BIZ-050, D-2595): a job type of an agency-licensed category (pest control), which no
/// class covers, is kept when R-11 is dropped, so the business stays active.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_agency_licensed_job_type_is_kept(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = r11_business(&mut db, None).await;
    let pest = ("pest_control", "scorpions");
    tick(&mut db, id, pest).await;
    let classes: (&[&str], &[&str]) = (&["R-11"], &[]);
    let change = reapply_license(&mut db, id, (map(), &catalog()), classes, when())
        .await
        .expect("re-applied");
    assert!(change.removed.iter().all(|c| c.category == "electrical"));
    assert!(!change.removed.is_empty() && !change.paused);
    let kept = job_types(&mut db, id).await.expect("read");
    assert_eq!(kept, vec![JobTypeChoice::new(pest.0, pest.1)]);
    let status = business(&mut db, id).await.expect("read").status;
    assert_eq!(status.code(), "active");
}

/// TD-BIZ-30 (BIZ-050, R13, D-2596): re-running the same offer at a later time queues no second
/// email and still ticks nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_offer_is_queued_once_on_a_later_rerun(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = r11_business(&mut db, None).await;
    let before = job_types(&mut db, id).await.expect("read");
    let maps = (map(), &catalog());
    let classes: (&[&str], &[&str]) = (&["R-11"], &["R-11", "R-37"]);
    let first = reapply_license(&mut db, id, maps, classes, when()).await;
    assert!(!first.expect("first").offered.is_empty());
    let later = (when().0, now() + SignedDuration::from_hours(3));
    let respelled: (&[&str], &[&str]) = (&["r-11"], &["R-37 ", "R-11"]);
    let again = reapply_license(&mut db, id, maps, respelled, later).await;
    assert!(again.expect("re-run").removed.is_empty());
    assert_eq!(notices(&mut db).await.len(), 1);
    assert_eq!(job_types(&mut db, id).await.expect("read"), before);
}

/// TD-BIZ-30 (BIZ-050, D-393, D-970): a self-paused business left with nothing is paused for
/// the rule, which replaces its own pause.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_self_paused_business_is_paused_for_the_rule(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = r11_business(&mut db, None).await;
    pause_self(&mut db, id, now()).await.expect("self-paused");
    let classes: (&[&str], &[&str]) = (&["R-11"], &[]);
    let change = reapply_license(&mut db, id, (map(), &catalog()), classes, when())
        .await
        .expect("re-applied");
    assert!(change.paused);
    let b = business(&mut db, id).await.expect("read");
    assert_eq!(b.status.code(), "paused");
    let reason = b.pause_reason.map(|r| r.code());
    assert_eq!(reason, Some("no_covered_job_type"));
}

/// TD-BIZ-30 (BIZ-050): an unknown business is refused and queues nothing; a dropped class is
/// named by the same reading as the license map, so a respelled kept class is not "dropped".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_30_unknown_business_and_class_spelling(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let classes: (&[&str], &[&str]) = (&["R-11"], &[]);
    let unknown = reapply_license(&mut db, Uuid::nil(), (map(), &catalog()), classes, when()).await;
    assert!(matches!(unknown, Err(BusinessesError::UnknownBusiness)));
    assert!(notices(&mut db).await.is_empty());
    let id = r11_business(&mut db, Some(WINDOWS)).await;
    let classes: (&[&str], &[&str]) = (&["R-11", "R-37"], &["r-37"]);
    let change = reapply_license(&mut db, id, (map(), &catalog()), classes, when()).await;
    assert!(!change.expect("re-applied").removed.is_empty());
    let tasks = notices(&mut db).await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["dropped_classes"], serde_json::json!(["R-11"]));
}
