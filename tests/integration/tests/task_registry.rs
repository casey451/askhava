//! Every task kind the app can queue has a handler in the production worker or a named owner, and
//! a no-handler dead letter alerts (T327, D-2285; TD-OPS-36 for JOB-006, TD-OPS-37 for OPS-004;
//! IC-27).
//!
//! TD-OPS-36 builds the registry exactly as the `worker` binary does,
//! `askhava_server::worker_registry` (which calls `register_handlers` and adds the worker-only
//! registrations), with stand-in settings that turn on every conditional registration (the vault
//! keys for `jobs.homeowner_text`, the Twilio settings for `picks.line_retry` and the body
//! deletes). It then scans the non-test Rust under `app/` (files under a `tests/` folder and
//! a file's top-level `#[cfg(test)] mod` items, through their closing brace, are skipped) for every
//! `const` or `static <NAME>_KIND: &str` and every string literal passed to `NewTask::new`, less
//! [`NOT_TASK_KINDS`]. Each found kind must
//! be registered or listed in `tests/task_kinds_deferred.json` with its owner task and reason; a
//! listed kind that is registered passes and is printed as stale. The scan must find
//! [`FLOOR`], so a scan that finds nothing fails. Outside the scan: a kind held in a constant not
//! named `*_KIND` and handed to `NewTask::new` through a helper; T327 found none (every dotted
//! constant not named `*_KIND` is an alert, audit action or subject), and a new one must be renamed
//! `*_KIND`.
//!
//! The layer table lets a test crate depend on modules but not on `askhava-server` (D-736), so the
//! composition root is compiled in place below, as the off-database stream and inbound route tests
//! do with their files.
//!
//! TD-OPS-37 dead-letters a task of a kind nothing registers through the production registry, then
//! runs the hourly check on the clock: one High `core.task_no_handler` alert naming the kind (its
//! subject) in the test and live environments, nothing new for a second dead letter while it is
//! open, nothing in dev and ci. All data is fake.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{
    Clock, Environment, Lease, ManualClock, NO_HANDLER, NewTask, Outcome, Registry, Settings,
    WorkerId, claim, enqueue,
};
use askhava_picks::api::ConfirmSettings;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the worker registry and the no-handler check are called here"
)]
mod server;
use server::*;

/// Constants named `*_KIND` that are not task kinds: each value and why.
const NOT_TASK_KINDS: [(&str, &str); 3] = [
    (
        "calls.number_pool",
        "an audit subject kind for a pool number",
    ),
    (
        "privacy.contact",
        "an audit subject kind for a vault contact",
    ),
    (
        "payments.fee_mode",
        "the audit subject kind of the fee mode switch",
    ),
];

/// Kinds the scan must find, or it is broken.
const FLOOR: [&str; 4] = [
    "payments.charge_check",
    "payments.reconcile",
    "intake.no_reply",
    "calls.safety_call",
];

/// A kind no code queues or registers, for the dead-letter cases.
const PLANTED: &str = "test.unhandled_kind";
/// Ten distinct fake numbers in the 555-01xx fiction range.
const NUMBERS: &str = "+19285550100,+19285550101,+19285550102,+19285550103,+19285550104,\
+19285550105,+19285550106,+19285550107,+19285550108,+19285550109";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Stand-in settings in dev that turn every conditional registration on.
fn stand_in_pairs(storage: &Path) -> Vec<(String, String)> {
    let base = format!("file://{}", storage.display());
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".into(),
        ),
        ("ASKHAVA_STORAGE_BASE_URL", base),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-not-a-real-key-000".into(),
        ),
        ("VAULT_KEY", format!("1:{}=", "A".repeat(43))),
        ("VAULT_INDEX_KEY", format!("1:{}BAQ=", "BAQE".repeat(10))),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000".into(),
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token".into()),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001".into(),
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "10".into()),
        ("ASKHAVA_MASKED_NUMBERS", NUMBERS.into()),
    ];
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// The registry the `worker` binary runs, built by the same function from stand-in settings.
fn production_registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t327-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let pairs = stand_in_pairs(&dir);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    let texts = job_text_keys(&settings, &stream.id_secret, pairs.clone())
        .expect("keys")
        .map(Arc::new);
    let parts = (texts, ConfirmSettings::default());
    worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// The registered kinds, read from the registry's `Debug`, which lists them as a set.
