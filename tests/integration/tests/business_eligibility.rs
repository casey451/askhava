//! TD-WEB-12's business-texting clause (T527; TXT-035, TERMS-014, SEC-043; D-303, D-305, D-3881):
//! the business-texting switch stays off unless the newest filed provider answer says yes and
//! names a business campaign. Every password, code, reference, answer and ID is fake.
//!
//! With no filing, a "no" answer, or a "yes" with no campaign, turning the business line on is
//! refused and it stays off; after a "yes" with a campaign it turns on, and a later "no" switches
//! it off. The homeowner line never needs a filing. Served: the settings page says why the switch
//! is refused; a stale-factor or cross-site `POST` changes nothing. The table is append-only for
//! the app role.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_integration::set_app_role;
use askhava_messaging::api::{
    Audience, BUSINESS_ELIGIBILITY_ACTION, BusinessEligibility, ModeStateError,
    business_eligibility, file_business_eligibility, load_lines, switch_off, switch_on,
};
use askhava_portals::api::{
    CookieJar, OwnerAuth, SecondFactor, bootstrap_owner, csrf_token, owner_sign_in,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Request, header};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// The composition root, compiled in for the router.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the router is used here")]
mod server;
#[allow(
    clippy::wildcard_imports,
    reason = "the included files name their siblings as `crate::`"
)]
use server::*;

const PASSWORD: &str = "fake t527 owner pass phrase";
const TOKEN: &str = "fake-t527-bootstrap-token-0123456789";
const HOST: &str = "laftly.test";
const ROUTE: &str = "/owner/business-texting";
const WHY: &str = "You can't turn it on yet.";
const REFUSED: &str = "Business texting is still off.";
const YES: &str = "&to=file&answered_on=2026-11-04&reference=FAKE-T527-CASE&answer=Fake%20yes&allowed=yes\
     &campaign_reference=FAKE-T527-CAMPAIGN";

type Jar = BTreeMap<String, String>;

/// A fake answer: `allowed`, with `campaign` as the business campaign reference.
fn answer(allowed: bool, campaign: &str) -> BusinessEligibility {
    BusinessEligibility {
        answered_on: "2026-11-04".parse().expect("date"),
        reference: "FAKE-T527-CASE".to_owned(),
        answer: "FAKE-T527 answer text as received.".to_owned(),
        allowed,
        campaign_reference: campaign.to_owned(),
    }
}

/// The stored (homeowner, business) switches.
async fn lines(db: &mut PgConnection) -> (bool, bool) {
    let l = load_lines(db, (None, None)).await.expect("lines");
    (l.homeowner.texting_on, l.business.texting_on)
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect(sql)
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_web_12_business_texting_stays_off_unless_the_filed_answer_is_yes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (owner, now) = (Uuid::from_u128(0x527), Timestamp::now());
    let by = (owner, now);
    let business = Audience::Business;
    let refused = Err(ModeStateError::NotEligible);
    assert_eq!(
        switch_on(&mut db, business, by).await,
        refused,
        "none filed"
    );
    let bad = BusinessEligibility {
        reference: " ".to_owned(),
        ..answer(true, "FAKE-T527-CAMPAIGN")
    };
    let filed = file_business_eligibility(&mut db, owner, &bad, now).await;
    assert_eq!(filed, Err(ModeStateError::BadFiling));
    assert!(business_eligibility(&mut db).await.expect("read").is_none());
    for (allowed, campaign) in [(false, "FAKE-T527-CAMPAIGN"), (true, ""), (true, "  ")] {
        let filing = answer(allowed, campaign);
        file_business_eligibility(&mut db, owner, &filing, now)
            .await
            .expect("filed");
        assert_eq!(switch_on(&mut db, business, by).await, refused);
        assert_eq!(lines(&mut db).await, (true, false), "business stays off");
    }
    assert_eq!(switch_off(&mut db, Audience::Homeowner, by).await, Ok(true));
    assert_eq!(switch_on(&mut db, Audience::Homeowner, by).await, Ok(true));
    let yes = answer(true, "FAKE-T527-CAMPAIGN");
    file_business_eligibility(&mut db, owner, &yes, now)
        .await
        .expect("filed");
    let (newest, _) = business_eligibility(&mut db)
        .await
        .expect("read")
        .expect("one");
    assert_eq!(newest, yes);
    assert_eq!(switch_on(&mut db, business, by).await, Ok(true));
    assert_eq!(lines(&mut db).await, (true, true), "homeowner unaffected");
    let no = answer(false, "FAKE-T527-CAMPAIGN");
    file_business_eligibility(&mut db, owner, &no, now)
        .await
        .expect("filed");
    assert_eq!(
        lines(&mut db).await,
        (true, false),
        "a later no switches it off"
    );
    let audits =
        "SELECT count(*) FROM ops.audit_log WHERE action = 'messaging.business_eligibility_filed'";
    assert_eq!(
        BUSINESS_ELIGIBILITY_ACTION,
        "messaging.business_eligibility_filed"
    );
    assert_eq!(count(&pool, audits).await, 5);
    let leaked = "SELECT count(*) FROM ops.audit_log a WHERE to_jsonb(a)::text LIKE '%FAKE-T527%'";
    assert_eq!(
        count(&pool, leaked).await,
        0,
        "no answer text in the audit log"
    );
    set_app_role(&mut db).await.expect("app role");
    let changed = sqlx::query("UPDATE messaging.business_eligibility SET allowed = true")
        .execute(&mut *db)
        .await;
    assert!(changed.is_err(), "append-only for the app role");
}

