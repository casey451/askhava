//! INT-043's one intake order on the text paths, and TXT-041's Spanish line (TD-INT-26,
//! TD-TXT-25's TXT-041 case; INT-043, TXT-041, INT-041, INT-029, PRIV-008, CARD-007; D-446,
//! D-491, D-2330, D-3189).
//!
//! The golden tables below are the "Intake order" table of `docs/spec/intake.md`, written out by
//! hand: a test fails if any two steps swap. The database cases run `intake::api` on a fresh
//! database with fake IDs, a fake 555 number and made-up words only.

use askhava_catalog::api::load_embedded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    AUTO_SEND_AFTER, Answer, CardIn, JobStart, Opened, Path, Recorded, SPANISH_NOT_YET_TEMPLATE,
    Shown, Step, TextIn, ZipStep, answer_narrowing, answer_zip, ask_zip, begin_job, card_digest,
    choose_with_card, keep_words, order, receive_text, show_built_card, spanish_line,
};
use askhava_integration::set_app_role;
use askhava_jobs::api::{job_facts, record_job_facts};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{CardParts, Place, description, stored_card, unsent_cards};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use Step::{
    CardShown, CardsOut, Choice, Design, FirstReply, FirstText, HowSoon, NameAddress, NightCount,
    Photos, Safe, SafetyCheck, SafetyText, TradeCheck, TradeQuestions, Zip,
};

const HEAD: [Step; 6] = [FirstText, SafetyCheck, FirstReply, Zip, TradeCheck, HowSoon];
const TAIL: [Step; 6] = [
    TradeQuestions,
    Photos,
    Design,
    NameAddress,
    CardShown,
    CardsOut,
];
const EMERGENCY_TAIL: [Step; 5] = [CardsOut, TradeQuestions, Photos, Design, NameAddress];
const LIST_HEAD: [Step; 7] = [
    FirstText, SafetyText, FirstReply, Safe, Zip, TradeCheck, HowSoon,
];
const DAY: &str = "2026-11-02T17:00:00Z";
const WORDS: &str = "the air conditioner blows warm air and the upstairs is hot";

fn golden(parts: &[&[Step]]) -> Vec<Step> {
    parts.concat()
}

/// TD-INT-26 (INT-043, D-446, D-460): each text path in the table's order with its cited rows.
#[test]
fn td_int_26_each_text_path_runs_the_table_order() {
    let regular = golden(&[&HEAD, &TAIL]);
    assert_eq!(order(Path::Regular, false), regular);
    assert_eq!(order(Path::Regular, true), regular, "no choice is offered");
    let urgent = golden(&[&HEAD, &[Choice], &TAIL]);
    assert_eq!(order(Path::Urgent, false), urgent);
    let urgent_now = golden(&[&HEAD, &[Choice], &EMERGENCY_TAIL]);
    assert_eq!(order(Path::Urgent, true), urgent_now);
    assert_eq!(
        order(Path::List911, false),
        golden(&[&LIST_HEAD, &[Choice], &TAIL])
    );
    let list_now = golden(&[&LIST_HEAD, &[Choice], &EMERGENCY_TAIL]);
    assert_eq!(order(Path::List911, true), list_now);
    let night = golden(&[&HEAD, &[NightCount, Choice], &EMERGENCY_TAIL]);
    assert_eq!(order(Path::Night(2), true), night);
    let none = golden(&[&HEAD, &[NightCount], &TAIL]);
    assert_eq!(order(Path::Night(0), true), none, "N = 0: no choice");
    let cited: Vec<&str> = order(Path::Urgent, false)
        .iter()
        .map(|s| s.cites())
        .collect();
    let table = [
        "CON-005", "INT-002", "CON-031", "INT-022", "INT-016", "INT-030", "INT-006", "INT-039",
        "INT-044", "INT-045", "INT-027", "INT-041", "TXT-032",
    ];
    assert_eq!(cited, table);
    let list = order(Path::List911, false);
    assert_eq!((list[1].cites(), list[3].cites()), ("INT-004", "INT-025"));
    for path in [Path::Regular, Path::Urgent, Path::List911, Path::Night(1)] {
        let steps = order(path, false);
        for i in 1..steps.len() {
            let mut swapped = steps.clone();
            swapped.swap(i - 1, i);
            assert_ne!(swapped, steps, "a swap of steps {i} is caught");
        }
    }
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-162".to_owned())
}

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn text(contact: Uuid, words: &str) -> TextIn<'_> {
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
        received_at: ts(DAY),
    }
}

async fn setup(pool: &PgPool) -> PgConnection {
    let mut db = pool.acquire().await.expect("conn").detach();
    load_embedded(&mut db).await.expect("catalog");
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    db
}

/// Steps 1 to 3 for a fake homeowner: (vault contact, request, first inbox ID).
async fn first_text(db: &mut PgConnection, (phone, words): (&str, &str)) -> (Uuid, Uuid, Uuid) {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x51), &key(0x61), &url).expect("vault");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault, &new).await.expect("contact");
    let first = text(her, words);
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let request = handled.expect("open").conversation.expect("request");
    (her, request, first.inbox_id)
}

