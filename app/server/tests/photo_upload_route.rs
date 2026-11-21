//! TD-INT-48 through the assembled router (T674, D-4360, D-5082; INT-044, PRIV-014, PRIV-015,
//! PRIV-025, PRIV-028, CARD-013, SEC-019, SEC-034, A11Y-027): the signed-in homeowner's photo
//! upload page and the short-lived photo links.
//! The fix-round cases (D-5084): a split request's photo stays with its own job, a press waits
//! for the homeowner's row lock, and each refused post is one `csrf_rejected` event ([`Events`]).
//!
//! Each test builds `build_router` with the webhooks' vault and [`PhotoParts`] over a scratch
//! folder, a fake homeowner with a session, her request and its job. The images are made here
//! ([`png`]: plain black, no metadata); every name, number and street is fake.

use askhava_adapters::api::LocalStorage;
use askhava_core::api::{IdSecret, NewTask, Settings, TestControl, enqueue};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    Answer, TextIn, confirmed_photos, paused_under_lock, receive_text, record_answer,
};
use askhava_jobs::api::{create, job_number};
use askhava_ops::api::{Actor, AuditRow, export_audit};
use askhava_portals::api::{
    CookieJar, HEIC_ERROR, REASK_ERROR, Realm, TOO_BIG_ERROR, TOO_MANY_PIXELS_ERROR, csrf_token,
    start_session,
};
use askhava_privacy::api::{
    MAX_PHOTO_BYTES, NewContact, PHOTO_DELETE_KIND, Vault, delete_photo, issue_link, job_photos,
    photo_path, store_contact,
};
use askhava_server::homeowner_portal::PhotoParts;
use askhava_server::{RouterParts, build_router, webhooks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use std::sync::{Arc, Mutex, PoisonError};

use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0001";
const BOUNDARY: &str = "fakeboundary674";

fn vault_key(byte: u8) -> SecretString {
    SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])))
}

