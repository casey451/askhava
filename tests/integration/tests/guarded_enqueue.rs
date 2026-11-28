//! The guarded enqueue seam (T857; CON-005, CON-007, CON-021, TXT-034, INT-043; D-5848).
//!
//! `conversation::queue_text` and `handlers::queue_body` find the request from `subject[0]` (a
//! request ID, or a job ID mapped to the request that opened it) and ask T856's guard first: an
//! `sms` request's texts reach the outbox, a request with no consent record is refused with
//! nothing kept, a job never borrows the caller's request, and a `web_chat`-only request keeps
//! its old chat texts (the chat exemption, until T801). `page_reply` returns a web request's
//! reply for the page with zero outbox rows and refuses any other request or session. Fake IDs,
//! fake words and fake 928-555-01xx numbers only.

use askhava_intake::api::rules::{AgeStop, Inbound, Settings, UNDER_18_TEMPLATE, consent_template};
use askhava_intake::api::{
    AUTO_SEND_AFTER, ChatError, ChatIn, ChatSettings, Checked, CodeStep, IntakeError, JobStart,
    Opened, PageReply, Preview, Shown, TextIn, ZIP_QUESTION_TEMPLATE, ask_zip, begin_job,
    chat_message, check_code, page_reply, receive_text, send_code, show_card, start_chat,
    stop_if_under_18,
};
use askhava_messaging::api::Language;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{CardParts, DEFAULT_MIN_WORDS, JobCard, Place, Timing, build_card};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The refusal every guarded seam returns (T856's `IntakeError::NoConsent`, D-5982).
const REFUSED: IntakeError = IntakeError::NoConsent;
const RIGHT: &str = "246810";
const LINK: &str = "https://example.invalid/r/fake-token";
const WORDS: &str = "my kitchen sink is leaking under the cabinet";

/// The fake code step: [`RIGHT`] is the right code; nothing leaves the test.
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

/// Session `k`'s fake number, 928 555 01xx.
fn phone(k: u8) -> String {
    format!("928-555-01{k:02}")
}

fn text_in(contact: Uuid, words: &str) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(contact.as_u128() + 1),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(0),
    }
}

/// A verified web-chat request on fake number `phone(k)`: (request, session, contact).
async fn web(db: &mut PgConnection, k: u8) -> (Uuid, Uuid, Uuid) {
    let (secret, vault) = (secret(), vault());
    let session = start_chat(db, &key(k), None, at(0)).await.expect("start");
    let first = ChatIn {
        message_id: Uuid::from_u128(0x8570_0000 + u128::from(k)),
        words: "my sink leaks",
        received_at: at(1),
    };
    let s = ChatSettings {
        language: Language::En,
        settings: Settings::default(),
        ip: None,
    };
    chat_message(db, &secret, &key(k), &first, s.settings)
        .await
        .expect("message");
    let number = SecretString::from(phone(k));
    send_code(db, (&vault, &FakeVerify), &key(k), &number, at(2))
        .await
        .expect("send");
    let checked = check_code(
        db,
        (&secret, &vault, &FakeVerify),
        &key(k),
        (&number, RIGHT),
        (s, at(3)),
    )
    .await
    .expect("check");
    let Checked::Verified(request, _) = checked else {
        panic!("not verified")
    };
    let contact: Uuid =
        sqlx::query_scalar("SELECT contact_id FROM intake.web_chat_session WHERE id = $1")
            .bind(session)
            .fetch_one(&mut *db)
            .await
            .expect("contact");
    (request, session, contact)
}

/// A text-opened request of `contact`; its first text writes its own `sms` record (T856).
async fn sms(db: &mut PgConnection, contact: Uuid) -> Uuid {
    let msg = text_in(contact, WORDS);
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let request = handled.conversation.expect("request");
    assert_eq!(
        records(db, request, "sms").await,
        1,
        "the fixture's sms record"
    );
    request
}

/// An open request of a new homeowner `contact` with no consent record at all.
async fn no_consent(db: &mut PgConnection, contact: Uuid) -> Uuid {
    let home: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, now()) RETURNING id",
    )
    .bind(contact)
    .fetch_one(&mut *db)
    .await
    .expect("homeowner");
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at) \
         VALUES ($1, 'en', $2, 'consent', now(), now()) RETURNING id",
    )
    .bind(home)
    .bind(Uuid::from_u128(contact.as_u128() + 1))
    .fetch_one(&mut *db)
    .await
    .expect("request")
}

