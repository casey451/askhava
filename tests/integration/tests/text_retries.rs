//! Text retries, the email outbox and email-provider webhooks on fresh databases (TD-TXT-05,
//! TD-OPS-14, TD-SEC-27, TD-CON-28; TXT-014, TXT-015, INT-023, OPS-038, SEC-050, SEC-051, CON-028,
//! CON-035). A carrier that always fails, a fake email provider that keeps one email per key (and
//! can refuse for a sending limit), and Svix-signed webhooks under a fake secret. All data is fake.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, Delivery, Email, EmailAdapter, IdempotencyKey, SentText, SmsAdapter,
    VerifiedWebhook, WebhookVendor, verify_webhook,
};
use askhava_messaging::api::{
    Audience, CALL_BACK_ALERT, COMPLAINT_ALERT, DailyLimit, EmailClass, EmailError, EmailEvent,
    EmailSent, EmailTo, GaveUp, Language, MailboxKind, NO_RUSH_CALL_BACK, NewEmail, NewText,
    Notice, OutboxError, PostalAddress, Reach, Recipient, Relation, RevokeChannel, Step, To,
    UpdateChannel, Urgency, WEBHOOK_GRACE, WEBHOOKS_SILENT_ALERT, check_webhook_silence, email_log,
    email_status, enqueue, give_up, next_reset, queue_email, receive_email_event, record_failure,
    send_checked, send_email, set_update_channel, suppress, update_channel,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const FROM: &str = "+19285550100";
const PHONE: &str = "928-555-0142";
/// The fake Svix signing key the test webhooks are signed with.
const HOOK_KEY: &[u8] = b"fake-svix-key-for-tests";

/// A carrier that never delivers.
struct DeadCarrier;

impl SmsAdapter for DeadCarrier {
    async fn send_text(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _from: &str,
        _body: &str,
    ) -> Result<SentText, AdapterError> {
        Err(AdapterError::NotSent)
    }
}

/// A fake email provider: one email per key; refuses `limit_refusals` sends with 429 first.
#[derive(Clone, Default)]
struct FakeMail {
    sent: Arc<Mutex<HashMap<String, (String, String)>>>,
    limit_refusals: Arc<Mutex<u32>>,
}

impl EmailAdapter for FakeMail {
    async fn send_email(
        &self,
        key: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let mut refusals = self.limit_refusals.lock().expect("lock");
        if *refusals > 0 {
            *refusals -= 1;
            return Err(AdapterError::Vendor {
                status: 429,
                code: None,
            });
        }
        let mut sent = self.sent.lock().expect("lock");
        let value = (email.to.to_owned(), email.text.to_owned());
        sent.entry(key.as_str().to_owned()).or_insert(value);
        Ok(format!("re_{:08}", sent.len()))
    }

    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

impl FakeMail {
    fn bodies(&self) -> Vec<(String, String)> {
        self.sent.lock().expect("lock").values().cloned().collect()
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

fn postal() -> PostalAddress {
    PostalAddress::new(
        "PO Box 1234, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    )
    .expect("box")
}

fn limit() -> DailyLimit {
    DailyLimit::new(100, 90).expect("limit")
}

fn at(s: &str) -> Timestamp {
    s.parse().expect("time")
}

fn homeowner(id: Uuid) -> Recipient<'static> {
    Recipient {
        id,
        audience: Audience::Homeowner,
        language: Language::En,
        manage_url: "https://askhava.com/q/fake-token",
    }
}

async fn contact(
    db: &mut PgConnection,
    vault: &Vault,
    (phone, email): (&str, Option<&str>),
) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Ada Example".to_owned())),
        phone: Some(SecretString::from(phone.to_owned())),
        email: email.map(|e| SecretString::from(e.to_owned())),
        address: None,
        onsite_contact: None,
    };
    store_contact(db, vault, &new).await.expect("contact")
}

/// Queues `template` to `her`, then fails the send and every retry, checking each step.
async fn fail_four_times(db: &mut Db, vault: &Vault, (her, template): (Uuid, &str)) -> Uuid {
    let text = NewText {
        template_id: template,
        subject: &[Uuid::from_u128(0x1042)],
        recipient: her,
        relation: Relation::OpenRequest,
        body: "Ask Hava: Your first quote for request 1042 is in. See it here: https://askhava.com/q/fake",
    };
    let (id, _) = enqueue(db, &secret(), &text).await.expect("queued");
    let first = at("2026-11-02T17:00:00Z");
    let to = To::Homeowner {
        vault,
        relation: Relation::OpenRequest,
    };
    let mut now = first;
    for after in [10, 20, 30, 0] {
        let sent = send_checked(db, (&secret(), &DeadCarrier, vault), id, (to, FROM), now).await;
        assert_eq!(sent.err(), Some(OutboxError::Carrier));
        let step = record_failure(db, id, now).await.expect("recorded");
        if after == 0 {
            assert_eq!(step, Step::GiveUp);
        } else {
            now = first + SignedDuration::from_mins(after);
            assert_eq!(step, Step::RetryAt(now));
        }
    }
    id
}

fn reach(vault: &Vault, urgency: Urgency) -> Reach<'_> {
    Reach::Homeowner {
        vault,
        urgency,
        no_rush: NO_RUSH_CALL_BACK,
    }
}

/// TD-TXT-05, TXT-014: an important text failing 3 retries over 30 minutes is emailed once and
/// shown in the portal; the text keeps no body and is never claimed again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_05_important_text_retried_then_emailed_and_in_the_portal(pool: PgPool) {
    let (vault, mut db) = (vault(), pool.acquire().await.expect("db"));
    let her = contact(&mut db, &vault, (PHONE, Some("ada@example.com"))).await;
    let id = fail_four_times(&mut db, &vault, (her, "first_quote")).await;
    let now = at("2026-11-02T17:30:00Z");
    let keys = (&secret(), &postal());
    let gave = give_up(
        &mut db,
        keys,
        (id, reach(&vault, Urgency::NoRush), &homeowner(her)),
        now,
    )
    .await;
    let Ok(Some(GaveUp::Emailed(email))) = gave else {
        panic!("not emailed: {gave:?}")
    };
    let again = give_up(
        &mut db,
        keys,
        (id, reach(&vault, Urgency::NoRush), &homeowner(her)),
        now,
    )
    .await;
    assert_eq!(again, Ok(None), "given up once");
    let (status, body): (String, Option<String>) =
        sqlx::query_as("SELECT status, body FROM messaging.outbox WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *db)
            .await
            .expect("row");
    assert_eq!((status.as_str(), body), ("failed", None));
    let shown: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.portal_notice WHERE recipient_id = $1")
            .bind(her)
            .fetch_one(&mut *db)
            .await
            .expect("portal");
    assert_eq!(shown, 1, "shown in the portal");
    let mail = FakeMail::default();
    let first = send_email(
        &mut db,
        (&secret(), &mail, limit()),
        email,
        EmailTo::Homeowner(&vault),
        now,
    )
    .await;
    assert!(matches!(first, Ok(EmailSent::Sent(_))), "{first:?}");
    let second = send_email(
        &mut db,
        (&secret(), &mail, limit()),
        email,
        EmailTo::Homeowner(&vault),
        now,
    )
    .await;
    assert_eq!(second, Ok(EmailSent::AlreadySent));
    let bodies = mail.bodies();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].0, "ada@example.com", "her vault address");
    assert!(
        bodies[0]
            .1
            .contains("Your first quote for request 1042 is in.")
    );
    assert!(bodies[0].1.contains("Manage email notices"));
}

