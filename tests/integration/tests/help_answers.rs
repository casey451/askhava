//! The plain-word auto-answers after HELP through the route's entry, `receive_raw_reply` (T482;
//! TD-CON-17, TD-CON-22; CON-034, CON-018, CON-035, OPS-036, WORD-009, INT-002, D-3753, D-3844,
//! D-3882, D-4293, D-4344). Fake IDs, fake keys, 555 numbers and fake text only.
//!
//! Each question in English and Spanish gets its answer ending with the "reply with what's wrong"
//! line, with no help request, no notification and one digest audit row; any other message, and
//! one within 24 hours after an answer, opens a help request with one notification; commands and
//! a safety phrase keep their paths; the cost answer's amounts equal `payments`' tiers; and the
//! four non-text opt-out channels list the number at `messaging::api::suppress` with no text.

use askhava_core::api::brand;
use askhava_intake::api::rules::{Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    Closing, JobStart, Opened, RawReply, Reply, Screened, TextIn, begin_job, note_help,
    receive_raw_reply, receive_text,
};
use askhava_jobs::api::job_number;
use askhava_messaging::api::{
    AUTO_ANSWERED_ACTION, CLOSE_ANSWER, COST_ANSWER, HELP_NOTICE_KIND, Language, PERSON_ANSWER,
    QUOTES_ANSWER, RevokeChannel, render, suppress,
};
use askhava_ops::api::HelpOpened;
use askhava_payments::api::Tier;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const SPANISH_ON: Settings = Settings {
    spanish_conversations_on: true,
    safety_call_on: false,
};

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x48), &key(0x49), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: u128, words: &str, when: Timestamp) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(contact << 64 | u128::from(when.as_second().unsigned_abs())),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: when,
    }
}

/// A fake request with one plumbing job, then HELP at minute 5; the job and its number.
async fn helped(db: &mut Db, contact: u128) -> (Uuid, i64) {
    let msg = text(
        contact,
        "my kitchen sink is leaking under the cabinet",
        at(0),
    );
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let start = JobStart {
        conversation: handled.conversation.expect("request"),
        words: "my kitchen sink is leaking under the cabinet",
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    note_help(db, Uuid::from_u128(contact), at(5))
        .await
        .expect("HELP");
    (job, job_number(db, job).await.expect("number"))
}

async fn reply(db: &mut Db, msg: &TextIn<'_>, settings: Settings) -> Reply {
    let raw = SecretString::from(msg.inbound.words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (msg, keys), (settings, &[]))
        .await
        .expect("reply")
}

async fn count(db: &mut Db, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut **db)
        .await
        .expect(sql)
}

async fn notices(db: &mut Db) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(HELP_NOTICE_KIND)
        .fetch_one(&mut **db)
        .await
        .expect("notices")
}

/// The queued row's template and body.
async fn queued(db: &mut Db, outbox: Uuid) -> (String, String) {
    sqlx::query_as("SELECT template_id, body FROM messaging.outbox WHERE id = $1")
        .bind(outbox)
        .fetch_one(&mut **db)
        .await
        .expect("row")
}

/// Moves the auto-answer's `created_at` to the simulated time it was sent.
async fn sent_at(db: &mut Db, outbox: Uuid, when: Timestamp) {
    sqlx::query("UPDATE messaging.outbox SET created_at = $2 WHERE id = $1")
        .bind(outbox)
        .bind(jiff_sqlx::Timestamp::from(when))
        .execute(&mut **db)
        .await
        .expect("backdate");
}

fn fix_line(language: Language) -> String {
    match language {
        Language::En => format!(
            "If that doesn't fix it, reply with what's wrong and the {} owner gets it right away.",
            brand().display_name()
        ),
        Language::Es => format!(
            "Si eso no lo resuelve, responda con lo que pasa y el dueño de {} lo atiende de inmediato.",
            brand().es().display_name()
        ),
    }
}

/// The asked question's answer: queued under `template`, no help request, no notification, one
/// digest row; its body.
async fn answered(db: &mut Db, contact: u128, words: &str, template: &str) -> (String, i64) {
    let (_, number) = helped(db, contact).await;
    let msg = text(contact, words, at(10));
    let before = notices(db).await;
    let Reply::HelpAnswered(sent) = reply(db, &msg, SPANISH_ON).await else {
        panic!("{words} is auto-answered");
    };
    let (id, body) = queued(db, sent.outbox_id).await;
    assert_eq!(id, template, "{words}");
    assert_eq!(notices(db).await, before, "{words}: no notification");
    let asker = Uuid::from_u128(contact);
    let requests = "SELECT count(*) FROM ops.help_request WHERE asker_id = $1";
    assert_eq!(
        count(db, requests, asker).await,
        0,
        "{words}: no help request"
    );
    let digest = "SELECT count(*) FROM ops.audit_log WHERE action = 'help.auto_answered' \
                  AND subject_id = $1";
    assert_eq!(
        count(db, digest, msg.inbox_id).await,
        1,
        "{words}: digest line"
    );
    (body, number)
}

