//! Help requests and the owner's HELP notification (T151; TD-OPS-11, TD-OPS-20, TD-OPS-22,
//! TD-PRIV-11; OPS-036, PRIV-026, PRIV-030, CON-034). Fake data only: 555 numbers, example.com
//! addresses, a made-up street.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_core::api::{Claimed, brand};
use askhava_intake::api::{
    CHARGE_QUESTION, business_help, help_answered, help_follow_up, note_help, receive_help_email,
    run_help_request_opened,
};
use askhava_messaging::api::{
    DailyLimit, HELP_NOTICE_KIND, HELP_PAGE_PATH, HelpNotice, HelpSender, MailboxKind,
    run_help_notice,
};
use askhava_ops::api::{
    HelpOpened, add_owner_reply, help_request, open_help_requests, set_help_closed,
};
use askhava_portals::api::{ChargeAsked, HELP_REQUEST_KIND, ask_about_charge};
use askhava_privacy::api::{
    CommonWords, EmailContext, InboundWords, Known, NameMemory, ScrubbedText, SignedDelivery,
    Vault, cut_problem_words, read_inbound_email, scrub,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const SENTINEL: &str =
    "why was I charged? call 928-555-0142 or fake.person@example.com, 123 Fake Street";
const LEAKS: [&str; 4] = ["555", "example.com", "Fake Street", "0142"];
/// Names from the WORD-021 scan's kind: none may reach a notification (TD-WORD-04, D-493).
const NAMES: [&str; 3] = ["Casey", "Pat", "Hava "];

type Sent = Arc<Mutex<Vec<(String, String, String)>>>;

/// The fake provider: records each email it took.
struct FakeMail {
    sent: Sent,
}

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        sent.push((e.to.to_owned(), e.subject.to_owned(), e.text.to_owned()));
        Ok(format!("re_{:08}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn sender() -> (HelpSender<FakeMail>, Sent) {
    let sent = Sent::default();
    let fake = FakeMail {
        sent: Arc::clone(&sent),
    };
    let mail = HelpSender::new(
        (
            SecretString::from("fake-id-secret-for-tests-only-0123456789"),
            fake,
        ),
        ("PO Box 100, Faketown, AZ 00000", MailboxKind::PoBox),
        DailyLimit::new(1000, 80).expect("limit"),
        SecretString::from("alerts@example.test"),
    );
    (mail.expect("sender"), sent)
}

/// Arizona `hh:mm` on Nov 10, 2026 (UTC-7).
fn az(hour: i64, minute: i64) -> Timestamp {
    let midnight: Timestamp = "2026-11-10T07:00:00Z".parse().expect("time");
    midnight + SignedDuration::from_mins(hour * 60 + minute)
}

fn words(text: &str) -> ScrubbedText {
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    scrub(text, &known, &mut NameMemory::default())
}

async fn notices(db: &mut PgConnection) -> Vec<Value> {
    sqlx::query_scalar::<_, Value>("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(HELP_NOTICE_KIND)
        .fetch_all(db)
        .await
        .expect("tasks")
}

async fn send_all(pool: &PgPool, mail: &HelpSender<FakeMail>, at: Timestamp) {
    let mut conn = pool.acquire().await.expect("conn");
    for payload in notices(&mut conn).await {
        let task = Claimed {
            id: 1,
            kind: HELP_NOTICE_KIND.to_owned(),
            payload,
            attempts: 1,
        };
        run_help_notice(pool, Some(mail), &task, at)
            .await
            .expect("sent");
    }
}

/// TD-OPS-11, TD-OPS-20: a business's "why was I charged?" at 02:10 is one email to the alert
/// address at once, with the subject, a reply-by time, one owner link and no phone, email, street
/// or personal name; nothing is texted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_11_td_ops_20_one_scrubbed_email_at_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (business, inbox) = (Uuid::from_u128(0xb1), Uuid::from_u128(0x1b1));
    let opened = business_help(&mut db, (inbox, business), &words(SENTINEL), az(2, 10)).await;
    let HelpOpened::Opened {
        request,
        notify: true,
    } = opened.expect("opened")
    else {
        panic!("not notified at once");
    };
    let (mail, sent) = sender();
    send_all(&pool, &mail, az(2, 10)).await;
    send_all(&pool, &mail, az(2, 11)).await;
    let emails = sent.lock().expect("lock").clone();
    assert_eq!(emails.len(), 1);
    let (to, subject, text) = &emails[0];
    assert_eq!(to, "alerts@example.test");
    let start = format!("{} HELP: business B-", brand().short_name());
    assert!(subject.starts_with(&start) && subject.contains(", reply by Nov 12, 2:10 AM"));
    assert!(text.contains(&format!("{HELP_PAGE_PATH}{request}")));
    assert!(text.contains("why was I charged?"));
    for leak in LEAKS.iter().chain(&NAMES) {
        assert!(
            !text.contains(leak) && !subject.contains(leak),
            "leaked {leak}"
        );
    }
    let texts: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(&mut *db)
        .await
        .expect("outbox");
    assert_eq!(texts, 0, "a HELP notification is never a text");
}

/// TD-OPS-11: 02:50 joins the 02:10 request with no new email; 03:11 is a new one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_11_joins_within_the_hour_and_opens_after(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let business = Uuid::from_u128(0xb2);
    let mut seen = Vec::new();
    for (n, at) in [(1, az(2, 10)), (2, az(2, 50)), (3, az(3, 11))] {
        let said = words("cargo incorrecto");
        let opened = business_help(&mut db, (Uuid::from_u128(n), business), &said, at);
        seen.push(opened.await.expect("help"));
    }
    assert_eq!(seen[1], HelpOpened::Joined(seen[0].request()));
    assert!(matches!(seen[2], HelpOpened::Opened { notify: true, .. }));
    assert_ne!(seen[2].request(), seen[0].request());
    assert_eq!(notices(&mut db).await.len(), 2);
}

/// TD-OPS-11: the 11th notification of a day waits for the 9:00 push.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_11_the_eleventh_of_a_day_waits_for_the_push(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let mut last = None;
    for n in 1..=11 {
        let who = (Uuid::from_u128(0x100 + n), Uuid::from_u128(0x200 + n));
        let at = az(2, i64::try_from(n).expect("n"));
        last = Some(
            business_help(&mut db, who, &words("why was I charged?"), at)
                .await
                .expect("help"),
        );
    }
    let Some(HelpOpened::Opened {
        request,
        notify: false,
    }) = last
    else {
        panic!("the 11th was notified");
    };
    assert_eq!(notices(&mut db).await.len(), 10);
    let show_from: Timestamp = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT show_from FROM ops.push_item WHERE subject_id = $1 AND done_at IS NULL",
    )
    .bind(request)
    .fetch_one(&mut *db)
    .await
    .expect("push item")
    .to_jiff();
    assert!(show_from <= az(9, 0), "listed by the 9:00 push");
}

