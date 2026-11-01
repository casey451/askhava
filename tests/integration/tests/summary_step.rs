//! The card approval step in the database (TD-INT-23: INT-041, CARD-007; TD-AI-17). Fake IDs, a
//! fake card and fake text only.
//!
//! The homeowner is texted the card (or the link) with no contact detail; SEND, OK or YES approves
//! it at once; EDIT asks what to change and the new card is shown; no reply in 30 minutes approves
//! the card as shown; an emergency job skips the step; and only the card she was shown passes the
//! approved-card check the card sender runs.

use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, CARD_EDIT_TEMPLATE, CARD_LINK_TEMPLATE, CARD_SHOWN_TEMPLATE,
    CARDS_APPROVED_KIND, CardStep, CardTimer, JobStart, Opened, Preview, Shown, TextIn,
    answer_card, approved_card, begin_job, card_digest, card_timer_step, receive_text, show_card,
};
use askhava_jobs::api::job_number;
use askhava_quotes::api::{CardParts, DEFAULT_MIN_WORDS, JobCard, Place, Timing, build_card};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const LINK: &str = "askhava.com/r/fake-token";
const TAIL: &str = "It goes out in 30 minutes. Reply EDIT to change it, or SEND to send it now.";

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x5_0000 + contact * 100 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

fn card(description: &str) -> JobCard {
    let parts = CardParts {
        category: "Plumbing".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: description.to_owned(),
        answers: vec![("Where is the leak".to_owned(), "Under a sink".to_owned())],
        timing: Some(Timing::NoRush),
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    build_card(&parts, DEFAULT_MIN_WORDS)
}

/// A fake request with one plumbing job: (request, job, job number).
async fn request(db: &mut Db, contact: u128) -> (Uuid, Uuid, i64) {
    let msg = text(contact, "my kitchen sink is leaking under the cabinet", 0);
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let id = handled.conversation.expect("request");
    let start = JobStart {
        conversation: id,
        words: msg.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    (id, job, job_number(db, job).await.expect("number"))
}

async fn show(db: &mut Db, (id, job): (Uuid, Uuid), card: &JobCard, minute: i64) -> Shown {
    let p = Preview {
        conversation: id,
        job,
        card,
        link: LINK,
        wait: AUTO_SEND_AFTER,
        at: at(minute),
    };
    show_card(db, &secret(), &p).await.expect("show")
}

async fn reply(db: &mut Db, (contact, word): (u128, &str), ids: (Uuid, Uuid)) -> CardStep {
    let msg = text(contact, word, 3);
    answer_card(db, &secret(), &msg, ids).await.expect("answer")
}

async fn body(db: &mut Db, outbox: Uuid) -> String {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(outbox)
        .fetch_one(&mut **db)
        .await
        .expect("body")
}

async fn approvals(db: &mut Db, job: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT payload->>'card' FROM core.task WHERE kind = $1 AND payload->>'job' = $2",
    )
    .bind(CARDS_APPROVED_KIND)
    .bind(job.to_string())
    .fetch_all(&mut **db)
    .await
    .expect("tasks")
}

fn timer(id: Uuid, job: Uuid, card: &JobCard, shown_at: i64) -> CardTimer {
    CardTimer {
        job,
        conversation: id,
        card: card_digest(card.text()),
        due: at(shown_at) + AUTO_SEND_AFTER,
    }
}

/// TD-INT-23, TD-AI-17 (INT-041, CARD-007): a short card is texted in full with no contact detail
/// and no "Reply OK"; SEND, OK or YES approves it at once; the approved card equals the shown card
/// character for character, and a card she wasn't shown never passes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_23_send_ok_or_yes_sends_the_card_she_was_shown(mut db: Db) {
    for (contact, word) in [(0x8101_u128, "SEND"), (0x8102, "ok"), (0x8103, "Yes")] {
        let (id, job, number) = request(&mut db, contact).await;
        let shown = card("Kitchen sink drips under the cabinet when the tap runs.");
        let Shown::Texted(sent) = show(&mut db, (id, job), &shown, 2).await else {
            panic!("texted");
        };
        assert_eq!(sent.template, CARD_SHOWN_TEMPLATE);
        let got = body(&mut db, sent.outbox_id).await;
        let head = format!("Ask Hava: Here's request {number} as businesses will see it: ");
        assert_eq!(got, format!("{head}{} {TAIL}", shown.text()));
        assert!(!got.to_lowercase().contains("reply ok") && !got.contains('@'));
        assert!(!got.contains("555"), "no phone");
        let mutant = card("Kitchen sink drips under the cabinet when the tap runs!");
        let step = reply(&mut db, (contact, word), (id, job)).await;
        assert_eq!(step, CardStep::Sent, "{word}");
        assert_eq!(approvals(&mut db, job).await, [card_digest(shown.text())]);
        assert!(
            approved_card(&mut db, (id, job), &shown)
                .await
                .expect("check")
        );
        assert!(
            !approved_card(&mut db, (id, job), &mutant)
                .await
                .expect("check")
        );
    }
}

