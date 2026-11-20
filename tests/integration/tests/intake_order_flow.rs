//! TD-INT-47 (T673, D-4360): INT-043's order end to end through the route's entry,
//! `intake::api::receive_raw_reply`, with PostgreSQL, fake data and 555 numbers only. A fake
//! homeowner's texts go from the first text through consent, the ZIP, the coverage line, "How
//! soon?", the trade questions, her name and address (INT-027) and SEND (INT-041) to card rows
//! sent to the seeded fake businesses by the registered `intake.cards_approved` handler
//! (CARD-007, TXT-032), with no card before SEND; on the urgent Emergency path the emergency card
//! goes out through the registered `intake.emergency_released` handler before the trade questions
//! and no card review is shown (INT-029). Every first text names one trade and one job type of it
//! (D-4441). Kept words carry no contact detail (PRIV-008).

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::Claimed;
use askhava_intake::api::rules::{CONSENT_TEMPLATE, Inbound, NAME_ADDRESS_TEMPLATE, Settings};
use askhava_intake::api::{
    CARD_SHOWN_TEMPLATE, CARDS_APPROVED_KIND, CARDS_OUT_TEMPLATE, CHOICE_TEMPLATE,
    COUNT_MANY_TEMPLATE, CardsOutKeys, EMERGENCY_RELEASED_KIND, HOW_SOON_TEMPLATE, Handled,
    RawReply, Reply, TRADE_QUESTION_TEMPLATE, TextIn, ZIP_QUESTION_TEMPLATE, receive_raw_reply,
    run_cards_approved, run_emergency_released,
};
use askhava_integration::set_app_role;
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
/// A first text naming one trade (`handyman`) and one job type of it, door repair (D-4441).
const DOOR: &str = "my front door is sticking badly";
/// Her fake answer to the name-and-address question.
const ANSWER: &str = "Testa 12 Sample Ave";
/// The fake city list the two handyman businesses are matched on.
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7311,Sample Fixit One LLC,,,2019-03-04\nCL-7312,Sample Fixit Two LLC,,,2019-03-04\n";
const Q: &str = TRADE_QUESTION_TEMPLATE;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x74), &url).expect("vault")
}

