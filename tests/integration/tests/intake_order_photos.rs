//! TD-INT-49 (T675, D-4360, D-5251, D-5337, D-5459): steps 9 and 10 of INT-043's order and the
//! `intake.picture_text` task, with PostgreSQL, the Twilio media stand-in, the storage stand-in,
//! fake images and 555 numbers only. Her replies go through the route's entry
//! (`intake::api::receive_raw_reply`); her picture texts go through the inbound route itself
//! (`app/server/src/inbound_text.rs`, compiled in by `#[path]`), so the task, its IDs-only payload
//! and the picture text's marker are the route's. The task runs through
//! `intake::api::run_picture_text_at` over [`StandIn`], a `PictureSource` that rebuilds the fake
//! Twilio URL from the SID tail and calls `privacy::api::take_picture_text`. Every first text but
//! the held cases' names one trade and one job type of it (D-4441).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, FetchError, IdempotencyKey, LocalStorage};
use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::{load_embedded, photo_asks};
use askhava_core::api::{
    Claimed, DEFAULT_MAX_ATTEMPTS, Environment, HandlerError, ManualClock, brand,
};
use askhava_intake::api::rules::{
    Inbound, NAME_ADDRESS_TEMPLATE, SAFETY_TEMPLATE, Settings as IntakeSettings,
};
use askhava_intake::api::{
    CARD_SHOWN_TEMPLATE, CONCEPT_OFFER_TEMPLATE, Handled, LEAVE_OUT_LINE, LOOK_ASK_TEMPLATE,
    LOOK_DESCRIBE_TEMPLATE, NO_PHOTO_LINE, PHOTO_ASK_TEMPLATE, PHOTO_CHECK_TEMPLATE,
    PHOTO_HEIC_TEMPLATE, PHOTO_OK, PHOTO_REASK_TEMPLATE, PICTURE_TEXT_KIND, PictureKeys,
    PictureSource, RawReply, Reply, TextIn, UPLOAD_PATH, receive_raw_reply, run_picture_text_at,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_portals::api::PHOTO_UPLOAD_ROUTE;
use askhava_privacy::api::{
    NewContact, PhotoError, PhotoSeal, PictureText, PictureTextMedia, Vault, fetch_twilio_media,
    store_contact, take_picture_text,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// The inbound route, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
#[allow(
    dead_code,
    reason = "only the inbound route and `Webhooks` are used; the retry items are the worker's"
)]
mod inbound_text;

const TOWN: &str = "lake_havasu_city";
/// A first text naming one trade (`handyman`) and one job type of it, door repair (D-4441).
const DOOR: &str = "my front door is sticking badly";
/// A first text naming no trade: the held cases.
const NO_TRADE: &str = "hello, quick question for you";
/// Her fake answer to the name-and-address question.
const ANSWER: &str = "Testa 12 Sample Ave";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7411,Sample Fixit One LLC,,,2019-03-04\n";
const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const LAFTLY: &str = "+19285550100";
const TWILIO: &str = "https://api.twilio.com/2010-04-01/Accounts/";
const ACCOUNT: &str = "AC00000000000000000000000000000000";
/// Steps 4 to 8 after [`DOOR`], the last answer getting the photo ask (as TD-INT-47 walks them).
const TO_STEP_9: [&str; 6] = [
    "86403",
    "2",
    "door repair",
    "1",
    "not sure",
    "it sticks at the top",
];

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x93), &key(0x94), &url).expect("vault")
}

/// `minute` minutes after 11:00 Arizona on a fixed day.
fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T11:00:00-07:00".parse().expect("time");
    start + SignedDuration::from_mins(minute)
}

/// A fake Twilio message SID.
fn mm(n: u32) -> String {
    format!("MM{n:032x}")
}

/// A fake Twilio media SID.
fn me(n: u32) -> String {
    format!("ME{n:032x}")
}

