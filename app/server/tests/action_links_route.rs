//! The email action links through the assembled router (TD-SEC-44, TD-BIZ-50; T301; BIZ-038,
//! PICK-040, PICK-007, SEC-025, SEC-026, SEC-034, SEC-042, SEC-048, BIZ-049; D-1955, D-2128,
//! D-2710, D-3189, D-3675).
//!
//! `server::build_router` over a real test database, no optional part configured: every kind's
//! `GET` shows its page twice and changes nothing; a token for another kind, object or person,
//! an expired token and a removed member's token get the one not-found answer; only the Quote
//! form's `POST` with its CSRF token acts, once (a firm quote with every quote fact), and a quote
//! with a phone number bounces and stores nothing; every other kind's `POST` is not found and
//! uses nothing; a cross-site or wrong-token `POST` is a 403 and changes nothing; every answer is
//! `no-store`. TD-BIZ-50: a price above the business's largest job is warned and sent only when
//! posted again with the warned cents; a business with no capability row is never warned. The
//! security events of the refusals are checked over HTTP by `door_matrix.rs` (TD-SEC-61). Every
//! name, phone and ID is fake. T456 (PICK-043, D-3890): the Hold link's `POST` acts by the link
//! alone, once, moving the stored Confirm end; a refused Hold says why (409) and leaves the link
//! unused, and a removed member's link on a held pick is the one not-found answer (the emergency
//! and Away refusals are `tests/integration/tests/hold_away.rs`'). T543 (BIZ-034, D-285,
//! D-3890): the Away link's `POST` acts once with the link's own person's session, storing the
//! end the hours give; out-of-range hours show the page again with the A11Y-030 error and leave
//! the link unused; another person's link or one naming another business is the one not-found
//! answer, unused. T463 (BIZ-028, D-309, D-3675, D-3890): the Busy link's `POST` acts once with
//! the link's own person's session, pausing job cards and storing the end; days outside 1 to 60
//! show the page again with `BUSY_DAYS_ERROR` and leave the link unused, as does a business that
//! is not active (409); another person's link or one naming another business is not found.
//! T457 (SEC-004, SEC-027, D-3675, D-3889): the Confirm link's `POST` with no session texts a
//! code through the fake Verify of `with_confirm_link_codes` and uses nothing; a wrong code uses
//! nothing; the right code uses the link, signs that device in and answers 303 to the Confirm
//! page; the link's own person's live session goes straight there unless their inbox is shared;
//! with no codes or no texting phone nothing is sent or used.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{
    AccountStatus, Acting, Capability, CapabilityAnswers, Checks, Event, LargestJob, PauseReason,
    Roles, add_member, add_owner, apply, away_until, business, busy_until,
    create as create_business, record_step_up, remove_member, save_capability,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Guarded, SystemClock};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor as AuditActor;
use askhava_picks::api::{ConfirmSettings, HoldAsk, PagePick, hold_ask, notice_sent, page_pick};
use askhava_portals::api::{
    ActionKind, ActionTarget, BUSY_DAYS_ERROR, CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SignIn,
    SignInAddress, SignInMail, csrf_token, ensure_pre_session, mint_action_link, open_action_link,
    send_sign_in_link, signin_post,
};
use askhava_portals::api::{
    CodeAddress, CodeFuture, CodeSent, CodeTo, ConfirmCodes, business_principal, set_inbox_shared,
};
use askhava_quotes::api::{
    CardBusiness, CardFacts, CardJob, CardParts, DEFAULT_MIN_WORDS, OfferTown, Place, build_card,
    card_digest, keep_card, mark_card_sent, send_cards, stored_facts,
};
use askhava_server::business_signup::IdentityCodes;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.example";

/// A token no link was minted with.
const UNISSUED: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";

/// The quote facts every quote carries (PICK-040), visiting ten days from now.
fn facts() -> String {
    let day = Timestamp::now() + SignedDuration::from_hours(240);
    let date = day.to_zoned(TimeZone::UTC).date();
    format!(
        "&trip_fee=none&parts=included&permit=not_needed&may_change=no&visit_date={date}\
&visit_part=morning&spanish=no"
    )
}

/// A firm $325 quote with every quote fact (PICK-040).
fn firm() -> String {
    format!("&amount=325&notes=Parts+and+labor+included.{}", facts())
}

/// The same quote with a phone number in its note (PICK-007).
fn phoned() -> String {
    format!("&amount=325&notes=Call+me+at+928+555+0190{}", facts())
}

/// One answer: status, whether it is `no-store`, and its body.
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: u16,
    no_store: bool,
    text: String,
}

/// How a `POST` is sent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    WrongToken,
}

/// One business: its ID and owner member.
#[derive(Clone, Copy)]
struct Team {
    business: Uuid,
    member: Uuid,
}

/// The router, the pool and a carded job of one business.
struct World {
    app: Router,
    pool: PgPool,
    team: Team,
    job: Uuid,
}

/// The assembled router with no optional part (D-481).
fn router(pool: PgPool) -> Router {
    askhava_server::build_router(pool, Router::new(), (None, None, None, None))
}

/// Sends `method path` signed out, with `extra` form fields.
async fn send(w: &World, request: (&Method, &str, &str), site: Site) -> Reply {
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    send_as(w, request, site, &jar).await
}

/// `method path` with `jar`'s cookies, its CSRF token (or a wrong one) and `extra` fields.
fn request(
    (method, path, extra): (&Method, &str, &str),
    site: Site,
    jar: &CookieJar,
) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        Site::Own | Site::Cross => csrf_token(jar).unwrap_or_default(),
    };
    let fetch = if site == Site::Cross {
        "cross-site"
    } else {
        "same-origin"
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", fetch)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request")
}