/// RFC 6238's 6-digit SHA-1 code for `secret` at `now`.
fn totp_code(secret: &[u8], now: Timestamp) -> String {
    let step = u64::try_from(now.as_second() / 30).expect("step");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("key");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 0x0f);
    let word = u32::from_be_bytes(hash[offset..offset + 4].try_into().expect("four bytes"));
    format!("{:06}", (word & 0x7fff_ffff) % 1_000_000)
}

/// The served router after the owner was bootstrapped.
async fn app(pool: &PgPool) -> Router {
    let mut db = pool.acquire().await.expect("db");
    bootstrap_owner(&mut db, Some(TOKEN), PASSWORD, Timestamp::now())
        .await
        .expect("bootstrap");
    let parts = server::RouterParts::from((None, None, None, None));
    server::build_router(pool.clone(), Router::new(), parts)
}

/// The owner signed in at `at` with a TOTP code (fresh for 5 minutes from `at`).
async fn owner_at(pool: &PgPool, at: Timestamp) -> Jar {
    let mut db = pool.acquire().await.expect("db");
    let secret: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM portals.owner_factor")
        .fetch_one(&mut *db)
        .await
        .expect("secret");
    let auth = OwnerAuth::new(HOST, &format!("https://{HOST}")).expect("relying party");
    let code = totp_code(&secret, at);
    let factor = (PASSWORD, SecondFactor::Totp(&code));
    let jar = owner_sign_in(&mut db, CookieJar::new(), factor, &auth, at).await;
    jar.expect("signed in")
        .iter()
        .map(|c| (c.name().to_owned(), c.value().to_owned()))
        .collect()
}

/// A `method` to `path` with `jar`'s cookies and CSRF token, cross-site when `cross`.
async fn send(
    app: &Router,
    (method, path): (&str, &str),
    jar: &Jar,
    fields: &str,
    cross: bool,
) -> (u16, String) {
    let line: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut headers = HeaderMap::new();
    let cookie = HeaderValue::from_str(&line.join("; ")).expect("cookie");
    headers.insert(header::COOKIE, cookie.clone());
    let csrf = csrf_token(&CookieJar::from_headers(&headers)).unwrap_or_default();
    let (origin, site) = if cross {
        ("https://elsewhere.test".to_owned(), "cross-site")
    } else {
        (format!("https://{HOST}"), "same-origin")
    };
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin)
        .header("sec-fetch-site", site)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{fields}")))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 27)), 40_000);
    request.extensions_mut().insert(ConnectInfo(peer));
    let answer = app.clone().oneshot(request).await.expect("answer");
    let status = answer.status().as_u16();
    let bytes = to_bytes(answer.into_body(), 1 << 22).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_web_12_the_served_switch_shows_why_and_needs_a_fresh_same_site_post(pool: PgPool) {
    let app = app(&pool).await;
    let now = Timestamp::now();
    let rows = "SELECT count(*) FROM messaging.business_eligibility";
    let stale = owner_at(&pool, now - SignedDuration::from_mins(6)).await;
    let (status, _) = send(&app, ("POST", ROUTE), &stale, YES, false).await;
    assert_eq!(status, 401, "a stale factor gets the step-up form");
    let fresh = owner_at(&pool, now).await;
    let (status, _) = send(&app, ("POST", ROUTE), &fresh, YES, true).await;
    assert_eq!(status, 403, "a cross-site post is refused");
    assert_eq!(count(&pool, rows).await, 0, "nothing filed");
    let (status, page) = send(&app, ("GET", "/owner/settings"), &fresh, "", false).await;
    assert_eq!(status, 200);
    assert!(
        page.contains("Business texting is off") && page.contains(WHY),
        "says why"
    );
    let (status, page) = send(&app, ("POST", ROUTE), &fresh, "&to=on", false).await;
    assert_eq!(status, 400, "{page}");
    assert!(page.contains(REFUSED) && page.contains(WHY));
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(lines(&mut db).await, (true, false));
    let (status, page) = send(&app, ("POST", ROUTE), &fresh, YES, false).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("FAKE-T527-CAMPAIGN") && !page.contains(WHY));
    let (status, page) = send(&app, ("POST", ROUTE), &fresh, "&to=on", false).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("Done. Business texting is on."));
    assert_eq!(lines(&mut db).await, (true, true), "homeowner unaffected");
    let (status, _) = send(&app, ("POST", ROUTE), &fresh, "&to=off", true).await;
    assert_eq!(status, 403);
    assert_eq!(
        lines(&mut db).await,
        (true, true),
        "a cross-site off changes nothing"
    );
}
