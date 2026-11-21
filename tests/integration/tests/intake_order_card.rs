//! TD-INT-47 (T673, D-4360, D-4880): step 12 of INT-043's order through the route's entry,
//! `intake::api::receive_raw_reply`, with PostgreSQL, fake data and 555 numbers only. After her
//! name and address (INT-027) the card built from her kept words (CARD-007, PRIV-008) is shown
//! by text when it fits 2 segments and by link otherwise, with no contact detail (INT-041); EDIT
//! or CAMBIAR asks what to change, the reply is kept and the new card shown; SEND, OK, YES and
//! ENVIAR approve at once; no reply approves after 30 minutes on the worker clock; after a 911
//! text and SAFE the card is shown again. Nothing here is on the emergency path (INT-029, in
//! `intake_order_flow.rs`) and no cards-out text is sent here (TXT-032, there too).

use std::collections::BTreeSet;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::brand;
use askhava_intake::api::rules::{Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, CARD_EDIT_TEMPLATE, CARD_LINK_TEMPLATE, CARD_SHOWN_TEMPLATE, CARD_TIMER_KIND,
    CARDS_APPROVED_KIND, CardTimer, Handled, Queued, RawReply, Reply, TextIn, card_timer_step,
    receive_raw_reply,
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
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

/// A first text naming one trade (`handyman`) and one job type of it, door repair (D-4441).
const DOOR: &str = "my front door is sticking badly";
/// Her fake answer to the name-and-address question.
const ANSWER: &str = "Testa 12 Sample Ave";
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7321,Sample Fixit One LLC,,,2019-03-04\n";
/// What a shown card must never hold: her fake name, street, a 555 number or an email sign.
const CONTACT: [&str; 4] = ["Testa", "Sample Ave", "555", "@"];
const TASKS: &str = "SELECT count(*) FROM core.task WHERE kind = $1";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x75), &key(0x76), &url).expect("vault")
}

/// The texts `words` from fake homeowner `her` queued, in order, `minute` minutes after 11:00
/// Arizona on a fixed day, through the route's entry.
async fn sent(db: &mut PgConnection, her: Uuid, words: &str, minute: i64) -> Vec<Queued> {
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
            texts
        }
        other => panic!("handled, got {other:?}"),
    }
}

/// The catalog, the fake city list and one active, welcomed fake handyman business serving the
/// town and ticking door repair; then the app role.
async fn setup(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    let listed = import_city_list(db, date(2026, 11, 1), CITY).await;
    listed.expect("city list");
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
    let named = save_business_name(db, b, ("Sample Fixit One LLC", None), None).await;
    named.expect("name");
    let actor = Actor::Business(b);
    let towns = set_towns(db, b, &["lake_havasu_city"], actor).await;
    towns.expect("towns");
    let choice = JobTypeChoice::new("handyman", "door repair");
    let allowed: BTreeSet<_> = [choice.clone()].into();
    let ticked = set_job_types(db, b, &[choice], &allowed, actor).await;
    ticked.expect("job types");
    set_app_role(db).await.expect("SET ROLE askhava_app");
}

/// A new fake homeowner (555 number `n`) texts `first`, then answers steps 4 to 11 in minutes 1
/// to 7; returns her contact and the one text her name-and-address answer got, with its body.
async fn to_card(db: &mut PgConnection, n: u32, first: &str) -> (Uuid, &'static str, String) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(format!("+1928555{n:04}"))),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let steps = [first, "86403", "2", "door repair", "1", "not sure"];
    for (minute, words) in (0..).zip(steps.into_iter().chain(["it sticks at the top"])) {
        sent(db, her, words, minute).await;
    }
    let (template, body) = one(db, her, ANSWER, 7).await;
    (her, template, body)
}

/// The one text `words` gets, with its body.
async fn one(db: &mut PgConnection, her: Uuid, words: &str, minute: i64) -> (&'static str, String) {
    let texts = sent(db, her, words, minute).await;
    assert_eq!(texts.len(), 1, "{words}: {texts:?}");
    let body = sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(texts[0].outbox_id)
        .fetch_one(db)
        .await;
    (texts[0].template, body.expect("body"))
}

