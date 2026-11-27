//! A request may be texted only with an `sms` consent record of its own, and the consent guard
//! (T856; CON-001, CON-005, CON-007, CON-021, INT-002, INT-004; D-5848, D-5918).
//!
//! Proves through `intake::api` and `portals::api` on a fresh database that a returning number's
//! second text-opened request gets its own `sms` record and is textable; that a web-opened request
//! is not textable until her first text, which gets the consent text and one `sms` record (two
//! racing first texts give one of each); that a `web_chat` record never makes `may_text` or
//! `may_text_request` true (the owner's help reply goes by email only); that a chat turn writes no
//! record and queues no consent text, also when her request closed under it; that the guard keeps
//! the chat exemption and refuses a request with no record; and that the INT-004 text still goes
//! before the consent text. Her first carrier text through the reply entry that a reply step answers
//! before the screen (a CLOSE, a "<n> YES", a LIST) also gets the consent text and one `sms`
//! record (D-5893). Fake numbers, IDs and words only.

use askhava_intake::api::rules::{Inbound, SAFETY_TEMPLATE, Settings, consent_template};
use askhava_intake::api::{
    ChatAnswer, ChatError, ChatIn, ChatSettings, Checked, CodeStep, Delivery, Handled, IntakeError,
    JobStart, Opened, RawReply, Reply, TextIn, TextKind, begin_job, chat_contact, chat_message,
    check_code, consent_records, may_text, may_text_request, receive_raw_reply, receive_reply,
    receive_text, request_text, send_code, start_chat,
};
use askhava_messaging::api::Language;
use askhava_ops::api::{Asker, HelpLimits, open_help_request};
use askhava_portals::api::{ReplyRefusal, send_owner_reply};
use askhava_privacy::api::Vault;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const RIGHT: &str = "246810";

/// The fake Verify step: the right code is [`RIGHT`].
struct FakeVerify;

impl CodeStep for FakeVerify {
    async fn send(&self, _number: &SecretString, _key: &str) -> Result<(), ChatError> {
        Ok(())
    }
    async fn check(&self, _number: &SecretString, code: &str) -> Result<bool, ChatError> {
        Ok(code == RIGHT)
    }
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x31), &key(0x32), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + jiff::SignedDuration::from_mins(minute)
}

fn key(n: u8) -> SecretString {
    SecretString::from(format!("fake-session-key-{n:02}"))
}

fn consent() -> &'static str {
    consent_template(Settings::default().safety_call_on)
}

/// A carrier text from `contact` with inbox ID `inbox` at `minute`.
fn text(contact: Uuid, inbox: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(inbox),
        contact,
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

async fn carrier(db: &mut PgConnection, msg: &TextIn<'_>) -> Handled {
    receive_text(db, &secret(), msg, Settings::default())
        .await
        .expect("text")
}

fn templates(handled: &Handled) -> Vec<&'static str> {
    handled.texts.iter().map(|q| q.template).collect()
}

/// The channels of request `id`'s consent records, oldest first.
async fn channels(db: &mut PgConnection, id: Uuid) -> Vec<(String, Option<Uuid>)> {
    consent_records(db, id)
        .await
        .expect("records")
        .into_iter()
        .map(|r| (r.channel, r.inbox_id))
        .collect()
}

/// Opens a web-chat request through the right code for fake number 928 555 01`k`; returns the
/// request and her vault contact.
async fn web_request(db: &mut PgConnection, k: u8) -> (Uuid, Uuid) {
    let (secret, vault) = (secret(), vault());
    start_chat(db, &key(k), None, at(0)).await.expect("start");
    let first = ChatIn {
        message_id: Uuid::from_u128(u128::from(k)),
        words: "my sink leaks",
        received_at: at(1),
    };
    chat_message(db, &secret, &key(k), &first, Settings::default())
        .await
        .expect("message");
    let number = SecretString::from(format!("928-555-01{k:02}"));
    send_code(db, (&vault, &FakeVerify), &key(k), &number, at(2))
        .await
        .expect("send");
    let s = ChatSettings {
        language: Language::En,
        settings: Settings::default(),
        ip: None,
    };
    let checked = check_code(
        db,
        (&secret, &vault, &FakeVerify),
        &key(k),
        (&number, RIGHT),
        (s, at(3)),
    )
    .await
    .expect("check");
    let Checked::Verified(id, _) = checked else {
        panic!("not verified");
    };
    let contact = chat_contact(db, &key(k))
        .await
        .expect("contact")
        .expect("set");
    (id, contact)
}

