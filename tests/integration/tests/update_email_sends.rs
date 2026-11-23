//! TD-TXT-39 (TXT-018, SEC-028, CON-035, TXT-027, TXT-030, TXT-017; T730, D-4727, D-5340,
//! D-5442, D-5533, D-5585, D-5617): quote updates by email for a homeowner who chose it, in
//! PostgreSQL with a recording stand-in for the email provider and Svix-signed webhooks under a
//! fake secret. Fake data only.
//!
//! T729's quote-page choice "Get updates by email instead" is driven through `portals`' routes;
//! then a `first_quote` update queues one update email (her quote page, the SEC-028 footer, how
//! to stop them and, while unconfirmed, the "Confirm this email" link that opens the page's
//! confirm button) and no text; with both a text and an email; with text a text only; the 911
//! text, the who's-calling and missed-call texts, a direct reply and a text asking for a text
//! answer stay text; an emergency job's `first_quote` is an email only; a hard bounce sets text
//! and marks the address, and the next update is a text; `intake::quote_update_step` tells an
//! email-only homeowner her second quote by email, and after a bounce by text.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, Delivery, Email, EmailAdapter, IdempotencyKey, VerifiedWebhook, WebhookVendor,
    verify_webhook,
};
use askhava_core::api::{IdSecret, LinkKind, LinkToken, ManualClock, brand, link_token};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, JobTextKeys, Opened, QuoteUpdateStep, TextIn, begin_job, quote_update_step,
    receive_text,
};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, first_quote, job_number, move_to};
use askhava_messaging::api::{
    DailyLimit, EmailEvent, EmailSent, EmailTo, Exemption, MailboxKind, NewText, PostalAddress,
    Relation, TEXT_ONLY_UPDATES, UPDATE_TEMPLATES, UpdateChannel, enqueue, enqueue_exempt,
    receive_email_event, send_email, set_update_channel, set_update_footer, update_channel,
};
use askhava_ops::api::Actor;
use askhava_portals::api::{UPDATES_ROUTE, quote_page_routes_with};
use askhava_privacy::api::{
    Field, NewContact, Purpose, Reader, UpdateAddress, UpdateStatus, Vault, read_contact,
    store_contact, store_update_address, update_status,
};
use askhava_quotes::api::UPDATE_TEXTS;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha2::Sha256;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const ID_KEY: &str = "fake-id-secret-for-tests-00000730";
const HOOK_KEY: &[u8] = b"fake-svix-key-for-t730-tests";
const ADDRESS: &str = "pat.example@mail.test";
/// 10:00 a.m. Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn at(secs: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("day") + SignedDuration::from_secs(secs)
}

fn id_secret() -> IdSecret {
    IdSecret::new(1, key()).expect("secret")
}

fn key() -> SecretString {
    SecretString::from(ID_KEY.to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x73), &key(0x30), &url).expect("vault")
}

/// The update emails' fake footer postal line, set once for this test process.
fn footer() {
    let postal = PostalAddress::new("PO Box 0730, Sample City, AZ 86400", MailboxKind::PoBox);
    set_update_footer(postal.expect("postal"));
}

fn app(pool: &PgPool) -> Router {
    let clock = Arc::new(ManualClock::new(at(60)));
    quote_page_routes_with(pool.clone(), clock, Some((vault(), id_secret())))
}

/// Fake homeowner `n` with a fake 555 phone, a request and an open job; its token, her contact
/// and the job.
async fn open_job(db: &mut PgConnection, n: u128) -> (LinkToken, Uuid, Uuid) {
    let phone = format!("+1928555{:04}", 100 + n % 100);
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone)),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: "my kitchen sink is leaking under the cabinet",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x730_0000 + n),
        contact,
        inbound,
        received_at: at(0),
    };
    let handled = receive_text(db, &key(), &msg, Settings::default()).await;
    let start = JobStart {
        conversation: handled.expect("open").conversation.expect("request"),
        words: "my kitchen sink is leaking",
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, at(2), Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    job_number(db, job).await.expect("number");
    let token = issue(db, &id_secret(), job, at(2)).await.expect("token");
    (token, contact, job)
}

