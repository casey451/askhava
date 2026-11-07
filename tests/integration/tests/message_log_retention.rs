//! The message log and its 30-day body cut (TD-TXT-07, TD-CON-25; TXT-019, TXT-020, CON-022,
//! CON-023, CON-036; D-093, D-1851 to D-1853).
//!
//! Proves on fresh databases that a homeowner's sent text is logged with her vault reference and
//! never her number, with its template ID, language, request, sent time, Twilio status and
//! attempts; that at 30 days no outbound body is left (a sent row keeps none, and a text still
//! unsent is cut to its template ID and ended) while every log row stays; that the inbound log
//! holds no raw or scrubbed words; and that a keyword opt-out stores the keyword, a natural-language
//! one the pattern ID, both with their inbox row, surviving the day-31 cut, append-only for the app
//! and never put in a `core.task` payload. Every phone, name and body is fake.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_integration::{is_permission_denied, set_app_role};
use askhava_messaging::api::{
    InboundText, Language, NewText, ProviderKeyword, Relation, To, cut_bodies, enqueue,
    message_log, on_inbound_text, render, send,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const FROM: &str = "+19285550100";
const REQUEST: Uuid = Uuid::from_u128(0x0093_0001);
const DAY: SignedDuration = SignedDuration::from_hours(24);

/// A fake carrier that accepts every text and records the numbers it was asked to text.
#[derive(Clone, Default)]
struct FakeCarrier {
    numbers: Arc<Mutex<Vec<String>>>,
}

impl SmsAdapter for FakeCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        let mut numbers = self.numbers.lock().expect("lock");
        numbers.push(to.to_owned());
        Ok(SentText {
            sid: format!("SM{:032}", numbers.len()),
            status: "queued".to_owned(),
        })
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x93), &key(0x39), &url).expect("vault")
}

async fn homeowner(db: &mut PgConnection, vault: &Vault, phone: &str) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Yara Testwell".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, vault, &new).await.expect("contact")
}

async fn queue(db: &mut PgConnection, her: Uuid, template: &str, body: &str) -> Uuid {
    let new = NewText {
        template_id: template,
        subject: &[REQUEST],
        recipient: her,
        relation: Relation::OpenRequest,
        body,
    };
    enqueue(db, &secret(), &new).await.expect("queued").0
}

/// Moves every outbox row's queue and send times back by `days`, as the clock would.
async fn age_outbox(pool: &PgPool, days: i32) {
    sqlx::query(
        "UPDATE messaging.outbox SET created_at = created_at - make_interval(days => $1), \
         sent_at = sent_at - make_interval(days => $1)",
    )
    .bind(days)
    .execute(pool)
    .await
    .expect("aged");
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// TD-TXT-07: the log holds the vault reference and the TXT-019 fields, never the number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_07_the_log_holds_a_vault_reference_never_a_number(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    let her = homeowner(&mut db, &vault, "928-555-0193").await;
    set_app_role(&mut db).await.expect("app role");
    let values = [("job", "HV-0093"), ("link", "https://askhava.com/q/fake93")];
    let spanish = render("first_quote", Language::Es, true, &values).expect("render");
    let id = queue(&mut db, her, "first_quote", &spanish.body).await;
    let to = To::Homeowner {
        vault: &vault,
        relation: Relation::OpenRequest,
    };
    let carrier = FakeCarrier::default();
    send(&mut db, (&secret(), &carrier), id, (to, FROM))
        .await
        .expect("sent");
    assert_eq!(*carrier.numbers.lock().expect("lock"), ["+19285550193"]);
    let entry = message_log(&mut db, id).await.expect("log");
    assert_eq!(entry.recipient, her);
    assert_eq!(entry.recipient_kind, "homeowner");
    assert_eq!(entry.template_id, "first_quote");
    assert_eq!(entry.language, Some(Language::Es));
    assert_eq!(entry.subject, [REQUEST]);
    assert!(entry.sent_at.is_some());
    assert_eq!(entry.status.as_str(), "sent");
    assert_eq!(entry.attempts, 1);
    assert!(!entry.body_kept);
    let english = render("first_quote", Language::En, true, &values).expect("render");
    let other = queue_other(&mut db, her, &english.body).await;
    let entry = message_log(&mut db, other).await.expect("log");
    assert_eq!(entry.language, Some(Language::En));
    let unknown = queue_other(&mut db, her, "Ask Hava: fake body").await;
    assert_eq!(
        message_log(&mut db, unknown).await.expect("log").language,
        None
    );
    let rows = "SELECT count(*) FROM messaging.outbox \
                WHERE row_to_json(outbox)::text ~ '555.?0193|9285550193'";
    assert_eq!(count(&pool, rows).await, 0, "no number in the log");
    let phone_columns = "SELECT count(*) FROM information_schema.columns \
                         WHERE table_schema = 'messaging' AND column_name ~ 'phone|number'";
    assert_eq!(count(&pool, phone_columns).await, 0);
}

async fn queue_other(db: &mut PgConnection, her: Uuid, body: &str) -> Uuid {
    let new = NewText {
        template_id: "first_quote",
        subject: &[REQUEST, Uuid::new_v4()],
        recipient: her,
        relation: Relation::OpenRequest,
        body,
    };
    enqueue(db, &secret(), &new).await.expect("queued").0
}