/// The fake Twilio media URL of `media` in `message`.
fn media_url(message: &str, media: &str) -> String {
    format!("{TWILIO}{ACCOUNT}/Messages/{message}/Media/{media}")
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbImage::from_pixel(width, height, image::Rgb([120, 130, 140]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

/// A temporary directory removed on drop.
struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

/// The Twilio media stand-in: serves each media SID's file (none: a fetch that times out), runs
/// the real guarded fetch for a URL off Twilio's host, and records every fetch and delete.
#[derive(Default)]
struct Media {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    fetched: Mutex<Vec<String>>,
    deleted: Mutex<Vec<String>>,
}

impl PictureTextMedia for Media {
    async fn fetch_media(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.fetched.lock().expect("fetched").push(url.to_owned());
        if !url.starts_with(TWILIO) {
            return fetch_twilio_media(url).await;
        }
        let sid = url.rsplit('/').next().unwrap_or_default();
        let file = self.files.lock().expect("files").get(sid).cloned();
        file.ok_or(FetchError::TimedOut)
    }

    async fn delete_media(&self, _: &IdempotencyKey, url: &str) -> Result<(), AdapterError> {
        self.deleted.lock().expect("deleted").push(url.to_owned());
        Ok(())
    }
}

/// The test `PictureSource` over the media and storage stand-ins (D-5337).
struct StandIn {
    _dir: Dir,
    storage: LocalStorage,
    seal: PhotoSeal,
    secret: SecretString,
    media: Media,
}

impl PictureSource for StandIn {
    async fn take(
        &self,
        db: &mut PgConnection,
        job: Uuid,
        url: &str,
        at: (u32, Timestamp),
    ) -> Result<PictureText, PhotoError> {
        let (message, media) = url.split_once('/').expect("the SID tail");
        let url = media_url(message, media);
        let keys = (&self.seal, &self.storage, &self.secret);
        take_picture_text(db, keys, &self.media, (job, &url), at).await
    }

    async fn discard(&self, url: &str) -> bool {
        let (message, media) = url.split_once('/').expect("the SID tail");
        let deleted = &self.media.deleted;
        deleted
            .lock()
            .expect("deleted")
            .push(media_url(message, media));
        true
    }
}

/// One test's parts: the app-role connection, the route, its clock, the task's keys and her.
struct Kit {
    pool: PgPool,
    db: PgConnection,
    hooks: Arc<Webhooks>,
    clock: Arc<ManualClock>,
    keys: Arc<PictureKeys<StandIn>>,
    her: Uuid,
    phone: String,
}

impl Kit {
    fn fetched(&self) -> Vec<String> {
        self.keys.source.media.fetched.lock().expect("f").clone()
    }

    fn deleted(&self) -> Vec<String> {
        self.keys.source.media.deleted.lock().expect("d").clone()
    }

    fn serve(&self, media: &str, bytes: Vec<u8>) {
        let files = &self.keys.source.media.files;
        files.lock().expect("files").insert(media.to_owned(), bytes);
    }
}

/// The task's stand-in keys; `broken` storage cannot write, so every store fails.
fn picture_keys(test: &str, broken: bool) -> Arc<PictureKeys<StandIn>> {
    let root = std::env::temp_dir().join(format!("askhava-t675-{test}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary directory");
    let store = if broken {
        std::fs::write(root.join("file"), b"x").expect("a file");
        root.join("file").join("under")
    } else {
        root.clone()
    };
    let key = SecretString::from(format!("1:{}", STANDARD.encode([5u8; 32])));
    let source = StandIn {
        _dir: Dir(root),
        storage: LocalStorage::new(&format!("file://{}", store.display())).expect("storage"),
        seal: PhotoSeal::new(&key).expect("seal"),
        secret: secret(),
        media: Media::default(),
    };
    Arc::new(PictureKeys {
        source,
        secret: secret(),
    })
}

/// An active, welcomed fake business serving the town and ticking `trade`'s `job_type`.
async fn business(db: &mut PgConnection, trade: &str, job_type: &str) {
    let before: Timestamp = "2026-11-02T10:00:00-07:00".parse().expect("time");
    let b = create(db, false, before).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY[$3])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .bind(trade)
    .execute(&mut *db)
    .await
    .expect("signed up");
    add_owner(db, b, Uuid::new_v4(), before)
        .await
        .expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let checked = apply(db, b, Event::Checked(ok), Actor::System, before).await;
    checked.expect("active");
    request_welcome(db, b, before).await.expect("welcome");
    welcome_sent(db, b, before).await.expect("sent");
    let named = save_business_name(db, b, ("Sample Fixit One LLC", None), None).await;
    named.expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(trade, job_type);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    let ticked = set_job_types(db, b, &[choice], &allowed, actor).await;
    ticked.expect("job types");
}

/// The catalog, the fake city list and one business of `trade` ticking `job_type`, then the app
/// role; a fake homeowner with 555 number `n` in the vault, and the route over this pool.
async fn setup(pool: &PgPool, n: u32, (trade, job_type): (&str, &str), broken: bool) -> Kit {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    let listed = import_city_list(&mut db, date(2026, 11, 1), CITY).await;
    listed.expect("city list");
    business(&mut db, trade, job_type).await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let phone = format!("+1928555{n:04}");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.clone())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &new).await.expect("her");
    let clock = Arc::new(ManualClock::new(at(0)));
    let hooks = Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::clone(&clock) as _,
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: secret(),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    });
    let keys = picture_keys(&format!("{n}"), broken);
    let pool = pool.clone();
    Kit {
        pool,
        db,
        hooks,
        clock,
        keys,
        her,
        phone,
    }
}

