//! TD-PICK-42 (PICK-042, PICK-054, PICK-011; D-244, D-466, D-493, T388): after the Confirm the
//! homeowner gets a copy of the accepted quote by email when an email is on file, and her quote
//! page shows it until its token expires. The Confirm is `picks::api::settle` on a $0 clearing
//! (`payments::api::no_fee_release` over a free-first-win entry); every row comes through a module
//! API, as in `app/server/tests/release_recovery.rs`. Fake data only.

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_businesses::api::AccountStatus;
use askhava_core::api::{Claimed, IdSecret, LinkToken, MIGRATOR, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer, run_accepted_quote};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_messaging::api::{AcceptedQuoteSender, MailboxKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{ACCEPTED_QUOTE_KIND, PagePick, Settled, page_pick, settle};
use askhava_portals::api::quote_page_routes;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, DayPart, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// 10:00 Arizona.
const NOW: i64 = 1_793_638_800;
const WORDS: &str = "Swap the capacitor and check the coil, parts and labor included";

fn now() -> Timestamp {
    Timestamp::from_second(NOW).expect("time")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x38), &key(0x39), &url).expect("vault")
}

fn sender() -> Arc<AcceptedQuoteSender> {
    let secret = SecretString::from("fake-email-key-secret-0000000388".to_owned());
    let postal = (
        "PO Box 1234, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    );
    Arc::new(AcceptedQuoteSender::new(secret, postal, vault()).expect("sender"))
}

/// Her vault contact (with `email` or none) and her request naming `job`, in Spanish when `es`.
async fn her_request(pool: &PgPool, job: Uuid, (email, es): (Option<&str>, bool)) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(if es {
            "+19285550189"
        } else {
            "+19285550188"
        })),
        email: email.map(SecretString::from),
        address: None,
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let inbound = Inbound {
        words: "my AC stopped cooling",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: Some(es),
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: now(),
    };
    let settings = IntakeSettings {
        spanish_conversations_on: true,
        safety_call_on: false,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(&mut db, &secret, &text, settings)
        .await
        .expect("request");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation: handled.conversation.expect("open request"),
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
}

fn card_job() -> CardJob {
    let job = Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    CardJob {
        towns: vec![town],
        candidates: vec![job],
        text: String::new(),
    }
}

fn card_business(id: Uuid) -> CardBusiness {
    CardBusiness {
        id,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

/// A firm $325 quote with an earliest visit on Nov 9, 2026.
fn quote_form() -> QuoteForm {
    let day = jiff::civil::date(2026, 11, 9);
    QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: WORDS.to_owned(),
        ai_read: false,
        earliest_visit: Some((day, DayPart::Morning)),
    }
}

/// A business, a job quoted by it and her request, page-picked and cleared with no fee due:
/// `(job, pick, the clearing's business)`.
async fn picked(pool: &PgPool, her: (Option<&str>, bool)) -> (Uuid, Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now())
        .await
        .expect("business");
    let job = create(&mut db, now(), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(&mut db, job, open, now(), Actor::System)
        .await
        .expect("open");
    let map = ClassMap::shipped().expect("map");
    let pool_of = [card_business(business)];
    send_cards(&mut db, map, job, &card_job(), &pool_of, now())
        .await
        .expect("cards");
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    let quote = parse_quote(quote_form(), &rules).expect("quote");
    submit_quote(&mut db, (job, business), &quote, now())
        .await
        .expect("saved");
    her_request(pool, job, her).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick");
    };
    let key = format!("t388.free.{job}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(&mut db, &entry).await.expect("ledger");
    (job, pick, business)
}

/// The Confirm: the settle on the pick's $0 clearing, committed.
async fn confirm(pool: &PgPool, (job, pick, business): (Uuid, Uuid, Uuid)) -> Settled {
    let mut tx = pool.begin().await.expect("tx");
    let cleared = no_fee_release(&mut tx, pick, (business, job))
        .await
        .expect("cleared");
    let settled = settle(&mut tx, &cleared).await.expect("settled");
    tx.commit().await.expect("commit");
    settled
}

/// The queued accepted-quote tasks, as the worker would claim them.
async fn accepted_tasks(pool: &PgPool) -> Vec<Claimed> {
    let rows: Vec<(i64, serde_json::Value)> =
        sqlx::query_as("SELECT id, payload FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(ACCEPTED_QUOTE_KIND)
            .fetch_all(pool)
            .await
            .expect("tasks");
    rows.into_iter()
        .map(|(id, payload)| Claimed {
            id,
            kind: ACCEPTED_QUOTE_KIND.to_owned(),
            payload,
            attempts: 1,
        })
        .collect()
}

/// The stored accepted-quote emails: subject, HTML and plain text.
async fn emails(pool: &PgPool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT subject, html, plain FROM messaging.email_outbox \
         WHERE template_id = 'accepted_quote_email'",
    )
    .fetch_all(pool)
    .await
    .expect("emails")
}

/// TD-PICK-42 (PICK-042, D-3074): the settle that moves a pick queues one task holding only the
/// pick, job and business IDs; a second settle of the same pick queues none.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_42_a_confirm_queues_one_task_and_a_second_settle_none(pool: PgPool) {
    let ids = picked(&pool, (Some("dana@example.com"), false)).await;
    assert_eq!(confirm(&pool, ids).await, Settled::Moved(ids.0));
    assert_eq!(confirm(&pool, ids).await, Settled::Was(ids.0));
    let tasks = accepted_tasks(&pool).await;
    assert_eq!(tasks.len(), 1);
    let want = serde_json::json!({"pick": ids.1, "job": ids.0, "business": ids.2});
    assert_eq!(tasks[0].payload, want);
}