/// TD-CON-17 (CON-034, D-3882): "where are my quotes" and "cotizaciones?" get the status, the
/// LIST line and the quote-page link, ending with the fix line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_where_are_my_quotes_in_english_and_spanish(mut db: Db) {
    let cases = [
        (0xb001, "where are my quotes", Language::En, "Reply LIST"),
        (0xb002, "cotizaciones?", Language::Es, "Responda LISTA"),
    ];
    for (contact, words, language, list) in cases {
        let (body, number) = answered(&mut db, contact, words, QUOTES_ANSWER).await;
        let link = format!("https://{}/q/", brand().domain());
        assert!(body.contains(&number.to_string()), "{body}");
        assert!(body.contains(list) && body.contains(&link), "{body}");
        assert!(body.ends_with(&fix_line(language)), "{body}");
    }
}

/// TD-CON-17 (CON-034): "how do I close this" and "ya no la necesito" get the CLOSE line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_how_to_close_in_english_and_spanish(mut db: Db) {
    let cases = [
        (0xb101, "how do I close this", Language::En),
        (0xb102, "ya no la necesito", Language::Es),
    ];
    for (contact, words, language) in cases {
        let (body, number) = answered(&mut db, contact, words, CLOSE_ANSWER).await;
        let n = number.to_string();
        let expected = render(CLOSE_ANSWER, language, true, &[("job", &n)]).expect("close");
        assert_eq!(body, expected.body, "{words}");
        assert!(body.ends_with(&fix_line(language)), "{body}");
    }
}

/// TD-CON-17, TD-CON-22 (CON-034, D-3844): "does this cost anything" and "¿cuánto cobran?" get
/// the free-for-you line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_22_does_this_cost_anything_in_english_and_spanish(mut db: Db) {
    let cases = [
        (0xb201, "does this cost anything", Language::En),
        (0xb202, "¿cuánto cobran?", Language::Es),
    ];
    for (contact, words, language) in cases {
        let (body, _) = answered(&mut db, contact, words, COST_ANSWER).await;
        let expected = render(COST_ANSWER, language, true, &[]).expect("cost");
        assert_eq!(body, expected.body, "{words}");
        assert!(body.ends_with(&fix_line(language)), "{body}");
    }
}

/// TD-CON-17 (CON-034, WORD-009): "is this a real person" and "¿es una persona?" get the WORD-009
/// answer, never a claim to be a person.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_is_this_a_real_person_in_english_and_spanish(mut db: Db) {
    let cases = [
        (0xb301, "is this a real person", Language::En),
        (0xb302, "¿es una persona?", Language::Es),
    ];
    for (contact, words, language) in cases {
        let (body, _) = answered(&mut db, contact, words, PERSON_ANSWER).await;
        let line = match language {
            Language::En => format!(
                "{} is a local service. Our texts are automated.",
                brand().display_name()
            ),
            Language::Es => format!(
                "{} es un servicio local. Nuestros textos son automatizados.",
                brand().es().display_name()
            ),
        };
        assert!(body.contains(&line), "{body}");
        assert!(body.ends_with(&fix_line(language)), "{body}");
    }
}

/// The amounts typed in `body`, in cents ("$25" is 2,500).
fn amounts(body: &str) -> Vec<u64> {
    body.split('$')
        .skip(1)
        .map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .map(|d| d.parse::<u64>().expect("amount") * 100)
        .collect()
}

/// TD-CON-22 (CON-034, D-3844): the cost answer's amounts, in both languages, equal the fee
/// tiers `payments` charges, so a fee change fails here.
#[test]
fn td_con_22_cost_answer_amounts_equal_the_fee_tiers() {
    let tiers = [Tier::Small, Tier::Medium, Tier::Large].map(Tier::cents);
    for language in [Language::En, Language::Es] {
        let body = render(COST_ANSWER, language, true, &[]).expect("cost").body;
        assert_eq!(amounts(&body), tiers, "{language:?}");
    }
}