/// The templates `words` from her queued, in order, at `minute`, through the route's entry.
async fn sent(kit: &mut Kit, words: &str, minute: i64) -> Vec<&'static str> {
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: kit.her,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    };
    let (raw, vault) = (SecretString::from(words.to_owned()), vault());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let settings = (IntakeSettings::default(), &[][..]);
    let reply = receive_raw_reply(&mut kit.db, &secret(), (&msg, keys), settings).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// Her texts `words` from `minute` on, one a minute; the templates of the last one.
async fn walk(kit: &mut Kit, words: &[&str], minute: i64) -> Vec<&'static str> {
    let mut last = Vec::new();
    for (m, words) in (minute..).zip(words) {
        last = sent(kit, words, m).await;
    }
    last
}

/// [`DOOR`] and [`TO_STEP_9`] in minutes 0 to 6: the photo ask is the last text.
async fn to_step_9(kit: &mut Kit) {
    sent(kit, DOOR, 0).await;
    let last = walk(kit, &TO_STEP_9, 1).await;
    assert_eq!(
        last,
        [PHOTO_ASK_TEMPLATE],
        "step 9 after the trade questions"
    );
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Her picture text `message` with body `words` and media `urls`, posted signed to the inbound
/// route at `minute`; the route's status.
async fn picture(
    kit: &Kit,
    (message, words): (&str, &str),
    urls: &[&str],
    minute: i64,
) -> StatusCode {
    kit.clock.set(at(minute));
    let count = urls.len().to_string();
    let names: Vec<String> = (0..urls.len()).map(|n| format!("MediaUrl{n}")).collect();
    let mut fields = vec![
        ("MessageSid", message),
        ("From", kit.phone.as_str()),
        ("To", LAFTLY),
        ("Body", words),
        ("NumMedia", count.as_str()),
    ];
    fields.extend(names.iter().map(String::as_str).zip(urls.iter().copied()));
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    let mut sorted = fields.clone();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let signature = STANDARD.encode(mac.finalize().into_bytes());
    let request = Request::post(INBOUND_PATH)
        .header("X-Twilio-Signature", signature)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(Arc::clone(&kit.hooks)).oneshot(request).await;
    answer.expect("answer").status()
}

/// The picture tasks due at `minute` and not done.
async fn due(kit: &Kit, minute: i64) -> Vec<(i64, Value)> {
    sqlx::query_as(
        "SELECT id, payload FROM core.task WHERE kind = $1 AND done_at IS NULL AND run_at <= $2 \
         ORDER BY id",
    )
    .bind(PICTURE_TEXT_KIND)
    .bind(at(minute).to_sqlx())
    .fetch_all(&kit.pool)
    .await
    .expect("tasks")
}

/// Runs every picture task due at `minute` as claim `attempts`, marking each that ended done.
async fn run(kit: &Kit, minute: i64, attempts: i32) -> Vec<Result<(), HandlerError>> {
    let mut ran = Vec::new();
    for (id, payload) in due(kit, minute).await {
        let task = Claimed {
            id,
            kind: PICTURE_TEXT_KIND.to_owned(),
            payload,
            attempts,
        };
        let keys = Arc::clone(&kit.keys);
        let done = run_picture_text_at(kit.pool.clone(), keys, task, at(minute)).await;
        if done.is_ok() {
            sqlx::query("UPDATE core.task SET done_at = now() WHERE id = $1")
                .bind(id)
                .execute(&kit.pool)
                .await
                .expect("done");
        }
        ran.push(done);
    }
    ran
}

/// How many rows `sql` counts, with `bind` as `$1`.
async fn count(kit: &Kit, sql: &'static str, bind: &str) -> i64 {
    let n = sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(&kit.pool)
        .await;
    n.expect("count")
}

/// How many `template` texts were queued.
async fn texts(kit: &Kit, template: &str) -> i64 {
    count(
        kit,
        "SELECT count(*) FROM messaging.outbox WHERE template_id = $1",
        template,
    )
    .await
}

/// The stored photos of every job.
async fn photos(kit: &Kit) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM privacy.photo ORDER BY stored_at")
        .fetch_all(&kit.pool)
        .await
        .expect("photos")
}

