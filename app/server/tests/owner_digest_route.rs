//! T815 (OPS-030, OPS-033, PRIV-006, SEC-005; D-5318, D-5319): the owner digest page served by
//! the assembled router against PostgreSQL behind T303's owner session (signed in through
//! T409's `owner_sign_in`). The owner sees the week's counts and a planted PROBLEM reply's words
//! with `no-store`, and the newest digest row is marked opened; signed out or a business session
//! gets the owner door's 401 with the sign-in link and marks nothing; no vault field shows. Every
//! name, word, number, token and secret here is fake.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_catalog::api::load_embedded;
use askhava_jobs::api::rules::CheckIn;
use askhava_jobs::api::{Answer, PROBLEM_ALERT, create, job_number, record_check_in_answer};
use askhava_ops::api::{Actor, NewAlert, Severity, digest_sent, latest_digest, raise_alert};
use askhava_portals::api::{
    CookieJar, OWNER_DIGEST_ROUTE, OwnerAuth, Realm, SecondFactor, bootstrap_owner, owner_sign_in,
    start_session,
};
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, NewContact, Vault, keep_problem_words, scrub, store_contact,
};
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use totp_rs::Secret;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const TOKEN: &str = "fake-t815-bootstrap-token-0123456789";
const PASSWORD: &str = "fake t815 owner pass phrase";
const WORDS: &str = "the sample porch light they fitted flickers again";
const NAME: &str = "Quilla Sampleton";
const PHONE: &str = "(928) 555-0173";
const EMAIL: &str = "quilla.sampleton@example.com";

type Jar = BTreeMap<String, String>;

struct Reply {
    status: u16,
    text: String,
}

/// The owner signed in now with a TOTP code.
async fn owner_jar(pool: &PgPool) -> Jar {
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
    own_jar(&jar.expect("owner signed in"))
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

async fn business_jar(pool: &PgPool) -> Jar {
    let mut db = pool.acquire().await.expect("db");
    let life = SignedDuration::from_hours(1);
    let who = (Realm::Business, Uuid::new_v4());
    let jar = start_session(&mut db, CookieJar::new(), who, life, Timestamp::now()).await;
    own_jar(&jar.expect("session"))
}

/// The page's answer for `jar`; every one must be `no-store` (SEC-034).
async fn get(app: &Router, jar: &Jar) -> Reply {
    let cookies: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut request = Request::builder()
        .uri(OWNER_DIGEST_ROUTE)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookies.join("; "))
        .body(Body::empty())
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 15)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let cache = parts.headers.get(header::CACHE_CONTROL);
    let cache = cache.and_then(|v| v.to_str().ok()).unwrap_or_default();
    assert!(cache.contains("no-store"), "{cache}");
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// A day-3 PROBLEM on a new job: its kept words, its answer row and its alert; returns the job's
/// number and the kept words.
async fn problem(db: &mut PgConnection) -> (i64, String) {
    let at = Timestamp::now() - SignedDuration::from_hours(24);
    let job = create(db, at, false, Actor::System).await.expect("job");
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(WORDS, &known, &mut NameMemory::default());
    let inbox = Uuid::new_v4();
    let kept = keep_problem_words(db, inbox, job, &scrubbed, false).await;
    assert!(kept.expect("kept"));
    let how = (CheckIn::Day3, Answer::Problem);
    let answered = record_check_in_answer(db, job, how, Actor::System).await;
    answered.expect("answer");
    let alert = NewAlert {
        kind: PROBLEM_ALERT,
        severity: Severity::Normal,
        subject: Some(("messaging.inbox", inbox)),
    };
    raise_alert(db, &alert).await.expect("alert");
    let number = job_number(db, job).await.expect("number");
    (number, scrubbed.as_str().to_owned())
}

/// A fictional homeowner's vault contact; returns its ID.
async fn contact(db: &mut PgConnection) -> Uuid {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x61), &key(0x62), &url).expect("vault");
    let new = NewContact {
        name: Some(SecretString::from(NAME.to_owned())),
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: Some(SecretString::from(EMAIL.to_owned())),
        ..NewContact::default()
    };
    store_contact(db, &vault, &new).await.expect("contact")
}

async fn opened(pool: &PgPool) -> Option<Timestamp> {
    let mut db = pool.acquire().await.expect("db");
    let row = latest_digest(&mut db)
        .await
        .expect("digest")
        .expect("a row");
    row.opened_at
}

/// D-5319: signed out and a business session get the 401 and mark nothing; the owner sees the
/// counts and the PROBLEM words, marks the digest opened, and sees no vault field.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn t815_only_the_owner_sees_the_digest_and_opens_it(pool: PgPool) {
    let jar = owner_jar(&pool).await;
    let mut db = pool.acquire().await.expect("db");
    let (number, words) = problem(&mut db).await;
    let vault_id = contact(&mut db).await;
    let sent = Timestamp::now() - SignedDuration::from_hours(2);
    digest_sent(&mut db, sent).await.expect("digest row");
    drop(db);
    let parts = RouterParts::from((None, None, None, None));
    let app = build_router(pool.clone(), Router::new(), parts);

    for refused in [Jar::new(), business_jar(&pool).await] {
        let reply = get(&app, &refused).await;
        assert_eq!(reply.status, 401);
        assert!(reply.text.contains("/owner/sign-in"));
        assert!(!reply.text.contains(&words));
        assert_eq!(opened(&pool).await, None, "a refused request marks nothing");
    }

    let reply = get(&app, &jar).await;
    assert_eq!(reply.status, 200, "{}", reply.text);
    assert!(reply.text.contains("Weekly digest"));
    assert!(
        reply
            .text
            .contains(&format!("PROBLEM replies: 1 (#{number})"))
    );
    assert!(reply.text.contains(&format!("Job {number}<")));
    assert!(reply.text.contains(&words), "the owner sees the kept words");
    assert!(
        opened(&pool).await.is_some(),
        "the owner's view opens the digest"
    );
    for private in [NAME, PHONE, EMAIL, "555-0173", &vault_id.to_string()] {
        assert!(!reply.text.contains(private), "{private}");
    }
}
