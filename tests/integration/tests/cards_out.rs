//! `intake`'s card sender on a fresh database (T251; TD-CARD-14: CARD-025, INT-029, CARD-001,
//! CARD-021, QH-005, TXT-032; TD-BIZ-41: CARD-019, CON-019, BIZ-036; D-1521, D-2203, D-2330,
//! D-2708, D-3189). Fake numbers (555-01xx), names, IDs and words only.
//!
//! A homeowner texts in, gets her consent text and a house-cleaning job, answers the ZIP question
//! (so `intake.property` holds her town), and `quotes`' store keeps her scrubbed words and the card
//! shown. Businesses are made active, welcomed, matched on a fake city list, serving the town and
//! ticking the job type. Each case runs `send_approved` or `send_emergency` and reads the card,
//! notice, facts and outbox rows. The ZIP step's Spanish texts and its privacy case across other
//! modules' tables are TD-INT-38's database half (the unit half is `app/intake/src/tests/zip_step.rs`).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, pause_self, request_welcome,
    save_business_name, set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::{category, load_embedded};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, CardStep, CardTimer, CardsOutKeys, CardsSent, IntakeError, JobStart, Opened,
    Preview, Shown, TextIn, ZIP_AGAIN_TEMPLATE, ZIP_QUESTION_TEMPLATE, ZipStep, answer_card,
    answer_zip, ask_zip, begin_job, card_timer_step, receive_text, send_approved, send_emergency,
    show_card, zip_body,
};
use askhava_integration::set_app_role;
use askhava_jobs::api::job_facts;
use askhava_licensing::api::import_city_list;
use askhava_messaging::api::{Language, held_until};
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_privacy::api::{
    CommonWords, Known, NameMemory, NewContact, Vault, scrub, store_contact,
};
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, JobCard, Place, build_card, card_digest, keep_card,
    keep_description, sent_card,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const CLEANING: &str = "house_cleaning";
const REGULAR: &str = "regular cleaning";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-9001,Sample Shine One LLC,,,2019-03-04\nCL-9002,Sample Shine Two LLC,,,2019-03-04\n\
CL-9003,Sample Shine Three LLC,,,2019-03-04\nCL-9004,Sample Shine Four LLC,,,2019-03-04\n";

/// 10:00 Arizona and 21:00 Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";
const NIGHT: &str = "2026-11-03T04:00:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn later(s: &str, minutes: i64) -> Timestamp {
    ts(s) + jiff::SignedDuration::from_mins(minutes)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-251".to_owned())
}

fn keys() -> CardsOutKeys {
    CardsOutKeys::new(secret(), vault())
}

fn text<'a>(contact: Uuid, words: &'a str, at: Timestamp) -> TextIn<'a> {
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: at,
    }
}

/// One request: her contact, the request and its job.
#[derive(Clone, Copy)]
struct Request {
    her: Uuid,
    conversation: Uuid,
    job: Uuid,
}

