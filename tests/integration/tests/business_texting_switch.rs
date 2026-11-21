//! Business texting follows the stored switch, never a constant (T526; TXT-035, BIZ-036, WEB-024,
//! D-3881, D-3882).
//!
//! One case walks the three states: the switch off, the switch on with no
//! `ASKHAVA_BUSINESS_NUMBER`, and both. In each it reads `businesses::api::business_texting_on`,
//! the served sign-up page and notice settings (the composition root compiled in place, as
//! `task_registry.rs` does, D-736), and runs a card notice through `run_business_notice` with a
//! `NoticeSender` built as the worker builds it. A second case shows a failed read counts as off.
//! The business page's own lines are T769's (D-4931). Every ID, address and number is fake.

use std::net::SocketAddr;
use std::sync::Arc;

use askhava_businesses::api::{
    Checks, ConsentChannel, Event, NoticeChannel, add_owner, apply, business_texting_on, create,
    may_text, record_consent, request_welcome, set_notice_channel, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Claimed, IdSecret, ManualClock};
use askhava_messaging::api::{
    Audience, BUSINESS_NOTICE_KIND, BusinessEligibility, MailboxKind, NoticeSender, NoticeWorker,
    SendChecks, SendingNumber, file_business_eligibility, run_business_notice, switch_on,
};
use askhava_ops::api::Actor;
use askhava_portals::api::{CookieJar, PageLanguage, Realm, notice_line, start_session};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_quotes::api::{CardNotice, queue_card_notice};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the router and the switch reader are called here"
)]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t526-not-a-real-key";
const POSTAL: &str = "PO Box 0000, Lake Havasu City, AZ 86403";
/// A fake business sending number (a 555-01xx line).
const NUMBER: &str = "+19285550126";
/// Noon in Arizona (19:00 UTC), inside the send window.
const NOON: &str = "2026-11-10T19:00:00Z";

fn key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51).into(), &key(0x61).into(), &url).expect("vault")
}

fn number(set: bool) -> Option<SendingNumber> {
    set.then(|| SendingNumber::parse(NUMBER).expect("number"))
}

/// `businesses::api::may_text` read at send time, as the composition's checks do.
struct Gate;

impl SendChecks for Gate {
    async fn may_text(&self, conn: &mut PgConnection, business: Uuid, phone: Uuid) -> bool {
        may_text(conn, business, phone).await.unwrap_or(false)
    }

    async fn accept_by(&self, _: &mut PgConnection, _: Uuid, _: Timestamp) -> Option<Timestamp> {
        None
    }
}

/// The router as `main` assembles it: the sign-up and the portal, with `ASKHAVA_BUSINESS_NUMBER`
/// set or not.
fn app(pool: &PgPool, with_number: bool) -> Router {
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@db.invalid/askhava".to_owned(),
        ),
        ("ASKHAVA_ID_SECRET", ID_SECRET.to_owned()),
        ("VAULT_KEY", key(0x51)),
        ("VAULT_INDEX_KEY", key(0x61)),
    ];
    let set = with_number.then(|| ("ASKHAVA_BUSINESS_NUMBER", NUMBER.to_owned()));
    let pairs = pairs.into_iter().chain(set).map(|(k, v)| (k.to_owned(), v));
    let signup = business_signup::ServedSignUp::from_settings(pool.clone(), pairs).expect("part");
    let parts = RouterParts::from((None, None, None, None))
        .with_sign_up(signup)
        .with_vault(Some(vault()))
        .with_business_number(number(with_number));
    build_router(pool.clone(), Router::new(), parts)
}

/// The body of a 200 `GET path` sent with `jar`'s cookies.
async fn page(app: &Router, path: &str, jar: &CookieJar) -> String {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .uri(path)
        .header(header::HOST, "laftly.test")
        .header(header::COOKIE, cookies.join("; "))
        .body(Body::empty())
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 7], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = app.clone().oneshot(request).await.expect("answer");
    assert_eq!(response.status(), 200, "{path}");
    let bytes = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// An active business whose owner chose text notices for a consented phone, and the owner's
