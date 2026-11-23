//! TD-CARD-16 (T666, D-4349, D-5526; CARD-023, CARD-019, CARD-020, PICK-024, PICK-051, SEC-019,
//! PRIV-006) through the assembled router over PostgreSQL: `GET /b/jobs/{j}`, the business job
//! page. A member of a business the card went to gets 200 with the card, its photo links, the
//! number sent and quotes so far and her own quote controls, and no homeowner name, phone or
//! street; another business's number, an unknown number or UUID and a malformed `{j}` get the same
//! not-found body with one security event each; signed out gets the 401 with the sign-in link; her
//! own job's UUID is a 303 to its number's page; with her business's accepted pick a "Confirm and
//! billing" member (also through a notice link's sign-in) gets a 303 to `/b/picks/{pick}` and a
//! "Cards and quotes" member the page with the closed line; a withdrawn, never-confirmed quote
//! offers "Send a new quote" (D-5619). The page offers "Not a fit for us" with its four reasons,
//! and a tap posted through T306's route is recorded, changes no routing or fee and links back to
//! the page (PICK-057, T668, D-5387). People sign in through `portals`' emailed-link sign-in, as
//! `cant_take_route.rs` does. Every name, phone, street, ID and key is fake.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::LocalStorage;
use askhava_businesses::api::{AccountStatus, Acting, Roles, add_member, add_owner, create};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{MIGRATOR, Settings, SystemClock, TestControl};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create as create_job, job_number, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor as AuditActor, not_a_fit_counts};
use askhava_payments::api::{
    EntryKind, LedgerEntry, NewEntry, ledger_append, ledger_totals, no_fee_release,
};
use askhava_picks::api::{PagePick, Settled, accepted_pick, page_pick, settle};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, ensure_pre_session, reasons, signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, PhotoSeal, Vault, clean, store_contact, store_photo};
use askhava_quotes::api::{
    CardBusiness, CardFacts, CardJob, CardParts, DEFAULT_MIN_WORDS, Format, KeptCard, OfferTown,
    Place, Price, QuoteForm, QuoteRules, build_card, card_digest, card_holders, keep_card,
    mark_card_sent, parse_quote, quote_standing, send_cards, submit_quote, withdraw_quote,
};
use askhava_server::homeowner_portal::PhotoParts;
use askhava_server::{RouterParts, build_router, webhooks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0666";
/// The fake homeowner's name, phone and street, which no job page may show (PRIV-006).
const HER: [&str; 3] = ["Dana Fakename", "9285550142", "41 Example Drive"];

/// An answer: status, `Location`, whether it is `no-store`, its body and its cookies.
struct Reply {
    status: u16,
    location: Option<String>,
    no_store: bool,
    text: String,
    cookies: Vec<String>,
}

/// A same-origin request as a browser sends it, with `body` as a form.
fn request(method: &Method, path: &str, jar: &CookieJar, body: String) -> Request<Body> {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 66], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let header_text = |name| {
        let value = parts.headers.get(name).and_then(|v| v.to_str().ok());
        value.map(str::to_owned)
    };
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        location: header_text(header::LOCATION),
        no_store: header_text(header::CACHE_CONTROL).is_some_and(|v| v.contains("no-store")),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        cookies,
    }
}

/// The one sign-in address of one person; the link it "emails" is kept.
struct OneMail {
    to: SignInAddress,
    sent: Mutex<Option<String>>,
}

impl SignInMail for OneMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        _: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(vec![self.to])
    }

    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let token = link.expose_secret().rsplit('/').next().map(str::to_owned);
        *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = token;
        Ok(())
    }
}

/// `to`'s person signed in through `portals`' emailed link, carrying `?job=<n>` when `next`
/// (PICK-024's notice sign-in): the jar and where the finished sign-in sends her.
async fn sign_in(pool: &PgPool, to: SignInAddress, next: Option<i64>) -> (CookieJar, String) {
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app =
        SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail)).into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let csrf = askhava_portals::api::csrf_token(&jar).expect("csrf");
    let form = format!("csrf={csrf}&email=person%40x.example");
    answer(&app, request(&Method::POST, SIGNIN_ROUTE, &jar, form)).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let job = next.map(|n| format!("?job={n}")).unwrap_or_default();
    let path = format!("/b/sign-in/{}{job}", token.expect("a link emailed"));
    let pressed = answer(
        &app,
        request(&Method::POST, &path, &jar, format!("csrf={csrf}")),
    )
    .await;
    let mut headers = HeaderMap::new();
    let cookies = pressed.cookies.join("; ").parse().expect("cookies");
    headers.insert(header::COOKIE, cookies);
    (
        CookieJar::from_headers(&headers),
        pressed.location.unwrap_or_default(),
    )
}