/// A homeowner with `phone` texts in, gets a house-cleaning job and answers the ZIP question.
async fn request(db: &mut PgConnection, phone: &str, at: Timestamp) -> Request {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "please clean my whole house every other week";
    let handled = receive_text(db, &secret(), &text(her, words, at), Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let job = job(db, conversation, CLEANING, at).await;
    let first = text(her, "", at);
    ask_zip(db, &secret(), (&first, conversation), Settings::default())
        .await
        .expect("asked");
    let reply = text(her, "86403", at);
    let zip = answer_zip(db, &secret(), (&reply, conversation), Settings::default()).await;
    assert!(matches!(zip.expect("zip"), ZipStep::Stored(_)));
    Request {
        her,
        conversation,
        job,
    }
}

async fn job(db: &mut PgConnection, conversation: Uuid, trade: &str, at: Timestamp) -> Uuid {
    let start = JobStart {
        conversation,
        words: "please clean my whole house every other week",
        emergency: false,
        trade,
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    job
}

/// Stores her scrubbed words and the card for `job` with its facts; returns the card.
async fn store(db: &mut PgConnection, r: Request, job: Uuid, emergency: bool) -> JobCard {
    let words = "Whole house cleaning every other week, three bedrooms";
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(words, &known, &mut NameMemory::default());
    let at = ts(DAY);
    let kept = keep_description(db, (r.conversation, job), Uuid::new_v4(), &scrubbed, at).await;
    assert_eq!(kept, Ok(true));
    let card = build_card(&parts(scrubbed.as_str()), DEFAULT_MIN_WORDS);
    let facts = CardFacts {
        category: CLEANING.to_owned(),
        candidates: vec![REGULAR.to_owned()],
        emergency,
    };
    let digest = card_digest(card.text());
    let ids = (r.conversation, job);
    keep_card(db, ids, &digest, &card, &facts, at)
        .await
        .expect("kept");
    card
}

fn parts(words: &str) -> CardParts {
    CardParts {
        category: "House cleaning".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: words.to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    }
}

/// Shows her the card (INT-041); returns the show's time.
async fn show(db: &mut PgConnection, r: Request, card: &JobCard, at: Timestamp) -> Timestamp {
    let preview = Preview {
        conversation: r.conversation,
        job: r.job,
        card,
        link: "https://askhava.com/q/fake-token",
        wait: AUTO_SEND_AFTER,
        at,
    };
    let shown = show_card(db, &secret(), &preview).await.expect("shown");
    assert!(matches!(shown, Shown::Texted(_)));
    at
}

/// She replies SEND.
async fn send_word(db: &mut PgConnection, r: Request, at: Timestamp) {
    let reply = text(r.her, "SEND", at);
    let step = answer_card(db, &secret(), &reply, (r.conversation, r.job)).await;
    assert!(!matches!(step.expect("answered"), CardStep::Nothing));
}

/// The wait ends with no reply: the timer of the show at `show` approves the card.
async fn wait_ends(db: &mut PgConnection, r: Request, card: &JobCard, show: Timestamp) {
    let timer = CardTimer {
        job: r.job,
        conversation: r.conversation,
        card: card_digest(card.text()),
        show,
        outbox: Uuid::nil(),
        wait: AUTO_SEND_AFTER,
        due: show + AUTO_SEND_AFTER,
        watch: false,
    };
    let approved = card_timer_step(db, &timer, show + AUTO_SEND_AFTER).await;
    assert_eq!(approved, Ok(true));
}

/// How a test business differs from an ordinary one that may get the card.
#[derive(Clone, Copy, Default)]
struct Shape {
    /// No welcome email went, so `may_get_cards` is false (TD-BIZ-41).
    no_consent: bool,
    /// A capability row: (largest job in cents or none for no limit, recurring work).
    capability: Option<(Option<i64>, bool)>,
    /// It paused itself.
    paused: bool,
}

/// An active business numbered `n` (1 to 4, its city-list name) serving the town and ticking
/// regular cleaning.
async fn business(db: &mut PgConnection, n: u128, shape: Shape) -> Uuid {
    let t = ts("2026-11-01T17:00:00Z");
    let b = create(db, false, t).await.expect("business");
    add_owner(db, b, Uuid::new_v4(), t).await.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(db, b, Event::Checked(ok), Actor::System, t)
        .await
        .expect("active");
    if !shape.no_consent {
        request_welcome(db, b, t).await.expect("welcome");
        welcome_sent(db, b, t).await.expect("sent");
    }
    let name = ["One", "Two", "Three", "Four"][usize::try_from(n - 1).expect("n")];
    let name = format!("Sample Shine {name} LLC");
    save_business_name(db, b, (&name, None), None)
        .await
        .expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(CLEANING, REGULAR);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    set_job_types(db, b, &[choice], &allowed, actor)
        .await
        .expect("job types");
    finish_shape(db, b, shape).await;
    b
}

async fn finish_shape(db: &mut PgConnection, b: Uuid, shape: Shape) {
    if let Some((largest, recurring)) = shape.capability {
        sqlx::query(
            "INSERT INTO businesses.capability (business_id, largest_job_cents, recurring, \
             changed_at) VALUES ($1, $2, $3, now())",
        )
        .bind(b)
        .bind(largest)
        .bind(recurring)
        .execute(&mut *db)
        .await
        .expect("capability");
    }
    if shape.paused {
        pause_self(db, b, ts("2026-11-01T18:00:00Z"))
            .await
            .expect("paused");
    }
}

/// The catalog and the fake city list, once per database; then the app role, as production runs.
async fn setup(pool: &PgPool) -> PgConnection {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    import_city_list(&mut db, date(2026, 11, 1), CITY)
        .await
        .expect("city list");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    db
}

/// (card rows, card-notice tasks) of `job`.
async fn counts(db: &mut PgConnection, job: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM quotes.job_card WHERE job_id = $1), \
         (SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice' \
          AND payload->>'job' = $1::text)",
    )
    .bind(job)
    .fetch_one(db)
    .await
    .expect("counts")
}

/// The businesses holding `job`'s card.
async fn holders(db: &mut PgConnection, job: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT business_id FROM quotes.job_card WHERE job_id = $1 ORDER BY 1")
        .bind(job)
        .fetch_all(db)
        .await
        .expect("holders")
}

/// The TXT-032 bodies queued to `her`.
async fn cards_out_texts(db: &mut PgConnection, her: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT body FROM messaging.outbox WHERE recipient_id = $1 \
         AND template_id = 'request_out' ORDER BY created_at",
    )
    .bind(her)
    .fetch_all(db)
    .await
    .expect("outbox")
}