/// TD-INT-23 (INT-041): a card too long for 2 segments sends the link text; no reply in 30
/// minutes sends the card as shown, and not a minute before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_23_a_long_card_links_and_goes_after_30_minutes(mut db: Db) {
    let (id, job, number) = request(&mut db, 0x8201).await;
    let long =
        card(&"The water heater leaks at the base and the pilot keeps going out. ".repeat(3));
    let Shown::Texted(sent) = show(&mut db, (id, job), &long, 2).await else {
        panic!("texted");
    };
    assert_eq!(sent.template, CARD_LINK_TEMPLATE);
    let want = format!(
        "Ask Hava: Request {number} is ready. Take a look before it goes out: {LINK} {TAIL}"
    );
    assert_eq!(body(&mut db, sent.outbox_id).await, want);
    let t = timer(id, job, &long, 2);
    let early = card_timer_step(&mut db, &t, at(31)).await.expect("early");
    assert!(!early && approvals(&mut db, job).await.is_empty());
    assert!(card_timer_step(&mut db, &t, at(32)).await.expect("due"));
    assert!(!card_timer_step(&mut db, &t, at(33)).await.expect("again"));
    assert_eq!(approvals(&mut db, job).await.len(), 1);
    assert!(
        approved_card(&mut db, (id, job), &long)
            .await
            .expect("check")
    );
}

/// TD-INT-23 (INT-041): EDIT asks what to change; the old card's timer then sends nothing; the
/// new card is shown and is the one that goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_23_edit_asks_and_shows_the_new_card(mut db: Db) {
    let (id, job, number) = request(&mut db, 0x8301).await;
    let first = card("Kitchen sink drips under the cabinet when the tap runs.");
    show(&mut db, (id, job), &first, 2).await;
    let CardStep::Edit(asked) = reply(&mut db, (0x8301, "EDIT"), (id, job)).await else {
        panic!("asks what to change");
    };
    assert_eq!(asked.template, CARD_EDIT_TEMPLATE);
    assert!(
        body(&mut db, asked.outbox_id)
            .await
            .contains(&number.to_string())
    );
    let old = timer(id, job, &first, 2);
    assert!(!card_timer_step(&mut db, &old, at(40)).await.expect("old"));
    let second = card("Bathroom sink drips under the cabinet when the tap runs.");
    let Shown::Texted(sent) = show(&mut db, (id, job), &second, 5).await else {
        panic!("the new card");
    };
    assert!(body(&mut db, sent.outbox_id).await.contains(second.text()));
    assert!(
        card_timer_step(&mut db, &timer(id, job, &second, 5), at(35))
            .await
            .expect("new")
    );
    assert!(
        approved_card(&mut db, (id, job), &second)
            .await
            .expect("check")
    );
    assert!(
        !approved_card(&mut db, (id, job), &first)
            .await
            .expect("check")
    );
    assert_eq!(
        show(&mut db, (id, job), &first, 36).await,
        Shown::AlreadySent
    );
}

/// TD-INT-23 (INT-041): a job chosen as an emergency skips the step: no text, no timer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_23_an_emergency_job_skips_the_step(mut db: Db) {
    let (id, job, _) = request(&mut db, 0x8401).await;
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, 'emergency_choice', 'emergency', now())",
    )
    .bind(id)
    .execute(&mut *db)
    .await
    .expect("choice");
    let shown = card("Kitchen sink drips under the cabinet when the tap runs.");
    assert_eq!(show(&mut db, (id, job), &shown, 2).await, Shown::Emergency);
    let none = reply(&mut db, (0x8401, "SEND"), (id, job)).await;
    assert_eq!(none, CardStep::Nothing);
    assert!(approvals(&mut db, job).await.is_empty());
}