/// TD-TXT-05, TXT-014: an email recipient whose audience is not the text's kind is refused and
/// changes nothing, so a homeowner's failed text is never emailed with a business footer; the
/// matching recipient then gives it up as usual.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_05_give_up_refuses_a_recipient_of_the_other_kind(pool: PgPool) {
    let (vault, mut db) = (vault(), pool.acquire().await.expect("db"));
    let her = contact(&mut db, &vault, (PHONE, Some("ada@example.com"))).await;
    let id = fail_four_times(&mut db, &vault, (her, "first_quote")).await;
    let now = at("2026-11-02T17:30:00Z");
    let keys = (&secret(), &postal());
    let business = Recipient {
        audience: Audience::Business,
        ..homeowner(her)
    };
    let wrong = give_up(
        &mut db,
        keys,
        (id, reach(&vault, Urgency::NoRush), &business),
        now,
    )
    .await;
    assert_eq!(wrong, Err(OutboxError::RecipientKind));
    let (status, emails): (String, i64) = sqlx::query_as(
        "SELECT o.status, (SELECT count(*) FROM messaging.email_outbox) \
         FROM messaging.outbox o WHERE o.id = $1",
    )
    .bind(id)
    .fetch_one(&mut *db)
    .await
    .expect("row");
    assert_ne!(status, "failed", "not given up");
    assert_eq!(emails, 0, "nothing emailed");
    let right = give_up(
        &mut db,
        keys,
        (id, reach(&vault, Urgency::NoRush), &homeowner(her)),
        now,
    )
    .await;
    assert!(matches!(right, Ok(Some(GaveUp::Emailed(_)))), "{right:?}");
}

