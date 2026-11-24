//! TD-PRIV-15, TD-PRIV-21 (T820, D-5399, D-5526; PRIV-031, CON-022): "Download my data" through
//! the assembled router over PostgreSQL also holds her withdrawn quotes (marked "Withdrawn: "), each
//! job's scrubbed description, her intake answers but never her address answer, her feedback with
//! its share tick, and her STOP, START, portal opt-out and HELP records, each kind in its own
//! section; a STOP under her number from before her contact was created is left out, and another
//! homeowner's file holds none of hers (nor hers his). Every record is built through module APIs;
//! fake data and 555 numbers only.

use std::collections::BTreeSet;

use askhava_businesses::api::{AccountStatus, create as create_business};
use askhava_core::api::{Settings, TestControl};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    Answer, RawReply, Reply, TextIn, jobs_of_contact, receive_raw_reply, record_answer,
};
use askhava_jobs::api::{JobStatus, feedback, move_to};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_messaging::api::{
    InboundText, ProviderKeyword, Relation, RevokeChannel, on_inbound_text, suppress,
};
use askhava_ops::api::{Actor, record_help_keyword};
use askhava_portals::api::{CookieJar, Realm, start_session};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, Withdrawal,
    confirm_quote, parse_quote, send_cards, submit_quote, withdraw_quote,
};
use askhava_server::{RouterParts, build_router, webhooks};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// Her fake number, address and first text.
const HER: (&str, &str, &str) = (
    "+19285550111",
    "12 Example Way",
    "my kitchen sink is leaking under the cabinet",
);
/// Another homeowner's fake number, address and first text.
const OTHER: (&str, &str, &str) = (
    "+19285550122",
    "34 Sample Road",
    "the garage door spring snapped yesterday",
);
const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";

fn vault_key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// The vault the router's webhooks build from [`pairs`].
fn vault() -> Vault {
    let key = |b: u8| SecretString::from(vault_key(b));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x52), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from(SECRET.to_owned())
}

/// Dev settings with the Twilio webhooks configured from fake values.
fn pairs() -> Vec<(String, String)> {
    let fixed = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        ("ASKHAVA_ID_SECRET", SECRET),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1"),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:1"),
        ("VAULT_KEY", &vault_key(0x51)),
        ("VAULT_INDEX_KEY", &vault_key(0x52)),
    ];
    fixed
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// The assembled router with the homeowner portal served.
fn app(pool: &PgPool) -> Router {
    let pairs = pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let hooks = webhooks(&settings, pool.clone(), pairs).expect("hooks");
    assert!(hooks.is_some(), "the webhooks are configured");
    build_router(
        pool.clone(),
        control.router(),
        RouterParts::from((hooks, None, None, None)),
    )
}

/// `contact`'s downloaded file, through a 24-hour homeowner session.
async fn download(app: &Router, pool: &PgPool, contact: Uuid) -> Value {
    let mut db = pool.acquire().await.expect("db");
    let (day, now) = (SignedDuration::from_hours(24), Timestamp::now());
    let session = (Realm::Homeowner, contact);
    let jar = start_session(&mut db, CookieJar::new(), session, day, now).await;
    let cookie: Vec<String> = (jar.expect("session").iter())
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let sent = Request::builder()
        .uri("/h/my-data")
        .header(header::HOST, "askhava.example")
        .header(header::COOKIE, cookie.join("; "))
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .expect("request");
    let answer = app.clone().oneshot(sent).await.expect("answer");
    assert_eq!(answer.status().as_u16(), 200);
    let bytes = to_bytes(answer.into_body(), 1 << 20).await.expect("body");
    serde_json::from_slice(&bytes).expect("a JSON file")
}

/// The words of `file`'s `section`.
fn words(file: &Value, section: &str) -> Vec<String> {
    let items = file[section].as_array().expect(section);
    (items.iter())
        .map(|i| i["words"].as_str().expect("words").to_owned())
        .collect()
}

/// A homeowner with `(phone, address, first text)`: her contact, and her request and job opened
/// by her first text through `receive_raw_reply`.
async fn homeowner(
    db: &mut PgConnection,
    (phone, address, first): (&str, &str, &str),
) -> (Uuid, Uuid, Uuid) {
    let new = NewContact {
        phone: Some(SecretString::from(phone.to_owned())),
        address: Some(SecretString::from(address.to_owned())),
        ..NewContact::default()
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: first,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: Timestamp::now(),
    };
    let (raw, vault) = (SecretString::from(first.to_owned()), vault());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let settings = (IntakeSettings::default(), &[][..]);
    let reply = receive_raw_reply(db, &secret(), (&text, keys), settings).await;
    let Reply::Handled(handled) = reply.expect("first text") else {
        panic!("a first text");
    };
    let request = handled.conversation.expect("a request");
    let jobs = jobs_of_contact(db, contact).await.expect("jobs");
    assert_eq!(jobs.len(), 1, "her first text opened one job");
    (contact, request, jobs[0])
}

