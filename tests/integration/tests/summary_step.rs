//! The card approval step in the database (TD-INT-23: INT-041, CARD-007; TD-AI-17). Fake IDs, a
//! fake card and fake text only.
//!
//! The homeowner is texted the card (or the link) with no contact detail; SEND, OK or YES approves
//! it at once; EDIT asks what to change and the new card is shown; no reply in 30 minutes approves
//! the card as shown; an emergency job skips the step; and only the card she was shown passes the
//! approved-card check the card sender runs. The wait starts once the text is seen sent; each
//! show is its own text and timer; SEND is audited as the homeowner, the auto-send as the system.

use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    APPROVED_ACTION, AUTO_SEND_AFTER, AUTO_SENT_ACTION, CARD_EDIT_TEMPLATE, CARD_LINK_TEMPLATE,
    CARD_SHOWN_TEMPLATE, CARD_TIMER_KIND, CARDS_APPROVED_KIND, CardStep, CardTimer, JobStart,
    Opened, Preview, RECHECK_AFTER, Shown, TextIn, answer_card, approved_card, begin_job,
    card_digest, card_timer_step, receive_text, show_card,
};
use askhava_jobs::api::job_number;
use askhava_quotes::api::{CardParts, DEFAULT_MIN_WORDS, JobCard, Place, Timing, build_card};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
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

/// The latest show's card timer task due last, as the worker would read it.
async fn latest_timer(db: &mut Db, job: Uuid) -> CardTimer {
    let p: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         ORDER BY (payload->>'show')::bigint DESC, run_at DESC LIMIT 1",
    )
    .bind(CARD_TIMER_KIND)
    .bind(job.to_string())
    .fetch_one(&mut **db)
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

/// Marks outbox row `id` sent, as `messaging::api::send` does (fake SID).
async fn mark_sent(db: &mut Db, id: Uuid) {
    sqlx::query(
        "UPDATE messaging.outbox SET status = 'sent', sent_at = now(), body = NULL, \
         provider_sid = 'SM' || repeat('a', 32) WHERE id = $1",
    )
    .bind(id)
    .execute(&mut **db)
    .await
    .expect("sent");
}

/// Runs the job's latest timer at its due time; returns whether it approved.
async fn run_latest(db: &mut Db, job: Uuid) -> (CardTimer, bool) {
    let t = latest_timer(db, job).await;
    let approved = card_timer_step(db, &t, t.due).await.expect("step");
    (t, approved)
}

/// The audit rows of the request's card approvals: (actor kind, action).
async fn audits(db: &mut Db, id: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT actor_kind, action FROM ops.audit_log WHERE subject_id = $1 \
         AND action LIKE 'intake.card_%' ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut **db)
    .await
    .expect("audit")
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
        let by_her = [("homeowner".to_owned(), APPROVED_ACTION.to_owned())];
        assert_eq!(audits(&mut db, id).await, by_her, "{word} is her approval");
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
/// minutes sends the card as shown, and not a minute before. The 30 minutes start once the text is
/// seen sent: while it is held (quiet hours) the timer only looks again (D-1278).
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
    let first = latest_timer(&mut db, job).await;
    assert!(first.watch && first.due == at(2) && first.outbox == sent.outbox_id);
    for held in 0..3 {
        let (t, approved) = run_latest(&mut db, job).await;
        assert!(
            !approved && t.watch,
            "held {held}: no approval while unsent"
        );
    }
    let held = latest_timer(&mut db, job).await;
    assert_eq!(
        held.due,
        at(2) + RECHECK_AFTER * 3,
        "it looks again each minute"
    );
    mark_sent(&mut db, sent.outbox_id).await;
    let (seen, approved) = run_latest(&mut db, job).await;
    assert!(!approved && seen.watch);
    let wait = latest_timer(&mut db, job).await;
    assert!(!wait.watch && wait.due == seen.due + AUTO_SEND_AFTER);
    let early = wait.due - SignedDuration::from_mins(1);
    assert!(!card_timer_step(&mut db, &wait, early).await.expect("early"));
    assert!(approvals(&mut db, job).await.is_empty());
    assert!(
        card_timer_step(&mut db, &wait, wait.due)
            .await
            .expect("due")
    );
    assert!(
        !card_timer_step(&mut db, &wait, wait.due)
            .await
            .expect("again")
    );
    assert_eq!(approvals(&mut db, job).await.len(), 1);
    let by_timer = [("system".to_owned(), AUTO_SENT_ACTION.to_owned())];
    assert_eq!(audits(&mut db, id).await, by_timer);
    assert!(
        approved_card(&mut db, (id, job), &long)
            .await
            .expect("check")
    );
}