/// TD-TXT-05, TXT-015, INT-023: 3 failed retries with no email on file make a call-back alert,
/// due in 24 hours for an urgent (or 911-list) job and in 4 days for "No rush"; nothing is emailed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_05_no_email_on_file_becomes_a_call_back(pool: PgPool) {
    let (vault, mut db) = (vault(), pool.acquire().await.expect("db"));
    let now = at("2026-11-02T17:30:00Z");
    let cases = [
        (
            Urgency::Urgent,
            "first_quote",
            "928-555-0143",
            at("2026-11-03T17:30:00Z"),
            "high",
        ),
        (
            Urgency::NoRush,
            "request_out",
            "928-555-0144",
            at("2026-11-06T17:30:00Z"),
            "normal",
        ),
    ];
    for (urgency, template, phone, due, severity) in cases {
        let her = contact(&mut db, &vault, (phone, None)).await;
        let id = fail_four_times(&mut db, &vault, (her, template)).await;
        let gave = give_up(
            &mut db,
            (&secret(), &postal()),
            (id, reach(&vault, urgency), &homeowner(her)),
            now,
        )
        .await;
        assert_eq!(gave, Ok(Some(GaveUp::CallBack(due))), "{urgency:?}");
        let alert: String = sqlx::query_scalar(
            "SELECT severity FROM ops.alert WHERE kind = $1 AND subject_id = $2",
        )
        .bind(CALL_BACK_ALERT)
        .bind(id)
        .fetch_one(&mut *db)
        .await
        .expect("alert");
        assert_eq!(alert, severity);
    }
    let emails: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.email_outbox")
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(emails, 0);
}

/// TD-TXT-05 (CON-028) and TD-CON-28 (CON-035): after an opt-out by any channel, the
/// confirmation and the "still open" line go by email when an address is on file, never by text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_28_opt_out_confirmed_and_still_open_line_by_email(pool: PgPool) {
    let (vault, mut db) = (vault(), pool.acquire().await.expect("db"));
    let her = contact(&mut db, &vault, (PHONE, Some("ada@example.com"))).await;
    let now = at("2026-11-02T18:00:00Z");
    let listed = suppress(
        &mut db,
        &vault,
        PHONE,
        (RevokeChannel::Portal, "portal"),
        now,
    )
    .await;
    assert_eq!(listed, Ok(true));
    let mail = FakeMail::default();
    for (template, notice) in [
        ("opt_out_email", Notice::OptedOut),
        ("still_open_email", Notice::StillOpen(1042)),
    ] {
        let email = NewEmail {
            template_id: template,
            subject: &[Uuid::from_u128(9)],
            class: EmailClass::Notice,
            notice,
        };
        let (id, _) = queue_email(
            &mut db,
            (&secret(), &postal()),
            &homeowner(her),
            &email,
            now,
        )
        .await
        .expect("queued");
        let sent = send_email(
            &mut db,
            (&secret(), &mail, limit()),
            id,
            EmailTo::Homeowner(&vault),
            now,
        )
        .await;
        assert!(matches!(sent, Ok(EmailSent::Sent(_))), "{sent:?}");
    }
    let bodies: Vec<String> = mail.bodies().into_iter().map(|(_, b)| b).collect();
    assert!(
        bodies
            .iter()
            .any(|b| b.contains("You won't get more texts or automated calls"))
    );
    assert!(
        bodies
            .iter()
            .any(|b| b.contains("Your request 1042 is still open. Close it on your quote page."))
    );
    let texts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1")
            .bind(her)
            .fetch_one(&mut *db)
            .await
            .expect("count");
    assert_eq!(texts, 0, "no text after the opt-out");
}

