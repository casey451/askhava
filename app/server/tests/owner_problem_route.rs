//! TD-BIZ-59 (T712, D-4423; JOB-016, BIZ-012, SEC-006): the owner's PROBLEM reports page and its
//! "Count as a complaint" press, served by the assembled router against PostgreSQL behind T303's
//! owner session (signed in through T409's `owner_sign_in`). Each day-3 PROBLEM goes through
//! `jobs::api::answer_check_in` with its scrubbed words kept, as TD-BIZ-58's list does. No session,
//! a business session, a cross-site `POST` and a wrong CSRF token record nothing; every answer is
//! `no-store`. The homeowner-session case runs in T348's `door_matrix.rs` over these two section 13
//! rows: this file builds its settled pick through `payments` (the $0 first win), and TD-FEE-07
//! forbids a file naming the homeowner realm from naming `payments` (D-5570). Every name, word,
//! token and secret here is fake.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    AccountStatus, PROBLEM_COUNTED_ACTION, counted_jobs, create as create_business, has_safety_word,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Guarded, MIGRATOR};
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{
    Answer, Inbound, JobStatus, PROBLEM_ALERT, answer_check_in, create, job_number, move_to,
    record_job_facts,
};
use askhava_licensing::api::{ClassMap, Holder, Job as LicensedJob, LicenseKind};
use askhava_ops::api::{Actor, EXPORT_PAGE_MAX, digest_alerts, export_audit};
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{PagePick, Settled, page_pick, settle};
use askhava_portals::api::{
    CookieJar, OwnerAuth, Realm, SecondFactor, bootstrap_owner, csrf_token, ensure_pre_session,
    owner_sign_in, start_session,
};
use askhava_privacy::api::{CommonWords, Known, NameMemory, keep_problem_words, scrub};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t712-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t712 owner pass phrase";
const LIST: &str = "/owner/problem-reports";
const PRESS: &str = "/owner/problem-complaint";
const WORDS: &str = "the sample tap still drips after the visit";
const LATER: &str = "the sample fence gate was left open";
const KINDS: [&str; 5] = ["No-show", "Conduct", "Work or price", "Safety", "Other"];

type Jar = BTreeMap<String, String>;

struct World {
    app: Router,
    pool: PgPool,
    jar: Jar,
    events: Events,
    _guard: tracing::subscriber::DefaultGuard,
}

struct Reply {
    status: u16,
    text: String,
    location: String,
}

