//! Two first texts from one new phone, and a business phone's STOP and stray texts, through
//! Twilio's inbound route on a real database (TD-TXT-32, TD-BIZ-39; TXT-012, PRIV-001, CON-008,
//! CON-019, CON-020, BIZ-017, BIZ-032; D-1715, D-1751, T256).
//!
//! The route is `app/server/src/inbound_text.rs`, compiled in with `#[path]` as in
//! `inbound_text_route.rs`. The race is forced, not slept on: a third connection holds
//! `privacy.contact_index` in SHARE mode, which lets both deliveries record their inbox row and
//! find no contact but holds each at its blind-index insert; once `pg_locks` shows both waiting
//! (the barrier) the lock is released and one contact wins the unique index while the other
//! delivery gets `PhoneTaken`. A test-only trigger that raises `unique_violation` stands in for a
//! winner that cannot be found again. All data is fake.

use std::sync::Arc;
use std::time::Duration;

use askhava_businesses::api::PHONE_STOPPED_KIND;
use askhava_core::api::{Environment, ManualClock};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::sender_add;
use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::Timestamp;
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{AssertSqlSafe, PgPool};
use tower::ServiceExt;

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HOME: &str = "+19285550143";
const SHOP: &str = "+19285550151";
const SHOP_STAFF: &str = "+19285550152";
const ASK_HAVA: &str = "+19285550100";

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let at: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(at)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from("test-only-id-secret-not-a-real-key".to_owned()),
        vault: Vault::new(&key(0x53), &key(0x63), &url).expect("vault"),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

fn sid(n: u32) -> String {
    format!("SM{n:032}")
}

/// POSTs one inbound text signed by the fake token over `BASE` + the path; the status.
async fn text(h: Arc<Webhooks>, n: u32, from: &str, words: &str) -> StatusCode {
    let sid = sid(n);
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", from),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    let encode = |v: &str| -> String {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let signature = STANDARD.encode(mac.finalize().into_bytes());
    let request = Request::post(INBOUND_PATH)
        .header("X-Twilio-Signature", signature)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    routes(h).oneshot(request).await.expect("answer").status()
}

async fn count(pool: &PgPool, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn total(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .expect("total")
}

/// Sends texts `1` and `2` from [`HOME`] at once, both held at the blind-index insert after
/// their absent-contact lookups until both wait (the barrier, failing after 30 seconds rather
/// than hanging), then released together.
async fn race(pool: &PgPool, h: &Arc<Webhooks>, first: &'static str, second: &'static str) {
    let mut lock = pool.begin().await.expect("lock connection");
    sqlx::query("LOCK TABLE privacy.contact_index IN SHARE MODE")
        .execute(&mut *lock)
        .await
        .expect("lock");
    let a = tokio::spawn(text(Arc::clone(h), 1, HOME, first));
    let b = tokio::spawn(text(Arc::clone(h), 2, HOME, second));
    let waiting = "SELECT count(*) FROM pg_locks \
                   WHERE relation = $1::regclass AND NOT granted";
    let barrier = async {
        while count(pool, waiting, "privacy.contact_index").await < 2 {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(30), barrier)
        .await
        .expect("both deliveries wait at the blind-index write");
    lock.commit().await.expect("release");
    assert_eq!(a.await.expect("first"), StatusCode::OK);
    assert_eq!(b.await.expect("second"), StatusCode::OK);
}

/// How often the text with MessageSid `n` reached intake: it opened the request or was heard in it.
async fn reached_intake(pool: &PgPool, n: u32) -> i64 {
    let sql = "SELECT (SELECT count(*) FROM intake.conversation WHERE first_inbox_id = i.id) \
               + (SELECT count(*) FROM intake.intake_answer WHERE inbox_id = i.id) \
               FROM messaging.inbox i WHERE i.message_sid = $1";
    count(pool, sql, &sid(n)).await
}

/// TD-TXT-32 (TXT-012, PRIV-001): two first texts racing for one new phone make one vault
/// contact, no orphan, and both reach intake once; each MessageSid again changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_32_two_first_texts_both_reach_intake(pool: PgPool) {
    let h = hooks(&pool);
    race(
        &pool,
        &h,
        "my kitchen sink is leaking",
        "it drips under the cabinet",
    )
    .await;
    assert_eq!(total(&pool, "privacy.contact").await, 1, "one contact");
    assert_eq!(total(&pool, "privacy.contact_index").await, 1);
    assert_eq!(total(&pool, "messaging.inbox").await, 2);
    assert_eq!(total(&pool, "intake.conversation").await, 1);
    assert_eq!(reached_intake(&pool, 1).await, 1, "first text once");
    assert_eq!(reached_intake(&pool, 2).await, 1, "second text once");
    let tables = [
        "privacy.contact",
        "privacy.contact_index",
        "intake.conversation",
        "messaging.inbox",
        "intake.intake_answer",
        "messaging.outbox",
    ];
    let mut before = Vec::new();
    for table in tables {
        before.push(total(&pool, table).await);
    }
    for n in [1, 2] {
        assert_eq!(text(Arc::clone(&h), n, HOME, "again").await, StatusCode::OK);
    }
    for (table, was) in tables.into_iter().zip(before) {
        assert_eq!(total(&pool, table).await, was, "{table} after the resends");
    }
}

/// TD-TXT-32 (CON-008): the second of a racing pair being a sole-word STOP still opts the
/// number out, with one contact and both inbox rows.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_32_a_racing_stop_still_opts_out(pool: PgPool) {
    let h = hooks(&pool);
    race(&pool, &h, "my kitchen sink is leaking", "STOP").await;
    assert_eq!(total(&pool, "privacy.contact").await, 1);
    assert_eq!(total(&pool, "messaging.inbox").await, 2);
    let stops = "SELECT count(*) FROM messaging.optout_event WHERE action = $1";
    assert_eq!(count(&pool, stops, "opt_out").await, 1);
    assert!(total(&pool, "messaging.suppression").await > 0);
}