/// Marks `n` fake emails sent at `at`, filling the day's count.
async fn fill_day(db: &mut PgConnection, n: i32, at: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at) \
         SELECT lpad(to_hex(g), 64, '0'), 'job_card', gen_random_uuid(), 'business', '{}', \
         'notice', 'sent', $2, 're_fill_' || g, $2, $2 FROM generate_series(1, $1) g",
    )
    .bind(n)
    .bind(jiff_sqlx::ToSqlx::to_sqlx(at))
    .execute(db)
    .await
    .expect("filled");
}

async fn queue_business(
    db: &mut PgConnection,
    (template, class): (&str, EmailClass),
    now: Timestamp,
) -> Uuid {
    let to = Recipient {
        id: Uuid::from_u128(0xb12),
        audience: Audience::Business,
        language: Language::En,
        manage_url: "https://askhava.com/b",
    };
    let email = NewEmail {
        template_id: template,
        subject: &[Uuid::from_u128(3)],
        class,
        notice: Notice::StillOpen(1),
    };
    queue_email(db, (&secret(), &postal()), &to, &email, now)
        .await
        .expect("queued")
        .0
}

/// TD-OPS-14, OPS-038: at 91 of 100 the open-cards, missed-cards and digest emails wait for the
/// reset while a job card, a pick notice, a reminder and a receipt still go; after the reset the
/// held ones go, and nothing is dropped.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_14_bulk_waits_for_the_reset_notices_still_go(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = at("2026-11-02T20:00:00Z");
    fill_day(&mut db, 91, now).await;
    let mail = FakeMail::default();
    let to = EmailTo::Business("owner@example.com");
    for template in ["job_card", "pick_notice", "confirm_reminder", "receipt"] {
        let id = queue_business(&mut db, (template, EmailClass::Notice), now).await;
        let sent = send_email(&mut db, (&secret(), &mail, limit()), id, to, now).await;
        assert!(
            matches!(sent, Ok(EmailSent::Sent(_))),
            "{template}: {sent:?}"
        );
    }
    let mut held = Vec::new();
    for template in ["open_cards", "missed_cards", "weekly_digest"] {
        let id = queue_business(&mut db, (template, EmailClass::Bulk), now).await;
        let sent = send_email(&mut db, (&secret(), &mail, limit()), id, to, now).await;
        assert_eq!(sent, Ok(EmailSent::Waiting(next_reset(now))), "{template}");
        assert_eq!(
            email_log(&mut db, id)
                .await
                .expect("log")
                .waiting_for
                .as_deref(),
            Some("daily_limit")
        );
        held.push(id);
    }
    let after = next_reset(now);
    for id in held {
        let sent = send_email(&mut db, (&secret(), &mail, limit()), id, to, after).await;
        assert!(matches!(sent, Ok(EmailSent::Sent(_))), "{sent:?}");
    }
    assert_eq!(mail.bodies().len(), 7, "nothing dropped");
}

