//! The worker's daily ROC and city license lists (TD-LIC-28: LIC-006, LIC-007, LIC-008, LIC-010,
//! LIC-039, CARD-015; T371, D-2704, D-2711, D-942).
//!
//! The `licensing.daily_lists` task runs through `run_daily_list` on an advanced clock over the
//! `file://` license source (`LicenseLists`) reading fixture lists named `roc` and `city` (D-2954),
//! with the active businesses from `businesses::api::active_license_numbers`, as the worker wires
//! them. Proves: the day's lists are recorded once and a re-run adds nothing; a list missing for
//! 12 hours raises the listed alert once, on the hourly re-run in the hour it falls due, and the
//! push alert once at 24 hours; while it fails each active business gets one lookup a day, at
//! least the throttle apart, never more than the batch cap a run, the rest on the next runs; a
//! list never recorded carries its failure start in the payload; the next good list ends the
//! failure. Fake data only: invented license numbers, no names, phones or addresses.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use askhava_adapters::api::LicenseLists;
use askhava_businesses::api::{active_license_numbers, create};
use askhava_core::api::{Claimed, Lease, SettingsError, WorkerId, claim, finish};
use askhava_licensing::api::{
    ActiveLicenses, CITY_HEADER, DAILY_LISTS_KIND, DailyList, DailyListSettings, FailureLimits,
    ImportError, LIST_HOUR_KEY, LIST_RETRY_KEY, LOOKUP_BATCH_KEY, ListFailureAlert, ListRun,
    ROC_HEADER, check_license, latest_check_active, run_daily_list, schedule_daily_lists,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The numbers of the three active businesses and of one still pending.
const ACTIVE: [&str; 3] = ["ROC937101", "ROC937102", "ROC937103"];
const PENDING: &str = "ROC937104";

/// The composition's active businesses, as `server` wires them.
struct Active;

impl ActiveLicenses for Active {
    async fn active_after(
        &self,
        conn: &mut PgConnection,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<(Uuid, String)>, ImportError> {
        let found = active_license_numbers(conn, after, limit).await;
        found.map_err(|_| ImportError::Database)
    }
}

/// The run's settings: a batch of 2 and a 100 ms throttle, the rest the defaults.
fn settings() -> DailyListSettings {
    let limits = FailureLimits {
        lookup_spacing: SignedDuration::from_millis(100),
        ..FailureLimits::default()
    };
    DailyListSettings {
        batch: 2,
        limits,
        ..DailyListSettings::default()
    }
}

fn at(s: &str) -> Timestamp {
    s.parse().expect("fake time")
}

fn hour(start: Timestamp, n: i64) -> Timestamp {
    start + SignedDuration::from_hours(n)
}

/// A fixture folder holding the ROC and city lists and a lookup answer per number (Suspended).
fn fixtures() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("t371-{}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("lookup")).expect("fixture dir");
    let mut roc = format!("{ROC_HEADER}\n");
    for number in ACTIVE.iter().chain([&PENDING]) {
        roc.push_str(&format!("{number},R-11,Active,2015-01-01\n"));
        let answer = format!("{ROC_HEADER}\n{number},R-11,Suspended,\n");
        std::fs::write(dir.join("lookup").join(number), answer).expect("lookup fixture");
    }
    std::fs::write(dir.join("roc.keep"), &roc).expect("roc copy");
    std::fs::write(dir.join("roc"), roc).expect("roc fixture");
    let city = format!("{CITY_HEADER}\nCL-1001,Fake Pools,,,2019-01-01\nCL-1002,Fake Vents,,,\n");
    std::fs::write(dir.join("city"), city).expect("city fixture");
    dir
}

fn source(dir: &std::path::Path) -> LicenseLists {
    LicenseLists::new(&format!("file://{}", dir.display())).expect("file source")
}

/// Claims every list run due at `now`, runs it and finishes it, as the worker does; by list.
async fn run_due(pool: &PgPool, source: &LicenseLists, now: Timestamp) -> Vec<ListRun> {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, now, Lease::WORKER.length, 10).await;
    let mut runs = Vec::new();
    for task in tasks.expect("claim") {
        assert_eq!(task.kind, DAILY_LISTS_KIND);
        let run = run_daily_list(pool, (source, &Active), &task, (now, settings())).await;
        runs.push(run.expect("run"));
        assert!(finish(pool, owner, &task, now).await.expect("finish"));
    }
    runs.sort_by_key(|run| run.list.name());
    runs
}

fn of(runs: &[ListRun], list: DailyList) -> Option<&ListRun> {
    runs.iter().find(|run| run.list == list)
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// Seeds the runs before 6 a.m. Arizona time on November 2 and records both lists at 6 a.m.
async fn first_day(pool: &PgPool, source: &LicenseLists) -> Timestamp {
    let mut conn = pool.acquire().await.expect("connection");
    for _ in 0..2 {
        let seeded = schedule_daily_lists(&mut conn, at("2026-11-02T12:00:00Z"), settings()).await;
        seeded.expect("seeded");
    }
    let six = at("2026-11-02T13:00:00Z");
    assert!(
        run_due(pool, source, six - SignedDuration::from_mins(1))
            .await
            .is_empty()
    );
    let runs = run_due(pool, source, six).await;
    assert_eq!(runs.len(), 2, "one run per list, seeded once");
    six
}

/// One business bound to `number` on the recorded ROC list, made active unless `pending`.
async fn bound(pool: &PgPool, number: &str, pending: bool, now: Timestamp) -> Uuid {
    let mut conn = pool.acquire().await.expect("connection");
    let id = create(&mut conn, false, now).await.expect("business");
    check_license(&mut conn, id, number).await.expect("checked");
    if !pending {
        sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
            .bind(id)
            .execute(&mut *conn)
            .await
            .expect("active");
    }
    id
}

/// TD-LIC-28: the day's ROC and city lists are recorded once, the next run is the next day's at
/// the run hour, and a second run for the same list date records nothing new.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_28_the_days_lists_are_recorded_once(pool: PgPool) {
    let dir = fixtures();
    let lists = source(&dir);
    let six = first_day(&pool, &lists).await;
    let rows = "SELECT count(*) FROM licensing.agency_record";
    let (imports, recorded) = (
        count(&pool, "SELECT count(*) FROM licensing.list_import").await,
        count(&pool, rows).await,
    );
    assert_eq!((imports, recorded), (2, 4));
    let rerun = Claimed {
        id: 0,
        kind: DAILY_LISTS_KIND.to_owned(),
        payload: json!({"list": "roc"}),
        attempts: 1,
    };
    let later = hour(six, 3);
    let run = run_daily_list(&pool, (&lists, &Active), &rerun, (later, settings())).await;
    let run = run.expect("re-run");
    assert!(run.recorded.expect("recorded before").already_imported);
    assert_eq!(run.next_run, at("2026-11-03T13:00:00Z"));
    assert_eq!(count(&pool, rows).await, 4, "a re-run adds nothing");
    let queued = "SELECT count(*) FROM core.task WHERE kind = 'licensing.daily_lists' \
                  AND done_at IS NULL AND run_at = '2026-11-03T13:00:00Z'";
    assert_eq!(
        count(&pool, queued).await,
        2,
        "tomorrow's run once per list"
    );
}

/// Runs the hourly re-runs from `start` for `hours` hours; returns the ROC runs.
async fn hourly(pool: &PgPool, lists: &LicenseLists, start: Timestamp, hours: i64) -> Vec<ListRun> {
    let mut roc = Vec::new();
    for n in 0..hours {
        let now = hour(start, n);
        let began = Instant::now();
        let runs = run_due(pool, lists, now).await;
        let run = of(&runs, DailyList::Roc)
            .expect("the failing list runs every hour")
            .clone();
        assert!(run.recorded.is_none() && run.next_run == hour(now, 1));
        assert!(run.lookups <= 2, "never more than the batch cap a run");
        if run.lookups == 2 {
            let took = SignedDuration::try_from(began.elapsed()).expect("elapsed");
            assert!(took >= SignedDuration::from_millis(100), "the throttle");
        }
        roc.push(run);
    }
    roc
}

/// TD-LIC-28: a ROC list missing from 6 a.m. raises the listed alert once at 12 hours, in that
/// hour's re-run, and the push alert once at 24; each active business is looked up once a day in
/// capped, throttled batches and its status follows; the next good list ends the failure.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_28_a_missing_list_alerts_once_and_looks_up_in_capped_batches(pool: PgPool) {
    let dir = fixtures();
    let lists = source(&dir);
    let six = first_day(&pool, &lists).await;
    let mut ids = Vec::new();
    for number in ACTIVE {
        ids.push(bound(&pool, number, false, six).await);
    }
    bound(&pool, PENDING, true, six).await;
    std::fs::remove_file(dir.join("roc")).expect("the list goes missing");
    let failing = hour(six, 24);
    let roc = hourly(&pool, &lists, failing, 25).await;
    let lookups: Vec<usize> = roc.iter().map(|run| run.lookups).collect();
    let mut want = vec![0; 25];
    (want[0], want[1], want[18], want[19]) = (2, 1, 2, 1);
    assert_eq!(
        lookups, want,
        "each active business once an Arizona day, the rest next run"
    );
    let raised: Vec<(usize, Vec<ListFailureAlert>)> = roc
        .iter()
        .enumerate()
        .filter(|(_, run)| !run.alerts.is_empty())
        .map(|(n, run)| (n, run.alerts.clone()))
        .collect();
    let (listed, still) = (ListFailureAlert::Listed, ListFailureAlert::StillFailing);
    assert_eq!(raised, [(12, vec![listed]), (24, vec![still])]);
    let alerts = "SELECT count(*) FROM ops.alert WHERE kind LIKE 'licensing.list_%' \
                  AND severity = 'normal'";
    assert_eq!(count(&pool, alerts).await, 2, "each alert once, never high");
    let mut conn = pool.acquire().await.expect("connection");
    for id in ids {
        let active = latest_check_active(&mut conn, id).await.expect("check");
        assert_eq!(
            active,
            Some(false),
            "the lookup updated the verified status"
        );
    }
    assert_good_list_ends_the_failure(&pool, &lists, &dir, hour(failing, 25)).await;
}