/// The router and the owner signed in now with a TOTP code.
async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let now = Timestamp::now();
    let enrolment = bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, now).await;
    let base32 = enrolment.expect("bootstrap").totp_secret;
    let secret = Secret::try_from_base32(base32.expose_secret()).expect("base32");
    let code = totp_code(secret.as_bytes(), now);
    let auth = OwnerAuth::new(HOST, "https://askhava.test").expect("party");
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, now).await;
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    let parts = RouterParts::from((None, None, None, None));
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        jar: own_jar(&jar.expect("owner signed in")),
        events,
        _guard: guard,
    }
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `at`.
fn totp_code(secret: &[u8], at: Timestamp) -> String {
    let step = u64::try_from(at.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

fn own_jar(jar: &CookieJar) -> Jar {
    jar.iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

fn csrf(jar: &Jar) -> String {
    let mut headers = HeaderMap::new();
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let value = HeaderValue::from_str(&cookies.join("; ")).expect("cookie");
    headers.insert(header::COOKIE, value);
    csrf_token(&CookieJar::from_headers(&headers)).unwrap_or_default()
}

async fn realm_session(pool: &PgPool, realm: Realm) -> Jar {
    let mut db = pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(1);
    let who = (realm, Uuid::new_v4());
    let jar = start_session(&mut db, CookieJar::new(), who, life, Timestamp::now()).await;
    own_jar(&jar.expect("session"))
}

/// A same-origin request with `jar`'s cookies and `body`.
fn request(method: &str, path: &str, jar: &Jar, body: String) -> Request<Body> {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 12)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// `request`'s answer; every one must be `no-store` (SEC-034).
async fn answer(w: &World, request: Request<Body>) -> Reply {
    let what = format!("{} {}", request.method(), request.uri());
    let response = w.app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let value = |name| {
        let found = parts.headers.get(name).and_then(|v| v.to_str().ok());
        found.unwrap_or_default().to_owned()
    };
    assert!(value(header::CACHE_CONTROL).contains("no-store"), "{what}");
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        location: value(header::LOCATION),
    }
}

async fn get(w: &World, path: &str, jar: &Jar) -> Reply {
    answer(w, request("GET", path, jar, String::new())).await
}

/// A same-origin press of `alert` with `kind` and `jar`'s CSRF token.
async fn press(w: &World, jar: &Jar, alert: &str, kind: &str) -> Reply {
    let body = format!("csrf={}&alert={alert}&kind={kind}", csrf(jar));
    answer(w, request("POST", PRESS, jar, body)).await
}

/// How many `PROBLEM_COUNTED_ACTION` audit rows there are (one per recorded complaint).
async fn recorded(w: &World) -> usize {
    let mut db = w.pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX).await;
    let rows = rows.expect("audit");
    rows.iter()
        .filter(|r| r.action == PROBLEM_COUNTED_ACTION)
        .count()
}

fn at() -> Timestamp {
    Timestamp::now() - SignedDuration::from_hours(120)
}

/// A job opened, carded to `business` (AC repair in Lake Havasu City) and quoted by it with a
/// firm $325 quote, as `call_status.rs` builds one.
async fn quoted_job(db: &mut PgConnection, business: Uuid) -> Uuid {
    let job = create(db, at(), false, Actor::System).await.expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, at(), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let kind = ("hvac_repair".to_owned(), "AC not cooling".to_owned());
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([kind.clone()]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let candidate = LicensedJob {
        category: kind.0,
        job_type: kind.1,
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    let card = CardJob {
        towns: vec![town],
        candidates: vec![candidate],
        text: String::new(),
    };
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(db, map, job, &card, &[to], at()).await;
    sent.expect("card");
    let quote = firm_quote();
    let quoted = submit_quote(db, (job, business), &quote, at()).await;
    quoted.expect("quoted");
    job
}

/// A firm $325 quote.
fn firm_quote() -> Quote {
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor included".to_owned(),
        ai_read: false,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    parse_quote(form, &rules).expect("quote")
}

/// A job quoted by a new business, picked on the page and settled `free` (the $0 first win), so
/// the job is Connected with a settled pick naming that business.
async fn settled_job(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let business = create_business(&mut db, false, at()).await;
    let business = business.expect("business");
    let job = quoted_job(&mut db, business).await;
    let types = ["AC not cooling".to_owned()];
    let facts = record_job_facts(&mut db, job, "hvac_repair", &types).await;
    facts.expect("job facts");
    let key = format!("t712.free.{job}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    });
    let added = ledger_append(&mut db, &entry.expect("entry")).await;
    added.expect("ledger");
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, at()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let mut tx = pool.begin().await.expect("tx");
    let settled = settle(&mut tx, &cleared.expect("cleared")).await;
    tx.commit().await.expect("commit");
    assert_eq!(settled.expect("settle"), Settled::Moved(job));
    job
}

/// A job walked to Connected with no pick at all.
async fn unpicked_job(db: &mut PgConnection) -> Uuid {
    let job = create(db, at(), false, Actor::System).await.expect("job");
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    for s in path.windows(2) {
        let moved = move_to(db, job, (s[0], s[1]), at(), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    job
}

/// A Connected job (with a settled pick when `picked`) and a day-3 PROBLEM of `words`, its
/// scrubbed words kept: `(job, job number, alert)`.
async fn reported(pool: &PgPool, words: &str, picked: bool) -> (Uuid, i64, Uuid) {
    let job = if picked {
        settled_job(pool).await
    } else {
        unpicked_job(&mut pool.acquire().await.expect("db")).await
    };
    let mut db = pool.acquire().await.expect("db");
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(words, &known, &mut NameMemory::default());
    let inbox = Uuid::new_v4();
    let safety = has_safety_word(words);
    let kept = keep_problem_words(&mut db, inbox, job, &scrubbed, safety).await;
    assert!(kept.expect("kept"));
    let how = (Answer::Problem, CheckIn::Day3, Contact::Email);
    let inbound = Inbound { inbox, safety };
    let now = (Timestamp::now(), Actor::System);
    let answered = answer_check_in(&mut db, job, how, inbound, now).await;
    assert!(answered.expect("answer").applied().is_some());
    let window = (at(), Timestamp::now() + SignedDuration::from_mins(1));
    let alerts = digest_alerts(&mut db, &[PROBLEM_ALERT], window.0, window.1).await;
    let subject = Some(("messaging.inbox".to_owned(), inbox));
    let alerts = alerts.expect("alerts");
    let alert = alerts.iter().find(|a| a.subject == subject).expect("alert");
    let number = job_number(&mut db, job).await.expect("number");
    (job, number, alert.id)
}

/// The `security event` log lines captured, `[kind, route]` (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<[String; 2]>>>);

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
            let found = fields.0.iter().find(|(n, _)| n == name);
            found.map(|(_, v)| v.clone()).unwrap_or_default()
        };
        if get("message") == "security event" {
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push([get("kind"), get("route")]);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn events(w: &World, kind: &str) -> usize {
    let all = w.events.0.lock().unwrap_or_else(PoisonError::into_inner);
    all.iter().filter(|e| e[0] == kind && e[1] == PRESS).count()
}

/// TD-BIZ-59 (JOB-016, BIZ-012, SEC-006): the owner home links to the list; the list shows each
/// report newest first, the uncounted one with the five kinds and none chosen; a press records one
/// complaint and answers 303 to the list, which then marks it counted with no form; a second press
/// records nothing more. No homeowner contact is shown.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_59_the_owner_counts_a_report_once(pool: PgPool) {
    let w = world(&pool).await;
    let (job, older, alert) = reported(&pool, WORDS, true).await;
    let (_, newer, _) = reported(&pool, LATER, true).await;
    let home = get(&w, "/owner/home", &w.jar).await;
    assert!(
        home.text
            .contains(&format!(r#"href="{LIST}">Problem reports</a>"#))
    );
    let list = get(&w, LIST, &w.jar).await;
    assert_eq!(list.status, 200, "{}", list.text);
    let (at_new, at_old) = (format!("Job {newer}<"), format!("Job {older}<"));
    let found = |t: &str| list.text.find(t).expect("listed");
    assert!(found(&at_new) < found(&at_old), "newest first");
    assert!(
        list.text
            .contains(&format!(r#"name="alert" value="{alert}""#))
    );
    assert_eq!(
        list.text.matches("Count as a complaint</button>").count(),
        2
    );
    for kind in KINDS {
        assert!(list.text.contains(&format!(">{kind}</label>")), "{kind}");
    }
    assert!(!list.text.contains(" checked>"), "no kind preselected");
    assert!(!list.text.contains("tel:") && !list.text.contains("mailto:"));
    let done = press(&w, &w.jar, &alert.to_string(), "work_or_price").await;
    assert_eq!((done.status, done.location.as_str()), (303, LIST));
    assert_eq!(recorded(&w).await, 1, "one complaint");
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(counted_jobs(&mut db, &[job]).await.expect("read"), [job]);
    let list = get(&w, LIST, &w.jar).await;
    assert_eq!(list.text.matches("Counted as a complaint").count(), 1);
    assert_eq!(
        list.text.matches("Count as a complaint</button>").count(),
        1
    );
    let again = press(&w, &w.jar, &alert.to_string(), "conduct").await;
    assert_eq!((again.status, again.location.as_str()), (303, LIST));
    assert_eq!(recorded(&w).await, 1, "a second press records nothing more");
}

/// TD-BIZ-59 (JOB-016): no kind, an unknown or malformed alert and a job with no picked business
/// show the list again with one line saying why and record nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_59_a_refused_press_records_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let (_, _, alert) = reported(&pool, WORDS, true).await;
    let (_, _, unpicked) = reported(&pool, LATER, false).await;
    let cases = [
        (alert.to_string(), "", "Choose what kind of complaint"),
        (alert.to_string(), "praise", "Choose what kind of complaint"),
        (Uuid::new_v4().to_string(), "other", "find that report."),
        ("not-an-id".to_owned(), "other", "find that report."),
        (unpicked.to_string(), "conduct", "a confirmed pick yet"),
    ];
    for (id, kind, line) in cases {
        let reply = press(&w, &w.jar, &id, kind).await;
        assert_eq!(reply.status, 400, "{id} {kind}");
        assert!(reply.text.contains("<title>Error: "), "{id} {kind}");
        assert!(reply.text.contains(line), "{id} {kind}: {line}");
    }
    let kept = press(&w, &w.jar, &unpicked.to_string(), "conduct").await;
    assert!(
        kept.text.contains(r#"value="conduct" checked>"#),
        "the kind is kept"
    );
    assert_eq!(recorded(&w).await, 0, "nothing recorded");
}

/// TD-BIZ-59 (SEC-006, SEC-019, SEC-042, SEC-048): no session and a business session are refused
/// on both routes (a homeowner session: `door_matrix.rs`, D-5570); a cross-site (`Sec-Fetch-Site` or `Origin`) or wrong-token
/// press from the owner is a 403 logged as `csrf_rejected`; none records anything.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_59_other_doors_and_forged_presses_record_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let (_, _, alert) = reported(&pool, WORDS, true).await;
    let id = alert.to_string();
    let visitor = own_jar(&ensure_pre_session(CookieJar::new()).expect("pre-session"));
    let business = realm_session(&pool, Realm::Business).await;
    for (who, jar) in [("none", &visitor), ("business", &business)] {
        let list = get(&w, LIST, jar).await;
        assert_eq!(list.status, 401, "{who} GET");
        assert!(!list.text.contains(WORDS), "{who} sees no report");
        let reply = press(&w, jar, &id, "conduct").await;
        assert!((400..500).contains(&reply.status), "{who} POST");
    }
    let good = format!("csrf={}&alert={id}&kind=conduct", csrf(&w.jar));
    let wrong = format!("csrf={}&alert={id}&kind=conduct", "0".repeat(64));
    let mut by_fetch = request("POST", PRESS, &w.jar, good.clone());
    let cross = HeaderValue::from_static("cross-site");
    by_fetch.headers_mut().insert("sec-fetch-site", cross);
    let mut by_origin = request("POST", PRESS, &w.jar, good);
    by_origin.headers_mut().remove("sec-fetch-site");
    let evil = HeaderValue::from_static("https://evil.example");
    by_origin.headers_mut().insert(header::ORIGIN, evil);
    let by_token = request("POST", PRESS, &w.jar, wrong);
    for (case, forged) in [
        ("fetch", by_fetch),
        ("origin", by_origin),
        ("token", by_token),
    ] {
        assert_eq!(answer(&w, forged).await.status, 403, "{case}");
    }
    assert_eq!(
        events(&w, "csrf_rejected"),
        3,
        "each forged press is logged"
    );
    assert_eq!(recorded(&w).await, 0, "nothing recorded");
}