/// The assembled router with the webhooks and the photo parts over a fresh scratch folder.
fn app(pool: &PgPool) -> Router {
    let mut pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        ("ASKHAVA_ID_SECRET", ID_SECRET),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:1"),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let key = |b: u8| format!("1:{}", STANDARD.encode([b; 32]));
    pairs.push(("VAULT_KEY".to_owned(), key(0x51)));
    pairs.push(("VAULT_INDEX_KEY".to_owned(), key(0x52)));
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let hooks = webhooks(&settings, pool.clone(), pairs).expect("hooks");
    let folder = std::env::temp_dir().join(format!("t674-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&folder).expect("scratch folder");
    let storage = LocalStorage::new(&format!("file://{}", folder.display())).expect("storage");
    let secret = SecretString::from(ID_SECRET.to_owned());
    let photos = PhotoParts::new(&vault_key(0x51), storage, secret).expect("photo parts");
    let parts = RouterParts::from((hooks, None, None, None)).with_photos(Some(photos));
    build_router(pool.clone(), control.router(), parts)
}

/// A fake homeowner on `phone` with a session, her request and its job: (jar, job, number,
/// request).
async fn homeowner(pool: &PgPool, phone: &str) -> (CookieJar, Uuid, i64, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&vault_key(0x51), &vault_key(0x52), &url).expect("vault");
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault, &new).await.expect("contact");
    let now = Timestamp::now() - SignedDuration::from_hours(1);
    let inbound = Inbound {
        words: "my sink leaks",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: now,
    };
    let secret = SecretString::from(ID_SECRET.to_owned());
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let job = create(&mut db, now, false, Actor::System)
        .await
        .expect("job");
    let code = job.simple().to_string();
    let named = Answer {
        conversation,
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(&mut db, &named).await.expect("her job");
    let number = job_number(&mut db, job).await.expect("number");
    let life = SignedDuration::from_hours(24);
    let who = (Realm::Homeowner, contact);
    let jar = start_session(&mut db, CookieJar::new(), who, life, now).await;
    (jar.expect("session"), job, number, conversation)
}

fn crc(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// A fake photo: a black 8-bit grayscale PNG of `width` by `height`, stored without compression.
fn png(width: usize, height: usize) -> Vec<u8> {
    let be = |n: usize| u32::try_from(n).expect("fits").to_be_bytes();
    let raw = vec![0_u8; (width + 1) * height];
    let mut data = vec![0x78, 0x01];
    let blocks = raw.chunks(65_535).count();
    for (i, block) in raw.chunks(65_535).enumerate() {
        let len = u16::try_from(block.len()).expect("block");
        data.push(u8::from(i + 1 == blocks));
        data.extend(len.to_le_bytes());
        data.extend((!len).to_le_bytes());
        data.extend(block);
    }
    data.extend(be(((raw.len() % 65_521) << 16) | 1));
    let header = [&be(width)[..], &be(height)[..], &[8, 0, 0, 0, 0]].concat();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [(b"IHDR", header), (b"IDAT", data), (b"IEND", Vec::new())] {
        let named = [&kind[..], &body[..]].concat();
        out.extend(be(body.len()));
        out.extend(&named);
        out.extend(crc(&named).to_be_bytes());
    }
    out
}

/// What a post carries: an upload, a form-encoded answer, or a cross-site answer.
enum Post {
    File(Vec<u8>),
    Form(&'static str),
    CrossSite(&'static str),
}

/// `GET path`, or `POST path` with `post` and the jar's CSRF token in place of `{csrf}`.
fn request(path: &str, jar: &CookieJar, post: Option<Post>) -> Request<Body> {
    let cookie: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let site = match post {
        Some(Post::CrossSite(_)) => "cross-site",
        _ => "same-origin",
    };
    let builder = Request::builder()
        .uri(path)
        .header(header::HOST, "askhava.example")
        .header(header::COOKIE, cookie.join("; "))
        .header("sec-fetch-site", site);
    let token = csrf_token(jar).unwrap_or_default();
    let request = match post {
        None => builder.method("GET").body(Body::empty()),
        Some(Post::Form(form) | Post::CrossSite(form)) => builder
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form.replace("{csrf}", &token))),
        Some(Post::File(file)) => {
            let part = |name: &str| {
                format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"")
            };
            let mut body = format!(
                "{}\r\n\r\n{token}\r\n{}; filename=\"fake.png\"\r\n\r\n",
                part("csrf"),
                part("photo")
            )
            .into_bytes();
            body.extend(file);
            body.extend(format!("\r\n--{BOUNDARY}--\r\n").into_bytes());
            builder
                .method("POST")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
        }
    };
    request.expect("request")
}

/// One answer: status, content type and body. Every answer must be `no-store` (SEC-034).
async fn call(
    app: &Router,
    path: &str,
    jar: &CookieJar,
    post: Option<Post>,
) -> (u16, String, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(request(path, jar, post))
        .await
        .expect("answer");
    let (parts, body) = response.into_parts();
    let text = |name| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(
        text(header::CACHE_CONTROL),
        "no-store",
        "{path} is no-store (SEC-034)"
    );
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    (
        parts.status.as_u16(),
        text(header::CONTENT_TYPE),
        bytes.to_vec(),
    )
}

/// `job`'s stored photos, newest first, and how many of them she confirmed.
async fn photos(pool: &PgPool, job: Uuid) -> (Vec<Uuid>, usize) {
    let mut db = pool.acquire().await.expect("db");
    let stored = job_photos(&mut db, job).await.expect("photos");
    let confirmed = confirmed_photos(&mut db, job).await.expect("confirmed");
    (stored, confirmed.len())
}

/// How many photos `job` holds.
async fn held(pool: &PgPool, job: Uuid) -> usize {
    photos(pool, job).await.0.len()
}

/// Uploads a good fake photo to request `number`; the preview's `/photos/<token>` path.
async fn upload(app: &Router, jar: &CookieJar, number: i64) -> String {
    let path = format!("/h/requests/{number}/photos");
    let (status, _, page) = call(app, &path, jar, Some(Post::File(png(400, 320)))).await;
    let page = String::from_utf8_lossy(&page).into_owned();
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("Send this photo") && page.contains("Take it off"));
    let from = page.find("/photos/").expect("the preview's link");
    page[from..from + "/photos/".len() + 64].to_owned()
}

