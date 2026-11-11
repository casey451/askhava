//! The homeowner's backup choice at the pick on a fresh database (TD-PICK-32; PICK-046,
//! PICK-047, PICK-020; D-400). Fake IDs, wording and tokens only.
//!
//! A job sits in Quotes ready with three confirmed quotes and a page pick of quote 1. The review
//! step offers the backup with "No backup" selected; a backup asked for and bound after the code
//! (D-2661) takes the pick at once when the first lapses, is refused at Confirm or fails in
//! payments, with its own pick notice and window (an emergency job's 60 minutes restart); a backup
//! no longer pickable, or none named, falls back to PICK-046; the latest bound choice counts; an
//! unbound choice, another browser's, another quote's or one after the end never counts.

use askhava_core::api::{Guarded, IdSecret, ManualClock};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_picks::api::{
    Backup, Browser, PICK_MADE_KIND, PagePick, PicksError, Refusal, backup_changed, backup_choice,
    backup_target, backup_text, bind_backup, choose_backup, lapse, page_pick, refused_at_confirm,
};
use askhava_portals::api::{PRE_SESSION_COOKIE, pick_flow_routes};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
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
const BROWSER: Browser = [7; 32];
const OTHER: Browser = [9; 32];

fn at(minutes: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("instant") + SignedDuration::from_mins(minutes)
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(0xb157 + n)
}

/// A job in Quotes ready with three confirmed quotes, picked on the page at quote 1.
async fn picked(db: &mut PgConnection) -> (Uuid, Uuid) {
    picked_job(db, false).await
}

/// As [`picked`], on a job chosen as an emergency when `emergency`.
async fn picked_job(db: &mut PgConnection, emergency: bool) -> (Uuid, Uuid) {
    let job = askhava_jobs::api::create(db, at(0), emergency, Actor::System)
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

/// Asks for `backup` with the final Pick of quote `quote` from [`BROWSER`] at `when`, then binds
/// it to waiting `pick` as the verified code does; returns what was bound.
async fn chosen(
    db: &mut PgConnection,
    (job, pick, quote): (Uuid, Uuid, usize),
    backup: Option<usize>,
    when: Timestamp,
) -> Option<usize> {
    choose_backup(db, (job, quote), (backup, &BROWSER), when)
        .await
        .expect("asked");
    bind_backup(db, pick, &BROWSER, when).await.expect("bound")
}

/// Ends `pick` as `status` with `reason` at `when`, as another module would.
async fn end_as(
    db: &mut PgConnection,
    pick: Uuid,
    (status, reason): (&str, Option<&str>),
    when: Timestamp,
) {
    sqlx::query("UPDATE picks.pick SET status = $2, end_reason = $3, ended_at = $4 WHERE id = $1")
        .bind(pick)
        .bind(status)
        .bind(reason)
        .bind(when.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("ended");
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
        chosen(&mut db, (job, pick, 1), Some(3), at(6)).await,
        Some(3)
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
    chosen(&mut db, (job, pick, 1), Some(2), at(6)).await;
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
    assert_eq!(chosen(&mut db, (job, pick, 1), Some(1), at(6)).await, None);
    assert_eq!(
        chosen(&mut db, (job, pick, 1), Some(3), at(10)).await,
        Some(3)
    );
    assert_eq!(chosen(&mut db, (job, pick, 1), None, at(11)).await, None);
    choose_backup(&mut db, (job, 1), (Some(3), &BROWSER), at(12))
        .await
        .expect("asked");
    let changed = backup_changed(&mut db, (job, 1), &BROWSER).await;
    assert_eq!(changed, Ok(true), "a different backup needs her code");
    assert_eq!(
        chosen(&mut db, (job, pick, 1), Some(2), at(12)).await,
        Some(2)
    );
    choose_backup(&mut db, (job, 1), (Some(2), &BROWSER), at(13))
        .await
        .expect("again");
    let same = backup_changed(&mut db, (job, 1), &BROWSER).await;
    assert_eq!(same, Ok(false), "the same backup again needs no code");
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
    assert_eq!(
        chosen(&mut db, (job, pick, 1), Some(3), at(250)).await,
        None
    );
    let late = backup_choice(&mut db, pick, false, (Actor::System, at(260)));
    assert_eq!(late.await, Ok(Backup::Done), "decided once, at the end");
    assert!(waiting(&mut db, job).await.is_empty());
}

/// The page served for a same-origin POST of `body` to `/q/<token>{suffix}` on a picked job,
/// without Verify keys: its status and words.
async fn posted(pool: &PgPool, suffix: &str, body: &str) -> (StatusCode, String) {
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
    let request = Request::post(format!("/q/{}{suffix}", token.as_str()))
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}&{body}")))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

/// TD-PICK-32 (PICK-047, D-2661): the review form, backup field and all, posted straight to the
/// code step is the code step, not a refusal (the end-to-end and accessibility journeys reach it
/// this way, D-1264).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_the_review_form_posted_to_the_code_step_is_accepted(pool: PgPool) {
    for backup in ["", "2"] {
        let (status, page) = posted(&pool, "/code", &format!("quote=1&backup={backup}")).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.contains("Finish your pick for request"), "{page}");
    }
}

/// TD-PICK-32 (PICK-047, D-400): the review step lists the other quotes in arrival order under
/// "If the business for quote #1 can't take it, move my pick to:", with "No backup" selected and
/// nothing else preselected.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_no_backup_is_the_default_on_the_review_step(pool: PgPool) {
    let (_, page) = posted(&pool, "", "quote=1").await;
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