/// One stored answer to `question` of `request`: an option's code, or a free answer.
async fn answer(
    db: &mut PgConnection,
    request: Uuid,
    question: &str,
    choice: Option<(&str, &str)>,
) {
    let options = choice.map(|c| vec![c]);
    let words = choice.map_or("12 Example Way", |(_, phrase)| phrase);
    let given = Answer {
        conversation: request,
        question,
        options: options.as_deref(),
        words,
        reply: (Uuid::new_v4(), Timestamp::now()),
    };
    record_answer(db, &given).await.expect("answer");
}

/// A business able to take a plumbing leak repair in Lake Havasu City, and that job's card.
fn plumber(business: Uuid) -> (CardBusiness, CardJob) {
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("plumbing_repair".to_owned(), "leak repair".to_owned())]),
        classes: vec!["R-37".to_owned()],
        licenses: Holder::default(),
    };
    let candidate = Job {
        category: "plumbing_repair".to_owned(),
        job_type: "leak repair".to_owned(),
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
    (to, card)
}

/// A business carded on `job`, whose firm quote is taken and then withdrawn.
async fn withdrawn_quote(db: &mut PgConnection, job: Uuid) {
    let now = Timestamp::now();
    let business = create_business(db, false, now).await.expect("business");
    let (to, card) = plumber(business);
    let open = (JobStatus::Intake, JobStatus::Open);
    let opened = move_to(db, job, open, now, Actor::System).await;
    assert!(opened.expect("open").applied().is_some(), "opened");
    let map = ClassMap::shipped().expect("the shipped license map");
    send_cards(db, map, job, &card, &[to], now)
        .await
        .expect("cards");
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 to fix the leak, parts and labor included".to_owned(),
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
    let quote = parse_quote(form, &rules).expect("quote");
    let taken = submit_quote(db, (job, business), &quote, now).await;
    assert_eq!(taken.expect("quoted"), Submitted::Saved);
    let confirmed = confirm_quote(db, job, business).await.expect("confirmed");
    assert!(confirmed.applied().is_some(), "confirmed");
    let withdrawn = withdraw_quote(db, (job, business), now).await;
    assert_eq!(withdrawn.expect("withdrawn"), Withdrawal::Withdrawn);
}

/// Moves `job` along the arrows from Intake to Done; a step it already passed moves nothing.
async fn done(db: &mut PgConnection, job: Uuid) {
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
        JobStatus::Done,
    ];
    for step in path.windows(2) {
        let moved = move_to(db, job, (step[0], step[1]), Timestamp::now(), Actor::System).await;
        let _step = moved.expect("move");
    }
}

/// One text from `phone` of `contact`'s through the opt-out check at `at`.
async fn texted(db: &mut PgConnection, (contact, phone): (Uuid, &str), words: &str, at: Timestamp) {
    let provider = match words {
        "STOP" => ProviderKeyword::OptOut,
        _ => ProviderKeyword::Start,
    };
    let msg = InboundText {
        inbox_id: Uuid::new_v4(),
        contact,
        relation: Relation::OpenRequest,
        phone,
        text: words,
        provider,
        emergency: false,
    };
    on_inbound_text(db, (&secret(), &vault()), &msg, at)
        .await
        .expect("inbound");
}

/// Her full record: a STOP under her number a day before her contact, her request, answers
/// (the address one included), withdrawn quote and feedback, then STOP, START, a portal opt-out
/// and HELP.
async fn her_records(db: &mut PgConnection) -> Uuid {
    let now = Timestamp::now();
    let earlier = now - SignedDuration::from_hours(24);
    suppress(
        db,
        &vault(),
        HER.0,
        (RevokeChannel::Text, "word:stop"),
        earlier,
    )
    .await
    .expect("an earlier holder's STOP");
    let (her, request, job) = homeowner(db, HER).await;
    answer(db, request, "bank.fixture", Some(("1", "sink"))).await;
    answer(db, request, "address_contact", None).await;
    withdrawn_quote(db, job).await;
    done(db, job).await;
    let kind = ("Fake feedback: the sink is dry now", false);
    feedback::store(db, job, kind, now).await.expect("feedback");
    let minute = |n| now + SignedDuration::from_mins(n);
    texted(db, (her, HER.0), "STOP", minute(1)).await;
    texted(db, (her, HER.0), "START", minute(2)).await;
    suppress(
        db,
        &vault(),
        HER.0,
        (RevokeChannel::Portal, "portal"),
        minute(3),
    )
    .await
    .expect("portal");
    record_help_keyword(db, her, minute(4)).await.expect("help");
    her
}

