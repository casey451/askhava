//! TD-INT-42 (T432, D-3431, D-3701 to D-3705; INT-027, INT-029, INT-002, PRIV-001): the
//! name-and-address question through Twilio's real inbound-text route on a real database. A regular
//! job is asked it once, after the area check; her answer lands on her own vault contact (the one
//! the release reads) and no second contact row is made; the scrubbed words, no audit row and no
//! log line hold the name or the street; an answer naming a street like a 911-list word opens no
//! 911 text; an answer with no street number is asked again once; an emergency job whose release
//! waited for the address has it released once through T255's path. A hazard sent while the
//! question is open keeps its 911 text, and SAFE is not her answer (D-3705).
//!
//! The ZIP step is not wired to a live text yet (D-3702), so her first text and the area check
//! run through `intake::api::receive_text` and `answer_zip`; this crate reads no module's table
//! (TD-SEC-10), so what was asked is read through `name_address_open`, the audit log and the
//! intake replies. Fake data only (555-01xx numbers, made-up names and streets).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{
    Claimed, Environment, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{
    Check, Inbound, NAME_ADDRESS_TEMPLATE, Settings as IntakeSettings, check,
};
use askhava_intake::api::{
    ADDRESS_IN_KIND, Answer, RawReply, Reply, Taken, TextIn, ZipStep, answer_zip,
    ask_last_question, emergency_events, name_address_open, reads_as_address, receive_raw_reply,
    receive_text, record_answer, run_address_needed, safe_hold,
};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor, export_audit};
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{ADDRESS_NEEDED_KIND, PagePick, Released, page_pick};
use askhava_privacy::api::{
    CONTACT_FIELDS_SET, InboundWords, NewContact, Vault, find_by_phone, read_inbound_for,
    store_contact,
};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::inbound_text::{INBOUND_PATH, Webhooks, answer_words, routes};
use askhava_server::release_sink::{PickReleaser, address_in};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HER: &str = "+19285550172";
const LAFTLY: &str = "+19285550100";
/// Her answer: a made-up first name and street.
const ANSWER: &str = "Testa 12 Sample Ave";
/// An answer whose street holds a 911-list word, in lower case with a short number and no
/// suffix, so neither the scrubber nor the address strip removes it.
const SMOKE_STREET: &str = "Testa 21 smoke tree";
/// A hazard with a digit in it, sent while the question is open (D-3705).
const OUTLET: &str = "smoke coming out of the outlet in 2 rooms";
/// November 12, 2026, 11:00 Arizona time.
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
        intake: IntakeSettings::default(),
    })
}

/// Every log field written while the route runs, with every span's fields.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<String>>);

impl Visit for Logs {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut all = self.0.lock().expect("logs");
        let _ = writeln!(all, "{}={value:?}", field.name());
    }
}

impl Subscriber for Logs {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        span.record(&mut self.clone());
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        values.record(&mut self.clone());
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        event.record(&mut self.clone());
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// POSTs inbound text `n` from her, signed by the fake token, with every log line kept in `logs`.
async fn text(pool: &PgPool, logs: &Logs, n: u32, words: &str) {
    let sid = format!("SM{n:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", HER),
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
    let _guard = tracing::subscriber::set_default(logs.clone());
    let answer = routes(hooks(pool)).oneshot(request).await.expect("answer");
    assert_eq!(answer.status(), StatusCode::OK);
}

/// One of her texts as `intake` reads it: plain scrubbed words, no flags.
fn her_text(contact: Uuid, words: &str) -> TextIn<'_> {
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
        received_at: now(),
    }
}

/// Her vault contact (phone only), her request opened by her first text, each of `jobs` named in
/// it and her ZIP checked: `(contact, request)`.
async fn her_request(pool: &PgPool, jobs: &[Uuid]) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from(HER)),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let (secret, settings) = (secret(), IntakeSettings::default());
    let first = her_text(contact, "my AC is out");
    let handled = receive_text(&mut db, &secret, &first, settings).await;
    let request = handled.expect("request").conversation.expect("open");
    for (i, job) in jobs.iter().enumerate() {
        let code = job.simple().to_string();
        let question = format!("job.hvac.p{i}");
        let answer = Answer {
            conversation: request,
            question: &question,
            options: Some(&[(code.as_str(), "fakejob")]),
            words: "fakejob",
            reply: (Uuid::new_v4(), now()),
        };
        record_answer(&mut db, &answer).await.expect("job named");
    }
    let zip = her_text(contact, "86403");
    let step = answer_zip(&mut db, &secret, (&zip, request), settings).await;
    assert!(matches!(step.expect("zip"), ZipStep::Stored(_)));
    (contact, request)
}