async fn chat(db: &mut PgConnection, k: u8, id: u128, words: &str) -> ChatAnswer {
    let msg = ChatIn {
        message_id: Uuid::from_u128(id),
        words,
        received_at: at(5),
    };
    chat_message(db, &secret(), &key(k), &msg, Settings::default())
        .await
        .expect("chat")
}

async fn close(db: &mut PgConnection, id: Uuid) {
    sqlx::query("UPDATE intake.conversation SET closed_at = $2 WHERE id = $1")
        .bind(id)
        .bind(jiff_sqlx::Timestamp::from(at(4)))
        .execute(db)
        .await
        .expect("close");
}

/// CON-005, CON-021: a returning number's second text-opened request gets its own `sms` record
/// from its first text and is textable.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_returning_numbers_second_text_request_is_textable(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let contact = Uuid::from_u128(0x0a01);
    let one = carrier(&mut db, &text(contact, 0x1001, "my sink leaks", 0)).await;
    let one = one.conversation.expect("first request");
    close(&mut db, one).await;
    let two = carrier(&mut db, &text(contact, 0x1002, "my fence fell over", 10)).await;
    assert_eq!(templates(&two), vec![consent()]);
    let two = two.conversation.expect("second request");
    assert_ne!(one, two);
    let sms = Some(Uuid::from_u128(0x1002));
    assert_eq!(channels(&mut db, two).await, vec![("sms".to_owned(), sms)]);
    assert!(may_text_request(&mut db, two).await.expect("two"));
    assert!(may_text_request(&mut db, one).await.expect("one"));
    assert!(
        !may_text_request(&mut db, Uuid::from_u128(0xdead))
            .await
            .expect("none")
    );
}

/// CON-005, CON-021, D-5848: a web-opened request has only its `web_chat` record, which never
/// makes `may_text` or `may_text_request` true; her first text gets the consent text and one
/// `sms` record under its own ID, and her next text neither.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_web_request_is_textable_only_after_her_first_text(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (id, contact) = web_request(&mut db, 1).await;
    assert_eq!(
        channels(&mut db, id).await,
        vec![("web_chat".to_owned(), None)]
    );
    assert!(!may_text_request(&mut db, id).await.expect("web"));
    assert!(!may_text(&mut db, contact).await.expect("web"));
    let first = carrier(
        &mut db,
        &text(contact, 0x2001, "it drips under the cabinet", 6),
    )
    .await;
    assert_eq!(first.conversation, Some(id));
    assert_eq!(templates(&first)[..1], [consent()]);
    let sms = ("sms".to_owned(), Some(Uuid::from_u128(0x2001)));
    assert_eq!(channels(&mut db, id).await[1..], [sms]);
    assert!(may_text_request(&mut db, id).await.expect("sms"));
    assert!(may_text(&mut db, contact).await.expect("sms"));
    let next = carrier(&mut db, &text(contact, 0x2002, "the kitchen one", 7)).await;
    assert!(!templates(&next).contains(&consent()));
    assert_eq!(channels(&mut db, id).await.len(), 2);
}