/// The templates `words` from fake homeowner `her` queued, in order, `minute` minutes after 11:00
/// Arizona on a fixed day, through the route's entry.
async fn sent(db: &mut PgConnection, her: Uuid, words: &str, minute: i64) -> Vec<&'static str> {
    let start: Timestamp = "2026-11-02T11:00:00-07:00".parse().expect("time");
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: start + SignedDuration::from_mins(minute),
    };
    let (raw, vault) = (SecretString::from(words.to_owned()), vault());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    let reply = receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[])).await;
    match reply.expect("reply") {
        Reply::Handled(Handled { texts, .. }) | Reply::NameAddress(_, Handled { texts, .. }) => {
            texts.iter().map(|q| q.template).collect()
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// An active, welcomed fake handyman business named `name`, serving the town, ticking door repair.
async fn business(db: &mut PgConnection, name: &str) -> Uuid {
    let before: Timestamp = "2026-11-02T10:00:00-07:00".parse().expect("time");
    let b = create(db, false, before).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['handyman'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(&mut *db)
    .await
    .expect("signed up as a handyman");
    let owner = add_owner(db, b, Uuid::new_v4(), before).await;
    owner.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let checked = apply(db, b, Event::Checked(ok), Actor::System, before).await;
    checked.expect("active");
    request_welcome(db, b, before).await.expect("welcome");
    welcome_sent(db, b, before).await.expect("sent");
    let named = save_business_name(db, b, (name, None), None).await;
    named.expect("name");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new("handyman", "door repair");
    let allowed: BTreeSet<_> = [choice.clone()].into();
    let ticked = set_job_types(db, b, &[choice], &allowed, actor).await;
    ticked.expect("job types");
    b
}

/// The catalog, the fake city list and both businesses, then the app role; a fake homeowner with
/// a 555 number in the vault. Returns the connection, both businesses in order, and her contact.
async fn setup(pool: &PgPool) -> (PgConnection, Vec<Uuid>, Uuid) {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    let listed = import_city_list(&mut db, date(2026, 11, 1), CITY).await;
    listed.expect("city list");
    let mut both = vec![
        business(&mut db, "Sample Fixit One LLC").await,
        business(&mut db, "Sample Fixit Two LLC").await,
    ];
    both.sort_unstable();
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550173".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &new).await;
    (db, both, her.expect("contact"))
}

/// Runs every queued `kind` event through the handler `server` registers for it; returns how many.
async fn run(pool: &PgPool, kind: &'static str) -> usize {
    let tasks: Vec<(i64, Value)> =
        sqlx::query_as("SELECT id, payload FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(kind)
            .fetch_all(pool)
            .await
            .expect("tasks");
    for (id, payload) in &tasks {
        let keys = Arc::new(CardsOutKeys::new(secret(), vault()));
        let task = Claimed {
            id: *id,
            kind: kind.to_owned(),
            payload: payload.clone(),
            attempts: 1,
        };
        let ran = if kind == CARDS_APPROVED_KIND {
            run_cards_approved(pool.clone(), keys, task).await
        } else {
            run_emergency_released(pool.clone(), keys, task).await
        };
        ran.expect("the registered handler ran");
    }
    tasks.len()
}

/// The businesses holding a card of any job, in order.
async fn holders(db: &mut PgConnection) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT business_id FROM quotes.job_card ORDER BY 1")
        .fetch_all(db)
        .await
        .expect("holders")
}

/// How many rows `sql` counts.
async fn count(db: &mut PgConnection, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.expect("count")
}

/// TD-INT-47 (INT-043, INT-041, INT-027, TXT-032, CARD-007, PRIV-008): the regular path, each text
/// in the table's order; no business gets a card before SEND, then both get the card she saw.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_regular_from_the_first_text_to_cards_sent(pool: PgPool) {
    let (mut db, both, her) = setup(&pool).await;
    let mut texts = Vec::new();
    let words = [DOOR, "86403", "2", "door repair", "1", "not sure"];
    let last = ["it sticks at the top", ANSWER];
    for (minute, words) in (0..).zip(words.into_iter().chain(last)) {
        texts.extend(sent(&mut db, her, words, minute).await);
    }
    let order = [
        CONSENT_TEMPLATE,
        ZIP_QUESTION_TEMPLATE,
        COUNT_MANY_TEMPLATE,
        HOW_SOON_TEMPLATE,
        Q,
        Q,
        Q,
        Q,
        NAME_ADDRESS_TEMPLATE,
        CARD_SHOWN_TEMPLATE,
    ];
    assert_eq!(texts, order);
    assert_eq!(run(&pool, CARDS_APPROVED_KIND).await, 0, "nothing approved");
    assert!(holders(&mut db).await.is_empty(), "no card before SEND");
    assert!(sent(&mut db, her, "SEND", 20).await.is_empty());
    assert_eq!(run(&pool, CARDS_APPROVED_KIND).await, 1);
    assert_eq!(holders(&mut db).await, both);
    let same = "SELECT count(*) FROM quotes.shown_card s JOIN intake.conversation c \
                ON c.job_id = s.job_id WHERE s.sent_at IS NOT NULL AND NOT s.emergency \
                AND s.card_text LIKE '%door is sticking%' AND s.card_text NOT LIKE '%Testa%' \
                AND s.card_text NOT LIKE '%Sample Ave%' AND s.card_text NOT LIKE '%555%'";
    assert_eq!(count(&mut db, same).await, 1, "the card she saw went out");
    let out = "SELECT count(*) FROM messaging.outbox WHERE template_id = $1";
    let told: i64 = sqlx::query_scalar(out)
        .bind(CARDS_OUT_TEMPLATE)
        .fetch_one(&mut db)
        .await
        .expect("out");
    assert_eq!(told, 1, "one TXT-032 text");
}

/// TD-INT-47 (INT-029, INT-027, INT-043): the urgent Emergency path. The emergency card goes to
/// both businesses before any trade question is answered; steps 8 to 11 follow and no card review
/// is shown, stored or approved.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_urgent_emergency_card_goes_before_the_trade_questions(pool: PgPool) {
    let (mut db, both, her) = setup(&pool).await;
    let mut texts = Vec::new();
    for (minute, words) in (0..).zip([DOOR, "86403", "1"]) {
        texts.extend(sent(&mut db, her, words, minute).await);
    }
    let order = [
        CONSENT_TEMPLATE,
        ZIP_QUESTION_TEMPLATE,
        COUNT_MANY_TEMPLATE,
        HOW_SOON_TEMPLATE,
        CHOICE_TEMPLATE,
    ];
    assert_eq!(texts, order);
    let after = sent(&mut db, her, "Emergency", 3).await;
    assert_eq!(run(&pool, EMERGENCY_RELEASED_KIND).await, 1);
    assert_eq!(holders(&mut db).await, both, "before the trade questions");
    let answers = "SELECT count(*) FROM intake.intake_answer WHERE question_id LIKE 'bank.%'";
    assert_eq!(count(&mut db, answers).await, 0);
    let mut rest = after;
    for minute in 4..12 {
        if rest.last() == Some(&NAME_ADDRESS_TEMPLATE) {
            break;
        }
        assert_eq!(rest, [Q], "steps 8 to 10 follow the card");
        rest = sent(&mut db, her, "1", minute).await;
    }
    assert_eq!(rest.last(), Some(&NAME_ADDRESS_TEMPLATE), "step 11");
    let stored = sent(&mut db, her, ANSWER, 15).await;
    assert!(!stored.contains(&CARD_SHOWN_TEMPLATE), "{stored:?}");
    let review = "SELECT count(*) FROM intake.intake_answer WHERE question_id LIKE 'card.%'";
    assert_eq!(count(&mut db, review).await, 0, "no card review");
    assert_eq!(run(&pool, CARDS_APPROVED_KIND).await, 0);
    assert_eq!(holders(&mut db).await, both, "once each");
}
