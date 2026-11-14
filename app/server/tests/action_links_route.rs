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
//! name, phone and ID is fake.

use std::collections::BTreeSet;

use askhava_businesses::api::{
    AccountStatus, Acting, Capability, CapabilityAnswers, LargestJob, Roles, add_member, add_owner,
    create as create_business, remove_member, save_capability,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor as AuditActor;
use askhava_picks::api::HoldAsk;
use askhava_portals::api::{
    ActionKind, ActionTarget, CookieJar, csrf_token, ensure_pre_session, mint_action_link,
    open_action_link,
};
use askhava_quotes::api::{
    CardBusiness, CardFacts, CardJob, CardParts, DEFAULT_MIN_WORDS, OfferTown, Place, build_card,
    card_digest, keep_card, mark_card_sent, send_cards, stored_facts,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, header};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
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

/// Sends `method path` with the CSRF token of `jar` (or a wrong one) and `extra` form fields.
async fn send(w: &World, (method, path, extra): (&Method, &str, &str), site: Site) -> Reply {
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        Site::Own | Site::Cross => csrf_token(&jar).unwrap_or_default(),
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
    let request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", fetch)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request");
    let response = w.app.clone().oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
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
        format!("/b/a/pause/{}/{token}", w.job),
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

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_44_every_other_kind_post_is_not_found_and_uses_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let answer = not_found(&w, &Method::POST).await;
    for kind in ActionKind::ALL
        .into_iter()
        .filter(|k| *k != ActionKind::Quote)
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