/// One same-origin request to the keyed quote page routes: status, body and the first cookie.
async fn call(pool: &PgPool, request: Request<Body>) -> (u16, String, String) {
    let response = app(pool).oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf8");
    (status, body, cookie)
}

/// `GET path` with `cookie`.
async fn get(pool: &PgPool, path: &str, cookie: &str) -> (String, String) {
    let request = Request::get(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("request");
    let (status, body, set) = call(pool, request).await;
    assert_eq!(status, 200, "{path}");
    (body, set)
}

/// T729's quote-page choice `choice` (`email` or `both`) with the fake address, end to end.
async fn choose(pool: &PgPool, token: &LinkToken, choice: &str) {
    let (page, cookie) = get(pool, &format!("/q/{}", token.as_str()), "").await;
    assert!(
        page.contains("Get updates by email instead</button>"),
        "{page}"
    );
    let (_, rest) = page.split_once("name=\"csrf\" value=\"").expect("csrf");
    let (csrf, _) = rest.split_once('"').expect("csrf value");
    let form = format!("csrf={csrf}&choice={choice}&email=pat.example%40mail.test&tries=0");
    let request = Request::post(UPDATES_ROUTE.replace("{token}", token.as_str()))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .expect("request");
    let (status, body, _) = call(pool, request).await;
    assert_eq!(status, 200);
    assert!(
        body.contains("We sent a confirm link to that email."),
        "{body}"
    );
}

/// Her quote page's address on the brand's domain.
fn page(token: &LinkToken) -> String {
    format!("https://{}/q/{}", brand().domain(), token.as_str())
}

/// A fake rendered body of `template` that links to her page.
fn body(template: &str, token: &LinkToken) -> String {
    format!(
        "{}: Fake words of {template} for request 1001. See them here: {} Reply LIST.",
        brand().short_name(),
        page(token)
    )
}

/// Queues `template` to `contact` about `subject` through `messaging::enqueue`.
async fn update(
    db: &mut PgConnection,
    (contact, template, subject): (Uuid, &str, &[Uuid]),
    body: &str,
) -> (Uuid, bool) {
    let text = NewText {
        template_id: template,
        subject,
        recipient: contact,
        relation: Relation::OpenRequest,
        body,
    };
    enqueue(db, &key(), &text).await.expect("enqueue")
}

/// How many `template` text rows `contact` has.
async fn texts(pool: &PgPool, contact: Uuid, template: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1 AND template_id = $2",
    )
    .bind(contact)
    .bind(template)
    .fetch_one(pool)
    .await
    .expect("texts")
}

/// Every email row to `contact`: its ID, template and plain part (empty once sent).
async fn emails(pool: &PgPool, contact: Uuid) -> Vec<(Uuid, String, String)> {
    sqlx::query_as(
        "SELECT id, template_id, coalesce(plain, '') FROM messaging.email_outbox \
         WHERE recipient_id = $1 ORDER BY queued_at, id",
    )
    .bind(contact)
    .fetch_all(pool)
    .await
    .expect("emails")
}

async fn status(db: &mut PgConnection, contact: Uuid) -> UpdateStatus {
    update_status(db, contact)
        .await
        .expect("status")
        .expect("an address")
}

/// The email provider stand-in: records each address and answers with a fake ID.
#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<String>>>);

