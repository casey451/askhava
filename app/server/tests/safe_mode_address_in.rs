//! TD-OPS-48 (OPS-001, OPS-024; T707, D-3263, D-3879, D-4326, D-4419): an emergency pick's
//! `intake.address_in` run, driven through the handler `register_address_in` registers, releases
//! nothing while safe mode is on and queues its next run 15 minutes later (first under
//! `intake.address_in.<pick>.held`, then `intake.address_in.<pick>.<run_at micros>`; a rerun
//! queues nothing), fails closed when `ops.safe_mode` cannot be read (D-4790), and releases once after
//! safe mode ends. Safe mode moves only through a critical alert and `ops::api::check_safe_mode`.
//! Every row comes through a module API; fake data only.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_calls::api::job_had_line;
use askhava_core::api::{
    Claimed, Guarded, Lease, MIGRATOR, ManualClock, NewTask, Outcome, Registry, Settings, WorkerId,
    claim, enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    ADDRESS_IN_KIND, Answer, TextIn, ZipStep, answer_zip, receive_text, record_answer,
    run_address_needed, take_name_address,
};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::{
    Actor, NewAlert, Severity, acknowledge_alert, check_safe_mode, raise_alert, record_push,
};
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{
    ADDRESS_NEEDED_KIND, BUSINESS_NOTICE_KIND, PagePick, Released, is_released, page_pick,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, Quote, QuoteForm, QuoteRules, Submitted,
    parse_quote, send_cards, submit_quote,
};
use askhava_server::release_sink::{PickReleaser, register_address_in};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;

/// 10:00 Arizona.
const NOW: i64 = 1_793_638_800;
const EVERY: SignedDuration = SignedDuration::from_mins(15);

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn id_secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn key(b: u8) -> String {
    format!("1:{}", STANDARD.encode([b; 32]))
}

fn vault() -> Vault {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (seal, index) = (SecretString::from(key(0x57)), SecretString::from(key(0x67)));
    Vault::new(&seal, &index, &url).expect("vault")
}

/// The worker settings pairs: dev, one fake masked number and the same fake vault keys.
fn pairs() -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> =
        "APP_ENV=dev DATABASE_URL=postgres://fake@localhost/fake \
        TWILIO_ACCOUNT_SID=AC00000000000000000000000000000000 TWILIO_AUTH_TOKEN=fake-auth-token \
        TWILIO_VERIFY_SERVICE_SID=VA00000000000000000000000000000001 MASKED_POOL_SIZE=1 \
        MASKED_NUMBERS=+15005550100"
            .split_whitespace()
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (format!("ASKHAVA_{k}"), v.to_owned()))
            .collect();
    pairs.push(("VAULT_KEY".to_owned(), key(0x57)));
    pairs.push(("VAULT_INDEX_KEY".to_owned(), key(0x67)));
    pairs
}

fn settings() -> Settings {
    Settings::from_pairs(pairs()).expect("settings")
}

fn masked() -> MaskedPool {
    let twilio = TwilioConfig::from_settings(&settings(), pairs()).expect("pool");
    twilio.pool().clone()
}

/// The registry with only the registered `intake.address_in` handler, on the fixed clock.
fn registry() -> Registry {
    let mut registry = Registry::new();
    let clock = Arc::new(ManualClock::new(now()));
    register_address_in(&mut registry, &settings(), (masked(), clock), pairs()).expect("handler");
    registry
}

/// Safe mode on: a critical alert pushed 25 hours ago and `check_safe_mode`; returns the alert.
async fn safe_mode_starts(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let alert = NewAlert {
        kind: "ops.t707_fake_critical",
        severity: Severity::Critical,
        subject: None,
    };
    let alert = raise_alert(&mut db, &alert).await.expect("alert");
    let pushed = now() - SignedDuration::from_hours(25);
    record_push(&mut db, alert, pushed).await.expect("push");
    assert!(check_safe_mode(&mut db, now()).await.expect("on"));
    alert
}

/// Safe mode off: `alert` acknowledged and `check_safe_mode` run again.
async fn safe_mode_ends(pool: &PgPool, alert: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    assert!(acknowledge_alert(&mut db, alert, now()).await.expect("ack"));
    assert!(!check_safe_mode(&mut db, now()).await.expect("off"));
}

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