/// The list is back at `now`: recorded, no alert, the next run daily; a miss the next morning
/// starts a new failure, whose listed alert waits 12 hours again.
async fn assert_good_list_ends_the_failure(
    pool: &PgPool,
    lists: &LicenseLists,
    dir: &std::path::Path,
    now: Timestamp,
) {
    std::fs::copy(dir.join("roc.keep"), dir.join("roc")).expect("the list is back");
    let runs = run_due(pool, lists, now).await;
    let run = of(&runs, DailyList::Roc).expect("the hourly re-run");
    assert!(!run.recorded.expect("recorded").already_imported);
    assert!(run.alerts.is_empty() && run.lookups == 0);
    let next = at("2026-11-05T13:00:00Z");
    assert_eq!(run.next_run, next, "daily again");
    std::fs::remove_file(dir.join("roc")).expect("missing again");
    let roc = hourly(pool, lists, next, 13).await;
    let raised: Vec<usize> = (0..13).filter(|&n| !roc[n].alerts.is_empty()).collect();
    assert_eq!(raised, [12], "a new failure counts from its own start");
    assert_eq!(roc[12].alerts, [ListFailureAlert::Listed]);
}

/// TD-LIC-28: a list never recorded carries its failure start in the payload: the listed alert
/// once at 12 hours, the push alert once at 24, and the recorded ROC list runs daily meanwhile.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_28_a_list_never_recorded_alerts_from_its_first_failure(pool: PgPool) {
    let dir = fixtures();
    std::fs::remove_file(dir.join("city")).expect("no city list yet");
    let lists = source(&dir);
    let mut conn = pool.acquire().await.expect("connection");
    let six = at("2026-11-02T13:00:00Z");
    schedule_daily_lists(&mut conn, hour(six, -1), settings())
        .await
        .expect("seeded");
    let mut raised = Vec::new();
    let mut roc_runs = 0;
    for n in 0..=24 {
        let runs = run_due(&pool, &lists, hour(six, n)).await;
        let city = of(&runs, DailyList::City).expect("the failing list runs every hour");
        assert!(
            city.recorded.is_none() && city.lookups == 0,
            "no city lookups"
        );
        if !city.alerts.is_empty() {
            raised.push((n, city.alerts.clone()));
        }
        roc_runs += usize::from(of(&runs, DailyList::Roc).is_some());
    }
    let (listed, still) = (ListFailureAlert::Listed, ListFailureAlert::StillFailing);
    assert_eq!(raised, [(12, vec![listed]), (24, vec![still])]);
    assert_eq!(roc_runs, 2, "the good list runs once a day");
}