/// TD-INT-48: her upload is stored re-encoded and shown through a live link; "Send this photo"
/// records one `photo_ok` row and a second press records nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_her_upload_is_stored_and_confirmed_once(pool: PgPool) {
    let app = app(&pool);
    let (jar, job, number, _) = homeowner(&pool, "+19285550141").await;
    let path = format!("/h/requests/{number}/photos");
    let (status, _, page) = call(&app, &path, &jar, None).await;
    let page = String::from_utf8_lossy(&page).into_owned();
    assert_eq!(status, 200);
    assert!(page.contains("<label for=\"photo\">") && page.contains("type=\"file\""));
    assert!(page.contains("Please leave people, house numbers and license plates out"));
    let link = upload(&app, &jar, number).await;
    let (status, kind, photo) = call(&app, &link, &CookieJar::new(), None).await;
    assert_eq!((status, kind.as_str()), (200, "image/jpeg"));
    assert!(
        photo.starts_with(&[0xFF, 0xD8]),
        "re-encoded as a JPEG (PRIV-014)"
    );
    assert_eq!(
        photos(&pool, job).await.1,
        0,
        "not confirmed yet (CARD-013)"
    );
    for _ in 0..2 {
        let yes = Some(Post::Form("csrf={csrf}&answer=yes"));
        assert_eq!(call(&app, &path, &jar, yes).await.0, 200);
        let (stored, confirmed) = photos(&pool, job).await;
        assert_eq!((stored.len(), confirmed), (1, 1));
    }
}

/// TD-INT-48: "Take it off" deletes her photo (row, audit row and queued object delete), never
/// another job's, kills its link, and every post is refused once her card was shown.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_take_it_off_removes_only_her_photo_before_the_card(pool: PgPool) {
    let app = app(&pool);
    let (jar, job, number, conversation) = homeowner(&pool, "+19285550142").await;
    let (other_jar, other_job, other_number, _) = homeowner(&pool, "+19285550143").await;
    upload(&app, &other_jar, other_number).await;
    let link = upload(&app, &jar, number).await;
    let path = format!("/h/requests/{number}/photos");
    let remove = || Some(Post::Form("csrf={csrf}&answer=remove"));
    let photo = photos(&pool, job).await.0[0];
    assert_eq!(call(&app, &path, &jar, remove()).await.0, 200);
    assert_eq!(held(&pool, job).await, 0);
    assert_eq!(held(&pool, other_job).await, 1);
    let mut db = pool.acquire().await.expect("db");
    let audit = export_audit(&mut db, 0, 10_000).await.expect("audit");
    let removed = |row: &&AuditRow| row.action == "privacy.photo_removed";
    assert_eq!(audit.iter().filter(removed).count(), 1);
    let mut again = NewTask::new(PHOTO_DELETE_KIND, serde_json::json!({}), Timestamp::now());
    again.dedupe_key = Some(format!("{PHOTO_DELETE_KIND}:purged:photos/{photo}"));
    let queued = enqueue(&mut db, &again).await.expect("enqueue");
    assert_eq!(
        queued, None,
        "the object's delete was queued with the row's delete"
    );
    assert_eq!(call(&app, &link, &jar, None).await.0, 404, "its link died");
    assert_eq!(
        call(&app, &path, &jar, remove()).await.0,
        200,
        "nothing waits"
    );
    assert_eq!(held(&pool, other_job).await, 1);

    upload(&app, &jar, number).await;
    let card = format!("card.{}", job.simple());
    let shown = Answer {
        conversation,
        question: &card,
        options: Some(&[("shown.fake.1", "shown")]),
        words: "shown",
        reply: (Uuid::new_v4(), Timestamp::now()),
    };
    record_answer(&mut db, &shown)
        .await
        .expect("her card was shown");
    let posts = [
        remove(),
        Some(Post::Form("csrf={csrf}&answer=yes")),
        Some(Post::File(png(400, 320))),
    ];
    for post in posts {
        let (status, _, page) = call(&app, &path, &jar, post).await;
        assert_eq!(status, 409);
        assert!(String::from_utf8_lossy(&page).contains("photos can't change now"));
    }
    assert_eq!((held(&pool, job).await, photos(&pool, job).await.1), (1, 0));
    assert_eq!(
        call(&app, &path, &jar, None).await.0,
        200,
        "her GET stays a 200"
    );
}