/// Outbox rows of `template` whose subject names `about` (a request or a job).
async fn rows(db: &mut PgConnection, about: Uuid, template: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE $1 = ANY(subject_ids) AND template_id = $2",
    )
    .bind(about)
    .bind(template)
    .fetch_one(db)
    .await
    .expect("rows")
}

async fn all_rows(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(db)
        .await
        .expect("all rows")
}

async fn records(db: &mut PgConnection, request: Uuid, channel: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM intake.consent_record WHERE conversation_id = $1 AND channel = $2",
    )
    .bind(request)
    .bind(channel)
    .fetch_one(db)
    .await
    .expect("records")
}

async fn still_open(db: &mut PgConnection, request: Uuid) -> bool {
    sqlx::query_scalar("SELECT closed_at IS NULL FROM intake.conversation WHERE id = $1")
        .bind(request)
        .fetch_one(db)
        .await
        .expect("open")
}

/// Seeds one fake answer on `request`, so a purge has something to remove.
async fn seed_answer(db: &mut PgConnection, request: Uuid) {
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, 'zip', 'z86403', now())",
    )
    .bind(request)
    .execute(db)
    .await
    .expect("seed");
}

/// Whether the purge removed every answer of `request`.
async fn purged(db: &mut PgConnection, request: Uuid) -> bool {
    zip_marks(db, request).await == 0
}

async fn zip_marks(db: &mut PgConnection, request: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM intake.intake_answer WHERE conversation_id = $1 AND question_id = 'zip'",
    )
    .bind(request)
    .fetch_one(db)
    .await
    .expect("zip marks")
}

/// A reply from `contact` with its own inbox ID `inbox`.
fn reply(contact: Uuid, inbox: u128, words: &str) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(inbox),
        ..text_in(contact, words)
    }
}

/// A plumbing job opened on `request` (any consent or none).
async fn job_of(db: &mut PgConnection, request: Uuid) -> Uuid {
    let start = JobStart {
        conversation: request,
        words: WORDS,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job")
    };
    job
}

fn card() -> JobCard {
    let parts = CardParts {
        category: "Plumbing".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "Water drips under the kitchen sink".to_owned(),
        answers: vec![("Where is the leak".to_owned(), "Under a sink".to_owned())],
        timing: Some(Timing::NoRush),
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    build_card(&parts, DEFAULT_MIN_WORDS)
}

/// `show_card` for `job` called on request `request` (its `queue_body` subject is `[job, ..]`).
async fn show(db: &mut PgConnection, (request, job): (Uuid, Uuid)) -> Result<Shown, IntakeError> {
    let card = card();
    let p = Preview {
        conversation: request,
        job,
        card: &card,
        link: LINK,
        wait: AUTO_SEND_AFTER,
        at: at(2),
    };
    show_card(db, &secret(), &p).await
}

/// CON-021, D-5848 (acceptance 1, conversation subject): an `sms` request's texts reach the
/// outbox through both seams: `queue_body` (`ask_zip`) and `queue_text` (`stop_if_under_18`).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_sms_requests_texts_reach_the_outbox(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let contact = Uuid::from_u128(0x85701);
    let request = sms(&mut db, contact).await;
    let zip = reply(contact, 0x85711, "ok");
    let asked = ask_zip(&mut db, &secret(), (&zip, request), Settings::default()).await;
    assert_eq!(asked.expect("queue_body").template, ZIP_QUESTION_TEMPLATE);
    assert_eq!(rows(&mut db, request, ZIP_QUESTION_TEMPLATE).await, 1);
    let young = reply(contact, 0x85712, "I am 16 years old");
    let stop = stop_if_under_18(&mut db, &secret(), &young, request, false).await;
    let (age, queued) = stop.expect("queue_text").expect("stopped");
    let queued = queued.expect("the guard let the under-18 text go");
    assert_eq!(
        (age, queued.template),
        (AgeStop::Under18, UNDER_18_TEMPLATE)
    );
    assert_eq!(rows(&mut db, request, UNDER_18_TEMPLATE).await, 1);
}