/// TD-LIC-28: the run hour, the re-run cadence and the batch cap are settings, each refused by key
/// out of range; an empty or unknown key keeps the defaults.
#[test]
fn td_lic_28_the_run_settings_are_read_by_key() {
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    let read = |list: &[(&str, &str)]| DailyListSettings::from_pairs(pairs(list));
    let empty = [(LIST_HOUR_KEY, " "), ("ASKHAVA_OTHER", "1")];
    assert_eq!(read(&empty), Ok(DailyListSettings::default()));
    let set = read(&[
        (LIST_HOUR_KEY, "5"),
        (LIST_RETRY_KEY, "30"),
        (LOOKUP_BATCH_KEY, "9"),
    ]);
    let set = set.expect("in range");
    assert_eq!(set.run_hour, 5);
    assert_eq!(set.retry_every, SignedDuration::from_mins(30));
    assert_eq!(set.batch, 9);
    for (key, bad) in [
        (LIST_HOUR_KEY, "24"),
        (LIST_RETRY_KEY, "4"),
        (LOOKUP_BATCH_KEY, "0"),
    ] {
        assert_eq!(read(&[(key, bad)]), Err(SettingsError::Invalid(key)));
    }
    assert_eq!(
        read(&[(LIST_RETRY_KEY, "soon")]),
        Err(SettingsError::Invalid(LIST_RETRY_KEY))
    );
}

