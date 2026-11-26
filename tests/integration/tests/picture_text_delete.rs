//! A picture text's media is deleted at the provider after a failed delete and after a store
//! that can never succeed (INT-044, PRIV-014; T783, D-5081, D-5550, D-5646).
//!
//! Against PostgreSQL and the Twilio stand-in (`python3 -m stand_ins.server`, needs `python3` on
//! the path), both `intake` handlers running as `askhava_app` through the task queue on a manual
//! clock. The stand-in cannot fail a media delete, so [`Faulty`]'s `discard` fails the first
//! `fails` times and then calls the real stand-in delete; its `take` stores through
//! `privacy::api::take_picture_text` with a fetch serving a fake PNG and a provider delete that
//! fails, so a stored photo answers `MediaState::DeletePending` (or, with `deletes`, the real
//! stand-in delete, `MediaState::Deleted`). Every number, SID, key and byte is fake.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use askhava_adapters::api::{
    AdapterError, FetchError, IdempotencyKey, LocalStorage, SmsMediaDelete, TwilioConfig,
    TwilioSms, twilio_clients,
};
use askhava_core::api::{Clock, DEFAULT_MAX_ATTEMPTS, Lease, ManualClock, Registry, Settings};
use askhava_core::api::{WorkerId, claim};
use askhava_intake::api::{
    PICTURE_DELETE_KIND, PICTURE_TEXT_KIND, PictureKeys, PictureSource, queue_picture_text,
    run_picture_delete, run_picture_text_at,
};
use askhava_integration::set_app_role;
use askhava_privacy::api::{
    PhotoError, PhotoSeal, PictureText, PictureTextMedia, take_picture_text,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ACCOUNT: &str = "AC00000000000000000000000000000000";
const TOKEN: &str = "fake-auth-token";

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-picture-text-deletes-0123456789")
}

fn t0() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("time")
}

/// The stand-in server on a port it picked itself (its first line names it); killed on drop.
struct StandIn(Child, u16);

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
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("listening line");
    std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
    let port = line
        .trim_end()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok());
    StandIn(
        child,
        port.unwrap_or_else(|| panic!("stand-in said {line:?}")),
    )
}

fn texting(port: u16) -> TwilioSms {
    let base = format!("http://127.0.0.1:{port}/twilio");
    let numbers: Vec<String> = (0..10).map(|i| format!("+1500555{:04}", 100 + i)).collect();
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "ci"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", ACCOUNT),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", &numbers.join(",")),
        ("ASKHAVA_TWILIO_BASE_URL", &base),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", &base),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("twilio config");
    twilio_clients(&config).expect("clients").0
}

/// One HTTP/1.1 exchange with the stand-in; returns the status code and the body.
async fn http(port: u16, head: &str, body: &str) -> (u16, String) {
    let auth = STANDARD.encode(format!("{ACCOUNT}:{TOKEN}"));
    let request = format!(
        "{head} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {auth}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.expect("answer");
    let answer = String::from_utf8_lossy(&answer).into_owned();
    let code = answer
        .get(9..12)
        .and_then(|c| c.parse().ok())
        .expect("status");
    (
        code,
        answer
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_owned())
            .unwrap_or_default(),
    )
}

/// A fake picture text received by the stand-in; returns its `{message SID}/{media SID}` tail.
async fn picture(port: u16) -> String {
    let body = json!({ "from": "+15005550010", "to": "+15005550006", "body": "",
                       "media": ["image/png"] });
    let (_, answer) = http(port, "POST /_stand_in/twilio/inbound", &body.to_string()).await;
    let parsed: serde_json::Value = serde_json::from_str(&answer).expect("json");
    let url = parsed["params"]["MediaUrl0"].as_str().expect("media url");
    let mut parts = url.rsplit('/');
    let media = parts.next().expect("media SID");
    format!("{}/{media}", parts.nth(1).expect("message SID"))
}

/// The stand-in's status for a fetch of `tail` (200 while it is kept, 404 once deleted).
async fn fetch_status(port: u16, tail: &str) -> u16 {
    let (message, media) = tail.split_once('/').expect("tail");
    let path = format!("/twilio/2010-04-01/Accounts/{ACCOUNT}/Messages/{message}/Media/{media}");
    http(port, &format!("GET {path}"), "").await.0
}

/// The picture fetch serving a fake PNG, its provider delete failing or (`deletes`) real.
struct Media<'a>(&'a TwilioSms, bool);