/// TD-PICK-42 (PICK-042, PICK-054, D-466): the handler stores one email in her language with the
/// type, amount, earliest visit, the business's words and the contacts offer, no reward; run
/// twice it stores nothing more; no address is stored.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_42_the_email_holds_the_quote_and_the_contacts_offer(pool: PgPool) {
    for (es, lines) in [
        (
            false,
            [
                "Firm price",
                "$325",
                "Nov 9",
                "Add Ask Hava to your contacts",
            ],
        ),
        (
            true,
            [
                "Precio fijo",
                "$325",
                "9 nov",
                "Agregue Ask Hava a sus contactos",
            ],
        ),
    ] {
        let ids = picked(&pool, (Some("dana@example.com"), es)).await;
        confirm(&pool, ids).await;
        let tasks = accepted_tasks(&pool).await;
        let task = tasks.last().expect("task").clone();
        run_accepted_quote(pool.clone(), Some(sender()), task.clone())
            .await
            .expect("run");
        run_accepted_quote(pool.clone(), Some(sender()), task)
            .await
            .expect("again");
        let stored = emails(&pool).await;
        let ours = stored.iter().find(|(_, html, _)| html.contains(lines[0]));
        let (subject, html, plain) = ours.expect("an email in her language");
        assert_eq!(stored.len(), usize::from(es) + 1, "one email per pick");
        for part in [html, plain] {
            for line in lines.iter().chain([&WORDS]) {
                assert!(part.contains(line), "{line}: {part}");
            }
            let lower = part.to_lowercase();
            for reward in ["reward", "discount", "credit", "recompensa", "descuento"] {
                assert!(!lower.contains(reward), "{reward}");
            }
            assert!(!part.contains("dana@example.com") && !part.contains("Dana"));
        }
        assert!(subject.starts_with("Ask Hava:"), "{subject}");
    }
}

/// TD-PICK-42 (PICK-042, D-493): with no email on file no email is stored and the task is done.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_42_no_email_on_file_queues_no_email(pool: PgPool) {
    let ids = picked(&pool, (None, false)).await;
    confirm(&pool, ids).await;
    let task = accepted_tasks(&pool).await.remove(0);
    run_accepted_quote(pool.clone(), Some(sender()), task)
        .await
        .expect("done");
    assert!(emails(&pool).await.is_empty());
}

async fn page(pool: &PgPool, clock: &Arc<ManualClock>, token: &LinkToken) -> (StatusCode, String) {
    let request = Request::get(format!("/q/{}", token.as_str()))
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let app = quote_page_routes(pool.clone(), Arc::clone(clock) as _);
    let response = app.oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    (
        parts.status,
        String::from_utf8(bytes.to_vec()).expect("utf8"),
    )
}

/// TD-PICK-42 (PICK-042, PICK-011, PICK-010): the quote page shows no accepted quote before the
/// Confirm, shows it after (number, type, amount, visit, words, contacts offer) and still at 29
/// days 23:59 after the job closes, and is a 404 at 30 days; it never shows her name or phone.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_42_the_page_shows_the_accepted_quote_until_the_token_expires(pool: PgPool) {
    let ids = picked(&pool, (Some("dana@example.com"), false)).await;
    let secret = IdSecret::new(
        1,
        SecretString::from("fake-id-secret-for-tests-00000388".to_owned()),
    );
    let mut db = pool.acquire().await.expect("db");
    let token = issue(&mut db, &secret.expect("secret"), ids.0, now())
        .await
        .expect("token");
    let clock = Arc::new(ManualClock::new(now()));
    let (_, before) = page(&pool, &clock, &token).await;
    assert!(!before.contains("You picked quote"));
    confirm(&pool, ids).await;
    let (status, after) = page(&pool, &clock, &token).await;
    assert_eq!(status, StatusCode::OK);
    for line in [
        "You picked quote #1",
        "Type: Firm price",
        "Amount: $325",
        "Earliest visit: Nov 9",
        WORDS,
        "Add Ask Hava to your contacts",
    ] {
        assert!(after.contains(line), "{line}");
    }
    assert!(!after.contains("Dana") && !after.contains("5550188"));
    let closed = now() + SignedDuration::from_hours(1);
    let close = (JobStatus::AwaitingConfirm, JobStatus::Cancelled);
    let _ = move_to(&mut db, ids.0, close, closed, Actor::System)
        .await
        .expect("closed");
    let thirty = SignedDuration::from_hours(30 * 24);
    clock.set(closed + thirty - SignedDuration::from_mins(1));
    let (status, late) = page(&pool, &clock, &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(late.contains("You picked quote #1"));
    clock.set(closed + thirty);
    assert_eq!(page(&pool, &clock, &token).await.0, StatusCode::NOT_FOUND);
}
