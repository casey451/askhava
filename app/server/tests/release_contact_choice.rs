//! TD-PICK-52 release cases (T266; PICK-018, PICK-042, PICK-022, PRIV-001, PRIV-003; D-1264,
//! D-1740, D-1761): `server`'s release sink passes the homeowner's stored contact choice as
//! `email_chosen`, on the first release and on T255's re-release after the address.
//!
//! Every row comes through module APIs (business, job, card, quote, page pick, contact choice,
//! vault contact and request, the $0 ledger entry); this crate seeds no module's schema
//! (TD-SEC-10). Each pick is released on the $0 free-first-win path, so the proof is the real one
//! `release_minter()` mints inside the sink. Her vault contact always holds the fake email, so
//! only the stored choice decides whether a release opens it. Fake data only.

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{Guarded, MIGRATOR, ManualClock, Settings};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, ZipStep, answer_zip, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{
    ContactChoice, PagePick, Released, ViewAnswer, page_pick, record_contact_choice, released_view,
};
use askhava_privacy::api::{
    NewContact, Vault, set_name_address, store_contact, store_shared_email,
};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::release_sink::{PickReleaser, address_in};
use askhava_server::session_minter;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::types::Uuid;

const EMAIL: &str = "testa.t266@example.com";

fn now() -> Timestamp {
    Timestamp::from_second(1_793_638_800).expect("time")
}

fn id_secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x66), &key(0x76), &url).expect("vault")
}

/// A masked pool of one fake number.
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

/// A business and its owner, an emergency job carded to it and quoted (Quotes ready, its facts
/// recorded, D-3118): `(business, owner, job)`.
async fn quoted_job(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now())
        .await
        .expect("business");
    let person = add_owner(&mut db, business, Uuid::new_v4(), now())
        .await
        .expect("owner");
    let job = create(&mut db, now(), true, Actor::System)
        .await
        .expect("job");
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
    carded(&mut db, business, job).await;
    let saved = submit_quote(&mut db, (job, business), &quote(), now()).await;
    assert_eq!(saved.expect("quote"), Submitted::Saved);
    (business, person, job)
}

/// `job`'s card sent to `business` alone (`hvac_repair`, `AC not cooling`).
async fn carded(db: &mut sqlx::PgConnection, business: Uuid, job: Uuid) {
    let candidate = Job {
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
    let card = CardJob {
        towns: vec![town],
        candidates: vec![candidate],
        text: String::new(),
    };
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(db, map, job, &card, &[to], now()).await;
    assert_eq!(sent.expect("cards").len(), 1);
}

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

/// Her request naming `job` and her ZIP, her vault contact holding her fake phone and the fake
/// email (`n` tells homeowners apart); returns the contact.
async fn her_request(pool: &PgPool, job: Uuid, n: u128) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        phone: Some(SecretString::from(format!("+1928555{:04}", 260 + n))),
        ..NewContact::default()
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let email = SecretString::from(EMAIL);
    store_shared_email(&mut db, &vault(), contact, &email)
        .await
        .expect("email");
    let text = |words, m: u128| TextIn {
        inbox_id: Uuid::from_u128(0x266e00 + 16 * n + m),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: now(),
    };
    let first = text("my AC is out", 0);
    let handled = receive_text(&mut db, &id_secret(), &first, IntakeSettings::default())
        .await
        .expect("request");
    let conversation = handled.conversation.expect("open request");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation,
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::from_u128(0x266f00 + n), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
    let zip = text("86403", 1);
    let (secret, settings) = (id_secret(), IntakeSettings::default());
    let step = answer_zip(&mut db, &secret, (&zip, conversation), settings).await;
    assert!(matches!(step.expect("zip"), ZipStep::Stored(_)));
    contact
}