/// TD-PICK-32 (PICK-047, D-2661): only a choice bound after the code counts: one only asked for
/// (a forwarded link without the code, or a pick then made by text), another browser's, or one
/// asked for another quote binds nothing and the lapse is PICK-046's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_an_unbound_choice_never_counts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    choose_backup(&mut db, (job, 1), (Some(3), &OTHER), at(6))
        .await
        .expect("other browser");
    assert_eq!(bind_backup(&mut db, pick, &BROWSER, at(7)).await, Ok(None));
    choose_backup(&mut db, (job, 2), (Some(3), &BROWSER), at(8))
        .await
        .expect("another quote");
    assert_eq!(bind_backup(&mut db, pick, &BROWSER, at(9)).await, Ok(None));
    choose_backup(&mut db, (job, 1), (Some(3), &OTHER), at(10))
        .await
        .expect("asked only");
    let ended = lapse(&mut db, pick, false, (Actor::System, at(300))).await;
    assert_eq!(ended, Ok(Backup::NoBackup));
    assert!(waiting(&mut db, job).await.is_empty());
}

/// TD-PICK-32 (PICK-047): a bound change made after the pick ended never counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_a_change_after_the_end_never_counts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    sqlx::query(
        "INSERT INTO picks.backup_choice \
         (job_id, for_quote, backup_quote, browser, pick_id, set_at) \
         VALUES ($1, 1, 2, $2, $3, $4)",
    )
    .bind(job)
    .bind(BROWSER.as_slice())
    .bind(pick)
    .bind(at(400).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("late row");
    let moved = lapse(&mut db, pick, false, (Actor::System, at(300))).await;
    assert!(matches!(moved, Ok(Backup::Moved(_))), "{moved:?}");
    assert_eq!(waiting(&mut db, job).await, vec![(business(3), 3)]);
}

/// TD-PICK-32 (PICK-047, PICK-032, CON-029): her own acts take no backup and send no PICK-046
/// text: a pick she replaced, one withdrawn by her pause, a pause on the job, or another pick
/// already waiting is `Done` and nothing moves.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_her_own_acts_take_no_backup(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    let Ok(PagePick::Picked(second)) =
        page_pick(&mut db, (job, false), 2, Actor::System, at(10)).await
    else {
        panic!("replaced");
    };
    end_as(&mut db, second, ("expired", None), at(20)).await;
    let replaced = backup_choice(&mut db, pick, false, (Actor::System, at(30))).await;
    assert_eq!(replaced, Ok(Backup::Done), "replaced");
    assert!(waiting(&mut db, job).await.is_empty());
    let (job, pick) = picked(&mut db).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    end_as(&mut db, pick, ("cancelled", Some("paused")), at(20)).await;
    let withdrawn = backup_choice(&mut db, pick, false, (Actor::System, at(30))).await;
    assert_eq!(withdrawn, Ok(Backup::Done), "withdrawn by her pause");
    assert!(waiting(&mut db, job).await.is_empty());
    let (job, pick) = picked(&mut db).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    let paused = lapse(&mut db, pick, true, (Actor::System, at(300))).await;
    assert_eq!(paused, Ok(Backup::Done), "paused: no PICK-046 text either");
    assert!(waiting(&mut db, job).await.is_empty());
    let (job, pick) = picked(&mut db).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    end_as(&mut db, pick, ("expired", None), at(20)).await;
    let again = page_pick(&mut db, (job, false), 2, Actor::System, at(25)).await;
    assert!(matches!(again, Ok(PagePick::Picked(_))), "{again:?}");
    let other = backup_choice(&mut db, pick, false, (Actor::System, at(30))).await;
    assert_eq!(other, Ok(Backup::Done), "another pick waits");
    assert_eq!(waiting(&mut db, job).await, vec![(business(2), 2)]);
}

/// TD-PICK-32 (PICK-047, PICK-044): on an emergency job the backup gets its own 60 minutes: the
/// job's wait restarts at the move.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_an_emergency_backup_gets_its_own_window(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked_job(&mut db, true).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    let refused = (pick, Refusal::License);
    let moved = refused_at_confirm(&mut db, refused, false, (Actor::System, at(30))).await;
    assert!(matches!(moved, Ok(Backup::Moved(_))), "{moved:?}");
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(row.status.code(), "awaiting_confirm");
    assert_eq!(row.timer_from, at(30), "its own window from the move");
}

/// TD-PICK-32 (PICK-047, FEE-041, FEE-018, FEE-020): a pick `payments` ended as payment failed
/// moves to the backup once the job is back in Quotes ready; while the job is still in Payment
/// pending nothing is decided and the caller is told.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_32_a_payments_end_moves_to_the_backup(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db).await;
    chosen(&mut db, (job, pick, 1), Some(3), at(6)).await;
    let step = (JobStatus::AwaitingConfirm, JobStatus::PaymentPending);
    let moved = move_to(&mut db, job, step, at(40), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "confirmed");
    end_as(&mut db, pick, ("payment_failed", None), at(50)).await;
    let early = backup_choice(&mut db, pick, false, (Actor::System, at(50))).await;
    assert_eq!(early, Err(PicksError::Job), "still Payment pending");
    let step = (JobStatus::PaymentPending, JobStatus::QuotesReady);
    let back = move_to(&mut db, job, step, at(51), Actor::System).await;
    assert!(matches!(back, Ok(Guarded::Applied(()))), "back");
    let moved = backup_choice(&mut db, pick, false, (Actor::System, at(51))).await;
    assert!(matches!(moved, Ok(Backup::Moved(_))), "{moved:?}");
    assert_eq!(waiting(&mut db, job).await, vec![(business(3), 3)]);
}