/// D-5848: two racing first texts into a web-opened request give one consent text and one `sms`
/// record (the conversation row lock).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn racing_first_texts_give_one_consent_text_and_one_record(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (id, contact) = web_request(&mut db, 2).await;
    let (mut a, mut b) = (
        pool.acquire().await.expect("a"),
        pool.acquire().await.expect("b"),
    );
    let (one, two) = (
        text(contact, 0x3001, "it drips", 6),
        text(contact, 0x3002, "under the sink", 6),
    );
    let (x, y) = tokio::join!(carrier(&mut a, &one), carrier(&mut b, &two));
    let sent = [x, y]
        .iter()
        .flat_map(templates)
        .filter(|t| *t == consent())
        .count();
    assert_eq!(sent, 1);
    let records = channels(&mut db, id).await;
    assert_eq!(records.iter().filter(|r| r.0 == "sms").count(), 1);
}

/// CON-005, R2-02, R2-03: a chat turn on a `web_chat`-only request queues no consent text and
/// writes no record, and a chat turn whose request closed under it (the stale read) writes no
/// `sms` record and opens nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_chat_turn_writes_no_consent_record(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (id, _) = web_request(&mut db, 3).await;
    let ChatAnswer::Texted(turn) = chat(&mut db, 3, 0x4001, "it drips under the cabinet").await
    else {
        panic!("not the text path");
    };
    assert!(!templates(&turn).contains(&consent()));
    assert_eq!(
        channels(&mut db, id).await,
        vec![("web_chat".to_owned(), None)]
    );
    close(&mut db, id).await;
    chat(&mut db, 3, 0x4002, "my fence fell over").await;
    let sms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM intake.consent_record WHERE channel = 'sms'")
            .fetch_one(&mut *db)
            .await
            .expect("sms");
    let opened: i64 = sqlx::query_scalar("SELECT count(*) FROM intake.conversation")
        .fetch_one(&mut *db)
        .await
        .expect("requests");
    assert_eq!((sms, opened), (0, 1));
}

/// D-5848: the guard keeps the chat exemption for a `web_chat`-only request, lets an `sms`
/// request text go, refuses a request with no record (fail closed), and lets the carrier
/// exemptions go.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_guard_refuses_a_request_with_no_record(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (web, _) = web_request(&mut db, 4).await;
    let guard = |r: Result<Delivery, IntakeError>| r.map_err(|e| e == IntakeError::NoConsent);
    let none = Some(Uuid::from_u128(0xbeef));
    let ask = request_text(&mut db, Some(web), TextKind::Request).await;
    assert_eq!(guard(ask), Ok(Delivery::Outbox));
    let ask = request_text(&mut db, none, TextKind::Request).await;
    assert_eq!(guard(ask), Err(true));
    let ask = request_text(&mut db, None, TextKind::Request).await;
    assert_eq!(guard(ask), Err(true));
    let ask = request_text(&mut db, none, TextKind::CarrierSafety).await;
    assert_eq!(guard(ask), Ok(Delivery::Outbox));
    for kind in [
        TextKind::CarrierConsent,
        TextKind::CarrierKeyword,
        TextKind::BusinessSignup,
    ] {
        assert_eq!(
            guard(request_text(&mut db, None, kind).await),
            Ok(Delivery::Outbox)
        );
    }
    let contact = Uuid::from_u128(0x0a05);
    let handled = carrier(&mut db, &text(contact, 0x5001, "my sink leaks", 0)).await;
    let sms = handled.conversation;
    assert_eq!(
        guard(request_text(&mut db, sms, TextKind::Request).await),
        Ok(Delivery::Outbox)
    );
}

/// INT-002, INT-004, CON-001: 911 before consent is unchanged for a new number, and her first
/// text into a web-opened request also gets the INT-004 text before the consent text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_911_text_still_goes_before_the_consent_text(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let new = carrier(
        &mut db,
        &text(Uuid::from_u128(0x0a06), 0x6001, "I smell gas", 0),
    )
    .await;
    assert_eq!(templates(&new), vec![SAFETY_TEMPLATE, consent()]);
    let (id, contact) = web_request(&mut db, 6).await;
    let first = carrier(&mut db, &text(contact, 0x6002, "I smell gas", 6)).await;
    assert_eq!(templates(&first)[..2], [SAFETY_TEMPLATE, consent()]);
    assert_eq!(channels(&mut db, id).await.len(), 2);
}