/// TD-CON-17 (CON-034, OPS-036): a message that is not a plain-word question opens a help
/// request with one notification, and no auto-answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_any_other_message_opens_a_help_request(mut db: Db) {
    let contact = 0xb401;
    helped(&mut db, contact).await;
    let words = "my quote page says expired but the plumber said he quoted";
    let reply = reply(&mut db, &text(contact, words, at(10)), SPANISH_ON).await;
    assert!(matches!(
        reply,
        Reply::HelpRequest(HelpOpened::Opened { notify: true, .. })
    ));
    assert_eq!(notices(&mut db).await, 1);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE action = $1")
        .bind(AUTO_ANSWERED_ACTION)
        .fetch_one(&mut *db)
        .await
        .expect("digest");
    assert_eq!(rows, 0, "no digest line for a help request");
}

/// TD-CON-17 (CON-034): a message 23:59 after an auto-answer opens a help request with a
/// notification, even a plain-word question.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_a_message_23_59_after_an_answer_notifies(mut db: Db) {
    let contact = 0xb501;
    helped(&mut db, contact).await;
    let first = text(contact, "does this cost anything", at(10));
    let Reply::HelpAnswered(sent) = reply(&mut db, &first, SPANISH_ON).await else {
        panic!("answered");
    };
    sent_at(&mut db, sent.outbox_id, at(10)).await;
    let later = at(10) + SignedDuration::from_mins(23 * 60 + 59);
    let reply = reply(
        &mut db,
        &text(contact, "where are my quotes", later),
        SPANISH_ON,
    )
    .await;
    assert!(matches!(reply, Reply::HelpRequest(_)), "{reply:?}");
    assert_eq!(notices(&mut db).await, 1);
}

/// TD-CON-17 (CON-034): at 24:00:01 after an auto-answer, a message after a new HELP is handled
/// as a HELP follow-up (its question answered again); with no new HELP it takes today's path.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_a_message_24_00_01_after_an_answer_is_a_new_follow_up(mut db: Db) {
    for (contact, help_again) in [(0xb601_u128, true), (0xb602, false)] {
        helped(&mut db, contact).await;
        let first = text(contact, "does this cost anything", at(10));
        let Reply::HelpAnswered(sent) = reply(&mut db, &first, SPANISH_ON).await else {
            panic!("answered");
        };
        sent_at(&mut db, sent.outbox_id, at(10)).await;
        let day = at(10) + SignedDuration::from_hours(24);
        if help_again {
            note_help(&mut db, Uuid::from_u128(contact), day)
                .await
                .expect("HELP");
        }
        let later = day + SignedDuration::from_secs(1);
        let msg = text(contact, "is this a real person", later);
        let reply = reply(&mut db, &msg, SPANISH_ON).await;
        if help_again {
            let Reply::HelpAnswered(again) = reply else {
                panic!("answered again, got {reply:?}");
            };
            assert_eq!(queued(&mut db, again.outbox_id).await.0, PERSON_ANSWER);
        } else {
            assert!(matches!(reply, Reply::Handled(_)), "{reply:?}");
        }
    }
    assert_eq!(notices(&mut db).await, 0);
}

/// TD-CON-17 (D-4293): within 24 hours of HELP, "CLOSE <number>" still closes and "LIST" keeps
/// its path; neither is answered nor opens a help request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_commands_keep_their_paths_after_help(mut db: Db) {
    let (_, number) = helped(&mut db, 0xb701).await;
    let close = format!("CLOSE {number}");
    let reply_close = reply(&mut db, &text(0xb701, &close, at(10)), SPANISH_ON).await;
    assert!(
        matches!(&reply_close, Reply::Closed(Closing::Closed(n), _) if n == &vec![number]),
        "{reply_close:?}"
    );
    helped(&mut db, 0xb702).await;
    let list = reply(&mut db, &text(0xb702, "LIST", at(10)), SPANISH_ON).await;
    assert!(
        !matches!(list, Reply::HelpAnswered(_) | Reply::HelpRequest(_)),
        "{list:?}"
    );
    assert_eq!(notices(&mut db).await, 0);
}

/// INT-038 (D-4142, D-4346): "I'm 15" within 24 hours of HELP is stopped for a
/// stated age under 18, never taken as a help follow-up.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn int_038_a_stated_age_under_18_after_help_still_stops(mut db: Db) {
    helped(&mut db, 0xb901).await;
    let msg = text(0xb901, "I'm 15", at(10));
    let reply = reply(&mut db, &msg, SPANISH_ON).await;
    assert!(matches!(reply, Reply::UnderAge(..)), "{reply:?}");
    assert_eq!(notices(&mut db).await, 0);
}