fn registered(registry: &Registry) -> BTreeSet<String> {
    let listed = format!("{registry:?}");
    listed
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Every `.rs` file under `dir`, skipping `tests` folders and build output.
fn source_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read dir");
    for path in entries.map(|e| e.expect("entry").path()) {
        let name = path.file_name().unwrap_or_default();
        if path.is_dir() && name != "tests" && name != "target" {
            source_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The first string literal in `text`, if any.
fn first_literal(text: &str) -> Option<&str> {
    let start = text.find('"')? + 1;
    let len = text[start..].find('"')?;
    Some(&text[start..start + len])
}

/// `source` without its top-level test modules (rustfmt's layout, which `cargo fmt --check`
/// holds): a `#[cfg(test)]` line then `mod <name>;` drops those two lines, then `mod <name> {`
/// drops through its closing `}` at column 0. Everything else is kept, before or after.
fn non_test_code(source: &str) -> String {
    let mut kept = String::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        let test_mod = line == "#[cfg(test)]"
            && lines.peek().is_some_and(|next| {
                let next = next
                    .trim_start_matches("pub ")
                    .trim_start_matches("pub(crate) ");
                next.starts_with("mod ")
            });
        if !test_mod {
            kept.push_str(line);
            kept.push('\n');
            continue;
        }
        let head = lines.next().unwrap_or_default();
        if head.ends_with('{') {
            lines.by_ref().find(|l| *l == "}");
        }
    }
    kept
}

/// The kinds in one file's non-test code: `*_KIND` constants and statics and literals given to
/// `NewTask::new`.
fn kinds_in(source: &str) -> BTreeSet<String> {
    let code = non_test_code(source);
    let code = code.as_str();
    let mut kinds = BTreeSet::new();
    for keyword in ["const ", "static "] {
        for (at, _) in code.match_indices(keyword) {
            let item = &code[at + keyword.len()..];
            let item = &item[..item.find(';').unwrap_or(item.len())];
            let name = item.split(':').next().unwrap_or_default().trim();
            if name.ends_with("_KIND") && item.contains("str") {
                kinds.extend(first_literal(item).map(str::to_owned));
            }
        }
    }
    for (at, _) in code.match_indices("NewTask::new(") {
        let rest = code[at + 13..].trim_start();
        if rest.starts_with('"') {
            kinds.extend(first_literal(rest).map(str::to_owned));
        }
    }
    kinds
}

/// Every kind the app's non-test code can queue, less [`NOT_TASK_KINDS`].
fn queued_kinds() -> BTreeSet<String> {
    let mut files = Vec::new();
    source_files(&root().join("app"), &mut files);
    let mut kinds = BTreeSet::new();
    for file in files {
        kinds.extend(kinds_in(&std::fs::read_to_string(&file).expect("source")));
    }
    for (kind, _) in NOT_TASK_KINDS {
        kinds.remove(kind);
    }
    kinds
}

/// One entry of `tests/task_kinds_deferred.json`.
struct Deferred {
    kind: String,
    owner: String,
    reason: String,
}

fn deferred_list(file: &Value) -> Vec<Deferred> {
    let field = |e: &Value, k: &str| e[k].as_str().unwrap_or_default().trim().to_owned();
    let entries = file["deferred"].as_array().expect("a `deferred` list");
    let entry = |e: &Value| Deferred {
        kind: field(e, "kind"),
        owner: field(e, "owner"),
        reason: field(e, "reason"),
    };
    entries.iter().map(entry).collect()
}

/// A task ID: `T` and three digits.
fn is_task_id(owner: &str) -> bool {
    owner.len() == 4 && owner.starts_with('T') && owner[1..].chars().all(|c| c.is_ascii_digit())
}

/// TD-OPS-36's verdict: the problems (a queued kind with neither handler nor entry, an entry
/// without owner or reason) and the stale entries (deferred but registered).
fn verdict(
    queued: &BTreeSet<String>,
    registered: &BTreeSet<String>,
    deferred: &[Deferred],
) -> (Vec<String>, Vec<String>) {
    let mut problems = Vec::new();
    for entry in deferred {
        if !is_task_id(&entry.owner) || entry.reason.is_empty() {
            problems.push(format!(
                "`{}`: deferred without owner task ID and reason",
                entry.kind
            ));
        }
    }
    let listed: BTreeSet<&str> = deferred.iter().map(|d| d.kind.as_str()).collect();
    for kind in queued {
        if !registered.contains(kind) && !listed.contains(kind.as_str()) {
            problems.push(format!("`{kind}`: queued, no handler, not deferred"));
        }
    }
    let stale = listed.into_iter().filter(|k| registered.contains(*k));
    (problems, stale.map(str::to_owned).collect())
}

fn read_deferred() -> Vec<Deferred> {
    let path = root().join("tests/task_kinds_deferred.json");
    let text = std::fs::read_to_string(path).expect("deferred list");
    deferred_list(&serde_json::from_str(&text).expect("deferred JSON"))
}

#[test]
fn td_ops_36_every_queued_kind_has_a_handler_or_a_named_owner() {
    let queued = queued_kinds();
    for kind in FLOOR {
        assert!(
            queued.contains(kind),
            "the scan missed `{kind}`: {queued:?}"
        );
    }
    let registered = registered(&production_registry(Arc::new(ManualClock::new(
        Timestamp::UNIX_EPOCH,
    ))));
    for kind in [
        "picks.line_retry",
        "jobs.homeowner_text",
        "messaging.paused_safety",
    ] {
        assert!(
            registered.contains(kind),
            "`{kind}` not registered: {registered:?}"
        );
    }
    let (problems, stale) = verdict(&queued, &registered, &read_deferred());
    for kind in &stale {
        println!("stale deferred entry, now registered: `{kind}`");
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn td_ops_36_a_planted_kind_or_an_ownerless_entry_fails_and_a_registered_one_is_stale() {
    let set = |kinds: &[&str]| {
        kinds
            .iter()
            .map(|k| (*k).to_owned())
            .collect::<BTreeSet<_>>()
    };
    let entry = |kind: &str, owner: &str, reason: &str| Deferred {
        kind: kind.into(),
        owner: owner.into(),
        reason: reason.into(),
    };
    let found = kinds_in(
        "pub const PLANT_KIND: &str =\n    \"test.planted\";\nlet t = NewTask::new( \"test.literal\", p, at);\n#[cfg(test)]\nmod tests { const X_KIND: &str = \"test.in_tests\"; }",
    );
    assert_eq!(found, set(&["test.literal", "test.planted"]));
    let after = kinds_in(
        "#[cfg(test)]\nmod tests;\nconst A_KIND: &str = \"test.after_file_mod\";\n#[cfg(test)]\nmod more {\n    const B_KIND: &str = \"test.in_block\";\n    fn f() {\n    }\n}\npub static C_KIND: &str = \"test.static\";\nNewTask::new(\"test.after_block\", p, at);\n",
    );
    assert_eq!(
        after,
        set(&["test.after_block", "test.after_file_mod", "test.static"]),
        "only the test module's own lines are skipped, and statics count"
    );
    let (problems, _) = verdict(&found, &set(&["test.literal"]), &[]);
    assert_eq!(
        problems,
        ["`test.planted`: queued, no handler, not deferred"]
    );
    let listed = [
        entry("test.planted", "", "a reason"),
        entry("test.literal", "T999", ""),
    ];
    let (problems, stale) = verdict(&found, &set(&["test.literal"]), &listed);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert_eq!(stale, ["test.literal"]);
    let (problems, _) = verdict(&set(&[]), &set(&[]), &[]);
    assert!(problems.is_empty());
    assert!(
        !FLOOR.iter().all(|k| set(&[]).contains(*k)),
        "an empty scan misses the floor"
    );
}

/// Queues one [`PLANTED`] task with one attempt and runs it through `registry`: dead-lettered.
async fn dead_letter(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let mut task = NewTask::new(
        PLANTED,
        json!({"job": Uuid::from_u128(0x7e57_0327)}),
        clock.now(),
    );
    task.max_attempts = 1;
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &task).await.expect("enqueue");
    drop(conn);
    run_due(pool, registry, clock).await;
    let dead: Option<String> = sqlx::query_scalar(
        "SELECT last_error FROM core.task WHERE kind = $1 AND dead_at IS NOT NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(PLANTED)
    .fetch_optional(pool)
    .await
    .expect("dead letter");
    assert_eq!(dead.as_deref(), Some(NO_HANDLER));
}

/// Claims and runs every task due on `clock`.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("owner");
    let tasks = claim(pool, owner, clock.now(), Lease::WORKER.length, 10)
        .await
        .expect("claim");
    for task in tasks {
        let outcome = registry.run(pool, clock, owner, task, Lease::WORKER).await;
        assert!(matches!(
            outcome,
            Ok(Outcome::Done | Outcome::Retry(_) | Outcome::Dead)
        ));
    }
}

/// The open `core.task_no_handler` alerts: severity, subject kind and subject ID.
async fn alerts(pool: &PgPool) -> Vec<(String, String, Uuid)> {
    sqlx::query_as(
        "SELECT severity, subject_kind, subject_id FROM ops.alert WHERE kind = $1 AND resolved_at IS NULL",
    )
    .bind(no_handler_alert::ALERT)
    .fetch_all(pool)
    .await
    .expect("alerts")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_37_a_no_handler_dead_letter_raises_one_high_alert_per_kind(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(
        "2026-11-02T17:00:00Z".parse().expect("time"),
    ));
    let production = production_registry(clock.clone());
    dead_letter(&pool, &production, &clock).await;
    let mut checks = Registry::new();
    no_handler_alert::register(&mut checks, Environment::Test, clock.clone());
    let mut conn = pool.acquire().await.expect("conn");
    no_handler_alert::schedule(&mut conn, clock.now())
        .await
        .expect("seed");
    no_handler_alert::schedule(&mut conn, clock.now())
        .await
        .expect("seed again");
    drop(conn);
    run_due(&pool, &checks, &clock).await;
    let one = vec![(
        "high".to_owned(),
        PLANTED.to_owned(),
        no_handler_alert::kind_id(PLANTED),
    )];
    assert_eq!(alerts(&pool).await, one, "one High alert naming the kind");
    dead_letter(&pool, &production, &clock).await;
    clock.advance(SignedDuration::from_hours(1));
    run_due(&pool, &checks, &clock).await;
    assert_eq!(
        alerts(&pool).await,
        one,
        "nothing new while the alert is open"
    );
    let checks_queued: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(no_handler_alert::CHECK_KIND)
        .fetch_one(&pool)
        .await
        .expect("checks");
    assert_eq!(
        checks_queued, 3,
        "the seed, then one per run for the next hour"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_37_dev_and_ci_raise_nothing_and_live_does(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(
        "2026-11-02T17:00:00Z".parse().expect("time"),
    ));
    dead_letter(&pool, &production_registry(clock.clone()), &clock).await;
    for environment in [Environment::Dev, Environment::Ci] {
        no_handler_alert::run_check(&pool, environment, clock.now())
            .await
            .expect("check");
    }
    assert!(
        alerts(&pool).await.is_empty(),
        "nothing raised in dev and ci"
    );
    no_handler_alert::run_check(&pool, Environment::Live, clock.now())
        .await
        .expect("check");
    let raised = alerts(&pool).await;
    assert_eq!(raised.len(), 1, "{raised:?}");
    let row: Value = sqlx::query_scalar("SELECT to_jsonb(a) FROM ops.alert a WHERE kind = $1")
        .bind(no_handler_alert::ALERT)
        .fetch_one(&pool)
        .await
        .expect("row");
    assert!(
        !row.to_string().contains("7e570327"),
        "no payload in the alert: {row}"
    );
}

/// How many checks run at once in the race case: the worker's own limit.
const AT_ONCE: i64 = 4;

/// Waits until `count` lock requests (table or advisory) are waiting, or panics after 30 seconds.
async fn wait_for_waiters(pool: &PgPool, count: i64) {
    for _ in 0..600 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks WHERE NOT granted AND locktype IN ('relation', 'advisory') \
             AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
        )
        .fetch_one(pool)
        .await
        .expect("pg_locks");
        if waiting >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the checks never reached the alert write");
}

/// D-2852: the checks race to the alert write. A held table lock lets every check read "no open
/// alert" yet holds each insert, so without the advisory lock each would raise one when released;
/// with it, the first holds the lock at its insert and the others wait before their read.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_37_checks_at_once_raise_one_alert(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(
        "2026-11-02T17:00:00Z".parse().expect("time"),
    ));
    dead_letter(&pool, &production_registry(clock.clone()), &clock).await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(u32::try_from(AT_ONCE).expect("small") + 2)
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("a pool for every check, the hold and the watch");
    let mut hold = pool.begin().await.expect("begin");
    sqlx::query("LOCK TABLE ops.alert IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *hold)
        .await
        .expect("table lock");
    let checks: Vec<_> = (0..AT_ONCE)
        .map(|_| {
            let pool = pool.clone();
            let now = clock.now();
            tokio::spawn(
                async move { no_handler_alert::run_check(&pool, Environment::Test, now).await },
            )
        })
        .collect();
    wait_for_waiters(&pool, AT_ONCE).await;
    hold.commit().await.expect("release");
    for check in checks {
        check.await.expect("joined").expect("check");
    }
    let raised = alerts(&pool).await;
    assert_eq!(raised.len(), 1, "one alert from checks at once: {raised:?}");
}