/// CON-021, D-5848: a number with only `web_chat` records gets the owner's help reply by email
/// only (`ReplyRefusal::EmailOnly`), nothing texted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_owner_help_reply_to_a_web_chat_only_number_is_email_only(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (_, contact) = web_request(&mut db, 7).await;
    let asked = (Uuid::from_u128(0x7001), at(6), at(60));
    let opened = open_help_request(
        &mut db,
        Asker::Homeowner(contact),
        asked,
        HelpLimits::default(),
    )
    .await
    .expect("help");
    let secret = secret();
    let press = Uuid::from_u128(0x7002);
    let reply = (opened.request(), press, "We will look at it today.");
    let sent = send_owner_reply(&mut db, (None, Some(&secret)), reply, at(7)).await;
    assert_eq!(sent, Err(ReplyRefusal::EmailOnly));
}

/// Her consent texts queued in the outbox.
async fn consent_texts(db: &mut PgConnection, contact: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1 AND template_id = $2",
    )
    .bind(contact)
    .bind(consent())
    .fetch_one(db)
    .await
    .expect("consent texts")
}

/// A carrier text through the route's reply entry, with its raw words.
async fn raw_reply(db: &mut PgConnection, msg: &TextIn<'_>) -> Reply {
    let (raw, vault) = (SecretString::from(msg.inbound.words.to_owned()), vault());
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

/// Asserts web-opened request `id` got one consent text more than its opening's `before` and one
/// `sms` record under `inbox`.
async fn texted_in(db: &mut PgConnection, (id, contact): (Uuid, Uuid), inbox: u128, before: i64) {
    assert_eq!(consent_texts(db, contact).await, before + 1);
    let sms = ("sms".to_owned(), Some(Uuid::from_u128(inbox)));
    assert_eq!(channels(db, id).await[1..], [sms]);
    assert!(may_text_request(db, id).await.expect("sms"));
}

/// D-5893, CON-005: a CLOSE as her first carrier text into a web-opened request, answered by the
/// close step before the screen, still queues the consent text and appends one `sms` record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_close_as_her_first_text_still_gets_consent_and_a_record(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (id, contact) = web_request(&mut db, 8).await;
    let before = consent_texts(&mut db, contact).await;
    let start = JobStart {
        conversation: id,
        words: "my sink leaks",
        emergency: false,
        trade: "plumbing",
        at: at(4),
    };
    let Opened::Created(_) = begin_job(&mut db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let reply = raw_reply(&mut db, &text(contact, 0x8001, "CLOSE", 6)).await;
    assert!(matches!(reply, Reply::Closed(..)), "{reply:?}");
    texted_in(&mut db, (id, contact), 0x8001, before).await;
}

/// D-5893, PICK-015, TXT-031: a "2 YES" from her phone and a LIST as her first carrier text into a
/// web-opened request each get the consent text first and one `sms` record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_pick_or_list_as_her_first_text_gets_consent_and_a_record(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (id, contact) = web_request(&mut db, 9).await;
    let before = consent_texts(&mut db, contact).await;
    let reply = raw_reply(&mut db, &text(contact, 0x9001, "2 YES", 6)).await;
    if let Reply::Handled(handled) = &reply {
        assert_eq!(templates(handled)[..1], [consent()]);
    }
    texted_in(&mut db, (id, contact), 0x9001, before).await;
    let (id, contact) = web_request(&mut db, 10).await;
    let before = consent_texts(&mut db, contact).await;
    let list = text(contact, 0x9002, "LIST", 6);
    receive_reply(&mut db, &secret(), &list, (Settings::default(), &[]))
        .await
        .expect("list");
    texted_in(&mut db, (id, contact), 0x9002, before).await;
}