/// INT-027 (D-4346): with her name-and-address question open, "Zelda Canary 12 Test Street" within 24
/// hours of HELP is her answer, never a help follow-up.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn int_027_a_name_and_address_answer_after_help_is_taken(mut db: Db) {
    let new = NewContact {
        phone: Some(SecretString::from("(928) 555-0181".to_owned())),
        ..NewContact::default()
    };
    let who = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let (job, _) = helped(&mut db, who.as_u128()).await;
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         SELECT id, 'name_address_asked', 'asked', to_timestamp($2) \
         FROM intake.conversation WHERE job_id = $1",
    )
    .bind(job)
    .bind(at(6).as_second() as f64)
    .execute(&mut *db)
    .await
    .expect("ask");
    let msg = text(who.as_u128(), "Zelda Canary 12 Test Street", at(10));
    let reply = reply(&mut db, &msg, SPANISH_ON).await;
    assert!(matches!(reply, Reply::NameAddress(..)), "{reply:?}");
    assert_eq!(notices(&mut db).await, 0);
}

/// JOB-021 (D-4293, D-4346): a lone number answering a waiting "Close which?" within 24 hours
/// of HELP closes that job, never a help follow-up.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn job_021_a_close_which_answer_after_help_closes(mut db: Db) {
    let contact = 0xbb01;
    let (job, number) = helped(&mut db, contact).await;
    let start = JobStart {
        conversation: conversation_of(&mut db, job).await,
        words: "the AC is warm too",
        emergency: false,
        trade: "hvac",
        at: at(2),
    };
    begin_job(&mut db, &start).await.expect("second job");
    let which = reply(&mut db, &text(contact, "CLOSE", at(7)), SPANISH_ON).await;
    assert!(
        matches!(which, Reply::Closed(Closing::Which(_), _)),
        "{which:?}"
    );
    let answer = number.to_string();
    let reply = reply(&mut db, &text(contact, &answer, at(10)), SPANISH_ON).await;
    assert!(
        matches!(&reply, Reply::Closed(Closing::Closed(n), _) if n == &vec![number]),
        "{reply:?}"
    );
    assert_eq!(notices(&mut db).await, 0);
}

async fn conversation_of(db: &mut Db, job: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM intake.conversation WHERE job_id = $1")
        .bind(job)
        .fetch_one(&mut **db)
        .await
        .expect("request")
}

/// TD-CON-17 (INT-002, CON-034): "smell gas" within 24 hours of HELP gets the safety text first,
/// with no auto-answer and no help request.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_smell_gas_after_help_gets_the_safety_text_first(mut db: Db) {
    helped(&mut db, 0xb801).await;
    let reply = reply(&mut db, &text(0xb801, "I smell gas", at(10)), SPANISH_ON).await;
    let Reply::Handled(handled) = reply else {
        panic!("today's path, got {reply:?}");
    };
    assert!(matches!(handled.screened, Screened::Matched(_)));
    assert_eq!(
        handled.texts.first().map(|q| q.template),
        Some(SAFETY_TEMPLATE)
    );
    assert_eq!(notices(&mut db).await, 0);
}

/// TD-CON-17 (CON-035, D-3155, D-3882): a help email, a quote-email reply, the help form's
/// "no more texts" and an owner-recorded request each list the number at once at
/// `messaging::api::suppress`, log the channel and time, and queue no text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_17_non_text_opt_outs_list_at_once_with_no_text(mut db: Db) {
    let cases = [
        (RevokeChannel::Email, "email", "+19285550181"),
        (RevokeChannel::EmailReply, "email_reply", "+19285550182"),
        (RevokeChannel::HelpForm, "help_form", "+19285550183"),
        (RevokeChannel::Owner, "owner", "+19285550184"),
    ];
    let vault = vault();
    for (channel, name, phone) in cases {
        let listed = suppress(&mut db, &vault, phone, (channel, "test"), at(20))
            .await
            .expect("suppress");
        assert!(listed, "{name}: newly listed");
        let (logged, when): (String, jiff_sqlx::Timestamp) = sqlx::query_as(
            "SELECT e.channel, s.since FROM messaging.suppression s \
             JOIN messaging.optout_event e ON e.id = s.event_id WHERE e.channel = $1",
        )
        .bind(name)
        .fetch_one(&mut *db)
        .await
        .expect("listed and logged");
        assert_eq!((logged.as_str(), when.to_jiff()), (name, at(20)));
    }
    let texts: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(&mut *db)
        .await
        .expect("outbox");
    assert_eq!(texts, 0, "no text confirms a non-text opt-out");
}