/// A new active business and its owner, signed in.
async fn owner(pool: &PgPool) -> (Acting, CookieJar) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let business = create(&mut db, false, now).await.expect("business");
    let mailbox = Uuid::new_v4();
    let member = add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    drop(db);
    let (jar, _) = sign_in(pool, SignInAddress { business, mailbox }, None).await;
    (Acting { business, member }, jar)
}

/// A member of `owner`'s business with `roles`; signed in with `next` as [`sign_in`] does.
async fn member(
    pool: &PgPool,
    owner: Acting,
    roles: Roles,
    next: Option<i64>,
) -> (CookieJar, String) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = Uuid::new_v4();
    add_member(&mut db, owner, mailbox, roles, now)
        .await
        .expect("member");
    drop(db);
    let business = owner.business;
    sign_in(pool, SignInAddress { business, mailbox }, next).await
}

/// `business` able to take an AC repair job in Lake Havasu City.
fn able(business: Uuid) -> CardBusiness {
    CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

/// The AC repair job as matching reads it.
fn matched() -> CardJob {
    let candidate = Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    CardJob {
        towns: vec![town],
        candidates: vec![candidate],
        text: String::new(),
    }
}

/// A firm $325 quote, read by AI (so waiting for its read-back confirm) when `ai_read`.
async fn quote(db: &mut PgConnection, (job, business): (Uuid, Uuid), ai_read: bool) {
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor included".to_owned(),
        ai_read,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    let at = Timestamp::now();
    submit_quote(db, (job, business), &quote, at)
        .await
        .expect("quoted");
}

/// A fake key in the vault's format.
fn key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// A fake homeowner (name, phone and street of [`HER`]) and her request, tied to a new open job:
/// (request, job).
async fn homeowner(db: &mut PgConnection) -> (Uuid, Uuid) {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let secret = |s: &str| Some(SecretString::from(s.to_owned()));
    let vault = Vault::new(&key(0x51).into(), &key(0x52).into(), &url).expect("vault");
    let new = NewContact {
        name: secret(HER[0]),
        phone: secret("+19285550142"),
        address: secret(HER[2]),
        ..NewContact::default()
    };
    let contact = store_contact(db, &vault, &new).await.expect("contact");
    let now = Timestamp::now();
    let inbound = Inbound {
        words: "my AC blows warm air",
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
    let id_secret = SecretString::from(ID_SECRET.to_owned());
    let handled = receive_text(db, &id_secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let job = create_job(db, now, false, AuditActor::System)
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
    record_answer(db, &named).await.expect("her job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, AuditActor::System).await;
    assert!(moved.expect("open").applied().is_some(), "opened");
    (conversation, job)
}

/// Her job carded to `businesses`, its card (two fake photos) kept and sent: (job, number).
async fn carded(pool: &PgPool, businesses: &[Uuid]) -> (Uuid, i64) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let (request, job) = homeowner(&mut db).await;
    let to: Vec<CardBusiness> = businesses.iter().copied().map(able).collect();
    let map = ClassMap::shipped().expect("the shipped license map");
    send_cards(&mut db, map, job, &matched(), &to, now)
        .await
        .expect("cards");
    let photos = vec![
        stored_photo(&mut db, job).await,
        stored_photo(&mut db, job).await,
    ];
    let parts = CardParts {
        category: "AC repair".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "The AC runs but the air from the vents stays warm.".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos,
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let digest = card_digest(card.text());
    let facts = CardFacts {
        category: "hvac_repair".to_owned(),
        candidates: vec!["AC not cooling".to_owned()],
        emergency: false,
    };
    let kept = keep_card(&mut db, (request, job), &digest, &card, &facts, now).await;
    assert_eq!(kept.expect("kept"), KeptCard::Stored);
    assert!(
        mark_card_sent(&mut db, job, &digest, now)
            .await
            .expect("sent")
    );
    let number = job_number(&mut db, job).await.expect("number");
    (job, number)
}

/// Her page pick of `business`'s quote 1 on `job`, settled `free` on the $0 path (FEE-012), so
/// it is the job's accepted pick: the pick.
async fn accepted(pool: &PgPool, (job, business): (Uuid, Uuid)) -> Uuid {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let made = page_pick(&mut db, (job, false), 1, AuditActor::System, now).await;
    let Ok(PagePick::Picked(pick)) = made else {
        panic!("a page pick")
    };
    let key = format!("t666.free.{job}.{business}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    });
    ledger_append(&mut db, &entry.expect("entry"))
        .await
        .expect("ledger");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let cleared = cleared.expect("a $0 decision clears the pick");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    assert_eq!(
        settle(&mut tx, &cleared).await.expect("settle"),
        Settled::Moved(job)
    );
    tx.commit().await.expect("commit");
    pick
}

/// The assembled router with the webhooks' ID secret and the photo parts.
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
    pairs.push(("VAULT_KEY".to_owned(), key(0x51)));
    pairs.push(("VAULT_INDEX_KEY".to_owned(), key(0x52)));
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let hooks = webhooks(&settings, pool.clone(), pairs).expect("hooks");
    let folder = std::env::temp_dir().join(format!("t666-{}", Uuid::new_v4()));
    let storage = LocalStorage::new(&format!("file://{}", folder.display())).expect("storage");
    let secret = SecretString::from(ID_SECRET.to_owned());
    let photos = PhotoParts::new(&key(0x51).into(), storage, secret).expect("photo parts");
    let parts = RouterParts::from((hooks, None, None, None)).with_photos(Some(photos));
    build_router(pool.clone(), control.router(), parts)
}

/// `GET /b/jobs/<j>` by `jar`'s person.
async fn open(app: &Router, j: &str, jar: &CookieJar) -> Reply {
    let path = format!("/b/jobs/{j}");
    answer(app, request(&Method::GET, &path, jar, String::new())).await
}

/// The security events logged while it is the default subscriber, by kind (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    fn count(&self, kind: &str) -> usize {
        let all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().filter(|k| *k == kind).count()
    }
}

/// The fields of one log event.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
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
        let get = |name: &str| {
            fields
                .0
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        if get("message").as_deref() == Some("security event") {
            let kind = get("kind").unwrap_or_default();
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push(kind);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
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

/// A plain black 64 by 64 PNG with no metadata, made here.
fn png() -> Vec<u8> {
    let be = |n: usize| u32::try_from(n).expect("fits").to_be_bytes();
    let (width, height) = (64, 64);
    let raw = vec![0_u8; (width + 1) * height];
    let len = u16::try_from(raw.len()).expect("one block");
    let mut data = vec![0x78, 0x01, 1];
    data.extend(len.to_le_bytes());
    data.extend((!len).to_le_bytes());
    data.extend(&raw);
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

/// A cleaned fake photo of `job`, sealed and stored through `privacy`: its ID.
async fn stored_photo(db: &mut PgConnection, job: Uuid) -> Uuid {
    let folder = std::env::temp_dir().join(format!("t666-{}", Uuid::new_v4()));
    let storage = LocalStorage::new(&format!("file://{}", folder.display())).expect("storage");
    let seal = PhotoSeal::new(&key(0x51).into()).expect("seal");
    let clean_photo = clean(png()).await.expect("clean");
    let secret = SecretString::from(ID_SECRET.to_owned());
    let keys = (&seal, &storage, &secret);
    let stored = store_photo(db, keys, job, &clean_photo, Timestamp::now()).await;
    stored.expect("stored photo")
}

/// The page shows no homeowner name, phone or street (PRIV-006).
fn shows_none_of_her(text: &str) {
    for her in HER
        .iter()
        .chain(&["928-555-0142", "(928) 555-0142", "Example Drive"])
    {
        assert!(!text.contains(her), "the page shows {her:?}");
    }
}

async fn catalog(pool: &PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
}

/// TD-CARD-16 (CARD-023, CARD-019, PICK-051, PRIV-006): a member of a business the card went to
/// gets the page with the card, both photo links, "Sent to 2 businesses. Quotes so far: 1." and
/// her standing quote's controls, and nothing of the homeowner; a "Cards and quotes" member too.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_card_16_a_carded_member_gets_the_page(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (y, _) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business, y.business]).await;
    let mut db = pool.acquire().await.expect("db");
    quote(&mut db, (job, x.business), false).await;
    drop(db);
    let cards = Roles::new(true, false).expect("roles");
    let (crew, _) = member(&pool, x, cards, None).await;
    let app = app(&pool);
    for who in [&jar, &crew] {
        let page = open(&app, &number.to_string(), who).await;
        assert_eq!((page.status, page.no_store), (200, true), "{}", page.text);
        assert!(
            page.text
                .contains("The AC runs but the air from the vents stays warm.")
        );
        assert!(
            page.text
                .contains("Sent to 2 businesses. Quotes so far: 1.")
        );
        assert_eq!(
            page.text.matches("src=\"/photos/").count(),
            2,
            "{}",
            page.text
        );
        assert!(page.text.contains("Withdraw your quote"));
        assert!(
            page.text
                .contains(&format!("action=\"/b/jobs/{number}/withdraw\""))
        );
        shows_none_of_her(&page.text);
    }
}

/// TD-CARD-16 (SEC-019, SEC-048, D-5526): another business's number and job UUID, an unknown
/// number and UUID and a malformed `{j}` are the same not-found body with one security event
/// each; signed out is the 401 with the sign-in link; her own job's UUID is a 303 to its number.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_card_16_the_one_not_found_answer(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (_, other) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business]).await;
    let app = app(&pool);
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let unknown = open(&app, "9999999", &jar).await;
    assert_eq!((unknown.status, unknown.no_store), (404, true));
    let asked = [
        (number.to_string(), &other),
        (job.to_string(), &other),
        (Uuid::new_v4().to_string(), &jar),
        ("not-a-job".to_owned(), &jar),
        ("-1".to_owned(), &jar),
        ("9999999".to_owned(), &jar),
    ];
    for (j, who) in asked {
        let before = events.count("authz_denied");
        let reply = open(&app, &j, who).await;
        assert_eq!((reply.status, &reply.text), (404, &unknown.text), "{j}");
        assert_eq!(
            events.count("authz_denied"),
            before + 1,
            "one event for {j}"
        );
        shows_none_of_her(&reply.text);
    }
    let out = ensure_pre_session(CookieJar::new()).expect("jar");
    let reply = open(&app, &number.to_string(), &out).await;
    assert_eq!(reply.status, 401);
    assert!(reply.text.contains("href=\"/b/sign-in\""), "{}", reply.text);
    let moved = open(&app, &job.to_string(), &jar).await;
    let to = format!("/b/jobs/{number}");
    assert_eq!(
        (moved.status, moved.location.as_deref()),
        (303, Some(to.as_str()))
    );
}

/// TD-CARD-16 (PICK-024, PICK-051): with her business's accepted pick, the owner and a "Confirm
/// and billing" member, also straight from a notice link's sign-in, get a 303 to the release view,
/// and a "Cards and quotes" member the page with the closed line.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_card_16_an_accepted_pick_leads_to_its_release(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business]).await;
    let mut db = pool.acquire().await.expect("db");
    quote(&mut db, (job, x.business), false).await;
    drop(db);
    let pick = accepted(&pool, (job, x.business)).await;
    let billing = Roles::new(false, true).expect("roles");
    let (bill, landed) = member(&pool, x, billing, Some(number)).await;
    let here = format!("/b/jobs/{number}");
    assert_eq!(landed, here, "the notice sign-in lands on the job page");
    let release = format!("/b/picks/{pick}");
    let app = app(&pool);
    for who in [&bill, &jar] {
        let reply = open(&app, &number.to_string(), who).await;
        assert_eq!(
            (reply.status, reply.location.as_deref()),
            (303, Some(release.as_str()))
        );
    }
    let cards = Roles::new(true, false).expect("roles");
    let (crew, _) = member(&pool, x, cards, None).await;
    let page = open(&app, &number.to_string(), &crew).await;
    assert_eq!(page.status, 200, "{}", page.text);
    assert!(page.text.contains("The homeowner has picked a quote"));
    assert!(!page.text.contains("Withdraw your quote"));
    shows_none_of_her(&page.text);
}

/// TD-CARD-16 (PICK-051, D-5619): a quote withdrawn before its read-back confirm reads as
/// withdrawn: "Send a new quote", never "Withdraw your quote".
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_card_16_a_withdrawn_never_confirmed_quote_reads_as_withdrawn(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business]).await;
    let mut db = pool.acquire().await.expect("db");
    quote(&mut db, (job, x.business), true).await;
    let app = app(&pool);
    let waiting = open(&app, &number.to_string(), &jar).await;
    assert!(waiting.text.contains("Send a quote"), "{}", waiting.text);
    let gone = withdraw_quote(&mut db, (job, x.business), Timestamp::now()).await;
    assert_eq!(
        gone.expect("withdrawn"),
        askhava_quotes::api::Withdrawal::Withdrawn
    );
    drop(db);
    let page = open(&app, &number.to_string(), &jar).await;
    assert_eq!(page.status, 200, "{}", page.text);
    assert!(page.text.contains("Send a new quote"), "{}", page.text);
    assert!(!page.text.contains("Withdraw your quote"));
    assert!(page.text.contains("Quotes so far: 0."));
}