/// CON-021, CON-007, D-5848 (acceptance 3), D-5981 (point 11): a request with neither record is
/// refused at both seams: no outbox row and no ZIP marker, yet the age stop still answers
/// `Some((Under18, None))` and closes the request, and answers `Some((Under13, None))` and purges an under-13
/// child's request (INT-038, PRIV-008); point 11.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_request_with_no_record_is_refused_with_nothing_kept(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let contact = Uuid::from_u128(0x85702);
    let request = no_consent(&mut db, contact).await;
    let before = all_rows(&mut db).await;
    let zip = reply(contact, 0x85721, "ok");
    let asked = ask_zip(&mut db, &secret(), (&zip, request), Settings::default()).await;
    assert_eq!(asked.expect_err("queue_body refused"), REFUSED);
    assert_eq!(
        zip_marks(&mut db, request).await,
        0,
        "the ZIP marker rolled back"
    );
    let young = reply(contact, 0x85722, "I am 16 years old");
    let stop = stop_if_under_18(&mut db, &secret(), &young, request, false).await;
    let stopped = stop
        .expect("never rolled back")
        .map(|(age, q)| (age, q.is_some()));
    assert_eq!(
        stopped,
        Some((AgeStop::Under18, false)),
        "Some((Under18, None)), never Ok(None)"
    );
    assert_eq!(
        all_rows(&mut db).await,
        before,
        "the refused text queued nothing"
    );
    assert!(
        !still_open(&mut db, request).await,
        "the request closed though the text was refused"
    );
    let child_contact = Uuid::from_u128(0x85703);
    let child = no_consent(&mut db, child_contact).await;
    seed_answer(&mut db, child).await;
    assert!(!purged(&mut db, child).await, "the seed is there");
    let before = all_rows(&mut db).await;
    let twelve = reply(child_contact, 0x85723, "I am 12 years old");
    let stop = stop_if_under_18(&mut db, &secret(), &twelve, child, false).await;
    let stopped = stop
        .expect("never rolled back")
        .map(|(age, q)| (age, q.is_some()));
    assert_eq!(
        stopped,
        Some((AgeStop::Under13, false)),
        "Some((Under13, None)), never Ok(None)"
    );
    assert_eq!(
        all_rows(&mut db).await,
        before,
        "the refused text queued nothing"
    );
    assert!(
        purged(&mut db, child).await,
        "an under-13 request is purged though the text was refused"
    );
}

/// CON-007, D-5848, T649, T645 (acceptance 4): the chat exemption keeps a `web_chat`-only
/// request's old chat texts in the outbox: the consent text, a ZIP question and INT-038's reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_chat_exemption_keeps_the_old_chat_texts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (request, _, contact) = web(&mut db, 51).await;
    assert_eq!(records(&mut db, request, "web_chat").await, 1);
    assert_eq!(records(&mut db, request, "sms").await, 0);
    let consent = consent_template(Settings::default().safety_call_on);
    assert_eq!(
        rows(&mut db, request, consent).await,
        1,
        "open_verified's consent text"
    );
    let zip = reply(contact, 0x85731, "ok");
    ask_zip(&mut db, &secret(), (&zip, request), Settings::default())
        .await
        .expect("queue_body");
    assert_eq!(rows(&mut db, request, ZIP_QUESTION_TEMPLATE).await, 1);
    let young = reply(contact, 0x85732, "I am 16 years old");
    let stop = stop_if_under_18(&mut db, &secret(), &young, request, false).await;
    assert!(matches!(
        stop.expect("queue_text"),
        Some((AgeStop::Under18, Some(_)))
    ));
    assert_eq!(rows(&mut db, request, UNDER_18_TEMPLATE).await, 1);
}

/// TXT-034, CON-005, D-5848 (acceptance 2): the page function returns a web request's reply
/// as it was given, with zero outbox rows.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_page_function_returns_a_web_reply_with_no_outbox_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (request, session, _) = web(&mut db, 52).await;
    let before = all_rows(&mut db).await;
    let body = "Fake reply words for the page.".to_owned();
    let shown = page_reply(
        &mut db,
        (request, session),
        (ZIP_QUESTION_TEMPLATE, body.clone()),
    )
    .await;
    let want = PageReply {
        conversation: request,
        template: ZIP_QUESTION_TEMPLATE,
        body,
    };
    assert_eq!(shown.expect("page"), want);
    assert_eq!(all_rows(&mut db).await, before, "zero outbox rows");
}