/// Whether her name-and-address question is open now.
async fn open(pool: &PgPool, contact: Uuid) -> bool {
    let mut db = pool.acquire().await.expect("db");
    name_address_open(&mut db, contact).await.expect("open")
}

/// The audit log, each row's `Debug` text, and the subjects of the `action` rows.
async fn audit(pool: &PgPool, action: &str) -> (String, Vec<Uuid>) {
    let mut db = pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, 1000).await.expect("export");
    let of = rows.iter().filter(|r| r.action == action);
    (format!("{rows:?}"), of.map(|r| r.subject_id).collect())
}

/// The scrubbed words the route reads from her `raw` text (PRIV-026, AI-003): the same reader,
/// her vault name and stated names, no sole-word lists. It keeps a first name it has not seen,
/// so the route hands on [`answer_words`] instead (D-3705).
async fn scrubbed(pool: &PgPool, contact: Uuid, raw: &str) -> String {
    let mut db = pool.acquire().await.expect("db");
    let words = InboundWords {
        emergency: &[],
        opt_out: &[],
        help: &[],
        start: &[],
    };
    let read = read_inbound_for(&mut db, &vault(), contact, raw, &words).await;
    read.expect("read").text.as_str().to_owned()
}

/// TD-INT-42: a regular job is asked once, after the area check; her answer goes onto her own
/// contact (no second contact row), and no scrubbed word, audit row or log line holds the name
/// or street. The
/// intake answers hold codes only (their `choice` column takes no capital letter or space).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_42_a_regular_answer_lands_on_her_own_contact(pool: PgPool) {
    let logs = Logs::default();
    let (contact, request) = her_request(&pool, &[]).await;
    assert!(!open(&pool, contact).await, "not before a reply");
    text(&pool, &logs, 2, "the unit upstairs").await;
    assert!(open(&pool, contact).await, "asked after the area check");
    let mut db = pool.acquire().await.expect("db");
    let reply = her_text(contact, "the unit upstairs");
    let twice = ask_last_question(&mut db, &secret(), (&reply, request), false).await;
    assert_eq!(twice.expect("ask"), None, "asked once per request");
    drop(db);
    let read = scrubbed(&pool, contact, ANSWER).await;
    let words = answer_words(&read, reads_as_address(ANSWER));
    assert!(
        !read.contains("Sample Ave"),
        "the scrubber removes the street"
    );
    text(&pool, &logs, 3, ANSWER).await;
    assert!(!open(&pool, contact).await, "answered");
    let (_, set) = audit(&pool, CONTACT_FIELDS_SET).await;
    assert_eq!(
        set,
        vec![contact, contact],
        "name and street on her own contact"
    );
    let (_, made) = audit(&pool, "contact_created").await;
    assert_eq!(made.len(), 1, "no second contact row");
    let mut db = pool.acquire().await.expect("db");
    let found = find_by_phone(&mut db, &vault(), HER).await.expect("find");
    assert_eq!(found, Some(contact), "the contact the release reads");
    drop(db);
    text(&pool, &logs, 4, "thanks").await;
    assert!(!open(&pool, contact).await, "not asked again");
    let (all, set) = audit(&pool, CONTACT_FIELDS_SET).await;
    assert_eq!(set.len(), 2, "a later text is no answer");
    let lines = logs.0.lock().expect("logs").clone();
    for needle in ["Testa", "Sample Ave"] {
        assert!(!words.contains(needle), "the scrubbed words hold {needle}");
        assert!(!all.contains(needle), "an audit row holds {needle}");
        assert!(!lines.contains(needle), "a log line holds {needle}");
    }
}

