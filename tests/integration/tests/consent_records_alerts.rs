//! Consent records kept, never exported, and the weekly opt-out rate alerts (TD-CON-09; CON-022,
//! CON-023, CON-024; D-3881, D-4082, T107).
//!
//! Proves on fresh databases that an Arizona week runs Monday 00:00 to Monday 00:00 at UTC-7; that
//! 2.99% of texted homeowners raises no alert and 3.00% raises one, once; that 5.00% of texted
//! businesses alerts and 4% does not, a business told apart by its `businesses.phone_stopped`
//! row; that a repeat STOP in the next week is not counted while STOP, START, STOP counts again;
//! that the opt-out log and message log outlast the body cut and the app cannot delete them; that
//! no module but `messaging` reads the opt-out tables; and that the weekly check is queued once
//! per week. Every digest comes from a test `Vault` with fake keys under the homeowner-phone index
//! purpose `messaging.optout_event` uses; every phone is a fake 928 555 01xx number.

use std::fs;
use std::path::{Path, PathBuf};

use askhava_integration::{is_permission_denied, set_app_role};
use askhava_messaging::api::{
    Audience, BUSINESS_OPTOUT_ALERT, CONSENT_KEEP_YEARS, HOMEOWNER_OPTOUT_ALERT,
    OPTOUT_RATE_CHECK_KIND, OptOutRates, WEEK_SUBJECT, Week, WeekRate, check_optout_rates,
    cut_bodies, schedule_optout_rate_check, weekly_optout_rates,
};
use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DAY: SignedDuration = SignedDuration::from_hours(24);
const WEEK: SignedDuration = SignedDuration::from_hours(24 * 7);
/// Monday Nov 9, 2026, 00:00 in Arizona.
const MONDAY: &str = "2026-11-09T07:00:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

/// Wednesday noon Arizona time in the week starting [`MONDAY`] plus `weeks`.
fn midweek(weeks: i32) -> Timestamp {
    ts(MONDAY) + WEEK * weeks + DAY * 2 + SignedDuration::from_hours(5)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x71), &key(0x17), &url).expect("vault")
}

/// The fake number `928 555 nnnn`'s digest under the homeowner-phone index key.
fn digest(vault: &Vault, n: u32) -> (i16, Vec<u8>) {
    let phone = normalize_phone(&format!("928-555-{n:04}")).expect("phone");
    let (version, bytes) = vault.blind_index(IndexPurpose::HomeownerPhone, &phone)[0];
    (i16::try_from(version).expect("version"), bytes.to_vec())
}

/// `count` texts sent at `at`, each to a distinct `kind` recipient.
async fn sent(pool: &PgPool, kind: &str, count: i32, at: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, body, status, provider_sid, sent_at) \
         SELECT encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'consent', \
           gen_random_uuid(), $1, ARRAY[gen_random_uuid()], NULL, 'sent', \
           'SM' || md5(gen_random_uuid()::text), $3::timestamptz \
         FROM generate_series(1, $2)",
    )
    .bind(kind)
    .bind(count)
    .bind(at.to_string())
    .execute(pool)
    .await
    .expect("sent");
}

/// One `action` event for `number` at `at`, carried by inbound text `inbox` when given.
async fn event(
    pool: &PgPool,
    action: &str,
    number: &(i16, Vec<u8>),
    inbox: Option<Uuid>,
    at: Timestamp,
) {
    let detected = if action == "start" {
        "word:start"
    } else {
        "word:stop"
    };
    sqlx::query(
        "INSERT INTO messaging.optout_event \
         (action, channel, detected, index_version, digest, inbox_id, effective_at) \
         VALUES ($1, 'text', $2, $3, $4, $5, $6::timestamptz)",
    )
    .bind(action)
    .bind(detected)
    .bind(number.0)
    .bind(&number.1)
    .bind(inbox)
    .bind(at.to_string())
    .execute(pool)
    .await
    .expect("event");
}

/// A business's STOP: the event with its inbox row, marked `phone_stopped` as the inbound route does.
async fn business_stop(pool: &PgPool, number: &(i16, Vec<u8>), at: Timestamp) {
    let inbox = Uuid::new_v4();
    let mut db = pool.acquire().await.expect("connection");
    askhava_businesses::api::phone_stopped(&mut db, Uuid::new_v4(), inbox, at)
        .await
        .expect("phone stopped");
    event(pool, "opt_out", number, Some(inbox), at).await;
}

async fn alerts(pool: &PgPool, kind: &str) -> Vec<(String, Uuid, String)> {
    sqlx::query_as(
        "SELECT subject_kind, subject_id, severity FROM ops.alert WHERE kind = $1 ORDER BY id",
    )
    .bind(kind)
    .fetch_all(pool)
    .await
    .expect("alerts")
}