/// TD-INT-49 (INT-044, PRIV-015): after the trade questions the photo ask goes with the job type's
/// asks, her upload page link (T674's route) and PRIV-015's line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_the_photo_ask_follows_the_trade_questions(pool: PgPool) {
    let mut kit = setup(&pool, 401, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    let body: String =
        sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = $1")
            .bind(PHOTO_ASK_TEMPLATE)
            .fetch_one(&kit.pool)
            .await
            .expect("the photo ask");
    let number: i64 = sqlx::query_scalar(
        "SELECT n.number FROM jobs.job_number n JOIN intake.conversation c ON c.job_id = n.job_id",
    )
    .fetch_one(&kit.pool)
    .await
    .expect("her job number");
    assert_eq!(UPLOAD_PATH, PHOTO_UPLOAD_ROUTE, "T674's page");
    let path = UPLOAD_PATH.replace("{job}", &number.to_string());
    let link = format!("https://{}{path}", brand().domain());
    let first = photo_asks("handyman");
    assert!(!first.is_empty(), "the handyman set asks for photos");
    for part in [NO_PHOTO_LINE, LEAVE_OUT_LINE, &link, &first[0]] {
        assert!(body.contains(part), "{part}: {body}");
    }
    assert!(
        body.starts_with(&format!("{}: ", brand().short_name())),
        "{body}"
    );
}