/// TXT-034, CON-021, D-5848: the page function shows nothing for another session, an `sms`
/// request or a request with no record; its own pair still passes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_page_function_refuses_anything_but_its_web_request(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (request, session, _) = web(&mut db, 53).await;
    let (_, other_session, _) = web(&mut db, 54).await;
    let words = || (ZIP_QUESTION_TEMPLATE, "Fake words.".to_owned());
    let wrong = page_reply(&mut db, (request, other_session), words()).await;
    assert_eq!(wrong, Err(IntakeError::UnknownRequest), "another session");
    let texted = sms(&mut db, Uuid::from_u128(0x85751)).await;
    let by_text = page_reply(&mut db, (texted, session), words()).await;
    assert_eq!(by_text, Err(IntakeError::UnknownRequest), "an sms request");
    let none = no_consent(&mut db, Uuid::from_u128(0x85752)).await;
    let empty = page_reply(&mut db, (none, session), words()).await;
    assert_eq!(empty, Err(IntakeError::UnknownRequest), "no record");
    assert!(
        page_reply(&mut db, (request, session), words())
            .await
            .is_ok(),
        "its own pair"
    );
}

/// D-5848 (job subject): a `[job, ..]` subject reaches the request that opened the job, whose
/// `sms` record lets the card text into the outbox.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_job_subject_reaches_the_request_that_opened_it(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let request = sms(&mut db, Uuid::from_u128(0x85761)).await;
    let job = job_of(&mut db, request).await;
    let Shown::Texted(queued) = show(&mut db, (request, job)).await.expect("shown") else {
        panic!("texted")
    };
    let first: Uuid =
        sqlx::query_scalar("SELECT subject_ids[1] FROM messaging.outbox WHERE id = $1")
            .bind(queued.outbox_id)
            .fetch_one(&mut *db)
            .await
            .expect("subject");
    assert_eq!(first, job);
}

/// CON-021, D-5848 (job subject, never borrowed): a caller on an `sms` request that queues about
/// a job another request (with no record) opened is refused: the job's own request decides,
/// never the caller's request or contact.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_job_subject_never_borrows_the_callers_request(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let request = sms(&mut db, Uuid::from_u128(0x85771)).await;
    let other = no_consent(&mut db, Uuid::from_u128(0x85772)).await;
    let job = job_of(&mut db, other).await;
    let before = all_rows(&mut db).await;
    assert_eq!(
        show(&mut db, (request, job)).await.expect_err("refused"),
        REFUSED
    );
    assert_eq!(all_rows(&mut db).await, before, "nothing queued");
}

/// CON-021, D-5848 (fail closed): a job two requests name resolves to neither, so the text is
/// refused even though the caller's own request has an `sms` record.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_job_two_requests_name_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let request = sms(&mut db, Uuid::from_u128(0x85781)).await;
    let other = sms(&mut db, Uuid::from_u128(0x85782)).await;
    let job = job_of(&mut db, request).await;
    sqlx::query(
        "INSERT INTO intake.request_job (conversation_id, job_id, opened_at) VALUES ($1, $2, now())",
    )
    .bind(other)
    .bind(job)
    .execute(&mut *db)
    .await
    .expect("second request names the job");
    let before = all_rows(&mut db).await;
    assert_eq!(
        show(&mut db, (request, job)).await.expect_err("refused"),
        REFUSED
    );
    assert_eq!(all_rows(&mut db).await, before, "nothing queued");
}

/// D-5848 (a close keeps the mapping): a job subject on a closed request still resolves to it, so
/// a text about the job reaches the outbox (T865's closing text relies on this).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_job_subject_on_a_closed_request_still_resolves(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let request = sms(&mut db, Uuid::from_u128(0x85791)).await;
    let job = job_of(&mut db, request).await;
    sqlx::query("UPDATE intake.conversation SET closed_at = now() WHERE id = $1")
        .bind(request)
        .execute(&mut *db)
        .await
        .expect("close");
    assert!(!still_open(&mut db, request).await);
    let before = all_rows(&mut db).await;
    let shown = show(&mut db, (request, job)).await.expect("shown");
    assert!(matches!(shown, Shown::Texted(_)));
    assert_eq!(all_rows(&mut db).await, before + 1, "one text queued");
}

/// TXT-034, CON-005, D-5848 (T856's carrier-text append): once she texts into her web request it has an `sms`
/// record of its own, so it goes on by text and the page function refuses it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_page_function_refuses_a_web_request_once_she_texts_in(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (request, session, contact) = web(&mut db, 55).await;
    let msg = reply(contact, 0x85781, "is anyone there");
    receive_text(&mut db, &secret(), &msg, Settings::default())
        .await
        .expect("her text");
    assert_eq!(
        records(&mut db, request, "sms").await,
        1,
        "T856's appended record"
    );
    let page = page_reply(
        &mut db,
        (request, session),
        (ZIP_QUESTION_TEMPLATE, String::new()),
    )
    .await;
    assert_eq!(page, Err(IntakeError::UnknownRequest));
}