/// What a "Not a fit for us" answer must leave alone on `job` for `business` (PICK-057): the
/// card holders and the accepted pick (routing), its own quote's standing and the job's ledger
/// totals (fees).
async fn routing_and_fees(pool: &PgPool, (job, business): (Uuid, Uuid)) -> String {
    let mut db = pool.acquire().await.expect("db");
    let holders = card_holders(&mut db, job).await.expect("holders");
    let pick = accepted_pick(&mut db, job)
        .await
        .expect("pick")
        .map(|p| p.pick);
    let standing = quote_standing(&mut db, (job, business)).await;
    let (from, to) = (Timestamp::UNIX_EPOCH, Timestamp::MAX);
    let fees = ledger_totals(&mut db, &[job], from, to).await;
    let standing = standing.expect("standing");
    format!(
        "{holders:?} {pick:?} {standing:?} {:?}",
        fees.expect("fees")
    )
}

/// The month's stored "Not a fit for us" answers, as (reason code, count) (OPS-039's input).
async fn fit_answers(pool: &PgPool) -> Vec<(&'static str, i64)> {
    let mut db = pool.acquire().await.expect("db");
    let counts = not_a_fit_counts(&mut db, Timestamp::now()).await;
    let counts = counts.expect("counts").into_iter();
    counts.map(|c| (c.reason.code(), c.count)).collect()
}

