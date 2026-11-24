//! TD-PICK-75 (T741, D-4727, D-5670; PICK-030, CON-013, CON-016): an opted-out homeowner's
//! "<number> YES" through Twilio's real inbound-text route (`POST /webhooks/twilio/sms`, signed
//! fake requests) on a real database. Setup: her vault contact holds a fake 555 number, her
//! request opens through `intake::api::receive_text`, each job reaches Quotes ready, carded to and
//! quoted by two fake businesses (`quotes::api`), the LIST text's Terms line is recorded or not
//! with `picks::api::record_terms_line`, and her number goes on the suppression list by a STOP (or
//! a CANCEL, which also pauses her jobs) through the same route. `server` names no other schema in
//! a query macro, so reads go through `query_to_xml` as `text_pick_route.rs` does. Fake data only.

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_businesses::api::AccountStatus;
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Environment, Guarded, MIGRATOR, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_messaging::api::OPTOUT_FOLLOWUP_KIND;
use askhava_ops::api::Actor;
use askhava_picks::api::record_terms_line;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::inbound_text::{INBOUND_PATH, Webhooks, routes};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HER: &str = "+19285550173";
const LAFTLY: &str = "+19285550100";
const FIRST: &str = "my kitchen sink is leaking under the cabinet";
const NOW: i64 = 1_794_506_400;

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x43), &key(0x53), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(now())),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: secret(),
        vault: vault(),
        environment: Environment::Test,
        intake: Settings::default(),
    })
}

/// Percent-encodes one form value.
fn encode(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// POSTs inbound text `n` from her fake number through the SMS route, signed by the fake token;
/// returns its `MessageSid`.
async fn text(pool: &PgPool, n: u32, words: &str) -> String {
    let sid = format!("SM{n:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", HER),
        ("To", LAFTLY),
        ("Body", words),
    ];
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(INBOUND_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(hooks(pool)).oneshot(request).await.expect("answer");
    assert_eq!(answer.status(), StatusCode::OK);
    sid
}

/// Her first text, an hour before the route's clock.
fn first_text(contact: Uuid) -> TextIn<'static> {
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound: Inbound {
            words: FIRST,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now() - SignedDuration::from_mins(60),
    }
}

/// Her vault contact and her open request with one job per trade, each in Quotes ready through
/// [`quoted`], with the Terms line recorded when `terms`.
async fn ready(pool: &PgPool, trades: &[&str], terms: bool) -> (Uuid, Vec<Uuid>) {
    let mut db = pool.acquire().await.expect("connection");
    let new = NewContact {
        phone: Some(SecretString::from(HER)),
        ..NewContact::default()
    };
    let her = store_contact(&mut db, &vault(), &new).await.expect("her");
    let first = first_text(her);
    let handled = receive_text(&mut db, &secret(), &first, Settings::default()).await;
    let id = handled.expect("open").conversation.expect("request");
    load_embedded(&mut db).await.expect("catalog");
    let mut jobs = Vec::new();
    for trade in trades {
        let at = first.received_at;
        let start = JobStart {
            conversation: id,
            words: FIRST,
            emergency: false,
            trade,
            at,
        };
        let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
            panic!("a new job");
        };
        let types = ["AC not cooling".to_owned()];
        record_job_facts(&mut db, job, "hvac_repair", &types)
            .await
            .expect("job facts");
        let open = (JobStatus::Intake, JobStatus::Open);
        assert!(matches!(
            move_to(&mut db, job, open, at, Actor::System).await,
            Ok(Guarded::Applied(()))
        ));
        quoted(&mut db, job, (at, terms)).await;
        jobs.push(job);
    }
    (her, jobs)
}

/// A fake business able to take an AC repair job in Lake Havasu City.
fn able(id: Uuid) -> CardBusiness {
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

/// The AC repair job as matching reads it.
fn card() -> CardJob {
    let candidates = vec![Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    }];
    let towns = vec![OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    }];
    CardJob {
        towns,
        candidates,
        text: String::new(),
    }
}