/// Every card-notice payload of `job`.
async fn notices(db: &mut PgConnection, job: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'job' = $1::text ORDER BY id",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("notices")
}

/// The facts row is exactly the stored category and candidates (D-2330).
async fn assert_facts(db: &mut PgConnection, job: Uuid) {
    let facts = job_facts(db, job).await.expect("facts").expect("a row");
    assert_eq!(facts.category, CLEANING);
    assert_eq!(facts.job_types, vec![REGULAR.to_owned()]);
}

/// TD-CARD-14 (CARD-025, INT-041, CARD-019, TXT-032, D-2330, D-3189): SEND and the end of the
/// wait each record one card and one notice per selected business, send the card she was shown
/// (marked sent), write the first facts row and queue one cards-out text; a rerun adds nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_14_send_and_the_end_of_the_wait_send_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    let both = [
        business(&mut db, 1, Shape::default()).await,
        business(&mut db, 2, Shape::default()).await,
    ];
    for (phone, by_send) in [("+19285550151", true), ("+19285550152", false)] {
        let r = request(&mut db, phone, ts(DAY)).await;
        let card = store(&mut db, r, r.job, false).await;
        let shown = show(&mut db, r, &card, ts(DAY)).await;
        if by_send {
            send_word(&mut db, r, later(DAY, 1)).await;
        } else {
            wait_ends(&mut db, r, &card, shown).await;
        }
        let digest = card_digest(card.text());
        let ids = (r.conversation, r.job, digest.as_str());
        for run in 0..2 {
            let sent = send_approved(&mut db, &keys(), ids, later(DAY, 40)).await;
            let Ok(CardsSent::Sent { recipients, text }) = sent else {
                panic!("sent");
            };
            assert_eq!(recipients.len(), if run == 0 { 2 } else { 0 });
            assert!(text.is_some(), "the outbox keeps one row per job");
            assert_eq!(counts(&mut db, r.job).await, (2, 2));
        }
        let mut expected = both.to_vec();
        expected.sort_unstable();
        assert_eq!(holders(&mut db, r.job).await, expected);
        let out = sent_card(&mut db, r.job)
            .await
            .expect("read")
            .expect("sent");
        assert_eq!(out.card.text(), card.text(), "character for character");
        for notice in notices(&mut db, r.job).await {
            let sent_text = notice["card"].as_str().expect("card");
            assert!(sent_text.starts_with(card.text()), "{sent_text}");
            assert!(sent_text.ends_with("Sent to 2 businesses."));
        }
        assert_facts(&mut db, r.job).await;
        let texts = cards_out_texts(&mut db, r.her).await;
        assert_eq!(texts.len(), 1, "one TXT-032 text");
        assert!(
            texts[0].contains("is out to 2 House cleaning businesses."),
            "{}",
            texts[0]
        );
    }
}

