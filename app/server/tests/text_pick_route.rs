//! TD-PICK-71 (T728, D-4727, D-5646, D-5732; PICK-015, PICK-031, PICK-016, TERMS-016, INT-002):
//! "<number> YES" from her phone through Twilio's real inbound-text route (`POST
//! /webhooks/twilio/sms`, signed fake requests) on a real database. Setup: her vault contact holds
//! a fake 555 number, her request opens through `intake::api::receive_text`, each job reaches
//! Quotes ready, carded to and quoted by two fake businesses (`quotes::api`), and the LIST text's
//! Terms line is recorded with `picks::api::record_terms_line`, as `answer_list` records it.
//! `server` names no other schema in a query macro, so reads go through `query_to_xml` as
//! `web_chat_stops.rs` does. Fake data only.

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_businesses::api::AccountStatus;

use askhava_catalog::api::load_embedded;
use askhava_core::api::{Environment, Guarded, MIGRATOR, ManualClock};
use askhava_intake::api::rules::{Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    JobStart, Opened, RawReply, Reply, TextIn, begin_job, receive_raw_reply, receive_reply,
    receive_text, terms_acceptances,
};
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_picks::api::{DONE_TEMPLATE, WHICH_TEMPLATE, record_terms_line};
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

/// POSTs inbound text `n` from `from` through the SMS route, signed by the fake token; returns
/// its `MessageSid`.
async fn text(pool: &PgPool, (from, n): (&str, u32), words: &str) -> String {
    let sid = format!("SM{n:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", LAFTLY),
        ("Body", words),
    ];
    let encode = |v: &str| -> String {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
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

/// One of her texts as `intake` reads it, `minutes` before the route's clock, with a fresh ID.
fn her_text(contact: Uuid, words: &str, minutes: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now() - SignedDuration::from_mins(minutes),
    }
}

/// Her vault contact (fake number `phone`) and her open request with one job per trade, each in
/// Quotes ready through [`quoted`].
async fn ready(pool: &PgPool, phone: &str, trades: &[&str]) -> (Uuid, Uuid, Vec<Uuid>) {
    let mut db = pool.acquire().await.expect("connection");
    let new = NewContact {
        phone: Some(SecretString::from(phone)),
        ..NewContact::default()
    };
    let her = store_contact(&mut db, &vault(), &new).await.expect("her");
    let first = her_text(her, FIRST, 60);
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
        quoted(&mut db, job, at).await;
        jobs.push(job);
    }
    (her, id, jobs)
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

/// Fake businesses 1 and 2 carded and quoting firm on `job`, the job in Quotes ready, and the
/// LIST text's Terms line recorded (PICK-016).
async fn quoted(db: &mut sqlx::PgConnection, job: Uuid, at: Timestamp) {
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
    record_terms_line(db, (job, Uuid::new_v4()), "quote_list_end:en:1", at)
        .await
        .expect("terms");
}

/// The one text value `v` of `sql` (one row), which reads other modules' tables, run through
/// `query_to_xml` as `web_chat_stops.rs` does.
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

/// The comma-separated `v` of `sql` as a list.
async fn list(pool: &PgPool, sql: String) -> Vec<String> {
    let v = value(pool, sql).await;
    v.split(',')
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The templates queued for `contact`, oldest first.
async fn sent(pool: &PgPool, contact: Uuid) -> Vec<String> {
    let sql = format!(
        "SELECT string_agg(template_id, ',' ORDER BY created_at, id) AS v FROM messaging.outbox \
         WHERE recipient_id = '{contact}'"
    );
    list(pool, sql).await
}

/// The job's picks as "business quote_number channel", oldest first.
async fn picks(pool: &PgPool, job: Uuid) -> Vec<String> {
    let sql = format!(
        "SELECT string_agg(business_id || ' ' || quote_number || ' ' || channel, ',' \
         ORDER BY picked_at, id) AS v FROM picks.pick WHERE job_id = '{job}'"
    );
    list(pool, sql).await
}

/// TD-PICK-71 (PICK-015, PICK-016, TERMS-016): after the LIST text's Terms line, "2 YES", "2 yes
/// please" and "2 SÍ" (each from its own fake phone) pick quote 2, logged with channel `text`, with
/// one acceptance (`text_pick`, this inbox row) and the pick reply as the only text, so no order
/// step, ZIP question or name-and-address answer ran.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_71_each_spelling_picks_through_the_route(pool: PgPool) {
    let phones = ["+19285550173", "+19285550174", "+19285550175"];
    for (n, (phone, words)) in (1..).zip(phones.into_iter().zip(["2 YES", "2 yes please", "2 SÍ"]))
    {
        let (her, id, jobs) = ready(&pool, phone, &["plumbing"]).await;
        let before = sent(&pool, her).await.len();
        let sid = text(&pool, (phone, n), words).await;
        assert_eq!(sent(&pool, her).await[before..], [DONE_TEMPLATE], "{words}");
        let picked = picks(&pool, jobs[0]).await;
        assert_eq!(picked, [format!("{} 2 text", Uuid::from_u128(0xb002))]);
        let mut db = pool.acquire().await.expect("connection");
        let accepted = terms_acceptances(&mut db, id).await.expect("acceptances");
        assert_eq!(accepted.len(), 1, "{words}");
        assert_eq!(accepted[0].channel, "text_pick");
        let inbox = format!("SELECT id AS v FROM messaging.inbox WHERE message_sid = '{sid}'");
        let row = Uuid::parse_str(&value(&pool, inbox).await).expect("inbox row");
        assert_eq!(accepted[0].inbox_id, Some(row));
    }
}