/// A second job, linked to request `conversation` as a split request links it (a `job.` row).
async fn second_job(pool: &PgPool, conversation: Uuid) -> (Uuid, i64) {
    let mut db = pool.acquire().await.expect("db");
    let now = Timestamp::now() - SignedDuration::from_mins(30);
    let job = create(&mut db, now, false, Actor::System)
        .await
        .expect("job");
    let code = job.simple().to_string();
    let named = Answer {
        conversation,
        question: "job.electrical.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(&mut db, &named)
        .await
        .expect("her second job");
    (job, job_number(&mut db, job).await.expect("number"))
}

/// TD-INT-48 (INT-044, CARD-013, D-5084): in a request split into two jobs, a photo confirmed on
/// one job is never among the other's confirmed photos, and `delete_photo` called with one job
/// and the other's photo answers `false` and keeps the row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_a_photo_stays_with_its_own_job_of_a_split_request(pool: PgPool) {
    let app = app(&pool);
    let (jar, first, first_number, conversation) = homeowner(&pool, "+19285550148").await;
    let (second, second_number) = second_job(&pool, conversation).await;
    upload(&app, &jar, second_number).await;
    let path = format!("/h/requests/{second_number}/photos");
    let yes = Some(Post::Form("csrf={csrf}&answer=yes"));
    assert_eq!(call(&app, &path, &jar, yes).await.0, 200);
    let photo = photos(&pool, second).await.0[0];
    let mut db = pool.acquire().await.expect("db");
    let on_second = confirmed_photos(&mut db, second).await;
    assert_eq!(on_second.expect("confirmed"), vec![photo]);
    let on_first = confirmed_photos(&mut db, first).await;
    assert_eq!(on_first.expect("confirmed"), Vec::<Uuid>::new());
    upload(&app, &jar, first_number).await;
    assert_eq!(photos(&pool, first).await.1, 0, "her new photo waits");
    let gone = delete_photo(&mut db, (first, photo), Timestamp::now()).await;
    assert_eq!(gone.ok(), Some(false), "a photo of another job is kept");
    assert_eq!(
        (held(&pool, second).await, held(&pool, first).await),
        (1, 1)
    );
    let audit = export_audit(&mut db, 0, 10_000).await.expect("audit");
    let removed = |row: &&AuditRow| row.action == "privacy.photo_removed";
    assert_eq!(audit.iter().filter(removed).count(), 0);
}