/// TD-CARD-14 (CARD-007, D-3189): a stored card she did not approve is refused and sends nothing;
/// a digest with no stored card sends nothing and fails, so the task retries.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_14_a_changed_card_is_refused(pool: PgPool) {
    let mut db = setup(&pool).await;
    let keys = keys();
    business(&mut db, 1, Shape::default()).await;
    let r = request(&mut db, "+19285550153", ts(DAY)).await;
    let card = store(&mut db, r, r.job, false).await;
    show(&mut db, r, &card, ts(DAY)).await;
    send_word(&mut db, r, later(DAY, 1)).await;
    let changed = build_card(&parts("Whole house cleaning every week, four bedrooms"), 5);
    let facts = CardFacts {
        category: CLEANING.to_owned(),
        candidates: vec![REGULAR.to_owned()],
        emergency: false,
    };
    let other = card_digest(changed.text());
    let ids = (r.conversation, r.job);
    keep_card(&mut db, ids, &other, &changed, &facts, ts(DAY))
        .await
        .expect("kept");
    let refused = send_approved(
        &mut db,
        &keys,
        (r.conversation, r.job, &other),
        later(DAY, 2),
    );
    assert_eq!(refused.await, Ok(CardsSent::Refused));
    let unknown = "0123456789abcdef0123456789abcdef";
    let missing = send_approved(
        &mut db,
        &keys,
        (r.conversation, r.job, unknown),
        later(DAY, 2),
    );
    assert_eq!(missing.await, Err(IntakeError::NoStoredCard));
    assert_eq!(counts(&mut db, r.job).await, (0, 0));
    assert!(sent_card(&mut db, r.job).await.expect("read").is_none());
    assert!(cards_out_texts(&mut db, r.her).await.is_empty());
}

/// TD-CARD-14 (INT-029, CARD-021, D-2203, D-3189): an emergency release sends the emergency job's
/// card at once with no approval, through `queue_emergency_card` (`emergency: true`), and never the
/// regular job's card waiting in its approval window; with no stored emergency card it sends
/// nothing and fails.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_14_an_emergency_release_sends_at_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    business(&mut db, 1, Shape::default()).await;
    let r = request(&mut db, "+19285550154", ts(DAY)).await;
    let none = send_emergency(&mut db, &keys(), r.conversation, ts(DAY)).await;
    assert_eq!(none, Err(IntakeError::NoStoredCard));
    let regular = store(&mut db, r, r.job, false).await;
    show(&mut db, r, &regular, ts(DAY)).await;
    let urgent = job(&mut db, r.conversation, "landscape_maintenance", ts(DAY)).await;
    store(&mut db, r, urgent, true).await;
    let sent = send_emergency(&mut db, &keys(), r.conversation, later(DAY, 1)).await;
    let Ok(CardsSent::Sent { recipients, .. }) = sent else {
        panic!("sent");
    };
    assert_eq!(recipients.len(), 1);
    assert_eq!(counts(&mut db, urgent).await, (1, 1));
    assert_eq!(
        counts(&mut db, r.job).await,
        (0, 0),
        "the regular card waits"
    );
    assert!(sent_card(&mut db, r.job).await.expect("read").is_none());
    let notice = &notices(&mut db, urgent).await[0];
    assert_eq!(notice["emergency"], Value::Bool(true));
    assert_facts(&mut db, urgent).await;
    assert_eq!(cards_out_texts(&mut db, r.her).await.len(), 1);
    let again = send_emergency(&mut db, &keys(), r.conversation, later(DAY, 2)).await;
    assert_eq!(
        again,
        Err(IntakeError::NoStoredCard),
        "nothing left to release"
    );
}