/// Runs the latest show's timers to the end of the wait with its text sent.
async fn run_out(db: &mut Db, job: Uuid) -> bool {
    let watching = latest_timer(db, job).await;
    mark_sent(db, watching.outbox).await;
    run_latest(db, job).await;
    run_latest(db, job).await.1
}

/// TD-INT-23 (INT-041): EDIT asks what to change; the old card's timer then sends nothing; the
/// new card is shown and is the one that goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_23_edit_asks_and_shows_the_new_card(mut db: Db) {
    let (id, job, number) = request(&mut db, 0x8301).await;
    let first = card("Kitchen sink drips under the cabinet when the tap runs.");
    show(&mut db, (id, job), &first, 2).await;
    let old = latest_timer(&mut db, job).await;
    let CardStep::Edit(asked) = reply(&mut db, (0x8301, "EDIT"), (id, job)).await else {
        panic!("asks what to change");
    };
    assert_eq!(asked.template, CARD_EDIT_TEMPLATE);
    assert!(
        body(&mut db, asked.outbox_id)
            .await
            .contains(&number.to_string())
    );
    let old_wait = CardTimer {
        watch: false,
        due: old.due + AUTO_SEND_AFTER,
        ..old.clone()
    };
    assert!(
        !card_timer_step(&mut db, &old_wait, at(40))
            .await
            .expect("old")
    );
    let second = card("Bathroom sink drips under the cabinet when the tap runs.");
    let Shown::Texted(sent) = show(&mut db, (id, job), &second, 5).await else {
        panic!("the new card");
    };
    assert!(body(&mut db, sent.outbox_id).await.contains(second.text()));
    assert!(run_out(&mut db, job).await, "the new card goes");
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

/// TD-INT-23 (INT-041): after EDIT the same card text shown again is a new text she gets, and
/// the first show's timer, due earlier, never approves it early: only the new show's timer does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_23_the_same_card_shown_again_is_a_new_text_and_wait(mut db: Db) {
    let (id, job, _) = request(&mut db, 0x8501).await;
    let same = card("Kitchen sink drips under the cabinet when the tap runs.");
    let Shown::Texted(one) = show(&mut db, (id, job), &same, 2).await else {
        panic!("texted");
    };
    mark_sent(&mut db, one.outbox_id).await;
    run_latest(&mut db, job).await;
    let old_wait = latest_timer(&mut db, job).await;
    assert!(!old_wait.watch);
    assert!(matches!(
        reply(&mut db, (0x8501, "EDIT"), (id, job)).await,
        CardStep::Edit(_)
    ));
    let Shown::Texted(two) = show(&mut db, (id, job), &same, 20).await else {
        panic!("texted again");
    };
    assert_ne!(one.outbox_id, two.outbox_id, "a new text, not deduped");
    assert!(body(&mut db, two.outbox_id).await.contains(same.text()));
    let early = card_timer_step(&mut db, &old_wait, old_wait.due).await;
    assert!(!early.expect("old"), "the first show's timer does nothing");
    assert!(approvals(&mut db, job).await.is_empty());
    assert!(
        run_out(&mut db, job).await,
        "the second show's timer approves"
    );
    assert!(
        approved_card(&mut db, (id, job), &same)
            .await
            .expect("check")
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