/// TD-INT-42, D-165: an answer with no street number writes nothing and is asked again once; an
/// answer whose street reads like a 911-list word opens no 911 text and is stored.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_42_a_street_is_no_hazard_and_no_number_asks_again(pool: PgPool) {
    let logs = Logs::default();
    let (contact, request) = her_request(&pool, &[]).await;
    text(&pool, &logs, 2, "the unit upstairs").await;
    text(&pool, &logs, 3, "Testa on Sample Avenue").await;
    assert!(open(&pool, contact).await, "still waiting for the address");
    assert!(audit(&pool, CONTACT_FIELDS_SET).await.1.is_empty());
    let raw = SecretString::from("Testa near Sample Avenue");
    let msg = her_text(contact, "Testa near Sample Avenue");
    let keys = RawReply {
        raw: &raw,
        vault: &vault(),
    };
    let mut db = pool.acquire().await.expect("db");
    let settings = (IntakeSettings::default(), &[][..]);
    let reply = receive_raw_reply(&mut db, &secret(), (&msg, keys), settings).await;
    let Reply::NameAddress(Taken::Again, handled) = reply.expect("reply") else {
        panic!("not asked again");
    };
    let asked: Vec<&str> = handled.texts.iter().map(|q| q.template).collect();
    assert_eq!(asked, vec![NAME_ADDRESS_TEMPLATE], "asked again once");
    assert!(
        matches!(check(SMOKE_STREET, false), Check::Match(_)),
        "the same words in any other reply would be a 911-list match"
    );
    drop(db);
    text(&pool, &logs, 4, SMOKE_STREET).await;
    let mut db = pool.acquire().await.expect("db");
    let events = emergency_events(&mut db, request).await.expect("events");
    assert!(events.is_empty(), "no 911 text");
    assert!(!safe_hold(&mut db, request).await.expect("hold"));
    assert_eq!(audit(&pool, CONTACT_FIELDS_SET).await.1.len(), 2);
    assert!(!open(&pool, contact).await, "answered");
}

fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

/// The AC job's card: one Lake Havasu City job type.
fn card_job() -> CardJob {
    let kind = Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    CardJob {
        towns: vec![OfferTown {
            code: "lake_havasu_city".to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![kind],
        text: String::new(),
    }
}

/// A firm $325 quote.
fn quote() -> askhava_quotes::api::Quote {
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

/// An emergency job, opened, carded to `business` and quoted, so it is in Quotes ready.
async fn emergency_job(pool: &PgPool, business: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now(), true, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let moved = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        now(),
        Actor::System,
    );
    assert_eq!(moved.await.expect("open"), Guarded::Applied(()));
    let town = || "lake_havasu_city".to_owned();
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from([town()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(&mut db, map, job, &card_job(), &[to], now()).await;
    assert_eq!(sent.expect("cards").len(), 1);
    let saved = submit_quote(&mut db, (job, business), &quote(), now()).await;
    assert_eq!(saved.expect("quote"), Submitted::Saved);
    job
}

/// The $0 free-first-win release of `pick`, cleared and settled through the real sink.
async fn released_free(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) -> Released {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t432.free.{job}.{business}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(&mut db, &entry).await.expect("ledger");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let clock = Arc::new(ManualClock::new(now()));
    let sink = PickReleaser::new(pool.clone(), vault(), masked(), clock);
    sink.settle_and_release(cleared.expect("cleared"))
        .await
        .expect("release")
}

async fn tasks(pool: &PgPool, kind: &str) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    let all = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    all.expect("claim")
        .into_iter()
        .filter(|t| t.kind == kind)
        .collect()
}

/// TD-INT-42, INT-029: an emergency job's release waited for the address; T255 asked again; her
/// answer through the route queues one `intake.address_in` event and the pick is released once
/// with her street.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_42_an_emergency_release_that_waited_gets_the_address_once(pool: PgPool) {
    let logs = Logs::default();
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now())
        .await
        .expect("biz");
    add_owner(&mut db, business, Uuid::new_v4(), now())
        .await
        .expect("owner");
    drop(db);
    let job = emergency_job(&pool, business).await;
    let (contact, _) = her_request(&pool, &[job]).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick");
    };
    let first = released_free(&pool, (business, job, pick)).await;
    assert!(matches!(first, Released::WaitingForAddress));
    for task in tasks(&pool, ADDRESS_NEEDED_KIND).await {
        run_address_needed(pool.clone(), secret(), task)
            .await
            .expect("asked again");
    }
    assert!(open(&pool, contact).await, "T255 asked again");
    text(&pool, &logs, 2, ANSWER).await;
    let arrived = tasks(&pool, ADDRESS_IN_KIND).await;
    assert_eq!(arrived.len(), 1, "one address_in event");
    let shown = address_in(&pool, (&vault(), &masked()), pick, now()).await;
    let Released::Shown(view) = shown.expect("re-release") else {
        panic!("not shown");
    };
    let street = view.address().expect("address").expose_secret().to_owned();
    assert_eq!(street, "12 Sample Ave");
    text(&pool, &logs, 3, ANSWER).await;
    assert!(
        tasks(&pool, ADDRESS_IN_KIND).await.is_empty(),
        "queued once"
    );
    let twice = address_in(&pool, (&vault(), &masked()), pick, now()).await;
    assert!(matches!(twice.expect("again"), Released::Already));
}