/// TD-TXT-07: every HELP reply variant, in English and Spanish, is logged under `help_reply` with
/// its language (D-1851).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_07_every_help_reply_variant_logs_its_language(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    let her = homeowner(&mut db, &vault, "928-555-0194").await;
    set_app_role(&mut db).await.expect("app role");
    let values = [("reply_time", "2 hours"), ("time", "Monday")];
    let variants = [
        "help_reply",
        "help_reply_ca",
        "help_reply_away",
        "help_reply_away_ca",
    ];
    for variant in variants {
        for language in [Language::En, Language::Es] {
            let body = render(variant, language, true, &values)
                .expect("render")
                .body;
            let new = NewText {
                template_id: "help_reply",
                subject: &[REQUEST, Uuid::new_v4()],
                recipient: her,
                relation: Relation::OpenRequest,
                body: &body,
            };
            let id = enqueue(&mut db, &secret(), &new).await.expect("queued").0;
            let entry = message_log(&mut db, id).await.expect("log");
            assert_eq!(entry.language, Some(language), "{variant}");
        }
    }
}

/// TD-TXT-07: at 30 days no outbound body is left, the log rows stay with their template IDs, and
/// the inbound log holds no words at all.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_07_bodies_become_template_ids_at_30_days(pool: PgPool) {
    let vault = vault();
    let mut db = pool.acquire().await.expect("connection");
    let her = homeowner(&mut db, &vault, "928-555-0194").await;
    set_app_role(&mut db).await.expect("app role");
    let stale = queue(&mut db, her, "first_quote", "Ask Hava: fake stale body").await;
    let now = Timestamp::now();
    assert_eq!(cut_bodies(&mut db, now).await.expect("cut"), 0, "too young");
    age_outbox(&pool, 30).await;
    let fresh = queue_other(&mut db, her, "Ask Hava: fake fresh body").await;
    assert_eq!(cut_bodies(&mut db, now).await.expect("cut"), 1);
    assert_eq!(
        cut_bodies(&mut db, now).await.expect("cut"),
        0,
        "safe twice"
    );
    let entry = message_log(&mut db, stale).await.expect("log kept");
    assert_eq!(entry.template_id, "first_quote");
    assert!(!entry.body_kept);
    assert!(message_log(&mut db, fresh).await.expect("log").body_kept);
    age_outbox(&pool, 5 * 365).await;
    cut_bodies(&mut db, now).await.expect("cut");
    let kept = "SELECT count(*) FROM messaging.outbox WHERE template_id = 'first_quote'";
    assert_eq!(count(&pool, kept).await, 2, "log entries stay");
    let bodies = "SELECT count(*) FROM messaging.outbox WHERE body IS NOT NULL";
    assert_eq!(count(&pool, bodies).await, 0);
    let inbound_words = "SELECT count(*) FROM information_schema.columns \
                         WHERE table_schema = 'messaging' AND table_name = 'inbox' \
                         AND column_name NOT IN ('id', 'message_sid', 'received_at')";
    assert_eq!(count(&pool, inbound_words).await, 0, "no inbound text kept");
}

async fn inbox_row(pool: &PgPool, n: u32, at: Timestamp) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO messaging.inbox (message_sid, received_at) \
         VALUES ($1, to_timestamp($2::bigint)) RETURNING id",
    )
    .bind(format!("SM{n:032}"))
    .bind(at.as_second())
    .fetch_one(pool)
    .await
    .expect("inbox row")
}

async fn opt_out(pool: &PgPool, vault: &Vault, (phone, text, n): (&str, &str, u32), at: Timestamp) {
    let mut db = pool.acquire().await.expect("connection");
    let her = homeowner(&mut db, vault, phone).await;
    let inbox_id = inbox_row(pool, n, at).await;
    set_app_role(&mut db).await.expect("app role");
    let msg = InboundText {
        inbox_id,
        contact: her,
        relation: Relation::OpenRequest,
        phone,
        text,
        provider: ProviderKeyword::None,
        emergency: false,
    };
    on_inbound_text(&mut db, (&secret(), vault), &msg, at)
        .await
        .expect("opt-out");
}

/// TD-CON-25: a keyword opt-out stores the keyword and a natural-language one the pattern ID, each
/// with its inbox row; both survive the day-31 cut, stay append-only and are never exported.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_25_opt_out_evidence_survives_the_body_cut(pool: PgPool) {
    let vault = vault();
    let then = Timestamp::now() - DAY * 31;
    opt_out(&pool, &vault, ("928-555-0195", "STOP", 1), then).await;
    opt_out(&pool, &vault, ("928-555-0196", "please remove me", 2), then).await;
    let mut db = pool.acquire().await.expect("connection");
    set_app_role(&mut db).await.expect("app role");
    age_outbox(&pool, 31).await;
    cut_bodies(&mut db, Timestamp::now()).await.expect("cut");
    let evidence: Vec<(String, bool)> = sqlx::query_as(
        "SELECT e.detected, i.id IS NOT NULL FROM messaging.optout_event e \
         LEFT JOIN messaging.inbox i ON i.id = e.inbox_id ORDER BY e.detected",
    )
    .fetch_all(&pool)
    .await
    .expect("events");
    let expected = [
        ("phrase:en.remove_me".to_owned(), true),
        ("word:stop".to_owned(), true),
    ];
    assert_eq!(evidence, expected);
    let update = sqlx::query("UPDATE messaging.optout_event SET detected = 'provider'")
        .execute(&mut *db)
        .await;
    assert!(is_permission_denied(&update), "append-only for the app");
    let delete = sqlx::query("DELETE FROM messaging.optout_event")
        .execute(&mut *db)
        .await;
    assert!(is_permission_denied(&delete), "kept for the CON-022 period");
    let exported = "SELECT count(*) FROM core.task \
                    WHERE payload::text ~ 'word:stop|en.remove_me|optout_event'";
    assert_eq!(count(&pool, exported).await, 0, "never exported (CON-023)");
}