async fn check(pool: &PgPool, now: Timestamp) -> OptOutRates {
    let mut db = pool.acquire().await.expect("connection");
    check_optout_rates(&mut db, now).await.expect("checked")
}

async fn rates(db: &mut PgConnection, weeks: i32) -> OptOutRates {
    let week = Week::containing(midweek(weeks));
    weekly_optout_rates(db, week).await.expect("rates")
}

/// TD-CON-09 (CON-024, D-4082): a week is Monday 00:00 to Monday 00:00 Arizona time.
#[test]
fn td_con_09_a_week_runs_monday_to_monday_in_arizona_time() {
    let week = Week::containing(ts("2026-11-16T06:59:59Z"));
    assert_eq!(week.start(), ts(MONDAY), "Sunday 23:59:59 in Arizona");
    assert_eq!(week.end(), ts("2026-11-16T07:00:00Z"));
    assert_eq!(week.monday(), jiff::civil::date(2026, 11, 9));
    let next = Week::containing(ts("2026-11-16T07:00:00Z"));
    assert_eq!(next, week.next());
    assert_eq!(next.previous(), week);
    assert_eq!(Week::containing(midweek(0)), week);
}

/// TD-CON-09 (CON-024): the thresholds at their edges: 2.99% of homeowners no, 3.00% yes; 4.99%
/// of businesses no, 5.00% yes; nobody texted, no rate.
#[test]
fn td_con_09_the_rate_rule_at_its_edges() {
    let rate = |audience, texted, opted_out| WeekRate {
        audience,
        texted,
        opted_out,
    };
    assert!(!rate(Audience::Homeowner, 10_000, 299).alerts());
    assert!(rate(Audience::Homeowner, 10_000, 300).alerts());
    assert!(!rate(Audience::Business, 10_000, 499).alerts());
    assert!(rate(Audience::Business, 10_000, 500).alerts());
    assert!(
        !rate(Audience::Homeowner, 0, 3).alerts(),
        "no texted count, no rate"
    );
}

/// TD-CON-09 (CON-024): 299 of 10,000 texted homeowners (2.99%) raises nothing; the 300th
/// (3.00%) raises one Normal alert about that week, and a second check raises no other.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_09_homeowners_alert_at_3_00_not_2_99(pool: PgPool) {
    let vault = vault();
    sent(&pool, "homeowner", 10_000, midweek(0)).await;
    sent(&pool, "homeowner", 50, midweek(-1)).await;
    for n in 0..299 {
        event(&pool, "opt_out", &digest(&vault, n), None, midweek(0)).await;
    }
    event(&pool, "opt_out", &digest(&vault, 900), None, midweek(1)).await;
    let monday_after = ts(MONDAY) + WEEK + SignedDuration::from_mins(1);
    let rates = check(&pool, monday_after).await;
    assert_eq!(
        (rates.homeowners.texted, rates.homeowners.opted_out),
        (10_000, 299)
    );
    assert!(
        alerts(&pool, HOMEOWNER_OPTOUT_ALERT).await.is_empty(),
        "2.99%"
    );
    event(&pool, "opt_out", &digest(&vault, 299), None, midweek(0)).await;
    let rates = check(&pool, monday_after).await;
    assert_eq!(rates.homeowners.opted_out, 300);
    check(&pool, monday_after).await;
    let week = Week::containing(midweek(0));
    let expected = vec![(WEEK_SUBJECT.to_owned(), week.id(), "normal".to_owned())];
    assert_eq!(
        alerts(&pool, HOMEOWNER_OPTOUT_ALERT).await,
        expected,
        "3.00%, once"
    );
    assert!(alerts(&pool, BUSINESS_OPTOUT_ALERT).await.is_empty());
}

/// TD-CON-09 (CON-024, D-4082): a business opt-out is told apart by its `phone_stopped` row:
/// 4 of 100 texted businesses raise nothing, the 5th (5.00%) raises the business alert, and the
/// homeowners' count holds only the opt-out with no such row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_09_businesses_alert_at_5_00_told_apart_by_phone_stopped(pool: PgPool) {
    let vault = vault();
    sent(&pool, "business", 100, midweek(0)).await;
    sent(&pool, "homeowner", 1_000, midweek(0)).await;
    for n in 100..104 {
        business_stop(&pool, &digest(&vault, n), midweek(0)).await;
    }
    event(
        &pool,
        "opt_out",
        &digest(&vault, 150),
        Some(Uuid::new_v4()),
        midweek(0),
    )
    .await;
    let monday_after = ts(MONDAY) + WEEK;
    let rates = check(&pool, monday_after).await;
    assert_eq!(
        (rates.businesses.texted, rates.businesses.opted_out),
        (100, 4)
    );
    assert_eq!(
        (rates.homeowners.texted, rates.homeowners.opted_out),
        (1_000, 1)
    );
    assert!(alerts(&pool, BUSINESS_OPTOUT_ALERT).await.is_empty(), "4%");
    business_stop(&pool, &digest(&vault, 104), midweek(0)).await;
    let rates = check(&pool, monday_after).await;
    assert_eq!(rates.businesses.opted_out, 5);
    assert_eq!(alerts(&pool, BUSINESS_OPTOUT_ALERT).await.len(), 1, "5.00%");
    assert!(
        alerts(&pool, HOMEOWNER_OPTOUT_ALERT).await.is_empty(),
        "0.1%"
    );
}

