//! TD-SEC-43 (T300, D-2117; SEC-004, SEC-009, SEC-024, SEC-034, SEC-042, SEC-048): the business
//! portal served by the assembled router over PostgreSQL. Each served `GET` path answers no
//! business person (signed out, an owner-portal session, a removed member; homeowner sessions are
//! `door_matrix.rs`'s, TD-SEC-61) with a 4xx
//! linking to `/b/sign-in`, never a redirect and no homeowner detail; another business's released
//! pick, a "Cards and quotes"-only member on a released pick or on billing, and an unknown pick ID
//! are the one not-found answer and read nothing from the vault; every answer is `no-store`; a
//! cross-site (`Sec-Fetch-Site` or `Origin`) or wrong-token `POST` to either form is refused,
//! changes nothing and is a `csrf_rejected` security event; a non-owner's channel change is
//! refused and changes nothing. People sign in through `portals`' emailed-link sign-in, as
//! `door_matrix.rs` does, since `server` writes no session row. The released pick is built through
//! module APIs only, as `door_matrix.rs` builds its own (T401): a quoted job, her request naming it,
//! her page pick and its $0 release. Security events are counted from their one log line, since
//! their table is `portals`' (TD-SEC-10). Every ID, name, address and key is fake.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{
    AccountStatus, Acting, OpenCardsTimes, Roles, add_member, add_owner, create, open_cards_times,
    remove_member,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Guarded, MIGRATOR, Settings, SystemClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create as create_job, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor as AuditActor, EXPORT_PAGE_MAX, export_audit};
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{PagePick, Released, Settled, page_pick, release, settle};
use askhava_portals::api::{
    CookieJar, PortalsError, Realm, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress,
    SignInMail, TIP_LINE, csrf_token, ensure_pre_session, signin_post, signin_request,
    start_session,
};
use askhava_privacy::api::{NewContact, store_contact};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};

/// The homeowner's fake first and last name, which no refusal may show.
const HER_NAME: &str = "Dana Testperson";
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

/// How a request is sent: same-origin with the right token, cross-site by `Sec-Fetch-Site`,
/// cross-site by `Origin` alone, or same-origin with a wrong token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    OtherOrigin,
    WrongToken,
}

fn request(
    method: &Method,
    path: &str,
    jar: &CookieJar,
    (site, extra): (Site, &str),
) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        _ => csrf_token(jar).unwrap_or_default(),
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-request-id", Uuid::new_v4().to_string())
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    builder = match site {
        Site::OtherOrigin => builder.header(header::ORIGIN, "https://elsewhere.example"),
        Site::Cross => builder.header("sec-fetch-site", "cross-site"),
        Site::Own | Site::WrongToken => builder
            .header("sec-fetch-site", "same-origin")
            .header(header::ORIGIN, format!("https://{HOST}")),
    };
    let mut request = builder
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 7], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// An answer as the cases judge it.
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: u16,
    redirect: bool,
    no_store: bool,
    text: String,
    cookies: Vec<String>,
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cache = parts.headers.get(header::CACHE_CONTROL);
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        redirect: parts.headers.contains_key(header::LOCATION),
        no_store: cache.is_some_and(|v| v.to_str().is_ok_and(|v| v.contains("no-store"))),
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

/// `to`'s person signed in through `portals`' emailed link (D-1724).
async fn business_jar(pool: &PgPool, to: SignInAddress) -> CookieJar {
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
    let asked = request(
        &Method::POST,
        SIGNIN_ROUTE,
        &jar,
        (Site::Own, "&email=person%40x.example"),
    );
    answer(&app, asked).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = answer(&app, request(&Method::POST, &path, &jar, (Site::Own, ""))).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        pressed.cookies.join("; ").parse().expect("cookies"),
    );
    CookieJar::from_headers(&headers)
}

/// An owner-portal session, which is no business member's (SEC-004). A homeowner session is
/// `door_matrix.rs`'s (TD-SEC-61 probes every served row as both homeowners), so this file, which
/// stores a ledger row, names no homeowner realm (TD-FEE-07).
async fn owner_portal(pool: &PgPool) -> CookieJar {
    let mut db = pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(12);
    let login = (Realm::Owner, Uuid::new_v4());
    start_session(&mut db, CookieJar::new(), login, life, Timestamp::now())
        .await
        .expect("session")
}

