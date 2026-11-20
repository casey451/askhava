//! TD-OPS-04, the local drill (T206; OPS-013, OPS-018, OPS-019; D-3889, D-4932): T109's nightly
//! export is taken as the snapshot, a text goes out afterwards at the Twilio stand-in, the test
//! decrypts the stored object with its own fake `age` identity (the host holds none) and hands
//! the plain dump to `run_drill`, which restores it into a fresh database and replays: Stripe,
//! down at first, is waited for; the text is found in the sent log and not sent again; nothing
//! is charged; the charge no pick holds is flagged and alerted; the drill is recorded. Fake data.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use askhava_adapters::api::{
    AdapterError, IdempotencyKey, LocalStorage, SmsAdapter, SmsSentLog, StorageAdapter,
    TwilioConfig, TwilioSms, twilio_clients,
};
use askhava_core::api::{Lease, Outcome, Registry, Settings, SystemClock, WorkerId, claim};
use askhava_messaging::api::{
    LaunchChannel, Line, Lines, NewText, Relation, SendingNumber, enqueue,
};
use askhava_ops::api::digest_alerts;
use askhava_payments::api::{ChargePage, StripeCharge};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_recovery::api::{
    ChargeSource, Drill, EXPORT_KIND, ItemKind, REPLAY_LISTED_ALERT, ReplaySources, Retry,
    register_export, replay, run_drill, schedule_export,
};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool};

const FROM: &str = "+15005550006";
const HER: &str = "+15005550142";
const BODY: &str = "Fake quote one";
const SECRET: &str = "fake-id-secret-for-tests-only-000000";
const QUICK: Retry = Retry {
    tries: 2,
    wait: Duration::from_millis(5),
};

/// The stand-in server on a port it picked itself, killed on drop.
struct StandIn(Child, String);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _killed = self.0.kill();
        let _reaped = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 stand-in");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("listening line");
    let base = line.trim_end().strip_prefix("stand-ins listening on ");
    StandIn(child, base.expect("listening").to_owned())
}

/// The settings both the export and the texting client read: `extra` over the fake Twilio keys.
fn pairs(base: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let api = format!("{base}/twilio");
    let pool: Vec<String> = (0..10).map(|i| format!("+1500555{:04}", 100 + i)).collect();
    let fixed = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", &pool.join(",")),
        ("ASKHAVA_TWILIO_BASE_URL", &api),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &api),
    ];
    let all = fixed.iter().chain(extra);
    all.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn vault() -> Vault {
    let (a, b) = ("UVFR".repeat(10) + "UVE=", "YWFh".repeat(10) + "YWE=");
    let url = "postgres://askhava_app@db.invalid/askhava".to_owned();
    Vault::new(
        &format!("1:{a}").into(),
        &format!("1:{b}").into(),
        &url.into(),
    )
    .expect("vault")
}

/// Stripe's list: unreachable for the first two calls, then one charge no pick holds.
struct Stripe(AtomicU32);

impl ChargeSource for Stripe {
    async fn list_page(
        &self,
        _: Timestamp,
        _: Option<&str>,
        _: usize,
    ) -> Result<ChargePage, AdapterError> {
        if self.0.fetch_add(1, Ordering::SeqCst) < 2 {
            return Err(AdapterError::NotSent);
        }
        let charge = StripeCharge {
            id: "ch_fakeunmatched".to_owned(),
            payment_intent: "pi_fake1".to_owned(),
            pick: Some(Uuid::from_u128(9)),
            amount_cents: 7500,
            refunded_cents: 0,
            dispute: None,
            balance_transaction: String::new(),
            fee_cents: 0,
        };
        Ok(ChargePage {
            charges: vec![charge],
            next: None,
        })
    }
}