/// TD-PICK-71 (PICK-031): with two jobs in Quotes ready, "2 YES" asks which and picks nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_71_two_jobs_ready_ask_which(pool: PgPool) {
    let (her, _, jobs) = ready(&pool, HER, &["plumbing", "electrical"]).await;
    let before = sent(&pool, her).await.len();
    text(&pool, (HER, 1), "2 YES").await;
    assert_eq!(sent(&pool, her).await[before..], [WHICH_TEMPLATE]);
    for job in jobs {
        assert!(picks(&pool, job).await.is_empty());
    }
}

/// TD-PICK-71 (D-4727): an open request with no live job falls through to today's reply.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_71_no_live_job_falls_through(pool: PgPool) {
    let (her, _, _) = ready(&pool, HER, &[]).await;
    let before = sent(&pool, her).await.len();
    text(&pool, (HER, 1), "2 YES").await;
    let added = sent(&pool, her).await.split_off(before);
    assert!(
        added.iter().all(|t| !t.starts_with("text_pick")),
        "{added:?}"
    );
}

/// TD-PICK-71 (INT-002, INT-004): a 911-list word gets the INT-004 text and picks nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_71_a_911_word_gets_the_safety_text_and_no_pick(pool: PgPool) {
    let (her, _, jobs) = ready(&pool, HER, &["plumbing"]).await;
    let before = sent(&pool, her).await.len();
    text(&pool, (HER, 1), "2 YES i smell gas").await;
    let added = sent(&pool, her).await.split_off(before);
    assert_eq!(added.first().map(String::as_str), Some(SAFETY_TEMPLATE));
    assert!(picks(&pool, jobs[0]).await.is_empty());
}

/// TD-PICK-71 (D-5646): after HELP, "2 yes please" still picks and opens no help request.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_71_after_help_a_yes_please_picks_and_opens_no_help(pool: PgPool) {
    let (her, _, jobs) = ready(&pool, HER, &["plumbing"]).await;
    text(&pool, (HER, 1), "HELP").await;
    let before = sent(&pool, her).await.len();
    text(&pool, (HER, 2), "2 yes please").await;
    assert_eq!(sent(&pool, her).await[before..], [DONE_TEMPLATE]);
    assert_eq!(picks(&pool, jobs[0]).await.len(), 1);
    let opened = value(
        &pool,
        "SELECT count(*) AS v FROM ops.help_request".to_owned(),
    )
    .await;
    assert_eq!(opened, "0");
}

/// TD-PICK-71 (PICK-015): web chat (`receive_reply`, and its raw turn, no inbox row) never picks.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_71_web_chat_never_picks(pool: PgPool) {
    let (her, _, jobs) = ready(&pool, HER, &["plumbing"]).await;
    let mut db = pool.acquire().await.expect("connection");
    let words = "2 YES";
    let settings = (Settings::default(), &[][..]);
    let msg = her_text(her, words, 1);
    let reply = receive_reply(&mut db, &secret(), &msg, settings).await;
    assert!(!matches!(reply.expect("reply"), Reply::Picked(_)));
    let raw_text = SecretString::from(words);
    let raw = RawReply {
        raw: &raw_text,
        vault: &vault(),
    };
    let msg = her_text(her, words, 0);
    let reply = receive_raw_reply(&mut db, &secret(), (&msg, raw), settings).await;
    assert!(!matches!(reply.expect("reply"), Reply::Picked(_)));
    assert!(picks(&pool, jobs[0]).await.is_empty());
}