/// How many `kind` tasks are queued.
async fn tasks(db: &mut PgConnection, kind: &str) -> i64 {
    let n = sqlx::query_scalar(TASKS).bind(kind).fetch_one(db).await;
    n.expect("tasks")
}

/// The newest card timer task, as the worker would read it.
async fn timer(db: &mut PgConnection) -> CardTimer {
    let p: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(CARD_TIMER_KIND)
    .fetch_one(db)
    .await
    .expect("timer");
    let id = |key: &str| Uuid::parse_str(p[key].as_str().expect("id")).expect("uuid");
    let time = |key: &str| Timestamp::from_microsecond(p[key].as_i64().expect("time")).expect("t");
    CardTimer {
        job: id("job"),
        conversation: id("conversation"),
        card: p["card"].as_str().expect("card").to_owned(),
        show: time("show"),
        outbox: id("outbox"),
        wait: SignedDuration::from_secs(p["wait"].as_i64().expect("wait")),
        due: time("due"),
        watch: p["watch"].as_bool().expect("watch"),
    }
}

/// TD-INT-47 (INT-041, INT-027, CARD-007, PRIV-008): the card is texted after her answer with no
/// contact detail; EDIT and CAMBIAR ask what to change, the reply is kept and the new card shown;
/// another reply sends nothing; after a 911 text and SAFE the card is shown again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_the_card_is_shown_edited_and_shown_again(mut db: Db) {
    setup(&mut db).await;
    let (her, template, body) = to_card(&mut db, 181, DOOR).await;
    assert_eq!(template, CARD_SHOWN_TEMPLATE);
    assert!(body.contains("door is sticking badly"), "{body}");
    let tail = "It goes out in 30 minutes. Reply EDIT to change it, or SEND to send it now.";
    assert!(body.ends_with(tail), "{body}");
    assert!(!CONTACT.iter().any(|c| body.contains(c)), "{body}");
    assert!(sent(&mut db, her, "what is this", 8).await.is_empty());
    for (minute, word) in [(9, "edit"), (11, "CAMBIAR")] {
        assert_eq!(one(&mut db, her, word, minute).await.0, CARD_EDIT_TEMPLATE);
        let (again, new) = one(&mut db, her, "the top hinge is loose too", minute + 1).await;
        assert_eq!(again, CARD_SHOWN_TEMPLATE, "{word}");
        assert!(new.contains("door is sticking badly") && new.contains("top hinge is loose"));
        assert!(!CONTACT.iter().any(|c| new.contains(c)), "{new}");
    }
    let alarm = one(&mut db, her, "now I smell gas in the kitchen", 13).await;
    assert_eq!(alarm.0, SAFETY_TEMPLATE);
    assert_eq!(one(&mut db, her, "SAFE", 14).await.0, CARD_SHOWN_TEMPLATE);
    assert_eq!(tasks(&mut db, CARDS_APPROVED_KIND).await, 0, "not sent yet");
}

/// TD-INT-47 (INT-041): SEND, OK, YES and ENVIAR each approve the shown card at once, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_send_ok_yes_and_enviar_approve_at_once(mut db: Db) {
    setup(&mut db).await;
    for (n, word) in (1..).zip(["SEND", "ok", "Yes.", "ENVIAR"]) {
        let (her, template, _) = to_card(&mut db, 190 + n, DOOR).await;
        assert_eq!(template, CARD_SHOWN_TEMPLATE);
        assert_eq!(tasks(&mut db, CARDS_APPROVED_KIND).await, i64::from(n) - 1);
        assert!(sent(&mut db, her, word, 8).await.is_empty(), "{word}");
        assert!(sent(&mut db, her, word, 9).await.is_empty(), "{word} again");
        assert_eq!(
            tasks(&mut db, CARDS_APPROVED_KIND).await,
            i64::from(n),
            "{word}"
        );
    }
}

