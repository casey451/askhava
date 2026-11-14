//! TD-INT-42 (T432, D-3431, D-3701 to D-3704; INT-027, INT-029, INT-002, PRIV-001): the
//! name-and-address question through Twilio's real inbound-text route on a real database. A
//! regular job is asked it once, after the area check; her answer lands on her own vault contact
//! (the one the release reads) and no second contact row is made; no database text column and no
//! log line holds the name or the street; an answer naming a street like a 911-list word opens no
//! 911 text; an answer with no street number is asked again once; an emergency job whose release
//! waited for the address has it released once through T255's path.
//!
//! The ZIP step is not wired to a live text yet (D-3431), so the area check runs through
//! `intake::api::answer_zip`; every other row comes through module APIs. Fake data only
//! (555-01xx numbers, made-up names and streets).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{
    Claimed, Environment, Guarded, MIGRATOR, ManualClock, Settings, WorkerId, claim,
};
use askhava_intake::api::rules::{
    Check, Inbound, NAME_ADDRESS_TEMPLATE, SAFETY_TEMPLATE, Settings as IntakeSettings, check,
};
use askhava_intake::api::{
    ADDRESS_IN_KIND, Answer, TextIn, ZipStep, answer_zip, record_answer, run_address_needed,
};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{Actor, export_audit};
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{ADDRESS_NEEDED_KIND, PagePick, Released, page_pick};
use askhava_privacy::api::{CONTACT_FIELDS_SET, NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::inbound_text::{INBOUND_PATH, Webhooks, routes};
use askhava_server::release_sink::{PickReleaser, address_in};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, PgPool};
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

/// Her vault contact (phone only), her request opened by a route text, each of `jobs` named in
/// it and her ZIP checked: `(contact, request)`.
async fn her_request(pool: &PgPool, logs: &Logs, jobs: &[Uuid]) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from(HER)),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    drop(db);
    text(pool, logs, 1, "my AC is out").await;
    let mut db = pool.acquire().await.expect("db");
    let request: Uuid = sqlx::query_scalar(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1 AND c.closed_at IS NULL",
    )
    .bind(contact)
    .fetch_one(&mut *db)
    .await
    .expect("open request");
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
    let zip = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound: Inbound {
            words: "86403",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now(),
    };
    let (secret, settings) = (secret(), IntakeSettings::default());
    let step = answer_zip(&mut db, &secret, (&zip, request), settings).await;
    assert!(matches!(step.expect("zip"), ZipStep::Stored(_)));
    (contact, request)
}

/// How many texts of `template` were queued.
async fn queued(pool: &PgPool, template: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE template_id = $1")
        .bind(template)
        .fetch_one(pool)
        .await
        .expect("outbox")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn fields_set(pool: &PgPool) -> usize {
    let mut db = pool.acquire().await.expect("db");
    let rows = export_audit(&mut db, 0, 1000).await.expect("export");
    rows.iter()
        .filter(|r| r.action == CONTACT_FIELDS_SET)
        .count()
}

/// Every text, varchar and JSON column of every app table holding `needle` (any case), by name.
async fn holding(pool: &PgPool, needle: &str) -> Vec<String> {
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT table_schema::text, table_name::text, column_name::text \
         FROM information_schema.columns c JOIN information_schema.tables t \
         USING (table_schema, table_name) WHERE t.table_type = 'BASE TABLE' \
         AND table_schema NOT IN ('pg_catalog', 'information_schema', 'public') \
         AND data_type IN ('text', 'character varying', 'json', 'jsonb', 'ARRAY')",
    )
    .fetch_all(pool)
    .await
    .expect("columns");
    let mut found = Vec::new();
    for (schema, table, column) in columns {
        let sql = format!(
            "SELECT count(*) FROM \"{schema}\".\"{table}\" WHERE \"{column}\"::text ILIKE $1"
        );
        let hits: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(format!("%{needle}%"))
            .fetch_one(pool)
            .await
            .expect("scan");
        if hits > 0 {
            found.push(format!("{schema}.{table}.{column}"));
        }
    }
    found
}

/// TD-INT-42: a regular job is asked once, after the area check; her answer goes onto her own
/// contact (no second contact row), and no stored column or log line holds the name or street.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_42_a_regular_answer_lands_on_her_own_contact(pool: PgPool) {
    let logs = Logs::default();
    let (contact, request) = her_request(&pool, &logs, &[]).await;
    assert_eq!(
        queued(&pool, NAME_ADDRESS_TEMPLATE).await,
        0,
        "not before a reply"
    );
    text(&pool, &logs, 2, "the unit upstairs").await;
    assert_eq!(
        queued(&pool, NAME_ADDRESS_TEMPLATE).await,
        1,
        "asked after the area check"
    );
    let contacts = count(&pool, "SELECT count(*) FROM privacy.contact").await;
    text(&pool, &logs, 3, ANSWER).await;
    let stored: String = sqlx::query_scalar(
        "SELECT choice FROM intake.intake_answer WHERE conversation_id = $1 \
         AND question_id = 'address_contact'",
    )
    .bind(request)
    .fetch_one(&pool)
    .await
    .expect("address contact");
    assert_eq!(stored, contact.simple().to_string(), "her own contact");
    assert_eq!(fields_set(&pool).await, 2, "one audit row per field, once");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM privacy.contact").await,
        contacts
    );
    text(&pool, &logs, 4, "thanks").await;
    assert_eq!(queued(&pool, NAME_ADDRESS_TEMPLATE).await, 1, "asked once");
    assert_eq!(fields_set(&pool).await, 2, "a later text is no answer");
    assert!(
        !holding(&pool, "86403").await.is_empty(),
        "the scan sees the ZIP"
    );
    for needle in ["Testa", "Sample Ave"] {
        assert_eq!(
            holding(&pool, needle).await,
            Vec::<String>::new(),
            "{needle}"
        );
        let all = logs.0.lock().expect("logs").clone();
        assert!(!all.contains(needle), "a log line holds {needle}");
    }
}

/// TD-INT-42, D-165: an answer whose street reads like a 911-list word opens no 911 text, and an
/// answer with no street number is asked again once, with nothing written.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_int_42_a_street_is_no_hazard_and_no_number_asks_again(pool: PgPool) {
    let logs = Logs::default();
    her_request(&pool, &logs, &[]).await;
    text(&pool, &logs, 2, "the unit upstairs").await;
    text(&pool, &logs, 3, "Testa on Sample Avenue").await;
    assert_eq!(
        queued(&pool, NAME_ADDRESS_TEMPLATE).await,
        2,
        "asked again once"
    );
    assert_eq!(fields_set(&pool).await, 0);
    assert!(
        matches!(check(SMOKE_STREET, false), Check::Match(_)),
        "the same words in any other reply would be a 911-list match"
    );
    text(&pool, &logs, 4, SMOKE_STREET).await;
    assert_eq!(queued(&pool, SAFETY_TEMPLATE).await, 0, "no 911 text");
    assert_eq!(fields_set(&pool).await, 2, "name and street written");
    assert_eq!(queued(&pool, NAME_ADDRESS_TEMPLATE).await, 2);
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
    her_request(&pool, &logs, &[job]).await;
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
    assert_eq!(queued(&pool, NAME_ADDRESS_TEMPLATE).await, 1);
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