/// TD-INT-48 (INT-044, D-5084): a "Send this photo" press waits for the homeowner's row lock
/// (taken here as `paused_under_lock` takes it), so its check and its write never land beside
/// another press or the card step; presses at once still confirm her photo once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_a_double_press_records_one_confirmation(pool: PgPool) {
    let app = app(&pool);
    let (jar, job, number, conversation) = homeowner(&pool, "+19285550149").await;
    upload(&app, &jar, number).await;
    let path = format!("/h/requests/{number}/photos");
    let yes = || Some(Post::Form("csrf={csrf}&answer=yes"));
    let mut held_lock = pool.begin().await.expect("transaction");
    let paused = paused_under_lock(&mut held_lock, conversation, job).await;
    assert_eq!(paused.ok(), Some(false), "her row is locked");
    let (router, to, cookies) = (app.clone(), path.clone(), jar.clone());
    let mut press = tokio::spawn(async move { call(&router, &to, &cookies, yes()).await.0 });
    let wait = std::time::Duration::from_millis(500);
    let waited = tokio::time::timeout(wait, &mut press).await;
    assert!(waited.is_err(), "the press waits for her row's lock");
    assert_eq!(photos(&pool, job).await.1, 0, "nothing recorded meanwhile");
    held_lock.rollback().await.expect("release");
    assert_eq!(press.await.ok(), Some(200));
    let again = || call(&app, &path, &jar, yes());
    let all = tokio::join!(again(), again(), again());
    assert_eq!((all.0.0, all.1.0, all.2.0), (200, 200, 200));
    let mut db = pool.acquire().await.expect("db");
    let confirmed = confirmed_photos(&mut db, job).await.expect("confirmed");
    assert_eq!(confirmed, job_photos(&mut db, job).await.expect("photos"));
    assert_eq!(confirmed.len(), 1);
}

/// The `kind` and `route` of every "security event" log line while it is the default subscriber
/// (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<(String, String)>>>);

/// One log event's `message`, `kind` and `route`.
#[derive(Default)]
struct Fields(Option<String>, String, String);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.record_debug(field, &value);
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}").trim_matches('"').to_owned();
        match field.name() {
            "message" => self.0 = Some(value),
            "kind" => self.1 = value,
            "route" => self.2 = value,
            _ => {}
        }
    }
}

impl tracing::Subscriber for Events {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.0.as_deref() == Some("security event") {
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push((fields.1, fields.2));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// TD-INT-48 (SEC-042, SEC-048): a cross-site post, a wrong token and a missing one, form-encoded
/// or multipart, are each a 403 with one `csrf_rejected` event naming the route template, never
/// her request's number, and store nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_a_refused_post_is_one_csrf_rejected_event(pool: PgPool) {
    let app = app(&pool);
    let (jar, job, number, _) = homeowner(&pool, "+19285550150").await;
    let path = format!("/h/requests/{number}/photos");
    let no_session = CookieJar::new();
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let refused = [
        (&jar, Post::CrossSite("csrf={csrf}&answer=yes")),
        (&jar, Post::Form("csrf=0000&answer=yes")),
        (&jar, Post::Form("answer=remove")),
        (&no_session, Post::File(png(400, 320))),
    ];
    for (sent, (jar, post)) in refused.into_iter().enumerate() {
        assert_eq!(call(&app, &path, jar, Some(post)).await.0, 403);
        let all = events.0.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(all.len(), sent + 1, "one event for each refusal");
        let (kind, route) = &all[sent];
        assert_eq!(kind, "csrf_rejected");
        assert_eq!(route, "/h/requests/{job}/photos", "the template");
    }
    assert_eq!(held(&pool, job).await, 0);
}

/// TD-INT-48 (PRIV-025, INT-044): a HEIC, an undecodable, a too-small and an over-limit file each
/// show the page's error with the focused summary, and nothing is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_a_refused_file_shows_the_pages_error(pool: PgPool) {
    let app = app(&pool);
    let (jar, job, number, _) = homeowner(&pool, "+19285550144").await;
    let path = format!("/h/requests/{number}/photos");
    let mut heavy = png(400, 320);
    heavy.resize(MAX_PHOTO_BYTES + 1, 0);
    let cases = [
        (b"\0\0\0\x18ftypheic\0\0\0\0mif1heic".to_vec(), HEIC_ERROR),
        (b"not a photo at all".to_vec(), REASK_ERROR),
        (png(400, 100), REASK_ERROR),
        (heavy, TOO_BIG_ERROR),
        (png(8_001, 1), TOO_MANY_PIXELS_ERROR),
    ];
    for (file, error) in cases {
        let (status, _, page) = call(&app, &path, &jar, Some(Post::File(file))).await;
        let page = String::from_utf8_lossy(&page).into_owned();
        assert_eq!(status, 400, "{error}");
        let shown = error.replace('\'', "&#39;");
        assert!(
            page.contains(&shown) || page.contains(error),
            "{error}: {page}"
        );
        assert!(page.contains("id=\"error-summary\" role=\"alert\" tabindex=\"-1\" autofocus"));
        assert!(page.contains("<title>Error: "));
    }
    assert_eq!(held(&pool, job).await, 0);
}