fn quote() -> Quote {
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

/// A business with its owner and its quoted emergency job (Quotes ready): `(business, job)`.
async fn emergency_job(pool: &PgPool) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now()).await;
    let business = business.expect("business");
    let owner = add_owner(&mut db, business, Uuid::new_v4(), now()).await;
    owner.expect("owner");
    let job = create(&mut db, now(), true, Actor::System).await;
    let job = job.expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, now(), Actor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let map = ClassMap::shipped().expect("the shipped license map");
    let pool_of = [card_business(business)];
    let sent = send_cards(&mut db, map, job, &card_job(), &pool_of, now()).await;
    assert_eq!(sent.expect("cards").len(), 1);
    let saved = submit_quote(&mut db, (job, business), &quote(), now()).await;
    assert_eq!(saved.expect("quote"), Submitted::Saved);
    (business, job)
}

fn text(contact: Uuid, words: &str, n: u128) -> TextIn<'_> {
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    TextIn {
        inbox_id: Uuid::from_u128(0x707e00 + n),
        contact,
        inbound,
        received_at: now(),
    }
}

/// Her request (a contact holding only her fake phone) naming `job`, with her ZIP: the request.
async fn her_request(pool: &PgPool, job: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from("+19285550192")),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault(), &new).await;
    let contact = contact.expect("contact");
    let settings = IntakeSettings::default();
    let first = text(contact, "my AC is out", 0);
    let handled = receive_text(&mut db, &id_secret(), &first, settings).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation,
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::from_u128(0x707f00), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
    let zip = text(contact, "86403", 1);
    let step = answer_zip(&mut db, &id_secret(), (&zip, conversation), settings).await;
    assert!(matches!(step.expect("zip"), ZipStep::Stored(_)));
    conversation
}

/// A $0 emergency pick released before her address (it waits), then her name and address
/// arrive, so `intake` queues its `intake.address_in` run: `(job, pick)`.
async fn waiting(pool: &PgPool) -> (Uuid, Uuid) {
    let (business, job) = emergency_job(pool).await;
    let conversation = her_request(pool, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick");
    };
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t707.free.{job}.{business}");
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
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let sink = PickReleaser::new(
        pool.clone(),
        vault(),
        masked(),
        Arc::new(ManualClock::new(now())),
    );
    let first = sink.settle_and_release(cleared.expect("cleared")).await;
    assert!(matches!(first.expect("waits"), Released::WaitingForAddress));
    let owner = WorkerId::new(pool).await.expect("worker");
    let claimed = claim(pool, owner, now(), SignedDuration::from_secs(60), 1000).await;
    let mut needed = claimed.expect("claim").into_iter();
    let needed = needed.find(|t| t.kind == ADDRESS_NEEDED_KIND);
    run_address_needed(pool.clone(), id_secret(), needed.expect("asked"))
        .await
        .expect("asked");
    let raw = SecretString::from("Dana 100 Example Way");
    let inbox = (conversation, Uuid::new_v4());
    take_name_address(&mut db, &vault(), inbox, &raw, now())
        .await
        .expect("address");
    (job, pick)
}

/// Claims every due task: the owner, `pick`'s `intake.address_in` runs and its business notices.
async fn claimed(pool: &PgPool, pick: Uuid) -> (WorkerId, Vec<Claimed>, usize) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = Timestamp::from_second(1_900_000_000).expect("time");
    let all = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    let (all, name) = (all.expect("claim"), pick.to_string());
    let of = |kind: &str| -> Vec<Claimed> {
        let named = |t: &&Claimed| t.kind == kind && t.payload["pick"] == name;
        all.iter().filter(named).cloned().collect()
    };
    (owner, of(ADDRESS_IN_KIND), of(BUSINESS_NOTICE_KIND).len())
}

/// Runs `task` through the registry as `owner`, over `pool`.
async fn run(pool: &PgPool, owner: WorkerId, task: &Claimed) -> Option<Outcome> {
    let (registry, clock) = (registry(), ManualClock::new(now()));
    let done = registry.run(pool, &clock, owner, task.clone(), Lease::WORKER);
    done.await.ok()
}