/// A quoted job, her request and page pick with `choice` stored in the pick's transaction
/// (`None`: no stored choice, as a text pick); with `address`, her name and address are on file
/// before the release. Returns `(business, owner, job, pick, contact)`.
async fn picked(
    pool: &PgPool,
    n: u128,
    choice: Option<ContactChoice>,
    address: bool,
) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let (business, person, job) = quoted_job(pool).await;
    let contact = her_request(pool, job, n).await;
    if address {
        named(pool, contact).await;
    }
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::Homeowner(contact), now()).await;
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick");
    };
    if let Some(choice) = choice {
        let stored = record_contact_choice(&mut tx, pick, choice, now()).await;
        assert!(stored.expect("choice"));
    }
    tx.commit().await.expect("commit");
    (business, person, job, pick, contact)
}

async fn named(pool: &PgPool, contact: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let (name, street) = (
        SecretString::from("Dana"),
        SecretString::from("100 Example Way"),
    );
    set_name_address(&mut db, &vault(), contact, &name, &street)
        .await
        .expect("name and address");
}

fn sink(pool: &PgPool) -> PickReleaser {
    let clock = Arc::new(ManualClock::new(now()));
    PickReleaser::new(pool.clone(), vault(), masked(), clock)
}

/// The $0 free-first-win decision for the pick, cleared, settled and released by the sink.
async fn released_free(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid)) -> Released {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t266.free.{job}.{business}");
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
    let released = sink(pool)
        .settle_and_release(cleared.expect("cleared"))
        .await;
    released.expect("release")
}

/// The email the release showed, then the email the owner's released view shows (T253's read).
async fn emails(
    pool: &PgPool,
    released: Released,
    (person, business, pick): (Uuid, Uuid, Uuid),
) -> [Option<String>; 2] {
    let Released::Shown(view) = released else {
        panic!("not shown");
    };
    let open = |v: Option<&SecretString>| v.map(|e| e.expose_secret().to_owned());
    let shown = open(view.email());
    let proof = session_minter().business(person, business);
    let ViewAnswer::Shown(seen) = released_view(pool, &vault(), &proof, pick)
        .await
        .expect("view")
    else {
        panic!("not viewable");
    };
    [shown, open(seen.email())]
}

/// TD-PICK-52 (PICK-018, PICK-042, D-1761): on the first release an email choice releases with
/// `email_chosen` true (the release view and the released view show the email); a call choice and
/// a pick with no stored choice (a text pick) release with it false (no email shown).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_52_the_first_release_follows_the_stored_choice(pool: PgPool) {
    let cases = [
        (Some(ContactChoice::Email), Some(EMAIL.to_owned())),
        (Some(ContactChoice::Call), None),
        (None, None),
    ];
    for (n, (choice, want)) in (0..).zip(cases) {
        let (business, person, job, pick, _) = picked(&pool, n, choice, true).await;
        let released = released_free(&pool, (business, job, pick)).await;
        let got = emails(&pool, released, (person, business, pick)).await;
        assert_eq!(got, [want.clone(), want], "case {n}");
    }
}

/// TD-PICK-52 (PICK-018, INT-029, D-1761): an emergency release that waited for the address
/// re-releases (T255's `address_in`) with the same stored choice: email shown for an email choice,
/// none for a call choice.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_52_the_re_release_follows_the_stored_choice(pool: PgPool) {
    let cases = [
        (ContactChoice::Email, Some(EMAIL.to_owned())),
        (ContactChoice::Call, None),
    ];
    for (n, (choice, want)) in (0..).zip(cases) {
        let (business, person, job, pick, contact) = picked(&pool, n, Some(choice), false).await;
        let waited = released_free(&pool, (business, job, pick)).await;
        assert!(matches!(waited, Released::WaitingForAddress), "case {n}");
        named(&pool, contact).await;
        let again = address_in(&pool, (&vault(), &masked()), pick, now()).await;
        let got = emails(&pool, again.expect("re-release"), (person, business, pick)).await;
        assert_eq!(got, [want.clone(), want], "case {n}");
    }
}