/// TD-OPS-22: open requests list with their due dates for the digest; a request still open at
/// its due time is in the push; the owner's reply, close and reopen keep references only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_22_open_requests_with_due_dates(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let at = az(10, 0);
    let said = words("why was I charged?");
    let ids = (Uuid::from_u128(7), Uuid::from_u128(0xb7));
    let opened = business_help(&mut db, ids, &said, at);
    let request = opened.await.expect("help").request();
    let due = at + SignedDuration::from_hours(48);
    assert_eq!(
        open_help_requests(&mut db).await.expect("open"),
        vec![(request, due)]
    );
    add_owner_reply(
        &mut db,
        request,
        Uuid::from_u128(8),
        at + SignedDuration::from_mins(5),
    )
    .await
    .expect("reply");
    let row = help_request(&mut db, request)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.messages.len(), 2);
    assert!(row.messages[1].1 && row.messages[0].0 == Uuid::from_u128(7));
    assert!(
        set_help_closed(&mut db, request, (true, at))
            .await
            .expect("close")
    );
    assert!(open_help_requests(&mut db).await.expect("open").is_empty());
    assert!(
        set_help_closed(&mut db, request, (false, at))
            .await
            .expect("reopen")
    );
    assert!(
        !set_help_closed(&mut db, request, (false, at))
            .await
            .expect("again")
    );
}

async fn seed_job(db: &mut PgConnection, contact: Uuid, job: Uuid) {
    let (home, conv, property) = (
        Uuid::from_u128(0xa1),
        Uuid::from_u128(0xa2),
        Uuid::from_u128(0xa3),
    );
    let kind = format!("job.plumbing.{}", &property.simple().to_string()[..12]);
    let statements = [
        "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from) VALUES ($1, 'quotes_ready', $6, $6, $6)",
        "INSERT INTO jobs.job_number (number, job_id, issued_at) VALUES (1209, $1, $6)",
        "INSERT INTO intake.homeowner (id, contact_id, created_at) VALUES ($2, $3, $6)",
        "INSERT INTO intake.conversation (id, homeowner_id, language, first_inbox_id, consent_template, consent_sent_at, opened_at) VALUES ($4, $2, 'en', $1, 'consent', $6, $6)",
        "INSERT INTO intake.property (id, homeowner_id, contact_id, town, created_at) VALUES ($5, $2, $3, 'lake_havasu_city', $6)",
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) VALUES ($4, $7, replace($1::text, '-', ''), $6)",
    ];
    for sql in statements {
        sqlx::query(sql)
            .bind(job)
            .bind(home)
            .bind(contact)
            .bind(conv)
            .bind(property)
            .bind(jiff_sqlx::Timestamp::from(az(1, 0)))
            .bind(&kind)
            .execute(&mut *db)
            .await
            .expect(sql);
    }
}