/// TD-INT-49 (INT-044, PRIV-014, SEC-041, CARD-013): a picture text whose body is "here" commits
/// with one task whose payload holds only the inbox ID and the Twilio SIDs, and does not move step
/// 9 on; the task fetches from the stand-in, stores the photo, deletes it at the provider and shows
/// it back; YES records `photo_ok` and the name-and-address question follows; the card she is
/// shown carries the photo. A rerun fetches nothing twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_picture_text_is_stored_confirmed_and_put_on_the_card(pool: PgPool) {
    let mut kit = setup(&pool, 402, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    let (message, media) = (mm(1), me(1));
    let url = media_url(&message, &media);
    let posted = picture(&kit, (&message, "here"), &[&url], 7).await;
    assert_eq!(posted, StatusCode::OK);
    let tasks = due(&kit, 7).await;
    assert_eq!(tasks.len(), 1, "one task per picture text");
    let payload = &tasks[0].1;
    let keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["inbox", "media", "message"], "IDs only");
    assert_eq!(payload["message"], message.as_str());
    assert_eq!(payload["media"], serde_json::json!([media]));
    assert!(!payload.to_string().contains("http") && !payload.to_string().contains("555"));
    assert_eq!(texts(&kit, NAME_ADDRESS_TEMPLATE).await, 0, "step 9 waits");
    assert!(kit.fetched().is_empty(), "nothing fetched in the webhook");
    kit.serve(&media, png(640, 480));
    assert_eq!(run(&kit, 8, 1).await, [Ok(())]);
    let stored = photos(&kit).await;
    assert_eq!(stored.len(), 1, "stored");
    assert_eq!(kit.fetched(), std::slice::from_ref(&url));
    assert_eq!(kit.deleted(), [url], "deleted at the provider once stored");
    assert_eq!(texts(&kit, PHOTO_CHECK_TEMPLATE).await, 1, "shown back");
    let again = Claimed {
        id: tasks[0].0,
        kind: PICTURE_TEXT_KIND.to_owned(),
        payload: payload.clone(),
        attempts: 2,
    };
    let rerun = run_picture_text_at(pool.clone(), Arc::clone(&kit.keys), again, at(8)).await;
    assert_eq!(rerun, Ok(()));
    assert_eq!(kit.fetched().len(), 1, "a rerun fetches nothing twice");
    assert_eq!(photos(&kit).await.len(), 1);
    assert_eq!(sent(&mut kit, "YES", 9).await, [NAME_ADDRESS_TEMPLATE]);
    let ok = "SELECT count(*) FROM intake.intake_answer WHERE question_id = $1";
    assert_eq!(count(&kit, ok, PHOTO_OK).await, 1);
    assert!(
        sent(&mut kit, ANSWER, 10)
            .await
            .contains(&CARD_SHOWN_TEMPLATE)
    );
    let on_card: Vec<Uuid> = sqlx::query_scalar("SELECT unnest(photo_ids) FROM quotes.shown_card")
        .fetch_all(&kit.pool)
        .await
        .expect("card photos");
    assert_eq!(on_card, stored, "the confirmed photo is on her card");
}

/// TD-INT-49 (INT-044, D-491): a store that fails deletes nothing at the provider and retries.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_failed_store_deletes_nothing_at_the_provider(pool: PgPool) {
    let mut kit = setup(&pool, 403, ("handyman", "door repair"), true).await;
    to_step_9(&mut kit).await;
    let (message, media) = (mm(2), me(2));
    let url = media_url(&message, &media);
    assert_eq!(
        picture(&kit, (&message, ""), &[&url], 7).await,
        StatusCode::OK
    );
    kit.serve(&media, png(640, 480));
    let ran = run(&kit, 8, 1).await;
    assert!(ran.len() == 1 && ran[0].is_err(), "retried: {ran:?}");
    assert_eq!(kit.fetched(), [url]);
    assert!(kit.deleted().is_empty(), "nothing deleted at the provider");
    assert!(photos(&kit).await.is_empty());
    assert_eq!(texts(&kit, NAME_ADDRESS_TEMPLATE).await, 0);
}

/// TD-INT-49 (INT-044, CARD-013): REMOVE deletes the shown photo and step 9 moves on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_remove_takes_the_photo_off(pool: PgPool) {
    let mut kit = setup(&pool, 404, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    let (message, media) = (mm(3), me(3));
    let url = media_url(&message, &media);
    assert_eq!(
        picture(&kit, (&message, "here"), &[&url], 7).await,
        StatusCode::OK
    );
    kit.serve(&media, png(640, 480));
    assert_eq!(run(&kit, 8, 1).await, [Ok(())]);
    assert_eq!(photos(&kit).await.len(), 1);
    assert_eq!(sent(&mut kit, "REMOVE", 9).await, [NAME_ADDRESS_TEMPLATE]);
    assert!(photos(&kit).await.is_empty(), "deleted");
    let ok = "SELECT count(*) FROM intake.intake_answer WHERE question_id = $1";
    assert_eq!(count(&kit, ok, PHOTO_OK).await, 0);
}