/// A person of a business: their acting IDs and signed-in cookies.
struct Person {
    by: Acting,
    jar: CookieJar,
}

/// A new active business's owner at `address`.
async fn owner(pool: &PgPool, address: &str) -> Person {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox");
    let business = create(&mut db, false, now).await.expect("business");
    let member = add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    drop(db);
    let jar = business_jar(pool, SignInAddress { business, mailbox }).await;
    Person {
        by: Acting { business, member },
        jar,
    }
}

/// A member of `owner`'s business at `address` with `roles`, signed in.
async fn member(pool: &PgPool, owner: Acting, address: &str, roles: Roles) -> Person {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox");
    let member = add_member(&mut db, owner, mailbox, roles, now)
        .await
        .expect("member");
    drop(db);
    let jar = business_jar(
        pool,
        SignInAddress {
            business: owner.business,
            mailbox,
        },
    )
    .await;
    Person {
        by: Acting {
            business: owner.business,
            member,
        },
        jar,
    }
}

/// An AC repair job in Lake Havasu City as matching reads it, and `business` able to take it.
fn card_for(business: Uuid) -> (CardJob, CardBusiness) {
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
    let candidate = Job {
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
    (card, to)
}

/// A firm $325 quote form and its rules.
fn firm_quote() -> (QuoteForm, QuoteRules) {
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
    (form, rules)
}

/// The job opened, carded to `business` and quoted by it (Quotes ready).
async fn quoted_job(db: &mut PgConnection, business: Uuid, now: Timestamp) -> Uuid {
    let job = create_job(db, now, false, AuditActor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, AuditActor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let (card, to) = card_for(business);
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(db, map, job, &card, &[to], now).await;
    assert_eq!(sent.expect("card").len(), 1);
    let (form, rules) = firm_quote();
    let quote = parse_quote(form, &rules).expect("quote");
    submit_quote(db, (job, business), &quote, now)
        .await
        .expect("quoted");
    job
}

/// Her vault contact (fake name, phone and street) and her text request naming `job`.
async fn her_request(db: &mut PgConnection, job: Uuid, now: Timestamp) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from(HER_NAME)),
        phone: Some(SecretString::from("+19285550192")),
        email: None,
        address: Some(SecretString::from("300 Fake Lane")),
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: "my AC is warm",
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
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(db, &secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation,
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(db, &answer).await.expect("job named");
    contact
}

/// The fake masked pool the release shows (CALL-017).
fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
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
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1"),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("twilio")
        .pool()
        .clone()
}

/// `business`'s page pick on a new quoted job, released to her on the $0 path (PICK-022, FEE-012,
/// D-1739): free first win, `no_fee_release`, settled `free`, the minted proof and the release.
async fn released_pick(pool: &PgPool, business: Uuid) -> Uuid {
    let now = Timestamp::now();
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let job = quoted_job(&mut db, business, now).await;
    let contact = her_request(&mut db, job, now).await;
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, AuditActor::System, now).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    let key = format!("t300.free.{job}.{business}");
    let entry = NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    };
    ledger_append(&mut db, &LedgerEntry::new(entry).expect("entry"))
        .await
        .expect("ledger");
    let cleared = no_fee_release(&mut db, pick, (business, job))
        .await
        .expect("cleared");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    assert_eq!(
        settle(&mut tx, &cleared).await.expect("settle"),
        Settled::Moved(job)
    );
    tx.commit().await.expect("commit");
    let proof = askhava_server::release_minter().mint(cleared, contact);
    let shown = release(pool, (&vault(), &masked()), (proof, false), now).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
    pick
}

/// The assembled router, with the vault the portal's vault pages read.
fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    let parts = RouterParts::from(hooks).with_vault(Some(vault()));
    build_router(pool.clone(), Router::new(), parts)
}

/// Audit actions recorded so far (vault reads and notice changes).
async fn actions(pool: &PgPool) -> Vec<String> {
    let mut db = pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, EXPORT_PAGE_MAX)
        .await
        .expect("audit");
    rows.into_iter().map(|row| row.action).collect()
}

