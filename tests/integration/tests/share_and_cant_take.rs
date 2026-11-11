//! The share box at the pick and "Can't take it" on a fresh database (TD-PICK-34, TD-PICK-19,
//! TD-PICK-43; PICK-049, PICK-050, A11Y-011, A11Y-012; D-443, D-446, D-492, D-3024, D-3032). Fake
//! IDs, wording and tokens only.
//!
//! A job sits in Quotes ready with three confirmed quotes, quote 1 from a signed-up business with
//! a signed-in owner. The final Pick with the share box unticked shows the review again with its
//! error and stores nothing; a tick is logged and bound to the pick; no page template renders a
//! consent, sharing, charge or feedback box ticked; the pick notice names the pick's "Can't take
//! it" page, and pressing it ends the pick at once with no fee and moves it to her backup.
//! With the page pick's keys over a Verify stand-in that records sends and passes only [`CODE`],
//! an unticked final Pick sends no code, and a browser that never ticked and goes straight to the
//! code step passes its code but stores no pick. Fake numbers (555-01xx) only.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{add_owner, create};
use askhava_core::api::{Claimed, Guarded, IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    Backup, Browser, ConfirmSettings, PagePick, bind_backup, bind_share, cant_take_link,
    choose_backup, page_pick, run_pick_notice, share_ticked, tick_share,
};
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, BusinessPrincipal, CookieJar, PRE_SESSION_COOKIE, PickCodeKeys, Realm,
    business_principal, cant_take_page, cant_take_pick, pick_flow_routes, start_session,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const DAY: &str = "2026-11-04T17:00:00Z";
const COOKIE: &str = "fake-pre-session-cookie-value-0214";
const BROWSER: Browser = [4; 32];
const PHONE: &str = "+19285550152";
const CODE: &str = "123456";

fn at(minutes: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("instant") + SignedDuration::from_mins(minutes)
}

/// A job in Quotes ready with three confirmed quotes; quote 1 is `first`'s.
async fn quoted(db: &mut PgConnection, first: Uuid) -> Uuid {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    ready(db, job, first).await;
    job
}

/// Walks `job` to Quotes ready with three confirmed quotes; quote 1 is `first`'s.
async fn ready(db: &mut PgConnection, job: Uuid, first: Uuid) {
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(0), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    for (n, business) in [
        (1, first),
        (2, Uuid::from_u128(0x2142)),
        (3, Uuid::from_u128(0x2143)),
    ] {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(business)
        .bind(at(n).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
}

/// A business with its owner signed in.
async fn signed_in(db: &mut PgConnection) -> (Uuid, BusinessPrincipal) {
    let business = create(db, false, at(0)).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(0x214d1), at(0))
        .await
        .expect("owner");
    let session = (Realm::Business, member);
    let jar = start_session(db, CookieJar::new(), session, BUSINESS_SESSION_LIFE, at(0))
        .await
        .expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(member)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let principal = business_principal(db, &jar, at(1)).await.expect("read");
    (business, principal.expect("signed in"))
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-214".to_owned())
}

/// A fresh quote-page token for `job`.
async fn token(pool: &PgPool, job: Uuid) -> String {
    let mut db = pool.acquire().await.expect("db");
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(&mut db, &link, job, at(6)).await.expect("token");
    token.as_str().to_owned()
}

/// Posts `body` to the final Pick of `job`'s quote page, without the Verify keys.
async fn final_pick(pool: &PgPool, job: Uuid, body: &str) -> (StatusCode, String) {
    let token = token(pool, job).await;
    let clock = Arc::new(ManualClock::new(at(7)));
    let app = pick_flow_routes::<FakeVerify>(pool.clone(), clock, None);
    post(app, &format!("/q/{token}/pick"), body).await
}

/// Posts `body` with the pre-session cookie's CSRF token to `path` of `app`, as a browser would.
async fn post(app: Router, path: &str, body: &str) -> (StatusCode, String) {
    let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let request = Request::post(path)
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}&{body}")))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

/// A Verify stand-in: records each send and passes only [`CODE`].
#[derive(Clone, Default)]
struct FakeVerify {
    sent: Arc<Mutex<Vec<String>>>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        _channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sent.lock().expect("lock").push(to.to_owned());
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

impl FakeVerify {
    fn sends(&self) -> usize {
        self.sent.lock().expect("lock").len()
    }
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x46), &key(0x56), &url).expect("vault")
}