impl EmailAdapter for Rec {
    async fn send_email(
        &self,
        _: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let mut to = self.0.lock().expect("lock");
        to.push(email.to.to_owned());
        Ok(format!("re_t730_{}", to.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// Sends email row `id` through the stand-in at `now`, its queue time set to a minute before
/// (the database's clock queued it); the provider ID.
async fn send(db: &mut PgConnection, mail: &Rec, id: Uuid, now: Timestamp) -> String {
    let queued = now - SignedDuration::from_mins(1);
    sqlx::query("UPDATE messaging.email_outbox SET queued_at = $1, not_before = $1 WHERE id = $2")
        .bind(queued.to_sqlx())
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("fixture: the email's queue time");
    let limit = DailyLimit::new(100, 90).expect("limit");
    let vault = vault();
    let sent = send_email(
        db,
        (&key(), mail, limit),
        id,
        EmailTo::Homeowner(&vault),
        now,
    )
    .await;
    let Ok(EmailSent::Sent(provider)) = sent else {
        panic!("sent: {sent:?}");
    };
    provider
}

/// A Svix-signed Resend `email.bounced` event for `provider`, checked at `now`.
fn bounce(provider: &str, now: Timestamp) -> VerifiedWebhook {
    let id = format!("msg_{provider}");
    let body = format!(
        r#"{{"type":"email.bounced","created_at":"{now}","data":{{"email_id":"{provider}"}}}}"#
    );
    let secs = now.as_second();
    let mut mac = Hmac::<Sha256>::new_from_slice(HOOK_KEY).expect("mac");
    mac.update(format!("{id}.{secs}.{body}").as_bytes());
    let signature = format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()));
    let stamp = secs.to_string();
    let headers = [
        ("svix-id", id.as_str()),
        ("svix-timestamp", stamp.as_str()),
        ("svix-signature", signature.as_str()),
    ];
    let delivery = Delivery {
        url: "https://askhava.com/webhooks/resend",
        body: body.as_bytes(),
        headers: &headers,
    };
    let secret = SecretString::from(format!("whsec_{}", STANDARD.encode(HOOK_KEY)));
    verify_webhook(WebhookVendor::Resend, &secret, delivery, secs).expect("signed")
}

/// D-5533: the two lists are disjoint, together are `UPDATE_TEXTS`' IDs, and hold neither the 911
/// text nor the missed-call text.
#[test]
fn td_txt_39_the_update_lists_split_update_texts() {
    let mut both: Vec<&str> = UPDATE_TEMPLATES
        .iter()
        .chain(&TEXT_ONLY_UPDATES)
        .copied()
        .collect();
    let total = both.len();
    both.sort_unstable();
    both.dedup();
    assert_eq!(both.len(), total, "the lists overlap");
    let mut ids: Vec<&str> = UPDATE_TEXTS.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable();
    assert_eq!(both, ids);
    for never in ["emergency_911", "caller_missed"] {
        assert!(!both.contains(&never), "{never}");
    }
}

/// TXT-018, SEC-028, CON-035, D-5340: the page's email choice, then `first_quote` is one update
/// email (her page, the footer, how to stop, the confirm link that opens the page's button) and
/// no text; a repeat stores nothing new.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_39_email_only_first_quote_is_one_email_and_no_text(pool: PgPool) {
    footer();
    let mut db = pool.acquire().await.expect("db");
    let (token, contact, job) = open_job(&mut db, 1).await;
    choose(&pool, &token, "email").await;
    assert_eq!(
        update_channel(&mut db, contact).await,
        Ok(UpdateChannel::Email)
    );
    let words = body("first_quote", &token);
    let queued = update(&mut db, (contact, "first_quote", &[job]), &words).await;
    assert_eq!(texts(&pool, contact, "first_quote").await, 0, "no text");
    let rows = emails(&pool, contact).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (id, template, plain) = &rows[0];
    assert_eq!(
        (queued, template.as_str()),
        ((*id, true), "update.first_quote")
    );
    let s = status(&mut db, contact).await;
    let confirm = link_token(&id_secret(), LinkKind::Action, s.row, &s.salt);
    let confirm_link = format!("{}?confirm={}", page(&token), confirm.as_str());
    assert!(plain.contains(&words), "{plain}");
    assert!(plain.contains(&format!("Your quote page: {}", page(&token))));
    assert!(plain.contains(&format!("Confirm this email: {confirm_link}")));
    assert!(plain.contains(&format!(
        "To stop update emails, choose updates by text only: {}",
        page(&token)
    )));
    assert!(plain.contains("You get these emails because you asked"));
    assert!(plain.contains("Manage email notices: "));
    assert!(plain.contains("PO Box 0730, Sample City, AZ 86400"));
    let path = format!("/q/{}?confirm={}", token.as_str(), confirm.as_str());
    let (opened, _) = get(&pool, &path, "").await;
    assert!(opened.contains("Confirm this email</button>"), "{opened}");
    let again = update(&mut db, (contact, "first_quote", &[job]), &words).await;
    assert_eq!(again, (*id, false));
    assert_eq!(emails(&pool, contact).await.len(), 1);
}

/// TXT-018: with both a text and an email; with no choice a text only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_39_both_is_a_text_and_an_email_and_text_is_a_text(pool: PgPool) {
    footer();
    let mut db = pool.acquire().await.expect("db");
    let (token, both, job) = open_job(&mut db, 2).await;
    choose(&pool, &token, "both").await;
    let (other_token, text_only, other) = open_job(&mut db, 3).await;
    update(
        &mut db,
        (both, "first_quote", &[job]),
        &body("first_quote", &token),
    )
    .await;
    let words = body("first_quote", &other_token);
    update(&mut db, (text_only, "first_quote", &[other]), &words).await;
    assert_eq!(texts(&pool, both, "first_quote").await, 1);
    assert_eq!(emails(&pool, both).await.len(), 1);
    assert_eq!(texts(&pool, text_only, "first_quote").await, 1);
    assert!(emails(&pool, text_only).await.is_empty());
}