/// The other homeowner's request, feedback, STOP and HELP.
async fn other_records(db: &mut PgConnection) -> Uuid {
    let (other, request, job) = homeowner(db, OTHER).await;
    answer(db, request, "bank.fixture", Some(("2", "spring"))).await;
    done(db, job).await;
    let words = ("Fake feedback: OTHER-ONLY garage words", true);
    let now = Timestamp::now();
    feedback::store(db, job, words, now)
        .await
        .expect("feedback");
    texted(db, (other, OTHER.0), "STOP", now).await;
    record_help_keyword(db, other, now).await.expect("help");
    other
}

/// Both homeowners' records through module APIs, then her file and his, through the router.
async fn files(pool: &PgPool) -> (Value, Value) {
    let app = app(pool);
    let mut db = pool.acquire().await.expect("db");
    let her = her_records(&mut db).await;
    let other = other_records(&mut db).await;
    drop(db);
    (
        download(&app, pool, her).await,
        download(&app, pool, other).await,
    )
}

/// TD-PRIV-15 (PRIV-031, D-5399, D-5526): her file keeps one section per kind; it holds her
/// withdrawn quote marked "Withdrawn: ", her job's scrubbed description, her trade answer but not
/// her address answer, and her feedback with its share tick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_15_her_file_holds_her_jobs_answers_feedback_and_withdrawn_quote(pool: PgPool) {
    let (file, _) = files(&pool).await;
    let keys: Vec<&str> = (file.as_object().expect("an object").keys())
        .map(String::as_str)
        .collect();
    let all = [
        "answers", "consent", "feedback", "jobs", "messages", "quotes", "requests",
    ];
    assert_eq!(keys, all);
    let quotes = words(&file, "quotes");
    assert_eq!(quotes.len(), 1, "{quotes:?}");
    assert!(
        quotes[0].starts_with("Withdrawn: Quote from "),
        "{quotes:?}"
    );
    assert!(quotes[0].contains("325 to fix the leak"), "{quotes:?}");
    let jobs = words(&file, "jobs");
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert!(jobs[0].contains("kitchen sink"), "{jobs:?}");
    assert_eq!(
        words(&file, "answers"),
        ["What is it about? Your answer: Sink or faucet"]
    );
    let feedback = words(&file, "feedback");
    assert_eq!(feedback.len(), 1, "{feedback:?}");
    assert!(feedback[0].contains("the sink is dry now"), "{feedback:?}");
    assert!(feedback[0].contains("did not tick the box"), "{feedback:?}");
    let text = file.to_string();
    assert!(
        !text.contains(HER.1) && !text.contains("Example Way"),
        "no address"
    );
}

/// TD-PRIV-15, TD-PRIV-21 (PRIV-031, CON-022, D-5399): her consent section holds her STOP, START,
/// portal opt-out and HELP records after her consent, not the STOP under her number from before
/// her contact was created; his file holds none of her items and hers none of his.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_21_her_records_are_hers_alone_and_start_at_her_contact(pool: PgPool) {
    let (file, his) = files(&pool).await;
    let consent = words(&file, "consent");
    assert!(
        consent[0].starts_with("You agreed to our texts"),
        "{consent:?}"
    );
    let records: Vec<&str> = consent.iter().skip(1).map(String::as_str).collect();
    let expected = [
        "You texted STOP",
        "You texted START",
        "Texts turned off in your account on our website",
        "You texted HELP",
    ];
    assert_eq!(records, expected, "no earlier holder's STOP: {consent:?}");
    let text = file.to_string();
    assert!(
        !text.contains("OTHER-ONLY") && !text.contains("garage"),
        "nothing of his"
    );
    let his = his.to_string();
    let hers = [
        "kitchen sink",
        "dry now",
        "Withdrawn",
        "our website",
        "START",
        "Sink or faucet",
    ];
    for item in hers {
        assert!(!his.contains(item), "{item} is not in his file");
    }
    assert!(his.contains("OTHER-ONLY") && his.contains("You texted HELP"));
}