/// The pick routes with the page pick's keys over `verify`.
fn keyed(pool: &PgPool, verify: &FakeVerify) -> Router {
    let terms = (
        "2026-11-01".to_owned(),
        "Sample homeowner terms for tests only.",
    );
    let keys = PickCodeKeys::new(verify.clone(), vault(), secret(), terms);
    let clock = Arc::new(ManualClock::new(at(7)));
    pick_flow_routes(pool.clone(), clock, Some(keys))
}

/// A homeowner with a fake vaulted phone texts in; her job walks to Quotes ready with three
/// confirmed quotes, so the keyed code step can send and check a code for it.
async fn her_job(db: &mut PgConnection) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: at(0),
    };
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: at(0),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    ready(db, job, Uuid::from_u128(0x2141)).await;
    job
}

async fn count(db: &mut PgConnection, sql: String) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(db)
        .await
        .expect("count")
}

/// TD-PICK-34, TD-PICK-19 (PICK-049, A11Y-011, A11Y-012, D-3032): pressing Pick with the share box
/// unticked shows the review again, the box unticked with its linked error and her backup kept,
/// and stores nothing; ticked, the press goes on as before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_an_unticked_share_box_picks_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quoted(&mut db, Uuid::from_u128(0x2141)).await;
    let (status, page) = final_pick(&pool, job, "quote=1&backup=3").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    for part in [
        "<title>Error: ",
        "id=\"share-summary\" role=\"alert\" tabindex=\"-1\" autofocus",
        "<a href=\"#share\">",
        "id=\"share-error\"",
        "aria-describedby=\"share-error\" aria-invalid=\"true\"",
        "with the business for quote #1 once they accept.",
        "<option value=\"3\" selected>Quote #3</option>",
    ] {
        assert!(page.contains(part), "missing {part}: {page}");
    }
    assert!(
        !page.contains(" checked"),
        "the box is never rendered ticked"
    );
    let error = page.find("id=\"share-error\"").expect("error");
    assert!(error < page.find("id=\"final-pick\"").expect("button"));
    for table in ["picks.share_consent", "picks.backup_choice", "picks.pick"] {
        let sql = format!("SELECT count(*) FROM {table}");
        assert_eq!(count(&mut db, sql).await, 0, "{table}: nothing stored");
    }
    let (_, ticked) = final_pick(&pool, job, "quote=1&share=yes").await;
    assert!(ticked.contains("Nothing was picked."), "{ticked}");
    assert!(!ticked.contains("share-error"), "{ticked}");
}

/// The rows a pick attempt can leave: share ticks, backups, code sends and picks.
async fn stored(db: &mut PgConnection) -> Vec<i64> {
    let mut rows = Vec::new();
    for table in [
        "picks.share_consent",
        "picks.backup_choice",
        "picks.code_attempt",
        "picks.pick",
    ] {
        rows.push(count(db, format!("SELECT count(*) FROM {table}")).await);
    }
    rows
}

/// TD-PICK-34 with the Verify keys (PICK-049, D-3032): an unticked final Pick shows the review
/// again with its error, sends no code and stores nothing; ticked, the code is sent as before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_with_keys_an_unticked_share_box_sends_no_code(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = her_job(&mut db).await;
    let verify = FakeVerify::default();
    let path = format!("/q/{}/pick", token(&pool, job).await);
    let app = keyed(&pool, &verify);
    let (status, page) = post(app.clone(), &path, "quote=1&backup=3").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("id=\"share-error\""), "{page}");
    assert!(page.contains("<option value=\"3\" selected>Quote #3</option>"));
    assert!(!page.contains("code-sent"), "{page}");
    assert_eq!(verify.sends(), 0, "no code sent");
    assert_eq!(stored(&mut db).await, [0, 0, 0, 0], "nothing stored");
    let (_, ticked) = post(app, &path, "quote=1&share=yes").await;
    assert!(ticked.contains("We texted a 6-digit code"), "{ticked}");
    assert_eq!(verify.sends(), 1, "ticked, the code is sent");
}