/// TD-INT-49 (INT-044, INT-019): a picture text on a first text with no trade is held (nothing
/// fetched, kept or deleted) and looks again every 15 minutes; once her job opens it is stored,
/// and shown for her YES only at step 9.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_picture_on_a_first_text_with_no_trade_waits_for_the_job(pool: PgPool) {
    let mut kit = setup(&pool, 405, ("handyman", "door repair"), false).await;
    let (message, media) = (mm(4), me(4));
    let url = media_url(&message, &media);
    assert_eq!(
        picture(&kit, (&message, NO_TRADE), &[&url], 0).await,
        StatusCode::OK
    );
    kit.serve(&media, png(640, 480));
    assert_eq!(run(&kit, 1, 1).await, [Ok(())]);
    assert!(kit.fetched().is_empty() && kit.deleted().is_empty(), "held");
    assert!(due(&kit, 15).await.is_empty(), "not before 15 minutes");
    assert_eq!(due(&kit, 16).await.len(), 1, "looks again after 15 minutes");
    sent(&mut kit, "86403", 2).await;
    sent(&mut kit, DOOR, 3).await;
    assert_eq!(run(&kit, 16, 1).await, [Ok(())]);
    assert_eq!(photos(&kit).await.len(), 1, "stored once the job opened");
    assert_eq!(
        texts(&kit, PHOTO_CHECK_TEMPLATE).await,
        0,
        "not before step 9"
    );
    let mut last = Vec::new();
    for (minute, words) in (17..).zip(["2", "door repair", "1", "not sure", "it sticks"]) {
        last = sent(&mut kit, words, minute).await;
        if last.contains(&PHOTO_ASK_TEMPLATE) {
            break;
        }
    }
    assert_eq!(
        last,
        [PHOTO_ASK_TEMPLATE, PHOTO_CHECK_TEMPLATE],
        "shown at step 9"
    );
    assert_eq!(sent(&mut kit, "yes", 30).await, [NAME_ADDRESS_TEMPLATE]);
}

/// TD-INT-49 (INT-044, INT-019): held 72 hours with no job, the picture keeps nothing and its
/// media is deleted at the provider.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_picture_held_72_hours_is_deleted_at_the_provider(pool: PgPool) {
    let kit = setup(&pool, 406, ("handyman", "door repair"), false).await;
    let (message, media) = (mm(5), me(5));
    let url = media_url(&message, &media);
    assert_eq!(
        picture(&kit, (&message, NO_TRADE), &[&url], 0).await,
        StatusCode::OK
    );
    sqlx::query("UPDATE core.task SET run_at = $1 WHERE kind = $2")
        .bind(at(72 * 60).to_sqlx())
        .bind(PICTURE_TEXT_KIND)
        .execute(&kit.pool)
        .await
        .expect("due in 72 hours");
    assert_eq!(run(&kit, 72 * 60, 1).await, [Ok(())]);
    assert!(kit.fetched().is_empty(), "nothing fetched");
    assert_eq!(kit.deleted(), [url], "deleted at the provider");
    assert!(photos(&kit).await.is_empty(), "nothing kept");
    assert!(due(&kit, 80 * 60).await.is_empty(), "no further look");
}