/// One text to a fake homeowner, queued and not yet sent: the outbox as the snapshot holds it.
async fn queue_text(pool: &PgPool) {
    let new = NewContact {
        name: Some("Fake Homeowner".to_owned().into()),
        phone: Some(HER.to_owned().into()),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let mut db = pool.acquire().await.expect("connection");
    let who = store_contact(&mut db, &vault(), &new).await;
    let text = NewText {
        template_id: "first_quote",
        subject: &[Uuid::from_u128(1)],
        recipient: who.expect("contact"),
        relation: Relation::OpenRequest,
        body: BODY,
    };
    let queued = enqueue(&mut db, &SECRET.to_owned().into(), &text).await;
    queued.expect("queued");
}

/// The worker's registry with T109's export handler over `store`, encrypting to `recipient`.
fn export_registry(url: &str, store: &Arc<LocalStorage>, recipient: &str) -> Registry {
    let keys = [
        ("ASKHAVA_EXPORT_RECIPIENT", recipient),
        ("ASKHAVA_BACKUP_DATABASE_URL", url),
    ];
    let settings = Settings::from_pairs(pairs("http://127.0.0.1:1", &keys)).expect("settings");
    let mut registry = Registry::new();
    let clock = Arc::new(SystemClock) as _;
    let parts = (Arc::clone(store), SECRET.to_owned().into(), clock);
    register_export(&mut registry, &settings, parts).expect("registered");
    registry
}

/// Runs T109's nightly export of `url` through the worker's registry into a local storage
/// stand-in, and returns the stored object's name and the plain dump, decrypted with the
/// test's own identity (PRIV-024: no identity is a setting).
async fn snapshot(pool: &PgPool, url: &str, scratch: &Path) -> (String, Vec<u8>) {
    let own = age::x25519::Identity::generate();
    let store = LocalStorage::new(&format!("file://{}", scratch.display())).expect("storage");
    let store = Arc::new(store);
    let registry = export_registry(url, &store, &own.to_public().to_string());
    let mut db = pool.acquire().await.expect("connection");
    schedule_export(&mut db, Timestamp::now())
        .await
        .expect("queued");
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::now() + SignedDuration::from_hours(48);
    let due = claim(pool, owner, later, Lease::WORKER.length, 10).await;
    let due = due
        .expect("claimed")
        .into_iter()
        .find(|t| t.kind == EXPORT_KIND);
    let task = due.expect("export task");
    let ran = registry
        .run(pool, &SystemClock, owner, task, Lease::WORKER)
        .await;
    assert!(matches!(ran, Ok(Outcome::Done)), "the export ran");
    let sql = "SELECT object_name FROM recovery.backup_check WHERE kind = 'export' AND ok";
    let name: String = sqlx::query_scalar(sql)
        .fetch_one(&mut *db)
        .await
        .expect("row");
    let sealed = store.read_object(&name).await.expect("stored object");
    assert!(
        !sealed.starts_with(b"PGDMP"),
        "the stored export is encrypted"
    );
    let plain = age::decrypt(&own, &sealed).expect("the test's identity opens it");
    assert!(plain.starts_with(b"PGDMP"), "a custom-format dump");
    (name, plain)
}

/// Takes the snapshot of the test's database into the scratch folder `fresh` and writes the
/// decrypted dump there: the server's URL, the object's name and the plain file, as the operator
/// hands them to the drill (D-4932).
async fn plain_export(pool: &PgPool, fresh: &str) -> (String, String, PathBuf) {
    let db = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await;
    let db: String = db.expect("name");
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL for #[sqlx::test]");
    let (host, _) = base.rsplit_once('/').expect("a database URL");
    let scratch = std::env::temp_dir().join(fresh);
    std::fs::create_dir_all(&scratch).expect("scratch folder");
    let (object, plain) = snapshot(pool, &format!("{host}/{db}"), &scratch).await;
    let dump = scratch.join("plain.dump");
    std::fs::write(&dump, plain).expect("plain dump");
    (host.to_owned(), object, dump)
}

/// The texting client at the stand-in `base`, and both lines with the homeowner's number.
fn texting(base: &str) -> (TwilioSms, Lines) {
    let pairs = pairs(base, &[]);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("config");
    let lines = Lines {
        homeowner: Line::at_launch(LaunchChannel::Sms, SendingNumber::parse(FROM).ok()),
        business: Line::at_launch(LaunchChannel::Sms, None),
    };
    (twilio_clients(&config).expect("clients").0, lines)
}

/// Runs `sql` (built from the test's own names only) on the test's database.
async fn exec(pool: &PgPool, sql: String) {
    let ran = sqlx::raw_sql(AssertSqlSafe(sql)).execute(pool).await;
    ran.expect("statement");
}

/// The drill's `ok` row for `run` naming `object`, and the one alert for what was listed, read
/// from the restored database.
async fn recorded(restored: &mut PgConnection, run: Uuid, object: &str) {
    let sql = "SELECT backup_object, outcome FROM recovery.restore_drill WHERE replay_run_id = $1";
    let row = sqlx::query_as(sql)
        .bind(run)
        .fetch_one(&mut *restored)
        .await;
    let row: (String, String) = row.expect("drill row");
    assert_eq!(row, (object.to_owned(), "ok".to_owned()));
    let hour = SignedDuration::from_hours(1);
    let span = (Timestamp::now() - hour, Timestamp::now() + hour);
    let alerts = digest_alerts(restored, &[REPLAY_LISTED_ALERT], span.0, span.1).await;
    assert_eq!(
        alerts.expect("alerts").len(),
        1,
        "one alert for what was listed"
    );
}

/// TD-OPS-04 (OPS-013, OPS-018, OPS-019): the export restores into a fresh database and the
/// replay there sends no text twice, charges nothing, waits for Stripe, flags the unmatched
/// charge with one alert and records the drill; a second replay changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_04_the_export_restores_and_the_replay_sends_and_charges_nothing_twice(
    pool: PgPool,
) {
    let server = stand_in();
    let (sms, lines) = texting(&server.1);
    queue_text(&pool).await;
    let fresh = format!("askhava_drill_{}", std::process::id());
    let (host, object, dump) = plain_export(&pool, &fresh).await;
    let key = IdempotencyKey::derive(&SECRET.to_owned().into(), "sms.send", &["fake-row-1"]);
    let sent = sms.send_text(&key, HER, FROM, BODY).await;
    sent.expect("sent after the snapshot");
    exec(&pool, format!("CREATE DATABASE \"{fresh}\"")).await;
    let fresh_url = format!("{host}/{fresh}");
    let (vault, stripe) = (vault(), Stripe(AtomicU32::new(0)));
    let from = ReplaySources {
        sms: &sms,
        stripe: &stripe,
        vault: &vault,
        lines: &lines,
        charges_since: Timestamp::now() - SignedDuration::from_hours(1),
        retry: QUICK,
    };
    let drill = Drill::new(dump, &object, fresh_url.clone().into(), QUICK).expect("drill");
    let report = run_drill(&drill, &from, Timestamp::now())
        .await
        .expect("drill")
        .replay;
    assert_eq!(stripe.0.load(Ordering::SeqCst), 3, "Stripe was waited for");
    assert_eq!((report.texts_marked, report.texts_unsent), (1, 0));
    assert_eq!((report.charges_booked, report.charges_already), (0, 0));
    let kinds: Vec<ItemKind> = report.items.iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [ItemKind::ChargeUnmatched]);
    let mut restored = PgConnection::connect(&fresh_url)
        .await
        .expect("fresh connection");
    recorded(&mut restored, report.run, &object).await;
    let again = replay(&mut restored, &from, Timestamp::now())
        .await
        .expect("second replay");
    assert_eq!((again.texts_marked, again.charges_booked), (0, 0));
    let day = SignedDuration::from_hours(24);
    let window = (Timestamp::now() - day, Timestamp::now() + day);
    let log = sms.list_sent(FROM, window).await.expect("sent log");
    assert_eq!(log.len(), 1, "no text was sent twice");
    drop(restored);
    exec(&pool, format!("DROP DATABASE \"{fresh}\" WITH (FORCE)")).await;
    std::fs::remove_dir_all(std::env::temp_dir().join(&fresh)).ok();
}