/// TD-LIC-28: a list with a changed header is a failing list; a retried run (same payload, its next
/// slot already queued) raises nothing twice; a payload naming no list is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_28_a_refused_list_fails_and_a_retry_raises_nothing_twice(pool: PgPool) {
    let dir = fixtures();
    std::fs::write(dir.join("roc"), "number,kind\nROC937101,R-11\n").expect("changed layout");
    let lists = source(&dir);
    let now = at("2026-11-03T01:00:00Z");
    let task = |payload| Claimed {
        id: 0,
        kind: DAILY_LISTS_KIND.to_owned(),
        payload,
        attempts: 1,
    };
    let failing = task(json!({
        "list": "roc",
        "failing_since": "2026-11-02T13:00:00Z",
        "last_run": "2026-11-03T00:00:00Z",
    }));
    let mut alerts = Vec::new();
    for _ in 0..2 {
        let run = run_daily_list(&pool, (&lists, &Active), &failing, (now, settings())).await;
        let run = run.expect("a failing list is not a handler error");
        assert!(run.recorded.is_none());
        alerts.push(run.alerts);
    }
    assert_eq!(alerts, [vec![ListFailureAlert::Listed], vec![]]);
    let nothing = task(json!({"list": "state"}));
    let refused = run_daily_list(&pool, (&lists, &Active), &nothing, (now, settings())).await;
    assert!(refused.is_err(), "no such list");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM licensing.list_import").await,
        0
    );
}