/// D-3705, INT-002: only a name, a house number and a short or hazard-free street is her answer;
/// a hazard around it, or a street with no number, is screened as any reply.
#[test]
fn td_int_42_only_a_numbered_street_skips_the_screen() {
    for answer in [ANSWER, SMOKE_STREET, "Testa 12 Smoke Sample Ln Apt 4"] {
        assert!(reads_as_address(answer), "{answer} is her answer");
    }
    let hazards = [
        OUTLET,
        "Fire at 12 Sample St",
        "Testa 12 Sample St gas leak in the kitchen",
    ];
    for text in hazards {
        assert!(!reads_as_address(text), "{text} is screened");
        assert!(
            matches!(check(text, false), Check::Match(_)),
            "{text} opens a 911 text"
        );
    }
    for text in ["Testa, Smoke Tree Ave", "Testa on Fire Rd"] {
        assert!(!reads_as_address(text), "{text} has no house number");
    }
}

/// D-3705, INT-002, INT-025: a hazard with a digit sent while the question is open opens the 911
/// text through the route and is not stored; SAFE is not her answer and asks the question once
/// more; her answer after that lands on her contact.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_42_a_hazard_keeps_its_911_text_and_safe_asks_again(pool: PgPool) {
    let logs = Logs::default();
    let (contact, request) = her_request(&pool, &[]).await;
    text(&pool, &logs, 2, "the unit upstairs").await;
    text(&pool, &logs, 3, OUTLET).await;
    let mut db = pool.acquire().await.expect("db");
    let events = emergency_events(&mut db, request).await.expect("events");
    assert_eq!(events.len(), 1, "the 911 text");
    assert!(
        audit(&pool, CONTACT_FIELDS_SET).await.1.is_empty(),
        "nothing stored"
    );
    assert!(open(&pool, contact).await, "still waiting for the address");
    let raw = SecretString::from("SAFE");
    let keys = RawReply {
        raw: &raw,
        vault: &vault(),
    };
    let settings = (IntakeSettings::default(), &[][..]);
    let safe = her_text(contact, "SAFE");
    let reply = receive_raw_reply(&mut db, &secret(), (&safe, keys), settings).await;
    let Reply::Handled(handled) = reply.expect("reply") else {
        panic!("SAFE taken as her answer");
    };
    let asked = handled
        .texts
        .iter()
        .filter(|q| q.template == NAME_ADDRESS_TEMPLATE);
    assert_eq!(asked.count(), 1, "asked again once");
    assert!(!safe_hold(&mut db, request).await.expect("hold"));
    drop(db);
    assert!(
        audit(&pool, CONTACT_FIELDS_SET).await.1.is_empty(),
        "SAFE is no answer"
    );
    text(&pool, &logs, 4, ANSWER).await;
    assert_eq!(
        audit(&pool, CONTACT_FIELDS_SET).await.1,
        vec![contact, contact]
    );
    assert!(!open(&pool, contact).await, "answered");
}