impl PictureTextMedia for Media<'_> {
    async fn fetch_media(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        let mut out = Vec::new();
        image::RgbImage::from_pixel(640, 480, image::Rgb([120, 130, 140]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("png");
        Ok(out)
    }

    async fn delete_media(&self, key: &IdempotencyKey, url: &str) -> Result<(), AdapterError> {
        if !self.1 {
            return Err(AdapterError::InvalidId);
        }
        self.0.delete_media_at(key, url).await
    }
}

/// The test `PictureSource`: see the module comment.
struct Faulty {
    _dir: std::path::PathBuf,
    storage: LocalStorage,
    seal: PhotoSeal,
    sms: TwilioSms,
    port: u16,
    fails: AtomicU32,
    deletes: bool,
}

impl Faulty {
    fn url(&self, tail: &str) -> String {
        let (message, media) = tail.split_once('/').expect("tail");
        format!(
            "http://127.0.0.1:{}/twilio/2010-04-01/Accounts/{ACCOUNT}/Messages/{message}/Media/{media}",
            self.port
        )
    }
}

impl PictureSource for Faulty {
    async fn take(
        &self,
        db: &mut PgConnection,
        job: Uuid,
        url: &str,
        at: (u32, Timestamp),
    ) -> Result<PictureText, PhotoError> {
        let (keys, media) = (
            (&self.seal, &self.storage, &secret()),
            Media(&self.sms, self.deletes),
        );
        take_picture_text(db, keys, &media, (job, &self.url(url)), at).await
    }

    async fn discard(&self, url: &str) -> bool {
        if self.fails.load(Ordering::SeqCst) > 0 {
            self.fails.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        let key = IdempotencyKey::derive(&secret(), "picture_text.media_discard", &[url]);
        self.sms.delete_media_at(&key, &self.url(url)).await.is_ok()
    }
}

/// One test's parts: the app-role pool, the registry over [`Faulty`], its clock and port.
struct Kit {
    pool: PgPool,
    registry: Registry,
    clock: Arc<ManualClock>,
    port: u16,
    _stand_in: StandIn,
}

/// The kit: `broken` storage cannot write; `discard` fails `fails` times first.
async fn kit(pool: &PgPool, test: &str, (broken, fails, deletes): (bool, u32, bool)) -> Kit {
    let stand = stand_in();
    let root = std::env::temp_dir().join(format!("askhava-t783-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    std::fs::write(root.join("file"), b"x").expect("a file");
    let store = if broken {
        root.join("file").join("under")
    } else {
        root.clone()
    };
    let key = SecretString::from(format!("1:{}", STANDARD.encode([5u8; 32])));
    let source = Faulty {
        _dir: root,
        storage: LocalStorage::new(&format!("file://{}", store.display())).expect("storage"),
        seal: PhotoSeal::new(&key).expect("seal"),
        sms: texting(stand.1),
        port: stand.1,
        fails: AtomicU32::new(fails),
        deletes,
    };
    let keys = Arc::new(PictureKeys {
        source,
        secret: secret(),
    });
    let clock = Arc::new(ManualClock::new(t0()));
    let mut registry = Registry::new();
    let (deletes_keys, at) = (Arc::clone(&keys), Arc::clone(&clock));
    registry.register(PICTURE_DELETE_KIND, move |_, task| {
        run_picture_delete(Arc::clone(&deletes_keys), task)
    });
    registry.register(PICTURE_TEXT_KIND, move |pool, task| {
        run_picture_text_at(pool, Arc::clone(&keys), task, at.now())
    });
    let app = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool");
    Kit {
        pool: app,
        registry,
        clock,
        port: stand.1,
        _stand_in: stand,
    }
}

/// A fake open request with a job, picture text `inbox` marked in it.
async fn request(pool: &PgPool, inbox: Uuid) {
    let at = t0().to_sqlx();
    sqlx::query(
        "WITH h AS (INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $3) RETURNING id),
         c AS (INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id,
               consent_template, consent_sent_at, opened_at)
               SELECT id, $2, 'en', $4, 'consent_en', $3, $3 FROM h RETURNING id)
         INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at)
         SELECT id, 'photo_pending', $5, $3 FROM c",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(at)
    .bind(Uuid::new_v4())
    .bind(inbox.simple().to_string())
    .execute(pool)
    .await
    .expect("fake request");
}

/// Queues the `intake.picture_text` task of picture text `inbox` with media `tail`.
async fn queue(kit: &Kit, inbox: Uuid, tail: &str) {
    let (message, media) = tail.split_once('/').expect("tail");
    let mut db = kit.pool.acquire().await.expect("connection");
    let queued = queue_picture_text(&mut db, inbox, (message, &[media]), t0()).await;
    assert!(queued.expect("queued"));
}

/// Runs every task due now, as the worker would, then moves the clock past the longest backoff.
async fn run_due(kit: &Kit) {
    let owner = WorkerId::new(&kit.pool).await.expect("worker");
    let now = kit.clock.now();
    let tasks = claim(&kit.pool, owner, now, SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    for task in tasks {
        let ran = kit
            .registry
            .run(&kit.pool, &*kit.clock, owner, task, Lease::WORKER)
            .await;
        ran.expect("run");
    }
    kit.clock.advance(SignedDuration::from_hours(2));
}

/// The delete-only tasks of `media` and the photos stored.
async fn counts(pool: &PgPool, media: &str) -> (i64, i64) {
    let key = format!("{PICTURE_DELETE_KIND}:{media}");
    let tasks = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE dedupe_key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("tasks");
    let photos = sqlx::query_scalar("SELECT count(*) FROM privacy.photo")
        .fetch_one(pool)
        .await;
    (tasks, photos.expect("photos"))
}

fn media_of(tail: &str) -> &str {
    tail.rsplit('/').next().expect("media SID")
}

/// INT-044, PRIV-014 (D-5081): a stored photo answering `DeletePending` whose `discard` fails
/// queues the delete-only task, and its retry deletes the media (404 after).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_delete_pending_photo_is_deleted_by_the_retry(pool: PgPool) {
    let kit = kit(&pool, "pending", (false, 1, false)).await;
    let (inbox, tail) = (Uuid::new_v4(), picture(kit.port).await);
    request(&pool, inbox).await;
    queue(&kit, inbox, &tail).await;
    run_due(&kit).await;
    assert_eq!(
        counts(&pool, media_of(&tail)).await,
        (1, 1),
        "stored, delete queued"
    );
    assert_eq!(
        fetch_status(kit.port, &tail).await,
        200,
        "still at the provider"
    );
    run_due(&kit).await;
    assert_eq!(
        fetch_status(kit.port, &tail).await,
        404,
        "deleted by the retry"
    );
}

/// INT-044 (D-5081): a store failing on every retry keeps nothing and deletes the media after
/// the last attempt.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_store_failing_every_retry_deletes_the_media(pool: PgPool) {
    let kit = kit(&pool, "store", (true, 0, false)).await;
    let (inbox, tail) = (Uuid::new_v4(), picture(kit.port).await);
    request(&pool, inbox).await;
    queue(&kit, inbox, &tail).await;
    for _ in 1..DEFAULT_MAX_ATTEMPTS {
        run_due(&kit).await;
        assert_eq!(
            fetch_status(kit.port, &tail).await,
            200,
            "kept until the last attempt"
        );
    }
    run_due(&kit).await;
    assert_eq!(
        fetch_status(kit.port, &tail).await,
        404,
        "deleted after the last attempt"
    );
    assert_eq!(
        counts(&pool, media_of(&tail)).await,
        (0, 0),
        "no task, no photo"
    );
}

/// INT-044 (D-5550): after the last failed store, a failed `discard` queues the delete-only task,
/// whose retry deletes the media.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_discard_after_the_last_store_is_retried(pool: PgPool) {
    let kit = kit(&pool, "last", (true, 1, false)).await;
    let (inbox, tail) = (Uuid::new_v4(), picture(kit.port).await);
    request(&pool, inbox).await;
    queue(&kit, inbox, &tail).await;
    for _ in 0..DEFAULT_MAX_ATTEMPTS {
        run_due(&kit).await;
    }
    assert_eq!(
        counts(&pool, media_of(&tail)).await,
        (1, 0),
        "delete queued, no photo"
    );
    assert_eq!(fetch_status(kit.port, &tail).await, 200);
    run_due(&kit).await;
    assert_eq!(
        fetch_status(kit.port, &tail).await,
        404,
        "deleted by the retry"
    );
}

/// INT-044 (D-5081): a media that deletes at once queues no task.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_media_deleted_at_once_queues_no_task(pool: PgPool) {
    let kit = kit(&pool, "once", (false, 0, true)).await;
    let (inbox, tail) = (Uuid::new_v4(), picture(kit.port).await);
    request(&pool, inbox).await;
    queue(&kit, inbox, &tail).await;
    run_due(&kit).await;
    assert_eq!(fetch_status(kit.port, &tail).await, 404);
    assert_eq!(
        counts(&pool, media_of(&tail)).await,
        (0, 1),
        "stored, no task"
    );
}

/// INT-044 (D-5646): on the `Hold::Discard` path (no request) a failed `discard` queues the
/// delete-only task, and its retry deletes the media.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_discard_of_a_held_picture_is_retried(pool: PgPool) {
    let kit = kit(&pool, "held", (false, 1, false)).await;
    let tail = picture(kit.port).await;
    queue(&kit, Uuid::new_v4(), &tail).await;
    run_due(&kit).await;
    assert_eq!(
        counts(&pool, media_of(&tail)).await,
        (1, 0),
        "delete queued"
    );
    assert_eq!(fetch_status(kit.port, &tail).await, 200);
    run_due(&kit).await;
    assert_eq!(
        fetch_status(kit.port, &tail).await,
        404,
        "deleted by the retry"
    );
}