/// TD-OPS-14, OPS-038, D-381: a provider refusal for the limit queues the email, which goes after
/// the reset; the notice log shows the wait, and its sent time is the real send.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_14_a_limit_refusal_waits_and_is_never_dropped(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = at("2026-11-02T20:00:00Z");
    fill_day(&mut db, 100, now).await;
    let mail = FakeMail::default();
    *mail.limit_refusals.lock().expect("lock") = 1;
    let to = EmailTo::Business("owner@example.com");
    let id = queue_business(&mut db, ("pick_notice", EmailClass::Notice), now).await;
    let refused = send_email(&mut db, (&secret(), &mail, limit()), id, to, now).await;
    assert_eq!(refused, Ok(EmailSent::Waiting(next_reset(now))));
    let early = send_email(&mut db, (&secret(), &mail, limit()), id, to, now).await;
    assert_eq!(
        early,
        Ok(EmailSent::Waiting(next_reset(now))),
        "waits until the reset"
    );
    let after = next_reset(now) + SignedDuration::from_mins(5);
    let sent = send_email(&mut db, (&secret(), &mail, limit()), id, to, after).await;
    assert!(matches!(sent, Ok(EmailSent::Sent(_))), "{sent:?}");
    let log = email_log(&mut db, id).await.expect("log");
    assert_eq!(
        (log.waiting_for.as_deref(), log.sent_at),
        (Some("provider_limit"), Some(after))
    );
}

/// A Resend event `body` with Svix message `id`, signed at `now` and passed through the check.
fn hook(id: &str, body: &str, now: i64) -> VerifiedWebhook {
    let mut mac = Hmac::<Sha256>::new_from_slice(HOOK_KEY).expect("mac");
    mac.update(format!("{id}.{now}.{body}").as_bytes());
    let signature = format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()));
    let stamp = now.to_string();
    let headers = [
        ("svix-id", id),
        ("svix-timestamp", stamp.as_str()),
        ("svix-signature", signature.as_str()),
    ];
    let delivery = Delivery {
        url: "https://askhava.com/webhooks/resend",
        body: body.as_bytes(),
        headers: &headers,
    };
    verify_webhook(
        WebhookVendor::Resend,
        &SecretString::from(format!("whsec_{}", STANDARD.encode(HOOK_KEY))),
        delivery,
        now,
    )
    .expect("signed")
}

fn event(kind: &str, at: &str) -> String {
    format!(r#"{{"type":"{kind}","created_at":"{at}","data":{{"email_id":"re_00000001"}}}}"#)
}

/// TD-SEC-27, SEC-050, SEC-051: an email webhook is stored once (a replayed ID changes nothing),
/// applied in event-time order, a complaint raises the alert, a failed write is not acknowledged,
/// and 24 hours of sends with no webhook raises the silence alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_27_email_webhooks_stored_once_in_event_order(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = at("2026-11-02T20:00:00Z");
    let secs = now.as_second();
    let delivered = hook(
        "msg_1",
        &event("email.delivered", "2026-11-02T19:00:02Z"),
        secs,
    );
    assert_eq!(
        receive_email_event(&mut db, &delivered, now).await,
        Ok(EmailEvent::Recorded)
    );
    assert_eq!(
        receive_email_event(&mut db, &delivered, now).await,
        Ok(EmailEvent::Repeat)
    );
    let late = hook("msg_0", &event("email.sent", "2026-11-02T19:00:01Z"), secs);
    assert_eq!(
        receive_email_event(&mut db, &late, now).await,
        Ok(EmailEvent::Recorded)
    );
    assert_eq!(
        email_status(&mut db, "re_00000001")
            .await
            .expect("status")
            .as_deref(),
        Some("email.delivered")
    );
    let complaint = hook(
        "msg_2",
        &event("email.complained", "2026-11-02T19:30:00Z"),
        secs,
    );
    assert_eq!(
        receive_email_event(&mut db, &complaint, now).await,
        Ok(EmailEvent::Recorded)
    );
    let alerts: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(COMPLAINT_ALERT)
        .fetch_one(&mut *db)
        .await
        .expect("alerts");
    assert_eq!(alerts, 1);
    let bad = hook("msg_3", "not json", secs);
    assert_eq!(
        receive_email_event(&mut db, &bad, now).await,
        Err(EmailError::BadWebhook)
    );
    not_email_events_never_reach_the_table(&mut db, secs, now).await;
    sqlx::query("ALTER TABLE messaging.email_event RENAME TO email_event_gone")
        .execute(&mut *db)
        .await
        .expect("plant");
    let lost = hook(
        "msg_4",
        &event("email.bounced", "2026-11-02T19:40:00Z"),
        secs,
    );
    assert_eq!(
        receive_email_event(&mut db, &lost, now).await,
        Err(EmailError::Database),
        "500, not acknowledged"
    );
    sqlx::query("ALTER TABLE messaging.email_event_gone RENAME TO email_event")
        .execute(&mut *db)
        .await
        .expect("restore");
    assert_eq!(
        receive_email_event(&mut db, &lost, now).await,
        Ok(EmailEvent::Recorded),
        "the resend is stored"
    );
    fill_day(&mut db, 3, now).await;
    let later = now + SignedDuration::from_hours(25);
    fill_day_after(&mut db, ('e', later - SignedDuration::from_mins(10))).await;
    assert_eq!(
        check_webhook_silence(&mut db, now).await,
        Ok(false),
        "webhooks came"
    );
    assert_eq!(
        check_webhook_silence(&mut db, later).await,
        Ok(false),
        "an email sent minutes ago has had no time for a webhook"
    );
    let lapsed = later - WEBHOOK_GRACE - SignedDuration::from_mins(1);
    fill_day_after(&mut db, ('f', lapsed)).await;
    assert_eq!(
        check_webhook_silence(&mut db, later).await,
        Ok(true),
        "sends with time for a webhook, none for 24 hours"
    );
    let silent: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(WEBHOOKS_SILENT_ALERT)
        .fetch_one(&mut *db)
        .await
        .expect("alerts");
    assert_eq!(silent, 1);
}