/// signed-in cookies. Returns the business, the phone and the cookies.
async fn business(db: &mut PgConnection) -> (Uuid, Uuid, CookieJar) {
    let t: Timestamp = NOON.parse().expect("time");
    let mailbox = mailbox_for(db, &vault(), "owner@x.example")
        .await
        .expect("mailbox");
    let b = create(db, false, t).await.expect("business");
    let owner = add_owner(db, b, mailbox, t).await.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    request_welcome(db, b, t).await.expect("welcome");
    welcome_sent(db, b, t).await.expect("sent");
    let phone: Uuid = sqlx::query_scalar(
        "INSERT INTO businesses.staff_phone (business_id, linked_by, linked_at) \
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(b)
    .bind(owner)
    .fetch_one(&mut *db)
    .await
    .expect("phone");
    record_consent(
        db,
        (b, owner, Some(phone)),
        &[ConsentChannel::Text],
        true,
        t,
    )
    .await
    .expect("box");
    set_notice_channel(db, b, (NoticeChannel::Text, true), t)
        .await
        .expect("text");
    let life = SignedDuration::from_hours(12);
    let login = (Realm::Business, owner);
    let jar = start_session(db, CookieJar::new(), login, life, Timestamp::now()).await;
    sqlx::query("INSERT INTO portals.business_session SELECT id, $1 FROM portals.session")
        .bind(b)
        .execute(db)
        .await
        .expect("business session");
    (b, phone, jar.expect("session"))
}

/// Queues `b`'s card notice for `job` as `intake` does (the switch read with `with_number`), runs
/// it through the worker's handler and returns how many (texts to `phone`, emails) are stored.
async fn card_sent(
    pool: &PgPool,
    (b, phone): (Uuid, Uuid),
    job: u128,
    with_number: bool,
) -> (i64, i64) {
    let (mut db, now) = (
        pool.acquire().await.expect("db"),
        NOON.parse().expect("time"),
    );
    let notice = CardNotice {
        number: 1042,
        category: "plumbing",
        town: "Lake Havasu City",
        card: "Kitchen sink drips under the cabinet. How soon: this week.",
    };
    let on = business_texting_on(&mut db, number(with_number)).await;
    let pair = (Uuid::from_u128(job), b);
    assert!(
        queue_card_notice(&mut db, pair, &notice, (on, now))
            .await
            .expect("card")
    );
    let (id, payload) =
        sqlx::query_as("SELECT id, payload FROM core.task ORDER BY id DESC LIMIT 1")
            .fetch_one(&mut *db)
            .await
            .expect("task");
    let secret = SecretString::from(ID_SECRET.to_owned());
    let links = IdSecret::new(1, secret.clone()).expect("links");
    let sender = NoticeSender::new((secret, links), POSTAL, MailboxKind::PoBox).expect("sender");
    let worker = Arc::new(NoticeWorker {
        sender: sender.with_business_number(number(with_number)),
        checks: Gate,
        clock: Arc::new(ManualClock::new(now)),
    });
    let task = Claimed {
        id,
        kind: BUSINESS_NOTICE_KIND.to_owned(),
        payload,
        attempts: 1,
    };
    run_business_notice(pool.clone(), worker, task)
        .await
        .expect("notice run");
    let texts = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1")
        .bind(phone)
        .fetch_one(&mut *db)
        .await;
    let emails = sqlx::query_scalar("SELECT count(*) FROM messaging.email_outbox")
        .fetch_one(&mut *db)
        .await;
    (texts.expect("texts"), emails.expect("emails"))
}

/// Whether the sign-up form and the notice settings offer text, each also showing its line for
/// that state (WEB-024 on every sign-up screen; the settings' email-only sentence).
async fn offered(pool: &PgPool, jar: &CookieJar, with_number: bool) -> (bool, bool) {
    let app = app(pool, with_number);
    let signup = page(&app, "/b/signup", &CookieJar::new()).await;
    let form = signup.contains("id=\"notice-choice\"");
    let line = notice_line(form, PageLanguage::En).0;
    let first = line.split('.').next().expect("a sentence");
    assert!(
        signup.contains(first),
        "the sign-up shows the WEB-024 line: {first}"
    );
    let settings = page(&app, "/b/notices", jar).await;
    let choice = settings.contains("value=\"text\"");
    assert_eq!(
        choice,
        !settings.contains("email is the only choice for now")
    );
    (form, choice)
}

/// Files a fake allowed provider answer naming a campaign (T527's gate), then turns the stored
/// business switch on; whether anything changed.
async fn switched_on(db: &mut PgConnection) -> bool {
    let (owner, now) = (Uuid::from_u128(0x526), NOON.parse().expect("time"));
    let allowed = BusinessEligibility {
        answered_on: "2026-11-04".parse().expect("date"),
        reference: "FAKE-T526-CASE".to_owned(),
        answer: "FAKE-T526 answer text as received.".to_owned(),
        allowed: true,
        campaign_reference: "FAKE-T526-CAMPAIGN".to_owned(),
    };
    file_business_eligibility(db, owner, &allowed, now)
        .await
        .expect("filed");
    let switched = switch_on(db, Audience::Business, (owner, now)).await;
    switched.expect("switched")
}

/// TXT-035, BIZ-036, WEB-024, D-3881, D-3882: off, on without a number, then on with one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn business_texting_follows_the_stored_switch_and_the_number(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let (b, phone, jar) = business(&mut db).await;
    let who = (b, phone);
    assert!(
        !business_texting_on(&mut db, number(true)).await,
        "off by default"
    );
    assert_eq!(
        offered(&pool, &jar, true).await,
        (false, false),
        "switch off"
    );
    assert_eq!(
        card_sent(&pool, who, 0x5261, true).await,
        (0, 1),
        "email only"
    );

    assert!(switched_on(&mut db).await, "the stored switch is on");
    assert!(
        !business_texting_on(&mut db, None).await,
        "no number is off"
    );
    assert_eq!(
        offered(&pool, &jar, false).await,
        (false, false),
        "no number"
    );
    assert_eq!(
        card_sent(&pool, who, 0x5262, false).await,
        (0, 2),
        "still email"
    );

    assert!(
        business_texting_on(&mut db, number(true)).await,
        "switch and number"
    );
    assert_eq!(
        offered(&pool, &jar, true).await,
        (true, true),
        "text is offered"
    );
    let sent = card_sent(&pool, who, 0x5263, true).await;
    assert_eq!(
        sent,
        (1, 2),
        "the consented phone gets the text, no new email"
    );
}

/// D-3881: a read that fails counts as off, for `businesses`' reader and the server's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_switch_read_counts_as_off(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    assert!(switched_on(&mut db).await);
    let set = number(true);
    assert!(business_portal::business_texting_on(&mut db, set.as_ref()).await);
    sqlx::query("BEGIN").execute(&mut *db).await.expect("begin");
    let broken = sqlx::query("SELECT 1 / 0").execute(&mut *db).await;
    assert!(
        broken.is_err(),
        "the transaction is aborted, so every read fails"
    );
    assert!(!business_texting_on(&mut db, set.clone()).await);
    assert!(!business_portal::business_texting_on(&mut db, set.as_ref()).await);
    sqlx::query("ROLLBACK")
        .execute(&mut *db)
        .await
        .expect("rollback");
    assert!(
        business_texting_on(&mut db, set).await,
        "readable again: on"
    );
}