/// TD-INT-48 (SEC-019, SEC-042): another homeowner's request and an unknown number are the one
/// not-found answer, no session is the sign-in refusal, a cross-site or wrong-token post (either body) is
/// a 403, and the right token with no
/// answer is the page's own 400.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_only_her_own_request_and_her_own_site(pool: PgPool) {
    let app = app(&pool);
    let (jar, _, number, _) = homeowner(&pool, "+19285550145").await;
    let (_, other_job, other_number, _) = homeowner(&pool, "+19285550146").await;
    let own = format!("/h/requests/{number}/photos");
    let theirs = format!("/h/requests/{other_number}/photos");
    for path in [
        theirs.as_str(),
        "/h/requests/999999/photos",
        "/h/requests/x/photos",
    ] {
        let seen = call(&app, path, &jar, None).await;
        assert_eq!((seen.0, seen.2.len()), (404, 0), "{path}");
        let sent = call(&app, path, &jar, Some(Post::File(png(400, 320)))).await;
        assert_eq!((sent.0, sent.2.len()), (404, 0), "{path}");
    }
    assert_eq!(held(&pool, other_job).await, 0);
    assert_eq!(call(&app, &own, &CookieJar::new(), None).await.0, 401);
    let refused = [
        Post::CrossSite("csrf={csrf}&answer=yes"),
        Post::Form("csrf=0000&answer=yes"),
        Post::Form("answer=remove"),
    ];
    for post in refused {
        assert_eq!(call(&app, &own, &jar, Some(post)).await.0, 403);
    }
    let wrong = app.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri(&own)
            .header(header::HOST, "askhava.example")
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"))
            .body(Body::from(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n0000\r\n--{BOUNDARY}--\r\n")))
            .expect("request"),
    );
    assert_eq!(
        wrong.await.expect("answer").status(),
        403,
        "a multipart post with a wrong token"
    );
    let (status, _, page) = call(&app, &own, &jar, Some(Post::Form("csrf={csrf}"))).await;
    assert_eq!(status, 400);
    assert!(String::from_utf8_lossy(&page).contains("type=\"file\""));
}

/// TD-INT-48 (PRIV-014, PRIV-028): an expired, an unknown and a malformed photo token are each a
/// bare `no-store` 404.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_48_a_dead_photo_link_is_404(pool: PgPool) {
    let app = app(&pool);
    let (jar, job, number, _) = homeowner(&pool, "+19285550147").await;
    let link = upload(&app, &jar, number).await;
    assert_eq!(call(&app, &link, &jar, None).await.0, 200);
    let mut db = pool.acquire().await.expect("db");
    let secret = IdSecret::new(1, SecretString::from(ID_SECRET.to_owned())).expect("secret");
    let past = Timestamp::now() - SignedDuration::from_hours(1);
    let photo = photos(&pool, job).await.0[0];
    let old = issue_link(&mut db, &secret, photo, past).await;
    let expired = photo_path(&old.expect("an expired link"));
    let unknown = format!("/photos/{}", "0".repeat(64));
    for path in [expired.as_str(), unknown.as_str(), "/photos/sign-in"] {
        let (status, _, body) = call(&app, path, &jar, None).await;
        assert_eq!((status, body.len()), (404, 0), "{path}");
    }
}