/// Whether dedupe key `key` is taken: a probe task under it is refused (`core::api::enqueue`).
async fn key_taken(pool: &PgPool, key: String) -> bool {
    let mut db = pool.acquire().await.expect("db");
    let mut probe = NewTask::new("t707.key_probe", json!({}), now());
    probe.dedupe_key = Some(key);
    enqueue(&mut db, &probe).await.expect("probe").is_none()
}

/// The release row and the masked line of `(job, pick)`.
async fn released(pool: &PgPool, (job, pick): (Uuid, Uuid)) -> (bool, bool) {
    let mut db = pool.acquire().await.expect("db");
    let row = is_released(&mut db, pick).await.expect("release row");
    (row, job_had_line(&mut db, job).await.expect("line"))
}

/// TD-OPS-48 (OPS-001, OPS-024, D-3263): in safe mode `intake`'s run releases nothing and queues
/// the next 15 minutes later under `<kind>.<pick>.held`, a rerun nothing; the next run defers
/// under `<kind>.<pick>.<its run_at micros>`; once safe mode ends the next run releases once.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_48_the_address_run_waits_in_safe_mode_then_releases_once(pool: PgPool) {
    let (job, pick) = waiting(&pool).await;
    let alert = safe_mode_starts(&pool).await;
    let (owner, first, _) = claimed(&pool, pick).await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].payload, json!({"pick": pick}));
    assert_eq!(run(&pool, owner, &first[0]).await, Some(Outcome::Done));
    run(&pool, owner, &first[0]).await;
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    let (owner, second, notices) = claimed(&pool, pick).await;
    assert_eq!((second.len(), notices), (1, 0), "one deferral, no notice");
    let due = now() + EVERY;
    assert_eq!(second[0].payload["run_at"], due.to_string());
    assert!(key_taken(&pool, format!("{ADDRESS_IN_KIND}.{pick}.held")).await);
    assert_eq!(run(&pool, owner, &second[0]).await, Some(Outcome::Done));
    run(&pool, owner, &second[0]).await;
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    let (owner, third, notices) = claimed(&pool, pick).await;
    assert_eq!(
        (third.len(), notices),
        (1, 0),
        "the rerun's deferral refused"
    );
    assert_eq!(third[0].payload["run_at"], (due + EVERY).to_string());
    let later = format!("{ADDRESS_IN_KIND}.{pick}.{}", due.as_microsecond());
    assert!(key_taken(&pool, later).await);
    safe_mode_ends(&pool, alert).await;
    assert_eq!(run(&pool, owner, &third[0]).await, Some(Outcome::Done));
    assert_eq!(released(&pool, (job, pick)).await, (true, true));
    let (_, rest, notices) = claimed(&pool, pick).await;
    assert_eq!((rest.len(), notices), (0, 1), "one notice, no run");
}

/// TD-OPS-48 (OPS-001, D-3879, D-4790): the read of `ops.safe_mode` fails (the gate's
/// connection, the pool's first, runs as `pg_monitor`, with no use of `ops`), so the handler
/// errors: no release row, no notice, and no deferral (only `intake`'s own run, to be retried).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_ops_48_an_unreadable_safe_mode_fails_closed(pool: PgPool) {
    let (job, pick) = waiting(&pool).await;
    let first = AtomicBool::new(true);
    let unreadable = PgPoolOptions::new()
        .before_acquire(|_, _| Box::pin(async { Ok(false) }))
        .after_connect(move |conn, _| {
            let gate = first.swap(false, Ordering::SeqCst);
            Box::pin(async move {
                if gate {
                    sqlx::query!("SET ROLE pg_monitor").execute(conn).await?;
                }
                Ok(())
            })
        })
        .connect_lazy_with(pool.connect_options().as_ref().clone());
    let (owner, runs, _) = claimed(&pool, pick).await;
    assert_eq!(runs.len(), 1);
    let outcome = run(&unreadable, owner, &runs[0]).await;
    assert!(matches!(outcome, Some(Outcome::Retry(_))), "{outcome:?}");
    assert_eq!(released(&pool, (job, pick)).await, (false, false));
    let (_, runs, notices) = claimed(&pool, pick).await;
    assert_eq!((runs.len(), notices), (1, 0));
    assert_eq!(
        runs[0].payload,
        json!({"pick": pick}),
        "intake's own run only"
    );
}