/// The security events logged while it is the default subscriber: one line per stored event,
/// written by the same `record_security_event` call (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    /// How many events of `kind` were logged.
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
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(kind);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// The five served `GET` paths, the job page on `pick`.
fn served(pick: Uuid) -> [String; 5] {
    [
        "/b".to_owned(),
        format!("/b/picks/{pick}"),
        "/b/billing".to_owned(),
        "/b/billing/statement.csv".to_owned(),
        "/b/notices".to_owned(),
    ]
}

async fn get(app: &Router, path: &str, jar: &CookieJar) -> Reply {
    answer(app, request(&Method::GET, path, jar, (Site::Own, ""))).await
}

/// TD-SEC-43 (SEC-004, SEC-034): the owner reaches every served page, `no-store`; signed out, an
/// owner-portal session and a removed member's session each get a 4xx
/// linking to `/b/sign-in`, never a redirect, with no homeowner detail, also `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_43_no_business_person_gets_the_sign_in_link(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let gone = member(&pool, x.by, "gone@x.example", Roles::BOTH).await;
    let mut db = pool.acquire().await.expect("db");
    remove_member(&mut db, x.by, gone.by.member, Timestamp::now())
        .await
        .expect("removed");
    drop(db);
    let (app, pick) = (app(&pool), released_pick(&pool, x.by.business).await);
    for path in served(pick) {
        let mine = get(&app, &path, &x.jar).await;
        assert!(
            mine.status == 200 && mine.no_store,
            "{path} for the owner: {}",
            mine.status
        );
    }
    let job = get(&app, &format!("/b/picks/{pick}"), &x.jar).await;
    assert!(
        job.text.contains(TIP_LINE) && job.text.contains("Dana picked you"),
        "released"
    );
    let outsiders = [
        ensure_pre_session(CookieJar::new()).expect("pre-session"),
        owner_portal(&pool).await,
        gone.jar,
    ];
    for (n, jar) in outsiders.iter().enumerate() {
        for path in served(pick) {
            let r = get(&app, &path, jar).await;
            let refused =
                (400..500).contains(&r.status) && !r.redirect && r.text.contains("/b/sign-in");
            assert!(
                refused && r.no_store,
                "outsider {n} on {path}: {} {:?}",
                r.status,
                r.text
            );
            assert!(
                !r.text.contains(TIP_LINE) && !r.text.contains("Dana"),
                "{path}"
            );
        }
    }
}

/// TD-SEC-43 (SEC-019, SEC-048): another business's owner on a released pick, a "Cards and
/// quotes"-only member on the released pick and on billing, and an unknown or malformed pick ID
/// all get the same not-found answer, read nothing from the vault and are `authz_denied` events.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_43_refusals_are_one_not_found_and_read_nothing(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let y = owner(&pool, "owner@y.example").await;
    let cards = Roles::new(true, false).expect("roles");
    let quoter = member(&pool, x.by, "quotes@x.example", cards).await;
    let (app, pick) = (app(&pool), released_pick(&pool, x.by.business).await);
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let reads = actions(&pool).await;
    let released = format!("/b/picks/{pick}");
    let unknown = format!("/b/picks/{}", Uuid::new_v4());
    let cases = [
        (&y.jar, released.as_str()),
        (&quoter.jar, released.as_str()),
        (&quoter.jar, "/b/billing"),
        (&quoter.jar, "/b/billing/statement.csv"),
        (&x.jar, unknown.as_str()),
        (&x.jar, "/b/picks/not-an-id"),
    ];
    let first = get(&app, cases[0].1, cases[0].0).await;
    assert!(first.status == 404 && first.no_store, "{first:?}");
    for (jar, path) in cases {
        assert_eq!(get(&app, path, jar).await, first, "{path}");
    }
    assert_eq!(actions(&pool).await, reads, "nothing read from the vault");
    assert_eq!(events.count("authz_denied"), 7);
    let home = get(&app, "/b", &quoter.jar).await;
    assert_eq!(home.status, 200, "a member still reaches the portal home");
    let mine = get(&app, &released, &x.jar).await;
    assert!(
        mine.status == 200 && actions(&pool).await != reads,
        "the owner's view is a read"
    );
}

