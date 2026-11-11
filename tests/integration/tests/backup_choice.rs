//! The homeowner's backup choice at the pick on a fresh database (TD-PICK-32; PICK-046,
//! PICK-047, PICK-020; D-400). Fake IDs, wording and tokens only.
//!
//! A job sits in Quotes ready with three confirmed quotes and a page pick of quote 1. The review
//! step offers the backup with "No backup" selected; a stored backup takes the pick at once when
//! the first lapses or is refused at Confirm, with its own pick notice; a backup no longer
//! pickable, or none named, falls back to PICK-046; the latest choice counts.

use askhava_core::api::{Guarded, IdSecret, ManualClock};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_picks::api::{
    Backup, PICK_MADE_KIND, PagePick, Refusal, backup_target, backup_text, choose_backup, lapse,
    page_pick, refused_at_confirm,
};
use askhava_portals::api::{PRE_SESSION_COOKIE, pick_flow_routes};
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use std::sync::Arc;
use tower::ServiceExt;

const DAY: &str = "2026-11-03T17:00:00Z";
const COOKIE: &str = "fake-pre-session-cookie-value-0157";

fn at(minutes: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("instant") + SignedDuration::from_mins(minutes)
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(0xb157 + n)
}

/// A job in Quotes ready with three confirmed quotes, picked on the page at quote 1.
async fn picked(db: &mut PgConnection) -> (Uuid, Uuid) {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(0), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    for n in 1..=3 {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(business(n))
        .bind(at(i64::try_from(n).expect("small")).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
    let Ok(PagePick::Picked(pick)) = page_pick(db, (job, false), 1, Actor::System, at(5)).await
    else {
        panic!("a page pick");
    };
    (job, pick)
}

async fn waiting(db: &mut PgConnection, job: Uuid) -> Vec<(Uuid, i32)> {
    sqlx::query_as(
        "SELECT business_id, quote_number FROM picks.pick \
         WHERE job_id = $1 AND status = 'awaiting_confirm'",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("picks")
}

async fn notices(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(PICK_MADE_KIND)
        .fetch_one(db)
        .await
        .expect("tasks")
}

/// TD-PICK-32 (PICK-047, PICK-037, D-400): with a backup named, the lapse moves the pick to it at
/// once as a new waiting pick with its own pick notice, the homeowner's text names both, the job
/// stays in Awaiting confirm, and a rerun moves nothing twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_a_lapse_moves_the_pick_to_the_backup(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    assert_eq!(
        choose_backup(&mut db, (job, 1), Some(3), at(4)).await,
        Ok(Some(3))
    );
    let before = notices(&mut db).await;
    let moved = lapse(&mut db, pick, false, (Actor::System, at(300))).await;
    assert!(matches!(moved, Ok(Backup::Moved(_))), "{moved:?}");
    assert_eq!(waiting(&mut db, job).await, vec![(business(3), 3)]);
    assert_eq!(
        notices(&mut db).await,
        before + 1,
        "PICK-037: its own notice"
    );
    let status = current(&mut db, job).await.expect("job").status.code();
    assert_eq!(status, "awaiting_confirm");
    let again = lapse(&mut db, pick, false, (Actor::System, at(301))).await;
    assert_eq!(again, Ok(Backup::Done));
    assert_eq!(notices(&mut db).await, before + 1);
    let text = backup_text("Sample Plumbing", "Sample Drains", Language::En);
    assert!(text.ends_with(": Sample Plumbing couldn't take it, so your backup, Sample Drains, has your pick now. We'll text you when they accept."), "{text}");
    assert!(backup_text("A", "B", Language::Es).contains("a su segunda opción, B."));
}

/// TD-PICK-32 (PICK-046, PICK-047): a backup no longer pickable, or none named, falls back to
/// PICK-046 with no new pick; Ask Hava never chooses one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_no_choosable_backup_falls_back_to_pick_046(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    choose_backup(&mut db, (job, 1), Some(2), at(4))
        .await
        .expect("chosen");
    sqlx::query(
        "UPDATE quotes.quote SET unavailable_at = $2 WHERE job_id = $1 AND business_id = $3",
    )
    .bind(job)
    .bind(at(60).to_sqlx())
    .bind(business(2))
    .execute(&mut *db)
    .await
    .expect("unavailable");
    let ended = lapse(&mut db, pick, false, (Actor::System, at(300))).await;
    assert_eq!(ended, Ok(Backup::NoBackup));
    assert!(waiting(&mut db, job).await.is_empty());
    let (job, pick) = picked(&mut db).await;
    let ended = lapse(&mut db, pick, false, (Actor::System, at(300))).await;
    assert_eq!(
        ended,
        Ok(Backup::NoBackup),
        "none named: none chosen for her"
    );
    assert!(waiting(&mut db, job).await.is_empty());
    let quotes = [
        (business(1), true),
        (business(2), false),
        (business(3), true),
    ];
    assert_eq!(backup_target(None, (1, business(1)), &quotes), None);
    assert_eq!(backup_target(Some(2), (1, business(1)), &quotes), None);
    assert_eq!(backup_target(Some(1), (1, business(1)), &quotes), None);
    assert_eq!(backup_target(Some(4), (1, business(1)), &quotes), None);
    assert_eq!(backup_target(Some(3), (1, business(3)), &quotes), None);
    assert_eq!(backup_target(Some(3), (1, business(1)), &quotes), Some(3));
}

