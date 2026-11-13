//! TD-CALL-16 (CALL-007, CALL-008, SEC-016, SEC-017, PRIV-016; D-1836, D-3118): signed fake
//! Twilio status callbacks of the dialed leg through the real route (`build_router`), at the URL
//! the dial names.
//! The pick is seeded through module APIs only (TD-SEC-10: no SQL outside the query macros in
//! `app/`), and the queued tasks are read by one claim at the end of a test.
//! Fake numbers (555-01xx and the 500-555 test range), fake names, CallSids and IDs only.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, create as create_business};
use askhava_calls::api::{
    AnsweredBy, Caller, Connected, LoggedCall, assign_number, call_log, call_start,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{
    Claimed, Environment, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job as LicensedJob, LicenseKind};
use askhava_messaging::api::sender_add;
use askhava_ops::api::Actor as AuditActor;
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{PagePick, Settled, UNREACHABLE_STEP_KIND, page_pick, settle};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::build_router;
use askhava_server::call_status::STATUS_PATH;
use askhava_server::inbound_text::Webhooks;
use askhava_server::inbound_voice::{VOICE_PATH, VoiceHooks};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt as _;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const NUMBER: &str = "+15005550100";
const HER: &str = "+19285550171";
const SHOP: &str = "+19285550172";

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

/// When the dialed leg's callbacks arrive: a minute after the dial.
fn later() -> Timestamp {
    at() + SignedDuration::from_mins(1)
}

fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", NUMBER),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let twilio = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    twilio.pool().clone()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

/// The voice route's state with its clock at `now`.
fn hooks(db: &PgPool, now: Timestamp) -> Arc<VoiceHooks> {
    let text = Webhooks {
        pool: db.clone(),
        clock: Arc::new(ManualClock::new(now)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    };
    Arc::new(VoiceHooks {
        hooks: Arc::new(text),
        pool: masked(),
        wait: askhava_calls::api::PinWait::default(),
    })
}

/// Her fake contact, a business with a linked phone, its quoted job page-picked and settled
/// `free` on the $0 path through module APIs only (as `door_matrix.rs` seeds it), and the job's
/// line: the line.
async fn setup(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let s = |v: &str| Some(SecretString::from(v.to_owned()));
    let new = NewContact {
        name: s("Testa Example"),
        phone: s(HER),
        email: None,
        address: s("123 Fake St, Lake Havasu City, AZ 86403"),
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let business = create_business(&mut db, false, at())
        .await
        .expect("business");
    let member = (business, Uuid::new_v4());
    sender_add(&mut db, &vault(), member, SHOP)
        .await
        .expect("link");
    let job = quoted_job(&mut db, business).await;
    drop(db);
    settled_free(pool, business, job).await;
    let mut db = pool.acquire().await.expect("db");
    let connected = Connected {
        job,
        homeowner: her,
        business,
    };
    let line = assign_number(&mut db, &masked(), connected, at()).await;
    line.expect("line").id()
}

/// A job opened, carded to `business` (AC repair in Lake Havasu City) and quoted by it.
async fn quoted_job(db: &mut PgConnection, business: Uuid) -> Uuid {
    let job = create(db, at(), false, AuditActor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, at(), AuditActor::System).await;
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
    send_cards(db, map, job, &card, &[to], at())
        .await
        .expect("card");
    submit_quote(db, (job, business), &firm_quote(), at())
        .await
        .expect("quoted");
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

/// `job`'s page pick of `business`'s quote, cleared on the $0 free-first-win path and settled to
/// `free` (FEE-010, FEE-012, PICK-022), so `picks::api::accepted_pick` answers it.
async fn settled_free(pool: &PgPool, business: Uuid, job: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let types = ["AC not cooling".to_owned()];
    let facts = record_job_facts(&mut db, job, "hvac_repair", &types).await;
    facts.expect("job facts");
    let key = format!("t271.free.{job}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    });
    ledger_append(&mut db, &entry.expect("entry"))
        .await
        .expect("ledger");
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, AuditActor::System, at()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let mut tx = pool.begin().await.expect("tx");
    let settled = settle(&mut tx, &cleared.expect("cleared")).await;
    tx.commit().await.expect("commit");
    assert_eq!(settled.expect("settle"), Settled::Moved(job));
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// POSTs `fields` to `path`, signed with `token` over the public URL; status and body.
async fn post(
    v: &Arc<VoiceHooks>,
    path: &str,
    fields: &[(&str, &str)],
    token: &str,
) -> (StatusCode, String) {
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    let mut sorted = fields.to_vec();
    sorted.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(token.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in sorted {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let signature = STANDARD.encode(mac.finalize().into_bytes());
    let request = Request::post(path)
        .header("X-Twilio-Signature", signature)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let router = build_router(
        v.hooks.pool.clone(),
        axum::Router::new(),
        (None, Some(Arc::clone(v)), None, None),
    );
    let answer = router.oneshot(request).await.expect("answer");
    let status = answer.status();
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

fn sid(n: u32) -> String {
    format!("CA{n:032}")
}

/// The business's linked phone calls in as parent call `n`; the dial's TwiML.
async fn dial(v: &Arc<VoiceHooks>, n: u32) -> String {
    let parent = sid(n);
    let fields = [("CallSid", parent.as_str()), ("From", SHOP), ("To", NUMBER)];
    let (status, body) = post(v, VOICE_PATH, &fields, TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    body
}

/// The dialed leg `n + 100` of parent call `n` reports the status `word`; the route's status code.
async fn callback(v: &Arc<VoiceHooks>, n: u32, word: &str, token: &str) -> StatusCode {
    let (parent, leg) = (sid(n), sid(n + 100));
    let mut fields = vec![
        ("CallSid", leg.as_str()),
        ("ParentCallSid", parent.as_str()),
        ("CallStatus", word),
        ("From", NUMBER),
        ("To", HER),
        ("Direction", "outbound-dial"),
    ];
    if word == "completed" {
        fields.extend([("CallDuration", "60"), ("AnsweredBy", "human")]);
    }
    post(v, STATUS_PATH, &fields, token).await.0
}

/// What call `n` on `line` left: the line's call log and the parent's stored start.
async fn written(
    pool: &PgPool,
    line: Uuid,
    n: u32,
) -> (Vec<LoggedCall>, Option<(Uuid, Timestamp)>) {
    let mut db = pool.acquire().await.expect("db");
    let log = call_log(&mut db, line).await.expect("log");
    (log, call_start(&mut db, &sid(n)).await.expect("start"))
}

/// Every task queued so far, claimed once at the end of a test (the request log's lines too), and
/// how many are `business_call` checks.
async fn queued(pool: &PgPool) -> (Vec<Claimed>, usize) {
    let owner = WorkerId::new(pool).await.expect("owner");
    let at = later() + SignedDuration::from_hours(1);
    let tasks = claim(pool, owner, at, SignedDuration::from_mins(5), 1_000).await;
    let tasks = tasks.expect("claim");
    let checks = tasks
        .iter()
        .filter(|t| t.kind == UNREACHABLE_STEP_KIND && t.payload["step"] == "business_call")
        .count();
    (tasks, checks)
}

/// Every app (non-sqlx) log line's fields, for the test's thread.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl Lines {
    fn push(&self, line: String) {
        self.0.lock().expect("lines").push(line);
    }
    fn all(&self) -> String {
        self.0.lock().expect("lines").join("\n")
    }
}

impl Subscriber for Lines {
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        !meta.target().starts_with("sqlx")
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut line = String::new();
        span.record(&mut Line(&mut line));
        self.push(line);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        let mut line = String::new();
        values.record(&mut Line(&mut line));
        self.push(line);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Line(&mut line));
        self.push(line);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// No fake phone number, in any form, is in `text` (PRIV-016).
fn no_phone(text: &str) {
    for phone in [HER, SHOP, NUMBER] {
        assert!(!text.contains(&phone[2..]), "a phone number was written");
    }
}

/// TD-CALL-16 (CALL-007, CALL-008, SEC-017, PRIV-016): the dial names this route; `initiated`
/// joins the dialed leg to the start the voice webhook stored and the final logs one business
/// call joined to it and queues one check; repeats change nothing; no phone in rows or logs.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_16_initiated_then_final_logs_one_call_joined_to_its_start(db: PgPool) {
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let line = setup(&db).await;
    let twiml = dial(&hooks(&db, at()), 1).await;
    let url = format!("statusCallback=\"{BASE}{STATUS_PATH}\"");
    assert!(twiml.contains(&url), "the dial names the status route");
    let v = hooks(&db, later());
    for status in [
        "initiated",
        "initiated",
        "completed",
        "completed",
        "initiated",
    ] {
        assert_eq!(callback(&v, 1, status, TOKEN).await, StatusCode::OK);
    }
    let (log, start) = written(&db, line, 1).await;
    assert_eq!(log.len(), 1, "one call logged");
    assert_eq!(log[0].caller, Caller::Business);
    assert_eq!(log[0].answered, AnsweredBy::Person);
    assert_eq!(
        log[0].started_at,
        Some(at()),
        "joined to the webhook's start"
    );
    assert_eq!(log[0].ended_at, later());
    assert_eq!(start, Some((line, at())));
    let (tasks, checks) = queued(&db).await;
    assert_eq!(checks, 1, "one business-call check");
    no_phone(&format!("{log:?} {start:?}"));
    no_phone(
        &tasks
            .iter()
            .map(|t| t.payload.to_string())
            .collect::<String>(),
    );
    no_phone(&lines.all());
}

/// TD-CALL-16 (SEC-017, D-1836): the final before the `initiated` logs the call joined to its
/// start once, and the late `initiated` changes nothing more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_16_a_final_before_initiated_changes_nothing_more(db: PgPool) {
    let line = setup(&db).await;
    dial(&hooks(&db, at()), 2).await;
    let v = hooks(&db, later());
    assert_eq!(callback(&v, 2, "completed", TOKEN).await, StatusCode::OK);
    let before = written(&db, line, 2).await;
    assert_eq!(callback(&v, 2, "initiated", TOKEN).await, StatusCode::OK);
    let after = written(&db, line, 2).await;
    assert_eq!(before, after);
    assert_eq!(after.0.len(), 1);
    assert_eq!(after.0[0].started_at, Some(at()));
    assert_eq!(queued(&db).await.1, 1);
}

/// TD-CALL-16 (D-1836): when the voice webhook's start write was lost, `initiated` creates the
/// start at the callback's time on her line.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_16_a_lost_start_is_created_by_the_callback(db: PgPool) {
    let line = setup(&db).await;
    let v = hooks(&db, later());
    assert_eq!(callback(&v, 3, "initiated", TOKEN).await, StatusCode::OK);
    assert_eq!(written(&db, line, 3).await.1, Some((line, later())));
}

/// TD-CALL-16 (SEC-016): a forged signature gets 403 and writes nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_call_16_a_forged_signature_gets_403_and_writes_nothing(db: PgPool) {
    let line = setup(&db).await;
    dial(&hooks(&db, at()), 4).await;
    let before = written(&db, line, 4).await;
    let v = hooks(&db, later());
    for status in ["initiated", "completed"] {
        let code = callback(&v, 4, status, "not_the_token").await;
        assert_eq!(code, StatusCode::FORBIDDEN);
    }
    assert_eq!(written(&db, line, 4).await, before);
    assert!(before.0.is_empty());
    assert_eq!(queued(&db).await.1, 0);
}