/// TD-CARD-14 and TD-BIZ-41 (CARD-001, CARD-016, BIZ-049, CON-020, D-2708, D-1715): on both paths
/// a self-paused business, one with no welcome (`may_get_cards` false), one whose largest job is
/// below the band floor and one that said no to recurring work get no card; one with no
/// capability row does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_14_skipped_businesses_get_no_card(pool: PgPool) {
    let mut db = setup(&pool).await;
    let gets = business(&mut db, 1, Shape::default()).await;
    let skipped = [
        Shape {
            paused: true,
            ..Shape::default()
        },
        Shape {
            no_consent: true,
            ..Shape::default()
        },
        Shape {
            capability: Some((Some(100_000), true)),
            ..Shape::default()
        },
    ];
    for (n, shape) in (2..).zip(skipped) {
        business(&mut db, n, shape).await;
    }
    business(
        &mut db,
        4,
        Shape {
            capability: Some((None, false)),
            ..Shape::default()
        },
    )
    .await;
    for (phone, emergency) in [("+19285550155", false), ("+19285550156", true)] {
        let r = request(&mut db, phone, ts(DAY)).await;
        let band = format!("band.{}", r.job.simple());
        sqlx::query(
            "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
             VALUES ($1, $2, '5000.00', now())",
        )
        .bind(r.conversation)
        .bind(band)
        .execute(&mut db)
        .await
        .expect("band answer");
        let card = store(&mut db, r, r.job, emergency).await;
        if emergency {
            send_emergency(&mut db, &keys(), r.conversation, ts(DAY))
                .await
                .expect("sent");
        } else {
            show(&mut db, r, &card, ts(DAY)).await;
            send_word(&mut db, r, later(DAY, 1)).await;
            let digest = card_digest(card.text());
            let ids = (r.conversation, r.job, digest.as_str());
            send_approved(&mut db, &keys(), ids, later(DAY, 2))
                .await
                .expect("sent");
        }
        assert_eq!(
            holders(&mut db, r.job).await,
            vec![gets],
            "emergency: {emergency}"
        );
    }
}

/// TD-CARD-14 (CARD-016, OPS-003): with the owner's "all" switch, then the trade's switch, on,
/// nobody gets the card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_14_an_owner_pause_sends_nothing(pool: PgPool) {
    let mut db = setup(&pool).await;
    business(&mut db, 1, Shape::default()).await;
    let trade = category(&mut db, CLEANING)
        .await
        .expect("read")
        .expect("stored")
        .id;
    let owner = Actor::Owner(Uuid::from_u128(0x251_0ce0));
    for (phone, switch) in [
        ("+19285550157", Switch::All),
        ("+19285550158", Switch::Trade(trade)),
    ] {
        set_switch(&mut db, switch, true, owner).await.expect("on");
        let r = request(&mut db, phone, ts(DAY)).await;
        let card = store(&mut db, r, r.job, false).await;
        show(&mut db, r, &card, ts(DAY)).await;
        send_word(&mut db, r, later(DAY, 1)).await;
        let digest = card_digest(card.text());
        let ids = (r.conversation, r.job, digest.as_str());
        send_approved(&mut db, &keys(), ids, later(DAY, 2))
            .await
            .expect("sent");
        assert_eq!(counts(&mut db, r.job).await, (0, 0));
        assert!(
            cards_out_texts(&mut db, r.her).await.is_empty(),
            "no text for 0 businesses"
        );
        set_switch(&mut db, switch, false, owner)
            .await
            .expect("off");
    }
}