/// TD-OPS-11, CON-034: HELP only records its time; her follow-up opens the request with her
/// job's number, status, trade and town and the message scrubbed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_11_a_homeowner_follow_up_after_help(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (contact, job) = (Uuid::from_u128(0xc1), Uuid::from_u128(0xd1));
    seed_job(&mut db, contact, job).await;
    note_help(&mut db, contact, az(2, 0)).await.expect("noted");
    assert!(
        notices(&mut db).await.is_empty(),
        "HELP alone notifies nobody"
    );
    assert!(
        help_answered(&mut db, contact, az(2, 10))
            .await
            .expect("read")
    );
    assert!(
        !help_answered(&mut db, contact, az(2, 0) + SignedDuration::from_hours(25))
            .await
            .expect("read")
    );
    let text = "the sink is still leaking, text me at 928-555-0142";
    help_follow_up(
        &mut db,
        (Uuid::from_u128(0xe1), contact),
        &words(text),
        az(2, 10),
    )
    .await
    .expect("help");
    let payload = notices(&mut db).await.pop().expect("one notice");
    let notice: HelpNotice = serde_json::from_value(payload).expect("notice");
    assert_eq!(
        (notice.asker.as_str(), notice.number.as_str()),
        ("homeowner", "1209")
    );
    assert_eq!(notice.status.as_deref(), Some("quotes_ready"));
    assert_eq!(notice.trade.as_deref(), Some("plumbing"));
    assert_eq!(notice.town.as_deref(), Some("lake_havasu_city"));
    assert!(notice.message.contains("leaking") && !notice.message.contains("555"));
}

/// TD-OPS-11: "Ask about a charge" from an action page opens one help request with one notice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_11_ask_about_a_charge_opens_one_request(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = Uuid::from_u128(0xd2);
    seed_job(&mut db, Uuid::from_u128(0xc2), job).await;
    let asked = ChargeAsked {
        business: Uuid::from_u128(0xb9),
        charge: Uuid::from_u128(0xf9),
        job,
    };
    ask_about_charge(&mut db, asked, az(2, 10))
        .await
        .expect("asked");
    let payload: Value = sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1")
        .bind(HELP_REQUEST_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("event");
    let task = Claimed {
        id: 1,
        kind: HELP_REQUEST_KIND.to_owned(),
        payload,
        attempts: 1,
    };
    run_help_request_opened(pool.clone(), task, az(2, 10))
        .await
        .expect("handled");
    let notice: HelpNotice =
        serde_json::from_value(notices(&mut db).await.pop().expect("notice")).expect("notice");
    assert_eq!(
        (notice.asker.as_str(), notice.number.as_str()),
        ("business", "1209")
    );
    assert_eq!(notice.message, CHARGE_QUESTION);
    assert_eq!(open_help_requests(&mut db).await.expect("open").len(), 1);
}

fn signed_email(text: &str, dmarc: &str) -> askhava_privacy::api::InboundEmail {
    let key = b"fake-resend-signing-key";
    let secret = SecretString::from(format!("whsec_{}", STANDARD.encode(key)));
    let vkey = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava");
    let vault = Vault::new(&vkey(0x31), &vkey(0x41), &url).expect("vault");
    let body = serde_json::to_vec(&serde_json::json!({"type": "email.received", "data": {
        "from": "fake.sender@example.com", "text": text,
        "auth": {"spf": "pass", "dkim": "pass", "dmarc": dmarc}}}))
    .expect("json");
    let now = Timestamp::now();
    let stamp = now.as_second().to_string();
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("key");
    mac.update(format!("msg_1.{stamp}.").as_bytes());
    mac.update(&body);
    let signature = format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()));
    let delivery = SignedDelivery {
        id: "msg_1",
        timestamp: &stamp,
        signature: &signature,
        body: &body,
    };
    let common = CommonWords::default();
    let words = InboundWords {
        emergency: &[],
        opt_out: &["stop"],
        help: &["help"],
        start: &["start"],
    };
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let ctx = EmailContext {
        secret: &secret,
        vault: &vault,
        words,
        known,
        now,
    };
    read_inbound_email(&delivery, &ctx, &mut NameMemory::default()).expect("read")
}

/// TD-PRIV-11: a signed help email's sentinel body is kept only in `privacy.message_body`
/// (never in an `ops` row) and is gone at day 31; a DMARC failure opens nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_priv_11_a_help_email_body_stays_in_privacy(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let at = Timestamp::now();
    let failing = signed_email("zq-sentinel-7f3a help", "fail");
    let failed = receive_help_email(&mut db, &failing, at);
    assert_eq!(failed.await.expect("read"), None);
    let email = signed_email("zq-sentinel-7f3a my sink leaks", "pass");
    let opened = receive_help_email(&mut db, &email, at)
        .await
        .expect("help")
        .expect("opened");
    let kept: Vec<String> = sqlx::query_scalar("SELECT words FROM privacy.message_body")
        .fetch_all(&mut *db)
        .await
        .expect("words");
    assert!(kept.len() == 1 && kept[0].contains("zq-sentinel-7f3a"));
    let ops: String = sqlx::query_scalar(
        "SELECT concat((SELECT string_agg(r::text, ' ') FROM ops.help_request r), \
         (SELECT string_agg(m::text, ' ') FROM ops.help_message m))",
    )
    .fetch_one(&mut *db)
    .await
    .expect("ops rows");
    assert!(ops.contains(&opened.request().to_string()) && !ops.contains("zq-sentinel"));
    let cut = cut_problem_words(&mut db, at + SignedDuration::from_hours(31 * 24)).await;
    assert_eq!(cut.expect("cut"), 1);
}