/// TD-PICK-34 (PICK-049, D-3032): a browser that never ticked the box and posts the code step
/// directly gets a code and passes it as today, but no pick is stored for the quote.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_a_code_without_a_tick_stores_no_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = her_job(&mut db).await;
    let verify = FakeVerify::default();
    let path = format!("/q/{}/code", token(&pool, job).await);
    let app = keyed(&pool, &verify);
    let (_, sent) = post(app.clone(), &path, "quote=1&send=new").await;
    assert!(sent.contains("We texted a 6-digit code"), "{sent}");
    assert_eq!(verify.sends(), 1);
    let finish = format!("quote=1&code={CODE}&reach=call&send=pick");
    let (status, page) = post(app, &path, &finish).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(!page.contains("tries left"), "the code passed: {page}");
    assert!(page.contains("your pick didn&#39;t go through"), "{page}");
    assert!(
        !page.contains("Done. The business has your pick."),
        "{page}"
    );
    assert_eq!(stored(&mut db).await[3], 0, "no pick without a tick");
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status.code(), "quotes_ready");
}

/// TD-PICK-34 (PICK-049, D-3032): a ticked final Pick is logged for that browser and quote only,
/// and the code step's bind logs it with the pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_a_pick_logs_the_share_tick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quoted(&mut db, Uuid::from_u128(0x2141)).await;
    assert!(
        !share_ticked(&mut db, (job, 1), &BROWSER)
            .await
            .expect("read")
    );
    tick_share(&mut db, (job, 1), &BROWSER, at(6))
        .await
        .expect("tick");
    assert!(
        share_ticked(&mut db, (job, 1), &BROWSER)
            .await
            .expect("read")
    );
    assert!(
        !share_ticked(&mut db, (job, 2), &BROWSER)
            .await
            .expect("read")
    );
    assert!(
        !share_ticked(&mut db, (job, 1), &[5; 32])
            .await
            .expect("read")
    );
    let Ok(PagePick::Picked(pick)) =
        page_pick(&mut db, (job, false), 1, Actor::System, at(7)).await
    else {
        panic!("a page pick");
    };
    bind_share(&mut db, ((job, 1), pick), &BROWSER, at(7))
        .await
        .expect("bound");
    let bound: i64 =
        sqlx::query_scalar("SELECT count(*) FROM picks.share_consent WHERE pick_id = $1")
            .bind(pick)
            .fetch_one(&mut *db)
            .await
            .expect("bound rows");
    assert_eq!(bound, 1, "the tick is logged with the pick");
}

/// The text of every page template in the workspace.
fn templates() -> Vec<(String, String)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../app");
    let mut found = Vec::new();
    for krate in std::fs::read_dir(root).expect("app").flatten() {
        let Ok(files) = std::fs::read_dir(krate.path().join("templates")) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "html") {
                let text = std::fs::read_to_string(&path).expect("template");
                found.push((path.display().to_string(), text));
            }
        }
    }
    found
}

/// TD-PICK-34 (PICK-049, D-492): no checkbox in any page template is ticked unconditionally;
/// a box shows ticked only from a value the person gave (the BIZ-049 job-type checklist and kept
/// answers alike), so no consent, sharing, charge or feedback box starts ticked.
#[test]
fn td_pick_34_no_box_on_any_page_starts_ticked() {
    let all = templates();
    assert!(all.iter().any(|(p, _)| p.ends_with("pick_review.html")));
    for (path, text) in all {
        for tag in text.split("<input").skip(1) {
            let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
            if !tag.contains("type=\"checkbox\"") {
                continue;
            }
            let mut bare = String::new();
            let mut rest = tag;
            while let Some(start) = rest.find("{% if") {
                bare.push_str(&rest[..start]);
                rest = rest[start..]
                    .split_once("{% endif %}")
                    .map_or("", |(_, r)| r);
            }
            bare.push_str(rest);
            assert!(
                !bare.contains("checked"),
                "{path}: a box starts ticked: {tag}"
            );
        }
    }
}

/// TD-PICK-43 (PICK-050, D-3024): the pick notice's values carry the pick's on-domain "Can't take
/// it" page, from the pick ID alone, beside the job and window.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_43_the_pick_notice_carries_cant_take(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (business, _) = signed_in(&mut db).await;
    let job = quoted(&mut db, business).await;
    let Ok(PagePick::Picked(pick)) =
        page_pick(&mut db, (job, false), 1, Actor::System, at(7)).await
    else {
        panic!("a page pick");
    };
    let payload = json!({"pick": pick, "job": job, "business": business});
    let task = Claimed {
        id: 1,
        kind: String::new(),
        payload,
        attempts: 1,
    };
    run_pick_notice(pool.clone(), ConfirmSettings::default(), task)
        .await
        .expect("notice");
    let values: serde_json::Value = sqlx::query_scalar(
        "SELECT payload->'values' FROM core.task WHERE kind = 'messaging.business_notice'",
    )
    .fetch_one(&mut *db)
    .await
    .expect("notice");
    let link = cant_take_link(pick);
    assert_eq!(values["cant_take"], json!(link));
    assert!(link.starts_with("https://") && link.ends_with(&format!("/b/picks/{pick}/cant-take")));
    assert_eq!(values["window"], 240);
}