/// TD-TXT-32 (TXT-012, D-1751): `PhoneTaken` with no contact found again answers 500 and keeps
/// no inbox row or contact, so the resend of that MessageSid is handled.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_32_a_lost_winner_rolls_back_and_the_resend_is_handled(pool: PgPool) {
    let h = hooks(&pool);
    let taken = "CREATE FUNCTION public.t256_taken() RETURNS trigger LANGUAGE plpgsql AS \
                 $$ BEGIN RAISE EXCEPTION 'taken' USING ERRCODE = 'unique_violation'; END $$";
    let trigger = "CREATE TRIGGER t256_taken BEFORE INSERT ON privacy.contact_index \
                   FOR EACH ROW EXECUTE FUNCTION public.t256_taken()";
    for sql in [taken, trigger] {
        sqlx::query(sql).execute(&pool).await.expect("trigger");
    }
    let words = "my kitchen sink is leaking";
    assert_eq!(
        text(Arc::clone(&h), 5, HOME, words).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(total(&pool, "messaging.inbox").await, 0);
    assert_eq!(total(&pool, "privacy.contact").await, 0);
    sqlx::query("DROP TRIGGER t256_taken ON privacy.contact_index")
        .execute(&pool)
        .await
        .expect("drop");
    assert_eq!(text(Arc::clone(&h), 5, HOME, words).await, StatusCode::OK);
    assert_eq!(total(&pool, "privacy.contact").await, 1);
    assert_eq!(reached_intake(&pool, 5).await, 1);
}

async fn link(pool: &PgPool, h: &Webhooks, business: Uuid, phone: &str) {
    let mut db = pool.acquire().await.expect("connection");
    sender_add(&mut db, &h.vault, (business, Uuid::new_v4()), phone)
        .await
        .expect("linked phone");
}

/// TD-BIZ-39 (BIZ-017, CON-008, D-1715): STOP from a linked business phone queues one
/// `phone_stopped` owner email (a resend changes nothing) and makes no homeowner contact.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_39_a_business_stop_reaches_businesses_once(pool: PgPool) {
    let h = hooks(&pool);
    let business = Uuid::from_u128(0x7256);
    link(&pool, &h, business, SHOP).await;
    for _ in 0..2 {
        assert_eq!(text(Arc::clone(&h), 1, SHOP, "STOP").await, StatusCode::OK);
    }
    let tasks = "SELECT count(*) FROM core.task WHERE kind = $1";
    assert_eq!(count(&pool, tasks, PHONE_STOPPED_KIND).await, 1);
    let theirs = "SELECT count(*) FROM core.task WHERE payload->>'business' = $1";
    assert_eq!(count(&pool, theirs, &business.to_string()).await, 1);
    assert_eq!(total(&pool, "privacy.contact").await, 0, "no homeowner");
    assert_eq!(total(&pool, "intake.homeowner_pause").await, 0);
    let phone = normalize_phone(SHOP).expect("fake phone");
    let listed = "SELECT count(*) FROM messaging.suppression WHERE digest = $1";
    for (_, digest) in h.vault.blind_index(IndexPurpose::HomeownerPhone, &phone) {
        let n: i64 = sqlx::query_scalar(listed)
            .bind(digest.as_slice())
            .fetch_one(&pool)
            .await
            .expect("listed");
        assert_eq!(n, 1, "the business phone is on the suppression list");
    }
}

/// TD-BIZ-39 (BIZ-032, D-1715): a business phone's text for which `auto_reply_due` holds queues
/// one `business_auto_reply`; a command word, HELP and a resend queue none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_39_auto_reply_only_when_due(pool: PgPool) {
    let h = hooks(&pool);
    let business = Uuid::from_u128(0x7257);
    link(&pool, &h, business, SHOP_STAFF).await;
    let replies = "SELECT count(*) FROM messaging.outbox WHERE template_id = $1";
    for (n, words) in [(1, "CONFIRM 1042"), (2, "HELP"), (3, "BUSY 5")] {
        assert_eq!(
            text(Arc::clone(&h), n, SHOP_STAFF, words).await,
            StatusCode::OK
        );
        assert_eq!(
            count(&pool, replies, "business_auto_reply").await,
            0,
            "{words}"
        );
    }
    for _ in 0..2 {
        let words = "Yes, why was I charged?";
        assert_eq!(
            text(Arc::clone(&h), 4, SHOP_STAFF, words).await,
            StatusCode::OK
        );
    }
    assert_eq!(count(&pool, replies, "business_auto_reply").await, 1);
    let row = "SELECT recipient_kind, recipient_id FROM messaging.outbox WHERE template_id = $1";
    let (kind, recipient): (String, Uuid) = sqlx::query_as(row)
        .bind("business_auto_reply")
        .fetch_one(&pool)
        .await
        .expect("reply row");
    assert_eq!(kind, "business");
    assert_eq!(
        recipient, business,
        "D-2042: the business ID, not the line ID"
    );
    assert_eq!(total(&pool, "privacy.contact").await, 0);
}