/// TD-CARD-14 (QH-005, D-042, TXT-032, D-241): a card approved at 21:00 Arizona is recorded and its
/// notice queued (held whole to 8 a.m. by `messaging`, TD-CARD-15, whose 24 hours start at the
/// send); the one TXT-032 text says it goes out at 8 a.m.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_14_night_cards_wait_for_8_am(pool: PgPool) {
    let mut db = setup(&pool).await;
    business(&mut db, 1, Shape::default()).await;
    let r = request(&mut db, "+19285550159", ts(DAY)).await;
    let card = store(&mut db, r, r.job, false).await;
    show(&mut db, r, &card, ts(DAY)).await;
    send_word(&mut db, r, ts(NIGHT)).await;
    let digest = card_digest(card.text());
    let ids = (r.conversation, r.job, digest.as_str());
    send_approved(&mut db, &keys(), ids, ts(NIGHT))
        .await
        .expect("sent");
    assert_eq!(counts(&mut db, r.job).await, (1, 1));
    assert_eq!(held_until(ts(NIGHT)), Some(ts("2026-11-03T15:00:00Z")));
    let texts = cards_out_texts(&mut db, r.her).await;
    assert_eq!(texts.len(), 1);
    assert!(texts[0].contains("It goes out at 8"), "{}", texts[0]);
    assert!(
        texts[0].contains("Quotes may be slow with only 1"),
        "{}",
        texts[0]
    );
}

/// TD-INT-38 (INT-049, TXT-039, PRIV-016): a Spanish request gets the Spanish ZIP question and
/// re-ask; the ZIP she sends is stored on the property and is in no answer, audit value, outbox
/// text or task payload.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_38_spanish_zip_texts_and_the_zip_goes_nowhere_else(pool: PgPool) {
    let mut db = setup(&pool).await;
    let spanish = Settings {
        spanish_conversations_on: true,
        safety_call_on: false,
    };
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550160".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let first = text(her, "Hola, mi lavabo gotea mucho", ts(DAY));
    let handled = receive_text(&mut db, &secret(), &first, spanish).await;
    let id = handled.expect("open").conversation.expect("request");
    for (words, minute) in [("", 1), ("no sé", 2), ("es 86404", 3)] {
        let msg = text(her, words, later(DAY, minute));
        let step = if words.is_empty() {
            ask_zip(&mut db, &secret(), (&msg, id), spanish)
                .await
                .map(|_| ZipStep::Waiting)
        } else {
            answer_zip(&mut db, &secret(), (&msg, id), spanish).await
        };
        step.expect("step");
    }
    let bodies: Vec<String> = sqlx::query_scalar(
        "SELECT body FROM messaging.outbox WHERE recipient_id = $1 \
         AND template_id LIKE 'zip_%' ORDER BY created_at, id",
    )
    .bind(her)
    .fetch_all(&mut db)
    .await
    .expect("outbox");
    let expected =
        [ZIP_QUESTION_TEMPLATE, ZIP_AGAIN_TEMPLATE].map(|t| zip_body(t, Language::Es, true));
    assert_eq!(bodies, expected);
    let found: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM intake.intake_answer a WHERE a::text LIKE '%86404%') + \
         (SELECT count(*) FROM ops.audit_log l WHERE l::text LIKE '%86404%') + \
         (SELECT count(*) FROM messaging.outbox o WHERE o::text LIKE '%86404%') + \
         (SELECT count(*) FROM core.task t WHERE t::text LIKE '%86404%')",
    )
    .fetch_one(&mut db)
    .await
    .expect("count");
    assert_eq!(found, 0);
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM intake.property WHERE zip = '86404'")
            .fetch_one(&mut db)
            .await
            .expect("property");
    assert_eq!(stored, 1);
}