/// A firm $325 quote (fake wording).
fn firm() -> (QuoteForm, QuoteRules) {
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

/// Fake businesses 1 and 2 carded and quoting firm on `job`, the job in Quotes ready, and, when
/// `terms`, the LIST text's Terms line recorded (PICK-016).
async fn quoted(db: &mut sqlx::PgConnection, job: Uuid, (at, terms): (Timestamp, bool)) {
    let business = |n: u128| able(Uuid::from_u128(0xb000 + n));
    let map = ClassMap::shipped().expect("the shipped license map");
    send_cards(db, map, job, &card(), &[business(1), business(2)], at)
        .await
        .expect("cards");
    for n in 1..=2_u128 {
        let (form, rules) = firm();
        let quote = parse_quote(form, &rules).expect("quote");
        let when = at + SignedDuration::from_mins(i64::try_from(n).expect("small"));
        submit_quote(db, (job, Uuid::from_u128(0xb000 + n)), &quote, when)
            .await
            .expect("quoted");
    }
    // Applied, or already moved on by the second quote: either way the job is in Quotes ready.
    let ready = (JobStatus::Open, JobStatus::QuotesReady);
    let _moved = move_to(db, job, ready, at, Actor::System)
        .await
        .expect("ready");
    if terms {
        record_terms_line(db, (job, Uuid::new_v4()), "quote_list_end:en:1", at)
            .await
            .expect("terms");
    }
}

/// The one text value `v` of `sql` (one row), run through `query_to_xml`.
async fn value(pool: &PgPool, sql: String) -> String {
    let v = sqlx::query_scalar!(
        r#"SELECT coalesce((xpath('/row/v/text()', query_to_xml($1::text, false, true, '')))[1]::text, '') AS "v!""#,
        sql
    )
    .fetch_one(pool)
    .await
    .expect("value");
    v.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// How many outbox rows (any kind) are queued for `contact`.
async fn queued(pool: &PgPool, contact: Uuid) -> String {
    let sql =
        format!("SELECT count(*) AS v FROM messaging.outbox WHERE recipient_id = '{contact}'");
    value(pool, sql).await
}

/// The job's picks as "business quote_number channel", oldest first.
async fn picks(pool: &PgPool, job: Uuid) -> Vec<String> {
    let sql = format!(
        "SELECT string_agg(business_id || ' ' || quote_number || ' ' || channel, ',' \
         ORDER BY picked_at, id) AS v FROM picks.pick WHERE job_id = '{job}'"
    );
    let v = value(pool, sql).await;
    v.split(',')
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether the CON-016 wrote-again follow-up is queued for inbound text `sid`.
async fn wrote_again(pool: &PgPool, sid: &str) -> bool {
    let sql = format!(
        "SELECT count(*) AS v FROM core.task t JOIN messaging.inbox i \
         ON t.dedupe_key = '{OPTOUT_FOLLOWUP_KIND}:' || i.id || ':wrote_again' \
         WHERE i.message_sid = '{sid}'"
    );
    value(pool, sql).await == "1"
}

/// Puts her number on the suppression list with `word` (STOP or CANCEL) through the route, then
/// sends `words` and checks it queued no outbox row and kept its wrote-again follow-up.
async fn opted_out_then(pool: &PgPool, her: Uuid, (word, words): (&str, &str)) {
    text(pool, 1, word).await;
    let before = queued(pool, her).await;
    let sid = text(pool, 2, words).await;
    assert_eq!(
        queued(pool, her).await,
        before,
        "{words}: no text (CON-013)"
    );
    assert!(wrote_again(pool, &sid).await, "{words}: CON-016 follow-up");
}

/// TD-PICK-75 (PICK-030, D-1614): after the Terms line, an opted-out "1 YES" picks quote 1 with
/// channel `page`, queues no text and keeps the wrote-again follow-up.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_75_after_the_terms_line_an_opted_out_yes_picks(pool: PgPool) {
    let (her, jobs) = ready(&pool, &["plumbing"], true).await;
    opted_out_then(&pool, her, ("STOP", "1 YES")).await;
    let picked = picks(&pool, jobs[0]).await;
    assert_eq!(picked, [format!("{} 1 page", Uuid::from_u128(0xb001))]);
}

/// TD-PICK-75 (PICK-030, D-478): before any Terms line nothing is picked.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_75_before_the_terms_line_nothing_is_picked(pool: PgPool) {
    let (her, jobs) = ready(&pool, &["plumbing"], false).await;
    opted_out_then(&pool, her, ("STOP", "1 YES")).await;
    assert!(picks(&pool, jobs[0]).await.is_empty());
}

/// TD-PICK-75 (D-1614): with two jobs open "which?" cannot be texted, so nothing is picked.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_75_two_open_jobs_pick_nothing(pool: PgPool) {
    let (her, jobs) = ready(&pool, &["plumbing", "electrical"], true).await;
    opted_out_then(&pool, her, ("STOP", "1 YES")).await;
    for job in jobs {
        assert!(picks(&pool, job).await.is_empty());
    }
}

/// TD-PICK-75 (CON-029): a CANCEL pauses her job, so a later "1 YES" picks nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_75_a_paused_job_picks_nothing(pool: PgPool) {
    let (her, jobs) = ready(&pool, &["plumbing"], true).await;
    opted_out_then(&pool, her, ("CANCEL", "1 YES")).await;
    assert!(picks(&pool, jobs[0]).await.is_empty());
}

/// TD-PICK-75 (CON-016): "hello" from the same number keeps its wrote-again follow-up.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_75_hello_keeps_its_wrote_again_follow_up(pool: PgPool) {
    let (her, jobs) = ready(&pool, &["plumbing"], true).await;
    opted_out_then(&pool, her, ("STOP", "hello")).await;
    assert!(picks(&pool, jobs[0]).await.is_empty());
}

/// TD-PICK-75 (CON-016, INT-002): a "1 YES" carrying a 911-list word keeps the paused-emergency
/// path and picks nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_75_a_911_word_picks_nothing(pool: PgPool) {
    let (_, jobs) = ready(&pool, &["plumbing"], true).await;
    text(&pool, 1, "STOP").await;
    let sid = text(&pool, 2, "1 YES i smell gas").await;
    assert!(picks(&pool, jobs[0]).await.is_empty());
    assert!(wrote_again(&pool, &sid).await);
}