/// TD-PICK-34, TD-PICK-43 (PICK-050, PICK-047, D-446): "Can't take it" says what it does before
/// its one button; pressing it ends the pick at once with no fee, records it, and the backup gets
/// the pick; a second press, or a press by a business not picked, changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_43_cant_take_ends_the_pick_and_the_backup_gets_it(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (business, principal) = signed_in(&mut db).await;
    let job = quoted(&mut db, business).await;
    let Ok(PagePick::Picked(pick)) =
        page_pick(&mut db, (job, false), 1, Actor::System, at(7)).await
    else {
        panic!("a page pick");
    };
    choose_backup(&mut db, (job, 1), (Some(3), &BROWSER), at(7))
        .await
        .expect("asked");
    bind_backup(&mut db, pick, &BROWSER, at(7))
        .await
        .expect("bound");
    let page = cant_take_page("1042", "csrf-sample").expect("page");
    assert!(page.contains("no introduction fee"), "{page}");
    assert_eq!(page.matches("<button").count(), 1);
    let other = quoted(&mut db, Uuid::from_u128(0x2149)).await;
    let Ok(PagePick::Picked(theirs)) =
        page_pick(&mut db, (other, false), 1, Actor::System, at(7)).await
    else {
        panic!("another pick");
    };
    let mut tx = pool.begin().await.expect("tx");
    let none = cant_take_pick(&mut tx, &principal, theirs, at(8)).await;
    assert_eq!(none.expect("refused"), None, "not this business's pick");
    let moved = cant_take_pick(&mut tx, &principal, pick, at(8))
        .await
        .expect("ended");
    let Some(Backup::Moved(new)) = moved else {
        panic!("moved to the backup: {moved:?}");
    };
    tx.commit().await.expect("commit");
    let ended: (String, Option<String>) =
        sqlx::query_as("SELECT status, end_reason FROM picks.pick WHERE id = $1")
            .bind(pick)
            .fetch_one(&mut *db)
            .await
            .expect("pick");
    assert_eq!(
        ended,
        ("cancelled".to_owned(), Some("cant_take".to_owned()))
    );
    let to: i32 = sqlx::query_scalar(
        "SELECT quote_number FROM picks.pick WHERE id = $1 AND status = 'awaiting_confirm'",
    )
    .bind(new)
    .fetch_one(&mut *db)
    .await
    .expect("backup pick");
    assert_eq!(to, 3, "the backup has the pick at once");
    for table in ["payments.charge_attempt", "payments.ledger_entry"] {
        let sql = format!("SELECT count(*) FROM {table}");
        assert_eq!(count(&mut db, sql).await, 0, "{table}: no fee");
    }
    let mut tx = pool.begin().await.expect("tx");
    let again = cant_take_pick(&mut tx, &principal, pick, at(9)).await;
    assert_eq!(again.expect("again"), None, "ended once");
}

/// TD-PICK-43 (PICK-050, PICK-046): with no backup the pick ends at once and the job goes back to
/// Quotes ready; the caller's PICK-046 text is due.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_43_cant_take_with_no_backup_reopens_the_job(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (business, principal) = signed_in(&mut db).await;
    let job = quoted(&mut db, business).await;
    let Ok(PagePick::Picked(pick)) =
        page_pick(&mut db, (job, false), 1, Actor::System, at(7)).await
    else {
        panic!("a page pick");
    };
    let mut tx = pool.begin().await.expect("tx");
    let done = cant_take_pick(&mut tx, &principal, pick, at(8))
        .await
        .expect("ended");
    tx.commit().await.expect("commit");
    assert_eq!(done, Some(Backup::NoBackup));
    let status = current(&mut db, job).await.expect("job").status;
    assert_eq!(status.code(), "quotes_ready", "back to Quotes ready");
}