/// Steps 4 and 5 of the regular path: the ZIP, then the job (the trade check), with her words kept.
async fn job(db: &mut PgConnection, (her, request, first): (Uuid, Uuid, Uuid)) -> Uuid {
    let asked = text(her, "");
    ask_zip(db, &secret(), (&asked, request), Settings::default())
        .await
        .expect("asked");
    let reply = text(her, "86403");
    let zip = answer_zip(db, &secret(), (&reply, request), Settings::default()).await;
    assert!(matches!(zip.expect("zip"), ZipStep::Stored(_)));
    let start = JobStart {
        conversation: request,
        words: WORDS,
        emergency: false,
        trade: "hvac_repair",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    let kept = keep_words(db, (request, job), first, (WORDS, ts(DAY))).await;
    assert_eq!(kept, Ok(true));
    job
}

fn card(request: Uuid, job: Uuid) -> CardIn {
    let parts = CardParts {
        category: "HVAC".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: String::new(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    CardIn {
        conversation: request,
        job,
        parts,
        category: "hvac_repair".to_owned(),
        candidates: vec!["ac_repair".to_owned(), "hvac_repair_other".to_owned()],
    }
}

async fn count(db: &mut PgConnection, her: Uuid, template: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1 AND template_id = $2",
    )
    .bind(her)
    .bind(template)
    .fetch_one(db)
    .await
    .expect("outbox")
}

/// TD-INT-26 (PRIV-008, INT-041, CARD-007, D-3189): her words are stored through
/// `keep_description`, and the card shown at step 12 is built from them and stored first.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_26_the_card_shown_is_built_from_her_stored_words(pool: PgPool) {
    let mut db = setup(&pool).await;
    let ids = first_text(&mut db, ("+19285550162", WORDS)).await;
    let job = job(&mut db, ids).await;
    let words = description(&mut db, job)
        .await
        .expect("read")
        .expect("kept");
    assert_eq!(words.text(), WORDS);
    let at = ("https://askhava.com/q/fake-token", AUTO_SEND_AFTER, ts(DAY));
    let shown = show_built_card(&mut db, &secret(), &card(ids.1, job), at).await;
    assert!(matches!(shown, Ok(Shown::Texted(_))));
    let stored = unsent_cards(&mut db, ids.1).await.expect("cards");
    assert_eq!(stored.len(), 1);
    assert!(stored[0].card.text().contains(WORDS) && !stored[0].facts.emergency);
    let digest = card_digest(stored[0].card.text());
    assert!(
        stored_card(&mut db, job, &digest)
            .await
            .expect("read")
            .is_some()
    );
    assert_eq!(count(&mut db, ids.0, "card_shown").await, 1);
}

/// TD-INT-26 (INT-029, D-2330, D-3189): the Emergency choice stores the marked card before the
/// release; a trade answer after the cards appends one narrowing row; a refused one changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_26_the_emergency_card_and_the_narrowing_after_the_cards(pool: PgPool) {
    let mut db = setup(&pool).await;
    let ids = first_text(&mut db, ("+19285550162", WORDS)).await;
    let job = job(&mut db, ids).await;
    let c = card(ids.1, job);
    let answered = choose_with_card(&mut db, &text(ids.0, "EMERGENCY"), &c).await;
    assert!(
        answered.expect("chosen").released,
        "released with the card stored"
    );
    let stored = unsent_cards(&mut db, ids.1).await.expect("cards");
    assert!(stored.len() == 1 && stored[0].facts.emergency);
    record_job_facts(&mut db, job, &c.category, &c.candidates)
        .await
        .expect("the card sender's first row");
    let reply = text(ids.0, "the ac one");
    let answer = Answer {
        conversation: ids.1,
        question: "hvac.which_unit",
        options: None,
        words: reply.inbound.words,
        reply: (reply.inbox_id, ts(DAY)),
    };
    let narrowed = ["ac_repair".to_owned()];
    let done = answer_narrowing(&mut db, &answer, (job, "hvac_repair", &narrowed)).await;
    assert!(matches!(done, Ok(Recorded::Stored(_))));
    let facts = job_facts(&mut db, job).await.expect("read").expect("row");
    assert_eq!((facts.seq, facts.job_types.clone()), (2, narrowed.to_vec()));
    let wider = c.candidates.as_slice();
    let refused = answer_narrowing(&mut db, &answer, (job, "hvac_repair", wider)).await;
    assert!(refused.is_ok(), "a refused row is logged, not an error");
    let after = job_facts(&mut db, job).await.expect("read").expect("row");
    assert_eq!(after.seq, 2, "nothing written");
}

/// TD-TXT-25 (TXT-041, D-446, D-491): a Spanish first message with Spanish conversations off gets
/// the one line once after the first reply; a later or English message gets none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_25_a_spanish_first_message_gets_the_line_once(pool: PgPool) {
    let mut db = setup(&pool).await;
    let words = "hola, necesito un plomero porque la tubería de la cocina gotea";
    let (her, request, first) = first_text(&mut db, ("+19285550163", words)).await;
    let mut msg = text(her, words);
    msg.inbox_id = first;
    let off = Settings::default();
    for _ in 0..2 {
        let line = spanish_line(&mut db, &secret(), (&msg, request), off).await;
        assert!(matches!(line, Ok(Some(_))));
    }
    assert_eq!(count(&mut db, her, SPANISH_NOT_YET_TEMPLATE).await, 1);
    let later = text(her, words);
    let none = spanish_line(&mut db, &secret(), (&later, request), off).await;
    assert!(matches!(none, Ok(None)), "only the first message");
    let on = Settings {
        spanish_conversations_on: true,
        ..off
    };
    let none = spanish_line(&mut db, &secret(), (&msg, request), on).await;
    assert!(
        matches!(none, Ok(None)),
        "Spanish on: the talk is in Spanish"
    );
    let (other, req2, first2) = first_text(&mut db, ("+19285550162", WORDS)).await;
    let mut english = text(other, WORDS);
    english.inbox_id = first2;
    let none = spanish_line(&mut db, &secret(), (&english, req2), off).await;
    assert!(matches!(none, Ok(None)));
}