/// Sends [`request`] through the router and reads its answer.
async fn send_as(w: &World, asked: (&Method, &str, &str), site: Site, jar: &CookieJar) -> Reply {
    let response = w.app.clone().oneshot(request(asked, site, jar)).await;
    let (parts, body) = response.expect("response").into_parts();
    let cache = parts.headers.get(header::CACHE_CONTROL);
    let no_store = cache
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("no-store"));
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        no_store,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// An active, verified R-39 business serving Lake Havasu City that ticked AC not cooling.
fn card_business(id: Uuid) -> CardBusiness {
    CardBusiness {
        id,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

/// An AC repair job in Lake Havasu City, as matching reads it.
fn card_job() -> CardJob {
    let job = Job {
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
        candidates: vec![job],
        text: String::new(),
    }
}

/// Keeps and marks sent `job`'s card, made from fake parts (D-3189).
async fn sent(db: &mut PgConnection, job: Uuid, now: Timestamp) {
    let parts = CardParts {
        category: "AC repair".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "The AC runs but the air from the vents stays warm.".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let digest = card_digest(card.text());
    let facts = CardFacts {
        category: "hvac_repair".to_owned(),
        candidates: vec!["AC not cooling".to_owned()],
        emergency: false,
    };
    let kept = keep_card(db, (Uuid::new_v4(), job), &digest, &card, &facts, now).await;
    kept.expect("kept");
    assert!(mark_card_sent(db, job, &digest, now).await.expect("sent"));
}

/// An open job with its facts row, carded to `team` with the card kept and sent.
async fn carded_job(db: &mut PgConnection, team: Team, now: Timestamp) -> Uuid {
    let job = create(db, now, false, AuditActor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, AuditActor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let types = ["AC not cooling".to_owned()];
    record_job_facts(db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let map = ClassMap::shipped().expect("license map");
    let to = [card_business(team.business)];
    let cards = send_cards(db, map, job, &card_job(), &to, now).await;
    assert_eq!(cards.expect("card").len(), 1);
    sent(db, job, now).await;
    job
}

/// A business with its owner and a carded job.
async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let now = Timestamp::now();
    let business = create_business(&mut db, false, now)
        .await
        .expect("business");
    let member = add_owner(&mut db, business, Uuid::new_v4(), now)
        .await
        .expect("owner");
    let team = Team { business, member };
    let job = carded_job(&mut db, team, now).await;
    World {
        app: router(pool.clone()),
        pool: pool.clone(),
        team,
        job,
    }
}

/// A `kind` link for `object` for `member` of `team`'s business, live from `from` to `ends`.
async fn mint(
    w: &World,
    (kind, object): (ActionKind, Uuid),
    member: Uuid,
    (from, ends): (Timestamp, Timestamp),
) -> String {
    let mut db = w.pool.acquire().await.expect("db");
    let target = ActionTarget {
        kind,
        object,
        member,
        business: w.team.business,
    };
    let hold = HoldAsk {
        by: Roles::BOTH,
        emergency: false,
        held: false,
        away_delayed: false,
    };
    let link = mint_action_link(&mut db, target, Some(hold), (from, ends)).await;
    let link = link.expect("minted");
    let path = link.expose_secret().split(HOST).last().map(str::to_owned);
    path.unwrap_or_else(|| link.expose_secret().to_owned())
}

/// A live `kind` link for the owner.
async fn live(w: &World, kind: ActionKind, object: Uuid) -> String {
    let now = Timestamp::now();
    let span = (now, now + SignedDuration::from_hours(2));
    mint(w, (kind, object), w.team.member, span).await
}

/// The path `/b/a/<kind>/<object>/<token>` of a full link.
fn path_of(link: &str) -> String {
    let at = link.find("/b/a/").expect("an action path");
    link[at..].to_owned()
}

/// Whether the link at `path` is still unused and unexpired.
async fn unused(w: &World, path: &str) -> bool {
    let parts: Vec<&str> = path.trim_start_matches("/b/a/").split('/').collect();
    let kind = ActionKind::from_slug(parts[0]).expect("kind");
    let object = Uuid::parse_str(parts[1]).expect("object");
    let mut db = w.pool.acquire().await.expect("db");
    open_action_link(&mut db, (kind, object), parts[2], Timestamp::now())
        .await
        .is_ok()
}

/// The one not-found answer: an unissued Quote token on the job.
async fn not_found(w: &World, method: &Method) -> Reply {
    let path = format!("/b/a/quote/{}/{UNISSUED}", w.job);
    let reply = send(w, (method, &path, ""), Site::Own).await;
    assert_eq!(reply.status, 404);
    assert!(reply.no_store, "the not-found answer is no-store (SEC-034)");
    reply
}

/// The object a `kind` link names in these cases: the job for Quote, the business otherwise.
fn object_for(w: &World, kind: ActionKind) -> Uuid {
    if kind == ActionKind::Quote {
        w.job
    } else {
        w.team.business
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_every_kind_get_shows_its_page_twice_and_changes_nothing(pool: PgPool) {
    let w = world(&pool).await;
    for kind in ActionKind::ALL {
        let path = path_of(&live(&w, kind, object_for(&w, kind)).await);
        for _ in 0..2 {
            let reply = send(&w, (&Method::GET, &path, ""), Site::Own).await;
            assert_eq!(reply.status, 200, "{kind:?}: {}", reply.text);
            assert!(reply.no_store, "{kind:?} is no-store (SEC-034)");
            assert!(
                reply.text.contains("name=\"csrf\""),
                "{kind:?} has its button"
            );
        }
        assert!(
            unused(&w, &path).await,
            "{kind:?}: a GET used the link (SEC-026)"
        );
    }
    let quote = path_of(&live(&w, ActionKind::Quote, w.job).await);
    let form = send(&w, (&Method::GET, &quote, ""), Site::Own).await;
    assert!(
        form.text.contains("Send quote for job"),
        "the Quote link opens the quote form"
    );
    assert!(form.text.contains("/static/portals/forms.css"));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_wrong_kind_object_person_expired_and_removed_are_not_found(pool: PgPool) {
    let w = world(&pool).await;
    let now = Timestamp::now();
    let mine = path_of(&live(&w, ActionKind::Quote, w.job).await);
    let token = mine.rsplit('/').next().expect("token").to_owned();
    let other = {
        let mut db = pool.acquire().await.expect("db");
        carded_job(&mut db, w.team, now).await
    };
    let stranger = {
        let mut db = pool.acquire().await.expect("db");
        let business = create_business(&mut db, false, now)
            .await
            .expect("business");
        add_owner(&mut db, business, Uuid::new_v4(), now)
            .await
            .expect("owner");
        business
    };
    let past = (
        now - SignedDuration::from_hours(3),
        now - SignedDuration::from_hours(1),
    );
    let expired = path_of(&mint(&w, (ActionKind::Quote, w.job), w.team.member, past).await);
    let removed = removed_member_link(&w, now).await;
    let cases = [
        format!("/b/a/hold/{}/{token}", w.job),
        format!("/b/a/quote/{other}/{token}"),
        format!("/b/a/quote/{stranger}/{token}"),
        format!("/b/a/nothing/{}/{token}", w.job),
        format!("/b/a/quote/not-a-uuid/{token}"),
        expired,
        removed,
    ];
    for method in [Method::GET, Method::POST] {
        let answer = not_found(&w, &method).await;
        for path in &cases {
            let reply = send(&w, (&method, path, &firm()), Site::Own).await;
            assert_eq!(
                reply, answer,
                "{method} {path}: not the one not-found answer"
            );
        }
    }
    assert!(unused(&w, &mine).await, "no wrong token used the real link");
    let mut db = pool.acquire().await.expect("db");
    let facts = stored_facts(&mut db, w.job, w.team.business)
        .await
        .expect("facts");
    assert!(facts.is_none(), "nothing was quoted");
}

/// A Quote link minted for a "Cards and quotes" member who is then removed (SEC-024).
async fn removed_member_link(w: &World, now: Timestamp) -> String {
    let mut db = w.pool.acquire().await.expect("db");
    let by = Acting {
        business: w.team.business,
        member: w.team.member,
    };
    let roles = Roles::new(true, false).expect("roles");
    let member = add_member(&mut db, by, Uuid::new_v4(), roles, now)
        .await
        .expect("member");
    drop(db);
    let span = (now, now + SignedDuration::from_hours(2));
    let link = mint(w, (ActionKind::Quote, w.job), member, span).await;
    let mut db = w.pool.acquire().await.expect("db");
    remove_member(&mut db, by, member, now)
        .await
        .expect("removed");
    path_of(&link)
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number(pool: PgPool) {
    let w = world(&pool).await;
    let path = path_of(&live(&w, ActionKind::Quote, w.job).await);
    let bounced = send(&w, (&Method::POST, &path, &phoned()), Site::Own).await;
    assert_eq!(bounced.status, 200);
    assert!(
        bounced.text.contains("Error: "),
        "the bounce shows its reason"
    );
    assert!(
        bounced.text.contains("id=\"notes-error\""),
        "the note's contact bounce shows by the notes box"
    );
    assert!(
        unused(&w, &path).await,
        "a bounced quote leaves the link unused"
    );
    let missing = send(&w, (&Method::POST, &path, "&notes=Hi"), Site::Own).await;
    assert_eq!(missing.status, 200);
    assert!(
        missing.text.contains("Error: "),
        "a missing price and facts show the form again"
    );
    assert!(
        missing.text.contains("<ul id=\"fee-lines\"></ul>"),
        "no fee line without a price"
    );
    let mut db = pool.acquire().await.expect("db");
    let none = stored_facts(&mut db, w.job, w.team.business)
        .await
        .expect("facts");
    assert!(none.is_none(), "nothing stored before a good quote");
    let sent = send(&w, (&Method::POST, &path, &firm()), Site::Own).await;
    assert_eq!(sent.status, 200, "{}", sent.text);
    assert!(sent.no_store);
    assert!(sent.text.contains("Quotes so far: 1."), "{}", sent.text);
    let facts = stored_facts(&mut db, w.job, w.team.business)
        .await
        .expect("facts");
    assert!(
        facts.is_some(),
        "the quote and its facts are stored together"
    );
    assert!(!unused(&w, &path).await, "the stored quote used the link");
    let again = send(&w, (&Method::POST, &path, &firm()), Site::Own).await;
    assert_eq!(
        again,
        not_found(&w, &Method::POST).await,
        "a used link acts no more"
    );
}

/// The kinds whose `POST` acts with a business session (T455, T543, T463, D-3675, D-3839,
/// D-3890).
const SESSION_ACTS: [ActionKind; 6] = [
    ActionKind::Away,
    ActionKind::Busy,
    ActionKind::Pause,
    ActionKind::Resume,
    ActionKind::AskCharge,
    ActionKind::Approve,
];

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_every_other_kind_post_is_not_found_and_uses_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let answer = not_found(&w, &Method::POST).await;
    for kind in ActionKind::ALL
        .into_iter()
        .filter(|k| *k != ActionKind::Quote && !SESSION_ACTS.contains(k))
        .filter(|k| *k != ActionKind::Confirm)
    {
        let path = path_of(&live(&w, kind, object_for(&w, kind)).await);
        let reply = send(&w, (&Method::POST, &path, "&days=3"), Site::Own).await;
        assert_eq!(
            reply, answer,
            "{kind:?}: not the one not-found answer (D-3675)"
        );
        assert!(unused(&w, &path).await, "{kind:?}: its POST used the link");
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_a_cross_site_or_wrong_token_post_is_refused(pool: PgPool) {
    let w = world(&pool).await;
    let path = path_of(&live(&w, ActionKind::Quote, w.job).await);
    for site in [Site::Cross, Site::WrongToken] {
        let reply = send(&w, (&Method::POST, &path, &firm()), site).await;
        assert_eq!(reply.status, 403, "a refused POST is a 403 (SEC-042)");
        assert!(reply.no_store);
    }
    assert!(
        unused(&w, &path).await,
        "a refused POST leaves the link unused"
    );
    let mut db = pool.acquire().await.expect("db");
    let facts = stored_facts(&mut db, w.job, w.team.business)
        .await
        .expect("facts");
    assert!(facts.is_none(), "a refused POST stores nothing");
}

/// Stores `largest` as `team`'s largest job (BIZ-049).
async fn largest_job(w: &World, largest: LargestJob) {
    let mut db = w.pool.acquire().await.expect("db");
    let answers = CapabilityAnswers {
        ticked: Vec::new(),
        towns: vec!["lake_havasu_city".to_owned()],
        capability: Capability {
            largest,
            recurring: false,
        },
        after_hours: false,
    };
    let saved = save_capability(
        &mut db,
        w.team.business,
        &[],
        &answers,
        (AuditActor::System, Timestamp::now()),
    );
    saved.await.expect("capability");
}

/// PICK-039, D-3672: the warned form, priced, shows its fee lines.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_50_a_quote_over_the_largest_job_is_warned_then_sent_once(pool: PgPool) {
    let w = world(&pool).await;
    largest_job(&w, LargestJob::Cents(20_000)).await;
    let path = path_of(&live(&w, ActionKind::Quote, w.job).await);
    let warned = send(&w, (&Method::POST, &path, &firm()), Site::Own).await;
    assert!(
        warned.text.contains("id=\"largest-warning\""),
        "{}",
        warned.text
    );
    assert!(
        warned
            .text
            .contains("name=\"over_largest\" value=\"32500\"")
    );
    assert!(unused(&w, &path).await, "a warned quote is not sent");
    assert!(
        warned.text.contains("<ul id=\"fee-lines\"><li>If picked,"),
        "the warned form shows the introduction fee by job size before submit (PICK-039)"
    );
    let other = format!("{}&over_largest=30000", firm());
    let again = send(&w, (&Method::POST, &path, &other), Site::Own).await;
    assert!(
        again.text.contains("id=\"largest-warning\""),
        "another amount is warned again"
    );
    assert!(unused(&w, &path).await);
    let confirmed = format!("{}&over_largest=32500", firm());
    let sent = send(&w, (&Method::POST, &path, &confirmed), Site::Own).await;
    assert!(sent.text.contains("Quotes so far: 1."), "{}", sent.text);
    assert!(!unused(&w, &path).await, "the confirmed quote is sent once");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_50_a_business_with_no_capability_row_is_never_warned(pool: PgPool) {
    let w = world(&pool).await;
    let path = path_of(&live(&w, ActionKind::Quote, w.job).await);
    let big = firm().replace("amount=325", "amount=900000");
    let sent = send(&w, (&Method::POST, &path, &big), Site::Own).await;
    assert!(!sent.text.contains("largest-warning"), "{}", sent.text);
    assert!(sent.text.contains("Quotes so far: 1."), "{}", sent.text);
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

/// A new "Confirm and billing" member of `w`'s business, signed in through its emailed sign-in
/// link on the router (D-1724): its ID and cookies.
async fn signed_in(w: &World) -> (Uuid, CookieJar) {
    let (now, mailbox) = (Timestamp::now(), Uuid::new_v4());
    let mut db = w.pool.acquire().await.expect("db");
    let by = Acting {
        business: w.team.business,
        member: w.team.member,
    };
    let roles = Roles::new(false, true).expect("roles");
    let member = add_member(&mut db, by, mailbox, roles, now)
        .await
        .expect("member");
    let to = SignInAddress {
        business: w.team.business,
        mailbox,
    };
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    send_sign_in_link(&mut db, &*mail, to, now)
        .await
        .expect("link");
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pre = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let press = Router::new().route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app = SignIn::new(w.pool.clone(), Arc::new(SystemClock), mail).into_router(press);
    let pressed = app.oneshot(request((&Method::POST, &path, ""), Site::Own, &pre));
    let pressed = pressed.await.expect("pressed");
    let cookies: Vec<&str> = pressed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    (member, CookieJar::from_headers(&headers))
}

/// `w`'s business moved by `event` (a fake earlier state), as the owner or sign-up would.
async fn moved(w: &World, event: Event) {
    let mut db = w.pool.acquire().await.expect("db");
    let now = Timestamp::now();
    let actor = AuditActor::Owner(Uuid::new_v4());
    apply(&mut db, w.team.business, event, actor, now)
        .await
        .expect("moved");
}

/// `w`'s business's status and pause reason, by their stored codes.
async fn status(w: &World) -> (&'static str, Option<&'static str>) {
    let mut db = w.pool.acquire().await.expect("db");
    let read = business(&mut db, w.team.business).await.expect("business");
    (read.status.code(), read.pause_reason.map(PauseReason::code))
}

/// A live `kind` link on `object` for `member`.
async fn live_for(w: &World, (kind, object): (ActionKind, Uuid), member: Uuid) -> String {
    let now = Timestamp::now();
    let span = (now, now + SignedDuration::from_hours(2));
    path_of(&mint(w, (kind, object), member, span).await)
}

/// A `POST` of `path` with no fields but the CSRF token.
async fn act(w: &World, path: &str, site: Site, jar: &CookieJar) -> Reply {
    send_as(w, (&Method::POST, path, ""), site, jar).await
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_pause_and_resume_act_once_with_the_persons_session(pool: PgPool) {
    let w = world(&pool).await;
    let (me, jar) = signed_in(&w).await;
    let pause = live_for(&w, (ActionKind::Pause, w.team.business), me).await;
    let out = send(&w, (&Method::POST, &pause, ""), Site::Own).await;
    assert_eq!(out.status, 401, "signed out is asked to sign in (SEC-004)");
    assert!(out.no_store && out.text.contains("/b/sign-in"));
    for site in [Site::Cross, Site::WrongToken] {
        assert_eq!(
            act(&w, &pause, site, &jar).await.status,
            403,
            "refused (SEC-042)"
        );
    }
    let pending = act(&w, &pause, Site::Own, &jar).await;
    assert_eq!(pending.status, 409, "{}", pending.text);
    assert!(
        pending.text.contains("Only an active account"),
        "it says why"
    );
    assert!(
        unused(&w, &pause).await,
        "a refused pause leaves the link unused"
    );
    assert_eq!(status(&w).await, ("pending", None));
    let checks = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    moved(&w, Event::Checked(checks)).await;
    let done = act(&w, &pause, Site::Own, &jar).await;
    assert_eq!(done.status, 200, "{}", done.text);
    assert!(done.no_store && done.text.contains("Job cards paused"));
    assert_eq!(status(&w).await, ("paused", Some("self")));
    let again = act(&w, &pause, Site::Own, &jar).await;
    assert_eq!(
        again,
        not_found(&w, &Method::POST).await,
        "a used link acts no more"
    );
    let resume = live_for(&w, (ActionKind::Resume, w.team.business), me).await;
    assert_eq!(act(&w, &resume, Site::Own, &jar).await.status, 200);
    assert_eq!(status(&w).await, ("active", None));
    moved(&w, Event::Pause(PauseReason::Owner)).await;
    let resume = live_for(&w, (ActionKind::Resume, w.team.business), me).await;
    let theirs = act(&w, &resume, Site::Own, &jar).await;
    assert_eq!(theirs.status, 409, "a pause not its own is not resumed");
    assert!(theirs.text.contains("Only a pause you set yourself"));
    assert!(unused(&w, &resume).await);
    assert_eq!(status(&w).await, ("paused", Some("owner")));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_another_persons_link_or_no_charge_is_not_found(pool: PgPool) {
    let w = world(&pool).await;
    let (me, jar) = signed_in(&w).await;
    let (_, other) = signed_in(&w).await;
    let answer = not_found(&w, &Method::POST).await;
    let pause = live_for(&w, (ActionKind::Pause, w.team.business), me).await;
    let theirs = send_as(&w, (&Method::POST, &pause, ""), Site::Own, &other).await;
    assert_eq!(theirs, answer, "another person's link (SEC-025, D-3614)");
    assert!(unused(&w, &pause).await);
    let uncharged = live_for(&w, (ActionKind::AskCharge, Uuid::new_v4()), me).await;
    let none = send_as(&w, (&Method::POST, &uncharged, ""), Site::Own, &jar).await;
    assert_eq!(
        none, answer,
        "no charge of this business (BIZ-032, FEE-031)"
    );
    assert!(unused(&w, &uncharged).await, "and the link stays unused");
}

/// PICK-043: `w`'s job quoted through its Quote link, then picked on the page, its Confirm
/// window started now (no SQL here: TD-SEC-10 keeps the server's tests to the crates' `api`).
async fn waiting_pick(w: &World) -> Uuid {
    let quote = path_of(&live(w, ActionKind::Quote, w.job).await);
    let sent = send(w, (&Method::POST, &quote, &firm()), Site::Own).await;
    assert_eq!(sent.status, 200, "{}", sent.text);
    let now = Timestamp::now();
    let mut tx = w.pool.begin().await.expect("tx");
    let made = page_pick(&mut tx, (w.job, false), 1, AuditActor::System, now).await;
    let PagePick::Picked(pick) = made.expect("pick") else {
        panic!("no pick")
    };
    let due = notice_sent(&mut tx, pick, now, ConfirmSettings::default()).await;
    assert!(due.expect("sent").is_some(), "a running window");
    tx.commit().await.expect("commit");
    pick
}

/// Whether a HOLD is stored for waiting pick `pick` (PICK-043).
async fn held(w: &World, pick: Uuid) -> bool {
    let mut db = w.pool.acquire().await.expect("db");
    let ask = hold_ask(&mut db, pick, Roles::BOTH).await.expect("ask");
    ask.expect("waits").1.held
}

/// TD-SEC-44, PICK-043, BIZ-038, D-3890: the Hold link's `GET`, twice, changes nothing; a
/// cross-site `POST` is refused; its own `POST` holds the pick by the link alone, once, and
/// names the new end; the used link is then the one not-found answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_the_hold_link_holds_the_pick_once(pool: PgPool) {
    let w = world(&pool).await;
    let pick = waiting_pick(&w).await;
    let path = path_of(&live(&w, ActionKind::Hold, pick).await);
    for _ in 0..2 {
        let page = send(&w, (&Method::GET, &path, ""), Site::Own).await;
        assert_eq!((page.status, page.no_store), (200, true));
    }
    let cross = send(&w, (&Method::POST, &path, ""), Site::Cross).await;
    assert_eq!(cross.status, 403);
    assert!(!held(&w, pick).await, "nothing held yet");
    assert!(unused(&w, &path).await);
    let done = send(&w, (&Method::POST, &path, ""), Site::Own).await;
    assert_eq!((done.status, done.no_store), (200, true), "{}", done.text);
    assert!(done.text.contains("id=\"new-end\""), "{}", done.text);
    assert!(held(&w, pick).await, "the HOLD is stored");
    assert!(!unused(&w, &path).await, "the link is used");
    let again = send(&w, (&Method::POST, &path, ""), Site::Own).await;
    assert_eq!(again, not_found(&w, &Method::POST).await);
}

/// TD-SEC-44, PICK-043, SEC-025, D-252, D-1956: a second Hold says why (409) and changes
/// nothing, its link unused; a removed member's Hold link is the one not-found answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_a_refused_hold_says_why_and_uses_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let pick = waiting_pick(&w).await;
    let first = path_of(&live(&w, ActionKind::Hold, pick).await);
    let second = path_of(&live(&w, ActionKind::Hold, pick).await);
    let by = Acting {
        business: w.team.business,
        member: w.team.member,
    };
    let mut db = pool.acquire().await.expect("db");
    let billing = Roles::new(false, true).expect("roles");
    let gone = add_member(&mut db, by, Uuid::new_v4(), billing, Timestamp::now()).await;
    let gone = gone.expect("member");
    let removed = live_for(&w, (ActionKind::Hold, pick), gone).await;
    let out = remove_member(&mut db, by, gone, Timestamp::now()).await;
    out.expect("removed");
    let done = send(&w, (&Method::POST, &first, ""), Site::Own).await;
    assert_eq!(done.status, 200, "{}", done.text);
    let refused = send(&w, (&Method::POST, &second, ""), Site::Own).await;
    assert_eq!(refused.status, 409);
    assert!(refused.text.contains("already held"), "{}", refused.text);
    assert!(unused(&w, &second).await, "a refused Hold uses nothing");
    let theirs = send(&w, (&Method::POST, &removed, ""), Site::Own).await;
    assert_eq!(theirs, not_found(&w, &Method::POST).await, "SEC-025");
    assert!(unused(&w, &removed).await);
}

/// `w`'s business's stored away end while it is ahead.
async fn away_end(w: &World) -> Option<Timestamp> {
    let mut db = w.pool.acquire().await.expect("db");
    away_until(&mut db, w.team.business, Timestamp::now())
        .await
        .expect("away read")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_the_away_link_sets_the_time_once_with_the_persons_session(pool: PgPool) {
    let w = world(&pool).await;
    let (me, jar) = signed_in(&w).await;
    let away = live_for(&w, (ActionKind::Away, w.team.business), me).await;
    let page = send_as(&w, (&Method::GET, &away, ""), Site::Own, &jar).await;
    assert!(page.text.contains("<label for=\"hours\">Hours until you"));
    assert!(
        page.text.contains("(1 to 4)</label>"),
        "the setting's limit"
    );
    let out = send(&w, (&Method::POST, &away, "&hours=2"), Site::Own).await;
    assert_eq!(out.status, 401, "signed out is asked to sign in (SEC-004)");
    for site in [Site::Cross, Site::WrongToken] {
        let refused = send_as(&w, (&Method::POST, &away, "&hours=2"), site, &jar).await;
        assert_eq!(refused.status, 403, "refused (SEC-042)");
    }
    for hours in ["0", "5", "two", ""] {
        let field = format!("&hours={hours}");
        let bad = send_as(&w, (&Method::POST, &away, &field), Site::Own, &jar).await;
        assert_eq!(bad.status, 422, "{hours:?}: {}", bad.text);
        assert!(bad.no_store && bad.text.contains("<title>Error: "));
        assert!(bad.text.contains("Enter a number of hours from 1 to 4."));
        assert!(bad.text.contains("aria-describedby=\"hours-error\""));
    }
    assert!(
        unused(&w, &away).await,
        "a refused submit leaves the link unused"
    );
    assert_eq!(away_end(&w).await, None, "and stores nothing");
    let set = send_as(&w, (&Method::POST, &away, "&hours=2"), Site::Own, &jar).await;
    assert_eq!(set.status, 200, "{}", set.text);
    assert!(
        set.no_store && set.text.contains("Away until "),
        "D-285's answer line"
    );
    assert!(away_end(&w).await.is_some_and(|t| t > Timestamp::now()));
    let again = send_as(&w, (&Method::POST, &away, "&hours=2"), Site::Own, &jar).await;
    assert_eq!(
        again,
        not_found(&w, &Method::POST).await,
        "a used link acts no more"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_another_persons_or_businesss_away_link_is_not_found(pool: PgPool) {
    let w = world(&pool).await;
    let (me, _) = signed_in(&w).await;
    let (_, other) = signed_in(&w).await;
    let answer = not_found(&w, &Method::POST).await;
    let mine = live_for(&w, (ActionKind::Away, w.team.business), me).await;
    let theirs = send_as(&w, (&Method::POST, &mine, "&hours=2"), Site::Own, &other).await;
    assert_eq!(theirs, answer, "another person's link (SEC-025)");
    assert!(unused(&w, &mine).await);
    let elsewhere = live_for(&w, (ActionKind::Away, Uuid::new_v4()), me).await;
    let (_, jar) = signed_in(&w).await;
    let wrong = send_as(&w, (&Method::POST, &elsewhere, "&hours=2"), Site::Own, &jar).await;
    assert_eq!(wrong, answer, "a link naming another business");
    assert!(unused(&w, &elsewhere).await, "and the link stays unused");
    assert_eq!(away_end(&w).await, None, "nothing stored");
}

/// `w`'s business's stored "Busy until" end while it is ahead.
async fn busy_end(w: &World) -> Option<Timestamp> {
    let mut db = w.pool.acquire().await.expect("db");
    busy_until(&mut db, w.team.business, Timestamp::now())
        .await
        .expect("busy read")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_the_busy_link_pauses_once_with_the_persons_session(pool: PgPool) {
    let w = world(&pool).await;
    let (me, jar) = signed_in(&w).await;
    let busy = live_for(&w, (ActionKind::Busy, w.team.business), me).await;
    let page = send_as(&w, (&Method::GET, &busy, ""), Site::Own, &jar).await;
    assert!(
        page.text.contains("Number of days (1 to 60)"),
        "{}",
        page.text
    );
    let out = send(&w, (&Method::POST, &busy, "&days=3"), Site::Own).await;
    assert_eq!(out.status, 401, "signed out is asked to sign in (SEC-004)");
    for site in [Site::Cross, Site::WrongToken] {
        let refused = send_as(&w, (&Method::POST, &busy, "&days=3"), site, &jar).await;
        assert_eq!(refused.status, 403, "refused (SEC-042)");
    }
    for days in ["0", "61", "two", ""] {
        let field = format!("&days={days}");
        let bad = send_as(&w, (&Method::POST, &busy, &field), Site::Own, &jar).await;
        assert_eq!(bad.status, 422, "{days:?}: {}", bad.text);
        assert!(bad.no_store && bad.text.contains(BUSY_DAYS_ERROR));
        assert!(
            bad.text.contains("aria-describedby=\"days-error\""),
            "A11Y-030"
        );
    }
    let pending = send_as(&w, (&Method::POST, &busy, "&days=3"), Site::Own, &jar).await;
    assert_eq!(pending.status, 409, "{}", pending.text);
    assert!(
        pending.text.contains("Only an active account"),
        "it says why"
    );
    assert!(
        unused(&w, &busy).await,
        "a refused submit leaves the link unused"
    );
    assert_eq!(
        (status(&w).await, busy_end(&w).await),
        (("pending", None), None)
    );
    let checks = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    moved(&w, Event::Checked(checks)).await;
    let set = send_as(&w, (&Method::POST, &busy, "&days=3"), Site::Own, &jar).await;
    assert_eq!(set.status, 200, "{}", set.text);
    assert!(
        set.no_store && set.text.contains("Busy until "),
        "BIZ-028's line"
    );
    assert_eq!(
        status(&w).await,
        ("paused", Some("self")),
        "job cards paused"
    );
    let end = busy_end(&w).await.expect("the end is stored");
    let ahead = end.duration_since(Timestamp::now());
    let days = SignedDuration::from_hours(71)..=SignedDuration::from_hours(72);
    assert!(days.contains(&ahead), "the end is 3 days on");
    let again = send_as(&w, (&Method::POST, &busy, "&days=3"), Site::Own, &jar).await;
    assert_eq!(
        again,
        not_found(&w, &Method::POST).await,
        "a used link acts no more"
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_another_persons_or_businesss_busy_link_is_not_found(pool: PgPool) {
    let w = world(&pool).await;
    let (me, _) = signed_in(&w).await;
    let (_, other) = signed_in(&w).await;
    let answer = not_found(&w, &Method::POST).await;
    let mine = live_for(&w, (ActionKind::Busy, w.team.business), me).await;
    let theirs = send_as(&w, (&Method::POST, &mine, "&days=3"), Site::Own, &other).await;
    assert_eq!(theirs, answer, "another person's link (SEC-025)");
    assert!(unused(&w, &mine).await);
    let elsewhere = live_for(&w, (ActionKind::Busy, Uuid::new_v4()), me).await;
    let (_, jar) = signed_in(&w).await;
    let wrong = send_as(&w, (&Method::POST, &elsewhere, "&days=3"), Site::Own, &jar).await;
    assert_eq!(wrong, answer, "a link naming another business");
    assert!(unused(&w, &elsewhere).await, "and the link stays unused");
    assert_eq!(busy_end(&w).await, None, "nothing stored");
}

/// The code the fake Verify accepts.
const RIGHT_CODE: &str = "424242";

/// A fake Verify: counts its sends and accepts only [`RIGHT_CODE`].
struct FakeVerify(Arc<AtomicUsize>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("sent".to_owned())
    }

    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == RIGHT_CODE)
    }
}

/// Everyone's code address: a fake 555 phone, or an email-only person.
struct FakeAddress(bool);

impl CodeAddress for FakeAddress {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        _: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let fake = SecretString::from("+19285550142");
        let to = if self.0 {
            CodeTo::Phone(fake)
        } else {
            CodeTo::Email(fake)
        };
        Box::pin(async move { Ok(to) })
    }
}

/// [`world`] whose router has the Confirm link's codes over the fake Verify (D-3889), and the
/// fake's send count; `phone` says whether people have a phone that gets texts.
async fn coded(pool: &PgPool, phone: bool) -> (World, Arc<AtomicUsize>) {
    let mut w = world(pool).await;
    let sends = Arc::new(AtomicUsize::new(0));
    let verify = IdentityCodes::new(FakeVerify(Arc::clone(&sends)));
    let key = SecretString::from("a-fake-key-of-thirty-two-bytes-00");
    let codes = ConfirmCodes::new(verify, FakeAddress(phone), key);
    let parts = askhava_server::RouterParts::from((None, None, None, None));
    let parts = parts.with_confirm_link_codes(codes);
    w.app = askhava_server::build_router(pool.clone(), Router::new(), parts);
    (w, sends)
}

/// A `POST` of `path` with `extra` fields and `jar`: its status, its `Location` and the cookies
/// it set.
async fn landed(w: &World, path: &str, extra: &str, jar: &CookieJar) -> (u16, String, CookieJar) {
    let asked = request((&Method::POST, path, extra), Site::Own, jar);
    let response = w.app.clone().oneshot(asked).await.expect("response");
    let header_text = |v: &header::HeaderValue| v.to_str().ok().map(str::to_owned);
    let to = response.headers().get(header::LOCATION);
    let to = to.and_then(header_text).unwrap_or_default();
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| Some(v.to_str().ok()?.split(';').next()?.to_owned()))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    let status = response.status().as_u16();
    (status, to, CookieJar::from_headers(&headers))
}

/// TD-SEC-44, SEC-004, BIZ-038, PICK-021, D-3675: signed out, the Confirm link's first press
/// texts one code and uses nothing; a wrong code uses nothing; the right code uses the link,
/// signs that device in as the link's person and answers 303 to the Confirm page, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_the_confirm_link_asks_for_a_code_then_signs_that_device_in(pool: PgPool) {
    let (w, sends) = coded(&pool, true).await;
    let pick = Uuid::new_v4();
    let path = path_of(&live(&w, ActionKind::Confirm, pick).await);
    let first = send(&w, (&Method::POST, &path, ""), Site::Own).await;
    assert_eq!(first.status, 200, "{}", first.text);
    assert!(first.no_store && first.text.contains("We texted a code to your phone."));
    assert!(first.text.contains("name=\"code\"") && first.text.contains("for=\"code\""));
    assert_eq!(sends.load(Ordering::SeqCst), 1, "one code sent");
    let wrong = send(&w, (&Method::POST, &path, "&code=000000"), Site::Own).await;
    assert_eq!(wrong.status, 422);
    assert!(wrong.text.contains("<title>Error: ") && wrong.text.contains("href=\"#code\""));
    assert!(wrong.text.contains("Check the text and type it again."));
    assert!(
        unused(&w, &path).await,
        "a sent or wrong code used the link"
    );
    let cross = send(&w, (&Method::POST, &path, "&code=424242"), Site::Cross).await;
    assert_eq!(cross.status, 403, "a cross-site code is refused (SEC-048)");
    assert!(unused(&w, &path).await, "a cross-site code used the link");
    let pre = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let right = format!("&code={RIGHT_CODE}");
    let (status, to, jar) = landed(&w, &path, &right, &pre).await;
    assert_eq!((status, to), (303, format!("/b/picks/{pick}/confirm")));
    assert!(!unused(&w, &path).await, "the right code uses the link");
    let mut db = w.pool.acquire().await.expect("db");
    let who = business_principal(&mut db, &jar, Timestamp::now()).await;
    let who = who
        .expect("read")
        .expect("that device is signed in (SEC-004)");
    assert_eq!(
        (who.member(), who.business()),
        (w.team.member, w.team.business)
    );
    let again = send(&w, (&Method::POST, &path, &right), Site::Own).await;
    assert_eq!(again, not_found(&w, &Method::POST).await, "a used link");
    assert_eq!(sends.load(Ordering::SeqCst), 1);
}

/// TD-SEC-44, SEC-004, SEC-027, BIZ-038: the link's own person's live session needs no code
/// (303, link used, nothing sent); another person's link in that session, and their own once
/// "This inbox is shared" is on, get the code step and use nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_a_live_session_skips_the_code_unless_the_inbox_is_shared(pool: PgPool) {
    let (w, sends) = coded(&pool, true).await;
    let (member, jar) = signed_in(&w).await;
    let pick = Uuid::new_v4();
    let own = live_for(&w, (ActionKind::Confirm, pick), member).await;
    let (status, to, _) = landed(&w, &own, "", &jar).await;
    assert_eq!((status, to), (303, format!("/b/picks/{pick}/confirm")));
    assert!(!unused(&w, &own).await, "the press used the link");
    assert_eq!(sends.load(Ordering::SeqCst), 0, "no code inside a session");
    let owners = path_of(&live(&w, ActionKind::Confirm, pick).await);
    let other = act(&w, &owners, Site::Own, &jar).await;
    assert_eq!(
        other.status, 200,
        "another person's link asks for their code"
    );
    assert!(unused(&w, &owners).await && sends.load(Ordering::SeqCst) == 1);
    let (mut db, now) = (w.pool.acquire().await.expect("db"), Timestamp::now());
    let by = Acting {
        business: w.team.business,
        member,
    };
    record_step_up(&mut db, by, now).await.expect("step-up");
    let who = business_principal(&mut db, &jar, now).await;
    let who = who.expect("read").expect("signed in");
    let phone = CodeTo::Phone(SecretString::from("+19285550142"));
    let shared = set_inbox_shared(&mut db, &who, (true, &phone), now).await;
    shared.expect("shared");
    drop(db);
    let next = live_for(&w, (ActionKind::Confirm, Uuid::new_v4()), member).await;
    let asked = act(&w, &next, Site::Own, &jar).await;
    assert_eq!(
        asked.status, 200,
        "a shared inbox needs a fresh code (SEC-027)"
    );
    assert!(asked.text.contains(&CodeSent::Sent.line()[..30]));
    assert!(unused(&w, &next).await && sends.load(Ordering::SeqCst) == 2);
}

/// TD-SEC-44, SEC-004, D-3889: with no codes configured the Confirm link sends, checks and uses
/// nothing and says the code could not be sent; a person with no phone that gets texts is told
/// to link one, with nothing sent or used.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_without_codes_or_a_texting_phone_the_confirm_link_uses_nothing(pool: PgPool) {
    let bare = world(&pool).await;
    let path = path_of(&live(&bare, ActionKind::Confirm, Uuid::new_v4()).await);
    for extra in ["", "&code=424242"] {
        let reply = send(&bare, (&Method::POST, &path, extra), Site::Own).await;
        assert_eq!(reply.status, 200, "{extra}");
        assert!(reply.text.contains("send the code. Press"), "{extra}");
        assert!(unused(&bare, &path).await, "{extra}: used with no codes");
    }
    let (w, sends) = coded(&pool, false).await;
    let path = path_of(&live(&w, ActionKind::Confirm, Uuid::new_v4()).await);
    let reply = send(&w, (&Method::POST, &path, ""), Site::Own).await;
    assert_eq!(reply.status, 200);
    assert!(reply.text.contains("link a phone that gets texts"));
    assert!(unused(&w, &path).await && sends.load(Ordering::SeqCst) == 0);
}