/// TD-INT-49 (INT-044): a picture text sent at the ZIP step is stored and confirmed only at step 9.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_picture_at_the_zip_step_is_confirmed_at_step_9(pool: PgPool) {
    let mut kit = setup(&pool, 407, ("handyman", "door repair"), false).await;
    sent(&mut kit, DOOR, 0).await;
    let (message, media) = (mm(6), me(6));
    let url = media_url(&message, &media);
    assert_eq!(
        picture(&kit, (&message, "86403"), &[&url], 1).await,
        StatusCode::OK
    );
    kit.serve(&media, png(640, 480));
    assert_eq!(run(&kit, 1, 1).await, [Ok(())]);
    assert_eq!(photos(&kit).await.len(), 1);
    assert_eq!(
        texts(&kit, PHOTO_CHECK_TEMPLATE).await,
        0,
        "not at the ZIP step"
    );
    let last = walk(&mut kit, &TO_STEP_9[1..], 2).await;
    assert_eq!(last, [PHOTO_ASK_TEMPLATE, PHOTO_CHECK_TEMPLATE]);
    assert_eq!(sent(&mut kit, "YES", 8).await, [NAME_ADDRESS_TEMPLATE]);
}

/// TD-INT-49 (INT-044): a fetch that still fails on the task's last attempt moves step 9 on with
/// its text, the name-and-address question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_failed_fetch_still_moves_step_9_on(pool: PgPool) {
    let mut kit = setup(&pool, 408, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    let (message, media) = (mm(7), me(7));
    let url = media_url(&message, &media);
    assert_eq!(
        picture(&kit, (&message, "here"), &[&url], 7).await,
        StatusCode::OK
    );
    let first = run(&kit, 8, 1).await;
    assert!(first.len() == 1 && first[0].is_err(), "retried: {first:?}");
    assert_eq!(texts(&kit, NAME_ADDRESS_TEMPLATE).await, 0);
    assert_eq!(run(&kit, 9, DEFAULT_MAX_ATTEMPTS).await, [Ok(())]);
    assert_eq!(
        texts(&kit, NAME_ADDRESS_TEMPLATE).await,
        1,
        "step 9 moved on"
    );
    assert!(photos(&kit).await.is_empty());
}

/// TD-INT-49 (INT-044, D-2874): a file that does not decode gets the one re-ask; a second one is
/// left off and step 9 moves on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_an_undecodable_file_is_re_asked_once(pool: PgPool) {
    let mut kit = setup(&pool, 409, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    for (n, minute) in [(8_u32, 7_i64), (9, 9)] {
        let (message, media) = (mm(n), me(n));
        let url = media_url(&message, &media);
        kit.serve(&media, b"not a photo at all".to_vec());
        assert_eq!(
            picture(&kit, (&message, ""), &[&url], minute).await,
            StatusCode::OK
        );
        assert_eq!(run(&kit, minute + 1, 1).await, [Ok(())]);
    }
    assert_eq!(texts(&kit, PHOTO_REASK_TEMPLATE).await, 1, "once");
    assert_eq!(texts(&kit, NAME_ADDRESS_TEMPLATE).await, 1, "then on");
    assert!(photos(&kit).await.is_empty());
}

/// TD-INT-49 (PRIV-025): a HEIC picture gets the plain reply asking for a JPEG or PNG.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_heic_picture_gets_the_priv_025_reply(pool: PgPool) {
    let mut kit = setup(&pool, 410, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    let (message, media) = (mm(10), me(10));
    let url = media_url(&message, &media);
    let mut heic = vec![0, 0, 0, 24];
    heic.extend_from_slice(b"ftypheic\0\0\0\0mif1heic");
    kit.serve(&media, heic);
    assert_eq!(
        picture(&kit, (&message, ""), &[&url], 7).await,
        StatusCode::OK
    );
    assert_eq!(run(&kit, 8, 1).await, [Ok(())]);
    assert_eq!(texts(&kit, PHOTO_HEIC_TEMPLATE).await, 1);
    assert_eq!(
        texts(&kit, NAME_ADDRESS_TEMPLATE).await,
        0,
        "she may send it again"
    );
    assert!(photos(&kit).await.is_empty());
}