/// TD-INT-47 (INT-041): with no reply the timer approves the card 30 minutes after its text was
/// sent, on the worker clock, and not a second earlier.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_no_reply_sends_the_card_after_30_minutes(mut db: Db) {
    setup(&mut db).await;
    to_card(&mut db, 186, DOOR).await;
    let watch = timer(&mut db).await;
    sqlx::query(
        "UPDATE messaging.outbox SET status = 'sent', sent_at = now(), body = NULL, \
                 provider_sid = 'SM' || repeat('a', 32) WHERE id = $1",
    )
    .bind(watch.outbox)
    .execute(&mut *db)
    .await
    .expect("sent");
    assert_eq!(card_timer_step(&mut db, &watch, watch.due).await, Ok(false));
    let wait = timer(&mut db).await;
    assert_eq!(wait.due, watch.due + AUTO_SEND_AFTER);
    assert_eq!(AUTO_SEND_AFTER, SignedDuration::from_mins(30));
    let early = wait.due - SignedDuration::from_secs(1);
    assert_eq!(card_timer_step(&mut db, &wait, early).await, Ok(false));
    assert_eq!(tasks(&mut db, CARDS_APPROVED_KIND).await, 0);
    assert_eq!(card_timer_step(&mut db, &wait, wait.due).await, Ok(true));
    assert_eq!(tasks(&mut db, CARDS_APPROVED_KIND).await, 1);
}

/// The text with each `/q/` followed by 64 hex characters cut to `/q/`, and how many were cut.
fn cut_quote_tokens(text: &str) -> (String, usize) {
    let mut out = String::new();
    let mut cut = 0;
    let mut rest = text;
    while let Some(at) = rest.find("/q/") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let hex = tail.bytes().take_while(u8::is_ascii_hexdigit).count();
        if hex == 64 {
            cut += 1;
            rest = &tail[64..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    (out, cut)
}

/// TD-INT-47 (INT-041, CARD-007): a card too long for 2 segments is shown by her page link, and
/// the text holds none of the card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_a_long_card_is_shown_by_link(mut db: Db) {
    setup(&mut db).await;
    let long = format!("{DOOR}{}", " and it scrapes the frame every time".repeat(8));
    let (_, template, body) = to_card(&mut db, 187, &long).await;
    assert_eq!(template, CARD_LINK_TEMPLATE);
    let link = format!("https://{}/q/", brand().domain());
    assert!(body.contains(&link) && !body.contains("scrapes"), "{body}");
    // The quote link's 64-hex token is random and may hold "555" by chance: cut it, exactly once.
    let (words, cut) = cut_quote_tokens(&body);
    assert_eq!(cut, 1, "one 64-hex quote token, cut once: {body}");
    assert!(!CONTACT.iter().any(|c| words.contains(c)), "{body}");
    assert_eq!(
        tasks(&mut db, CARD_TIMER_KIND).await,
        1,
        "its wait runs too"
    );
}

/// TD-INT-47 (INT-041, INT-043, D-5041): her name and address stored while a 911 text waits for
/// SAFE show no card; SAFE then shows it for the first time with its wait, and SEND approves it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_47_safe_shows_a_card_held_by_a_911_text(mut db: Db) {
    setup(&mut db).await;
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550171".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(&mut db, &vault(), &new).await.expect("her");
    let steps = [DOOR, "86403", "2", "door repair", "1", "not sure"];
    for (minute, words) in (0..).zip(steps.into_iter().chain(["it sticks at the top"])) {
        sent(&mut db, her, words, minute).await;
    }
    let alarm = one(&mut db, her, "now I smell gas in the kitchen", 7).await;
    assert_eq!(alarm.0, SAFETY_TEMPLATE);
    assert!(sent(&mut db, her, ANSWER, 8).await.is_empty(), "held");
    assert_eq!(tasks(&mut db, CARD_TIMER_KIND).await, 0, "no card yet");
    let (template, body) = one(&mut db, her, "SAFE", 9).await;
    assert_eq!(template, CARD_SHOWN_TEMPLATE);
    assert!(!CONTACT.iter().any(|c| body.contains(c)), "{body}");
    assert_eq!(tasks(&mut db, CARD_TIMER_KIND).await, 1, "its wait runs");
    assert!(sent(&mut db, her, "SEND", 10).await.is_empty());
    assert_eq!(tasks(&mut db, CARDS_APPROVED_KIND).await, 1, "sent on SEND");
}