/// TXT-018, TXT-027, TXT-030, PICK-029: with email only, the 911 text, the who's-calling and
/// missed-call texts, a direct reply (`quote_list`) and a text asking for a text answer
/// (`check_in_day3`) go by text and no email is stored; the code path's contact email (PICK-029)
/// never holds her update address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_39_safety_codes_and_replies_stay_text(pool: PgPool) {
    footer();
    let mut db = pool.acquire().await.expect("db");
    let (token, contact, job) = open_job(&mut db, 4).await;
    choose(&pool, &token, "email").await;
    let kinds = [
        "emergency_911",
        "whos_calling",
        "caller_missed",
        "quote_list",
        "check_in_day3",
    ];
    for template in kinds {
        update(
            &mut db,
            (contact, template, &[job]),
            &body(template, &token),
        )
        .await;
        assert_eq!(texts(&pool, contact, template).await, 1, "{template}");
    }
    assert!(emails(&pool, contact).await.is_empty(), "no email");
    let access = (Reader::Messaging, Purpose::Send);
    let own = read_contact(&mut db, &vault(), access, contact, &[Field::Email]).await;
    assert!(
        own.expect("contact").email.is_none(),
        "codes never reach it"
    );
}

/// D-5585: an emergency job's `first_quote` through `enqueue_exempt` is the email only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_39_an_emergency_first_quote_is_the_email_only(pool: PgPool) {
    footer();
    let mut db = pool.acquire().await.expect("db");
    let (token, contact, job) = open_job(&mut db, 5).await;
    choose(&pool, &token, "email").await;
    let words = body("first_quote", &token);
    let text = NewText {
        template_id: "first_quote",
        subject: &[job],
        recipient: contact,
        relation: Relation::OpenRequest,
        body: &words,
    };
    let queued = enqueue_exempt(&mut db, &key(), &text, Exemption::EmergencyJob).await;
    let rows = emails(&pool, contact).await;
    assert_eq!(queued, Ok((rows[0].0, true)));
    assert_eq!(rows.len(), 1);
    assert_eq!(texts(&pool, contact, "first_quote").await, 0);
}