/// TD-SEC-43 (SEC-009, SEC-042, SEC-048): a cross-site `POST` (by `Sec-Fetch-Site` or by
/// `Origin`) or one with a wrong CSRF token to either form is a 403 that changes nothing and is
/// one `csrf_rejected` event; the same form sent properly is stored.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_43_cross_site_and_wrong_token_posts_change_nothing(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let app = app(&pool);
    let forms = [
        ("/b/open-cards", "&time=8am"),
        ("/b/notices", "&channel=email"),
    ];
    let times = async || {
        let mut db = pool.acquire().await.expect("db");
        open_cards_times(&mut db, x.by.member)
            .await
            .expect("times")
            .times()
    };
    let (before, audit) = (times().await, actions(&pool).await);
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    assert_eq!(before, OpenCardsTimes::DEFAULT.times());
    for (n, site) in [Site::Cross, Site::OtherOrigin, Site::WrongToken]
        .into_iter()
        .enumerate()
    {
        for (k, (path, body)) in forms.into_iter().enumerate() {
            let r = answer(&app, request(&Method::POST, path, &x.jar, (site, body))).await;
            assert_eq!(r.status, 403, "{site:?} {path}");
            assert_eq!(
                events.count("csrf_rejected"),
                n * 2 + k + 1,
                "{site:?} {path}"
            );
        }
    }
    assert_eq!(times().await, before, "no time stored");
    assert_eq!(actions(&pool).await, audit, "no channel change logged");
    for (path, body) in forms {
        let r = answer(
            &app,
            request(&Method::POST, path, &x.jar, (Site::Own, body)),
        )
        .await;
        assert!(r.status == 303 && r.no_store, "{path}: {}", r.status);
    }
    assert_ne!(times().await, before, "the proper form is stored");
}

/// TD-SEC-43 (SEC-024, BIZ-036): a member who is not the owner sees the notice page without the
/// form, and a channel change from them is the not-found answer, stores nothing and is logged as
/// refused; the owner's change is stored with its audit row.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_43_only_the_owner_changes_the_notice_channel(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let staff = member(&pool, x.by, "staff@x.example", Roles::BOTH).await;
    let app = app(&pool);
    let page = get(&app, "/b/notices", &staff.jar).await;
    assert!(
        page.status == 200 && page.text.contains("owner@x.example"),
        "{}",
        page.status
    );
    assert!(
        !page.text.contains("action=\"/b/notices\""),
        "no form for a member"
    );
    let send = |jar| {
        request(
            &Method::POST,
            "/b/notices",
            jar,
            (Site::Own, "&channel=email"),
        )
    };
    let refused = answer(&app, send(&staff.jar)).await;
    assert_eq!(refused.status, 404);
    let logged = actions(&pool).await;
    assert!(
        logged
            .iter()
            .any(|a| a == "businesses.notice_channel_refused")
    );
    assert!(
        !logged
            .iter()
            .any(|a| a == "businesses.notice_channel_changed")
    );
    let stored = answer(&app, send(&x.jar)).await;
    assert_eq!(stored.status, 303);
    let logged = actions(&pool).await;
    assert!(
        logged
            .iter()
            .any(|a| a == "businesses.notice_channel_changed")
    );
}

/// TD-SEC-43 (SEC-010): a post to either form with an unknown field, or with a channel value the
/// form does not offer, or with no channel, is refused with a 4xx and stores nothing; no channel
/// change is logged.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_43_forms_refuse_unknown_fields_and_values(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let app = app(&pool);
    let times = async || {
        let mut db = pool.acquire().await.expect("db");
        open_cards_times(&mut db, x.by.member)
            .await
            .expect("times")
            .times()
    };
    let (before, audit) = (times().await, actions(&pool).await);
    let posts = [
        ("/b/notices", "&channel=garbage"),
        ("/b/notices", "&channel=email&evil=1"),
        ("/b/notices", ""),
        ("/b/open-cards", "&time=8am&foo=bar"),
    ];
    for (path, body) in posts {
        let r = answer(
            &app,
            request(&Method::POST, path, &x.jar, (Site::Own, body)),
        )
        .await;
        assert!(
            (400..500).contains(&r.status),
            "{path} {body}: {}",
            r.status
        );
    }
    assert_eq!(times().await, before, "no time stored");
    assert_eq!(actions(&pool).await, audit, "no channel change logged");
}