/// SEC-051: a verified event outside `email.*` is acknowledged and stored nowhere, and an
/// `email.*` event with an empty email ID or a malformed type is refused, both before any write
/// (so the table's checks never roll back into a 500 the provider resends forever).
async fn not_email_events_never_reach_the_table(db: &mut PgConnection, secs: i64, now: Timestamp) {
    let before: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM messaging.email_event) \
         + (SELECT count(*) FROM messaging.webhook_event WHERE source = 'resend')",
    )
    .fetch_one(&mut *db)
    .await
    .expect("count");
    let other =
        r#"{"type":"contact.created","created_at":"2026-11-02T19:50:00Z","data":{"id":"c_1"}}"#;
    let ignored = hook("msg_5", other, secs);
    assert_eq!(
        receive_email_event(db, &ignored, now).await,
        Ok(EmailEvent::Ignored)
    );
    let no_id =
        r#"{"type":"email.delivered","created_at":"2026-11-02T19:50:00Z","data":{"email_id":""}}"#;
    let shapes = [
        ("msg_6", no_id.to_owned()),
        (
            "msg_7",
            event("email.Delivered-Now", "2026-11-02T19:50:00Z"),
        ),
    ];
    for (id, body) in shapes {
        let bad = hook(id, &body, secs);
        assert_eq!(
            receive_email_event(db, &bad, now).await,
            Err(EmailError::BadWebhook),
            "{id}"
        );
    }
    let after: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM messaging.email_event) \
         + (SELECT count(*) FROM messaging.webhook_event WHERE source = 'resend')",
    )
    .fetch_one(&mut *db)
    .await
    .expect("count");
    assert_eq!(after, before, "nothing stored");
}

/// One more sent email at `at`, under a key of 64 `key` letters that `fill_day` does not use.
async fn fill_day_after(db: &mut PgConnection, (key, at): (char, Timestamp)) {
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at) \
         VALUES (repeat($2, 64), 'job_card', gen_random_uuid(), 'business', '{}', 'notice', \
         'sent', $1, 're_later_' || $2, $1, $1)",
    )
    .bind(jiff_sqlx::ToSqlx::to_sqlx(at))
    .bind(key.to_string())
    .execute(db)
    .await
    .expect("sent");
}

/// TXT-018: the update channel is text until she chooses email or both.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_update_channel_is_text_until_chosen(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let her = Uuid::from_u128(0x5e);
    assert_eq!(update_channel(&mut db, her).await, Ok(UpdateChannel::Text));
    let now = at("2026-11-02T18:00:00Z");
    for choice in [
        UpdateChannel::Email,
        UpdateChannel::Both,
        UpdateChannel::Text,
    ] {
        set_update_channel(&mut db, her, choice, now)
            .await
            .expect("set");
        assert_eq!(update_channel(&mut db, her).await, Ok(choice));
    }
}