/// D-5442, TXT-018: the update email goes to her update address; its hard bounce sets text and
/// marks the address bounced, and the next update is a text only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_39_a_hard_bounce_sets_text_and_the_next_update_is_a_text(pool: PgPool) {
    footer();
    let mut db = pool.acquire().await.expect("db");
    let (token, contact, job) = open_job(&mut db, 6).await;
    choose(&pool, &token, "email").await;
    let (id, _) = update(
        &mut db,
        (contact, "first_quote", &[job]),
        &body("first_quote", &token),
    )
    .await;
    let mail = Rec::default();
    let provider = send(&mut db, &mail, id, at(120)).await;
    assert_eq!(*mail.0.lock().expect("lock"), vec![ADDRESS.to_owned()]);
    let hook = bounce(&provider, at(180));
    assert_eq!(
        receive_email_event(&mut db, &hook, at(180)).await,
        Ok(EmailEvent::Recorded)
    );
    assert_eq!(
        update_channel(&mut db, contact).await,
        Ok(UpdateChannel::Text)
    );
    assert!(status(&mut db, contact).await.bounced);
    let subject = [job, Uuid::from_u128(2)];
    update(
        &mut db,
        (contact, "quote_update", &subject),
        &body("quote_update", &token),
    )
    .await;
    assert_eq!(texts(&pool, contact, "quote_update").await, 1);
    assert_eq!(emails(&pool, contact).await.len(), 1, "no second email");
}

/// Business `n`'s confirmed quote on `job`, received at `at`.
async fn quote(db: &mut PgConnection, job: Uuid, n: u128, at: Timestamp) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0x730_0000 + n))
    .bind(at.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

/// D-5617, TXT-017: an email-only homeowner told her first quote by email gets the second by a
/// `quote_update` email and no text from `quote_update_step`; after a hard bounce the next
/// update is a text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_39_quote_update_step_emails_then_texts_after_a_bounce(pool: PgPool) {
    footer();
    let mut db = pool.acquire().await.expect("db");
    let (token, contact, job) = open_job(&mut db, 7).await;
    let address = UpdateAddress::parse(ADDRESS).expect("address");
    store_update_address(&mut db, &vault(), contact, &address, at(3))
        .await
        .expect("stored");
    set_update_channel(&mut db, contact, UpdateChannel::Email, at(3))
        .await
        .expect("channel");
    quote(&mut db, job, 1, at(4)).await;
    let moved = first_quote(&mut db, job, at(4)).await;
    assert!(moved.expect("first quote").applied().is_some());
    update(
        &mut db,
        (contact, "first_quote", &[job]),
        &body("first_quote", &token),
    )
    .await;
    sqlx::query("UPDATE messaging.email_outbox SET queued_at = $1 WHERE recipient_id = $2")
        .bind(at(5).to_sqlx())
        .bind(contact)
        .execute(&mut *db)
        .await
        .expect("fixture: the first email's queue time");
    let keys = JobTextKeys::new(id_secret(), key(), vault());
    quote(&mut db, job, 2, at(3 * 3600)).await;
    let step = quote_update_step(&mut db, &keys, (job, false), at(3 * 3600 + 10)).await;
    assert!(matches!(step, Ok(QuoteUpdateStep::Queued(_))), "{step:?}");
    assert_eq!(texts(&pool, contact, "quote_update").await, 0, "no text");
    let rows = emails(&pool, contact).await;
    let told = rows
        .iter()
        .find(|r| r.1 == "update.quote_update")
        .expect("the update email");
    let provider = send(&mut db, &Rec::default(), told.0, at(3 * 3600 + 20)).await;
    let hook = bounce(&provider, at(3 * 3600 + 30));
    receive_email_event(&mut db, &hook, at(3 * 3600 + 30))
        .await
        .expect("bounce");
    quote(&mut db, job, 3, at(6 * 3600 + 30)).await;
    let step = quote_update_step(&mut db, &keys, (job, false), at(6 * 3600 + 40)).await;
    assert!(matches!(step, Ok(QuoteUpdateStep::Queued(_))), "{step:?}");
    assert_eq!(
        texts(&pool, contact, "quote_update").await,
        1,
        "a text after the bounce"
    );
    assert_eq!(emails(&pool, contact).await.len(), 2);
}