/// TD-PICK-32 (PICK-047, PICK-020): she can change the backup before the pick ends (the latest
/// choice counts, removing it is "No backup", the same quote is never its own backup), a change
/// after the end never counts, and a Confirm refused for the license moves the pick at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_the_backup_can_change_until_the_pick_ends(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    assert_eq!(
        choose_backup(&mut db, (job, 1), Some(1), at(4)).await,
        Ok(None)
    );
    choose_backup(&mut db, (job, 1), Some(3), at(10))
        .await
        .expect("first");
    choose_backup(&mut db, (job, 1), None, at(11))
        .await
        .expect("removed");
    choose_backup(&mut db, (job, 1), Some(2), at(12))
        .await
        .expect("changed");
    let refused = (pick, Refusal::License);
    let moved = refused_at_confirm(&mut db, refused, false, (Actor::System, at(200))).await;
    assert!(matches!(moved, Ok(Backup::Moved(_))), "{moved:?}");
    assert_eq!(waiting(&mut db, job).await, vec![(business(2), 2)]);
    let reason: Option<String> =
        sqlx::query_scalar("SELECT end_reason FROM picks.pick WHERE id = $1")
            .bind(pick)
            .fetch_one(&mut *db)
            .await
            .expect("reason");
    assert_eq!(reason.as_deref(), Some("ineligible"));
    let (job, pick) = picked(&mut db).await;
    let gone = refused_at_confirm(
        &mut db,
        (pick, Refusal::Account),
        false,
        (Actor::System, at(200)),
    );
    assert_eq!(gone.await, Ok(Backup::NoBackup));
    choose_backup(&mut db, (job, 1), Some(3), at(250))
        .await
        .expect("late");
    let late = askhava_picks::api::backup_choice(&mut db, pick, false, (Actor::System, at(260)));
    assert_eq!(late.await, Ok(Backup::Done), "decided once, at the end");
    assert!(waiting(&mut db, job).await.is_empty());
}

/// TD-PICK-32 (PICK-047, D-400): the review step lists the other quotes in arrival order under
/// "If the business for quote #1 can't take it, move my pick to:", with "No backup" selected and
/// nothing else preselected.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_no_backup_is_the_default_on_the_review_step(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, _) = picked(&mut db).await;
    let secret = SecretString::from("test-only-id-secret-not-a-real-key-157".to_owned());
    let link = IdSecret::new(1, secret).expect("id");
    let token = issue(&mut db, &link, job, at(6)).await.expect("token");
    let clock = Arc::new(ManualClock::new(at(7)));
    let app = pick_flow_routes::<askhava_adapters::api::TwilioVerify>(pool.clone(), clock, None);
    let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let request = Request::post(format!("/q/{}", token.as_str()))
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}&quote=1")))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let page = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(
        page.contains("If the business for quote #1 can't take it, move my pick to:"),
        "{page}"
    );
    assert!(page.contains("<option value=\"\" selected>No backup</option>"));
    let two = page
        .find("<option value=\"2\">Quote #2</option>")
        .expect("quote 2");
    let three = page
        .find("<option value=\"3\">Quote #3</option>")
        .expect("quote 3");
    assert!(two < three, "arrival order");
    assert_eq!(
        page.matches(" selected").count(),
        1,
        "nothing else preselected"
    );
    assert!(
        !page.contains("<option value=\"1\">"),
        "never the picked quote"
    );
}