/// TD-INT-49 (INT-044, INT-004, INT-005): no photo is asked or shown while a 911 text waits for
/// SAFE; after SAFE the order resumes and step 9 shows the photo then.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_no_photo_is_asked_while_safe_is_awaited(pool: PgPool) {
    let mut kit = setup(&pool, 411, ("handyman", "door repair"), false).await;
    sent(&mut kit, DOOR, 0).await;
    walk(&mut kit, &TO_STEP_9[..5], 1).await;
    let alarm = sent(&mut kit, "it sticks and now I smell gas in the kitchen", 6).await;
    assert!(alarm.contains(&SAFETY_TEMPLATE), "{alarm:?}");
    let (message, media) = (mm(11), me(11));
    let url = media_url(&message, &media);
    kit.serve(&media, png(640, 480));
    assert_eq!(
        picture(&kit, (&message, ""), &[&url], 7).await,
        StatusCode::OK
    );
    assert_eq!(run(&kit, 8, 1).await, [Ok(())]);
    assert_eq!(
        texts(&kit, PHOTO_ASK_TEMPLATE).await,
        0,
        "no ask during the hold"
    );
    assert_eq!(texts(&kit, PHOTO_CHECK_TEMPLATE).await, 0, "nothing shown");
    let mut last = sent(&mut kit, "SAFE", 9).await;
    for minute in 10..14 {
        if last.contains(&PHOTO_ASK_TEMPLATE) {
            break;
        }
        last = sent(&mut kit, "it sticks at the top", minute).await;
    }
    assert_eq!(
        last,
        [PHOTO_ASK_TEMPLATE, PHOTO_CHECK_TEMPLATE],
        "after SAFE"
    );
}

/// TD-INT-49 (INT-045, INT-046, D-5251): a `design: true` category asks the look-pictures and the
/// describe-the-look questions after the photos, then step 11; no concept image is made. The
/// keyword reading names no design category yet and a painter needs an ROC license, so the
/// handyman request's trade reading is planted as `painting` after its coverage line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_design_category_asks_the_look_questions(pool: PgPool) {
    let mut kit = setup(&pool, 412, ("handyman", "door repair"), false).await;
    sent(&mut kit, DOOR, 0).await;
    sent(&mut kit, "86403", 1).await;
    sqlx::query("UPDATE intake.intake_answer SET choice = 'painting' WHERE question_id = $1")
        .bind("trade_reading")
        .execute(&kit.pool)
        .await
        .expect("the request read as painting");
    let mut last = sent(&mut kit, "2", 2).await;
    for minute in 3..14 {
        if last.contains(&PHOTO_ASK_TEMPLATE) {
            break;
        }
        last = sent(&mut kit, "1", minute).await;
    }
    assert_eq!(last, [PHOTO_ASK_TEMPLATE], "step 9");
    assert_eq!(sent(&mut kit, "no", 20).await, [LOOK_ASK_TEMPLATE]);
    assert_eq!(sent(&mut kit, "no", 21).await, [LOOK_DESCRIBE_TEMPLATE]);
    let done = sent(&mut kit, "light gray, modern, matte", 22).await;
    assert_eq!(done, [NAME_ADDRESS_TEMPLATE], "step 11");
    assert_eq!(
        texts(&kit, CONCEPT_OFFER_TEMPLATE).await,
        0,
        "concept images off"
    );
}

/// TD-INT-49 (SEC-041): a media URL off Twilio's hosts, or of another message, is refused at the
/// route: no task, nothing fetched.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_49_a_media_url_off_twilio_is_refused(pool: PgPool) {
    let mut kit = setup(&pool, 413, ("handyman", "door repair"), false).await;
    to_step_9(&mut kit).await;
    let (message, media) = (mm(12), me(12));
    let off = format!(
        "https://media.example.test/2010-04-01/Accounts/{ACCOUNT}/Messages/{message}/Media/{media}"
    );
    let other = media_url(&mm(13), &media);
    let posted = picture(&kit, (&message, "here"), &[&off, &other], 7).await;
    assert_eq!(posted, StatusCode::OK);
    assert!(due(&kit, 60).await.is_empty(), "no task");
    assert!(run(&kit, 60, 1).await.is_empty());
    assert!(kit.fetched().is_empty(), "nothing fetched");
    assert!(photos(&kit).await.is_empty());
}