/// TD-CON-09 (CON-024, D-4082): a repeat STOP from a listed number in the next week is not that
/// week's opt-out; STOP, START, STOP counts again; one number twice in a week counts once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_09_a_repeat_stop_is_not_counted_and_stop_start_stop_is(pool: PgPool) {
    let vault = vault();
    let (repeat, again, twice) = (
        digest(&vault, 101),
        digest(&vault, 102),
        digest(&vault, 103),
    );
    for number in [&repeat, &again] {
        event(&pool, "opt_out", number, None, midweek(0)).await;
    }
    event(&pool, "opt_out", &repeat, None, midweek(1)).await;
    event(&pool, "start", &again, None, midweek(1)).await;
    event(&pool, "opt_out", &again, None, midweek(1) + DAY).await;
    event(&pool, "opt_out", &twice, None, midweek(1)).await;
    event(&pool, "opt_out", &twice, None, midweek(1) + DAY).await;
    let mut db = pool.acquire().await.expect("connection");
    assert_eq!(
        rates(&mut db, 0).await.homeowners.opted_out,
        2,
        "both newly listed"
    );
    assert_eq!(
        rates(&mut db, 1).await.homeowners.opted_out,
        2,
        "STOP, START, STOP and one new number; the repeat STOP is not counted"
    );
}

/// TD-CON-09 (CON-022, CON-023): the opt-out log, the message log and the suppression list
/// outlast the body cut to the last day of the 5 years, the app cannot delete or change an
/// opt-out event, and no module but `messaging` reads the opt-out tables.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_09_records_are_kept_5_years_and_never_exported(pool: PgPool) {
    assert_eq!(CONSENT_KEEP_YEARS, 5);
    let vault = vault();
    let last = midweek(0);
    let number = digest(&vault, 120);
    event(&pool, "opt_out", &number, None, last).await;
    sent(&pool, "homeowner", 3, last).await;
    let nearly_five_years = last + DAY * (365 * 5);
    let mut db = pool.acquire().await.expect("connection");
    set_app_role(&mut db).await.expect("app role");
    cut_bodies(&mut db, nearly_five_years).await.expect("cut");
    let kept: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM messaging.optout_event), \
                (SELECT count(*) FROM messaging.outbox WHERE status = 'sent')",
    )
    .fetch_one(&pool)
    .await
    .expect("kept");
    assert_eq!(kept, (1, 3));
    let delete = sqlx::query("DELETE FROM messaging.optout_event")
        .execute(&mut *db)
        .await;
    assert!(is_permission_denied(&delete), "kept for the CON-022 period");
    let update = sqlx::query("UPDATE messaging.optout_event SET channel = 'owner'")
        .execute(&mut *db)
        .await;
    assert!(is_permission_denied(&update), "append-only for the app");
    assert_eq!(
        readers_outside_messaging(),
        Vec::<PathBuf>::new(),
        "CON-023"
    );
}

/// Every non-test Rust file under `app/` outside `app/messaging` that names an opt-out table.
fn readers_outside_messaging() -> Vec<PathBuf> {
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
    let mut files = Vec::new();
    rust_files(&app, &mut files);
    files
        .into_iter()
        .filter(|f| !f.starts_with(app.join("messaging")))
        .filter(|f| !f.components().any(|c| c.as_os_str() == "tests"))
        .filter(|f| {
            let text = fs::read_to_string(f).expect("source");
            text.contains("optout_event") || text.contains("messaging.suppression")
        })
        .collect()
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// TD-CON-09 (CON-024, D-3881): the weekly check is queued for the next Arizona Monday 00:00,
/// once however often it is scheduled.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_09_the_weekly_check_is_queued_once_per_week(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    for now in [midweek(0), midweek(0) + DAY] {
        schedule_optout_rate_check(&mut db, now)
            .await
            .expect("queued");
    }
    let runs: Vec<(String,)> = sqlx::query_as(
        "SELECT to_char(run_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI') FROM core.task \
         WHERE kind = $1",
    )
    .bind(OPTOUT_RATE_CHECK_KIND)
    .fetch_all(&pool)
    .await
    .expect("tasks");
    assert_eq!(runs, vec![("2026-11-16T07:00".to_owned(),)]);
}