/// TD-CARD-16 (PICK-057, D-5387): the page offers "Not a fit for us" with its four one-tap
/// reasons, posting to `/b/not-a-fit/<job>/job-page`; a tap is recorded, changes no routing or
/// fee and its thanks page links back to `/b/jobs/<number>`; another business's tap on the same
/// form is the not-found answer and records nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_card_16_the_page_offers_not_a_fit(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (_, other) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business]).await;
    let app = app(&pool);
    let page = open(&app, &number.to_string(), &jar).await;
    assert_eq!(page.status, 200, "{}", page.text);
    let action = format!("/b/not-a-fit/{job}/job-page");
    assert!(
        page.text.contains(&format!("action=\"{action}\"")),
        "{}",
        page.text
    );
    for (code, label) in reasons() {
        assert!(page.text.contains(&format!("value=\"{code}\"")), "{code}");
        assert!(page.text.contains(label), "{label}");
    }
    let before = routing_and_fees(&pool, (job, x.business)).await;
    let tap = |who: &CookieJar| {
        let csrf = csrf_token(who).expect("csrf");
        let body = format!("csrf={csrf}&reason={}", reasons()[1].0);
        request(&Method::POST, &action, who, body)
    };
    let theirs = answer(&app, tap(&other)).await;
    assert_eq!(
        (theirs.status, theirs.no_store),
        (404, true),
        "{}",
        theirs.text
    );
    assert!(fit_answers(&pool).await.is_empty());
    let thanks = answer(&app, tap(&jar)).await;
    assert_eq!(thanks.status, 200, "{}", thanks.text);
    assert!(thanks.text.contains("Thanks, got it"), "{}", thanks.text);
    let back = format!("id=\"back-to-job\" href=\"/b/jobs/{number}\"");
    assert!(thanks.text.contains(&back), "{}", thanks.text);
    assert_eq!(fit_answers(&pool).await, [(reasons()[1].0, 1)]);
    assert_eq!(routing_and_fees(&pool, (job, x.business)).await, before);
    shows_none_of_her(&thanks.text);
}
