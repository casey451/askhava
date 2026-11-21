//! TD-CARD-18: the hourly open-cards email run, `picks.open_cards_email`, on a fresh PostgreSQL
//! database (T684; CARD-022, BIZ-037, BIZ-038, OPS-038; D-4361, D-5017). The links are the
//! portal's real action links; every business, job, town and address is fake.

use std::sync::Arc;

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::{
    Acting, OpenCardsTime, OpenCardsTimes, Roles, add_member, add_owner, create,
    set_open_cards_times,
};
use askhava_core::api::{Claimed, HandlerError, brand};
use askhava_messaging::api::{
    DailyLimit, EmailSent, EmailTo, MailboxKind, PostalAddress, next_reset, send_email,
};
use askhava_picks::api::{
    DigestLink, OPEN_CARDS_EMAIL_KIND, OPEN_CARDS_TEMPLATE, OpenCardsLinks, OpenCardsParts,
    run_open_cards_email,
};
use askhava_portals::api::{ActionKind, ActionTarget, mint_action_link};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// 8:05 a.m., 12:05 p.m. and 5:05 p.m. Arizona time (UTC-7) on November 10, 2026.
const EIGHT: &str = "2026-11-10T15:05:00Z";
const NOON: &str = "2026-11-10T19:05:00Z";
const FIVE: &str = "2026-11-11T00:05:00Z";
/// A card sent then ends at 7 p.m. Arizona time that day: open at all three runs.
const SENT: &str = "2026-11-10T02:00:00Z";

fn at(text: &str) -> Timestamp {
    text.parse().expect("time")
}

/// The composition's link maker, as `app/server/src/lib.rs` implements it.
struct Links;

impl OpenCardsLinks for Links {
    async fn link(
        &self,
        db: &mut PgConnection,
        (business, member): (Uuid, Uuid),
        to: DigestLink,
        now_ends: (Timestamp, Timestamp),
    ) -> Result<SecretString, HandlerError> {
        let (kind, object) = match to {
            DigestLink::Quote(job) => (ActionKind::Quote, job),
            DigestLink::Confirm(pick) => (ActionKind::Confirm, pick),
        };
        let target = ActionTarget {
            kind,
            object,
            member,
            business,
        };
        mint_action_link(db, target, None, now_ends)
            .await
            .map_err(|_| HandlerError("no link"))
    }
}

/// A provider that takes every email; the held email must never reach it.
struct Provider;

impl EmailAdapter for Provider {
    async fn send_email(&self, _: &IdempotencyKey, _: &Email<'_>) -> Result<String, AdapterError> {
        Ok("re_fake_1".to_owned())
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0001".to_owned())
}

/// Runs one seeding statement over `ids` (`$1`, a uuid array) and `text` (`$2`).
async fn seed(db: &mut PgConnection, sql: &'static str, ids: &[Uuid], text: &str) {
    let q = sqlx::query(sql).bind(ids).bind(text.to_owned());
    q.execute(db).await.expect("seed");
}

/// A job in `status` whose `trade` card in Fake Town went to `business` at `sent`; the job ID.
async fn card(db: &mut PgConnection, business: Uuid, about: (&str, &str), sent: &str) -> Uuid {
    let (status, trade) = about;
    let ids = [Uuid::new_v4(), business];
    let job = "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from) \
               VALUES (($1::uuid[])[1], $2, '2026-11-09T00:00Z', '2026-11-09T00:00Z', \
               '2026-11-09T00:00Z')";
    seed(db, job, &ids, status).await;
    let shown = "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, \
                 place, photo_ids, category, candidates, emergency, recorded_at, sent_at) \
                 VALUES (gen_random_uuid(), ($1::uuid[])[1], '0123456789abcdef0123456789abcdef', \
                 'Fake card', 'Fake Town', 'house', '{}', $2, '{}', false, '2026-11-09T00:00Z', \
                 '2026-11-09T00:00Z')";
    seed(db, shown, &ids, trade).await;
    let sent_card = "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) \
                     VALUES (($1::uuid[])[1], ($1::uuid[])[2], false, $2::timestamptz)";
    seed(db, sent_card, &ids, sent).await;
    ids[0]
}

/// An unwithdrawn firm quote of `business` on `job`.
async fn quote(db: &mut PgConnection, job: Uuid, business: Uuid) {
    let sql = "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
               confirm_on_site, notes, original, ai_read, confirmed, received_at) VALUES \
               (($1::uuid[])[1], ($1::uuid[])[2], 'firm', 10000, false, '', 'Fake quote', false, \
               true, $2::timestamptz)";
    seed(db, sql, &[job, business], "2026-11-10T03:00:00Z").await;
}

/// An active business with its owner (the default 5 p.m.), a "Cards and quotes" member who
/// chose 8 a.m. and 5 p.m. and a "Confirm and billing" member, who gets no job cards: the
/// business and the three mailboxes in that order.
async fn team(db: &mut PgConnection) -> (Uuid, [Uuid; 3]) {
    let start = at("2026-11-01T00:00:00Z");
    let business = create(db, false, start).await.expect("business");
    let active = "UPDATE businesses.business SET status = $2 WHERE id = ($1::uuid[])[1]";
    seed(db, active, &[business], "active").await;
    let boxes = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let member = add_owner(db, business, boxes[0], start).await;
    let owner = Acting {
        business,
        member: member.expect("owner"),
    };
    let roles = Roles::new(true, false).expect("roles");
    let member = add_member(db, owner, boxes[1], roles, start).await;
    let cards = Acting {
        business,
        member: member.expect("cards member"),
    };
    let times = OpenCardsTimes::new(&[OpenCardsTime::EightAm, OpenCardsTime::FivePm]);
    let saved = set_open_cards_times(db, cards, times.expect("two times"), start).await;
    saved.expect("times");
    let roles = Roles::new(false, true).expect("roles");
    let billing = add_member(db, owner, boxes[2], roles, start).await;
    billing.expect("billing member");
    (business, boxes)
}

/// One run of the handler at `when`, with the footer postal line set.
async fn run(pool: &PgPool, when: &str) {
    let postal = PostalAddress::new("PO Box 0000, Fake Town, AZ 86403", MailboxKind::PoBox);
    let parts = Arc::new(OpenCardsParts {
        secret: secret(),
        postal: Some(postal.expect("postal")),
        links: Links,
    });
    let task = Claimed {
        id: 1,
        kind: OPEN_CARDS_EMAIL_KIND.to_owned(),
        payload: serde_json::json!({}),
        attempts: 1,
    };
    let done = run_open_cards_email(pool.clone(), parts, task, at(when)).await;
    done.expect("run");
}

/// Every stored open-cards email, oldest first: row, mailbox, subject, plain part and class.
async fn emails(db: &mut PgConnection) -> Vec<(Uuid, Uuid, String, String, String)> {
    let sql = "SELECT id, recipient_id, subject, plain, class FROM messaging.email_outbox \
               WHERE template_id = $1 ORDER BY queued_at, subject";
    let q = sqlx::query_as(sql).bind(OPEN_CARDS_TEMPLATE);
    q.fetch_all(db).await.expect("emails")
}

fn still_open() -> String {
    let fill = brand().fill("{brand_short}: Your job requests still open", false);
    fill.expect("the brand fills")
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_18_each_chosen_slot_once_listing_only_open_unquoted_cards(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (business, boxes) = team(&mut db).await;
    let plumbing = card(&mut db, business, ("open", "plumbing"), SENT).await;
    // 25 minutes left at 8:05 a.m.; quoted; and on a job that takes no more quotes.
    card(
        &mut db,
        business,
        ("open", "electrical"),
        "2026-11-09T15:30:00Z",
    )
    .await;
    let quoted = card(&mut db, business, ("open", "roofing"), SENT).await;
    quote(&mut db, quoted, business).await;
    card(&mut db, business, ("closed", "painting"), SENT).await;

    run(&pool, EIGHT).await;
    run(&pool, EIGHT).await;
    let sent = emails(&mut db).await;
    assert_eq!(
        sent.len(),
        1,
        "once, and only to the person who chose 8 a.m."
    );
    let (_, to, subject, plain, class) = &sent[0];
    assert_eq!(
        (*to, subject, class.as_str()),
        (boxes[1], &still_open(), "bulk")
    );
    let line = format!(
        "plumbing in Fake Town. Quote by 7 p.m.: https://{}/b/a/quote/{plumbing}/",
        brand().domain()
    );
    assert!(
        plain.contains(&line),
        "trade, town, end time and Quote link"
    );
    for absent in ["electrical", "roofing", "painting", "Fake card"] {
        assert!(!plain.contains(absent), "{absent} is not listed");
    }
    assert!(plain.contains("(tap ⋯, then Open in browser)."));

    run(&pool, NOON).await;
    assert_eq!(emails(&mut db).await.len(), 1, "nobody chose noon");
    run(&pool, FIVE).await;
    run(&pool, FIVE).await;
    let sent = emails(&mut db).await;
    let mut to: Vec<Uuid> = sent[1..].iter().map(|e| e.1).collect();
    let mut expected = vec![boxes[0], boxes[1]];
    to.sort_unstable();
    expected.sort_unstable();
    assert_eq!(to, expected, "5 p.m.: the owner and the member, once each");
    assert!(
        sent.iter().all(|e| e.1 != boxes[2]),
        "no job cards, no email"
    );
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(OPEN_CARDS_EMAIL_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(queued, 3, "each run queues the next hour's run once");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_18_none_when_no_card_qualifies(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (business, _) = team(&mut db).await;
    card(
        &mut db,
        business,
        ("open", "electrical"),
        "2026-11-09T15:30:00Z",
    )
    .await;
    let quoted = card(&mut db, business, ("open", "roofing"), SENT).await;
    quote(&mut db, quoted, business).await;
    run(&pool, EIGHT).await;
    run(&pool, FIVE).await;
    assert_eq!(emails(&mut db).await, vec![]);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_18_a_waiting_pick_comes_first_with_the_pick_waiting_subject(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (business, boxes) = team(&mut db).await;
    card(&mut db, business, ("open", "plumbing"), SENT).await;
    let picked = card(&mut db, business, ("awaiting_confirm", "roofing"), SENT).await;
    quote(&mut db, picked, business).await;
    let ids = [Uuid::new_v4(), picked, business];
    let number = "INSERT INTO jobs.job_number (number, job_id, issued_at) \
                  VALUES (1042, ($1::uuid[])[2], $2::timestamptz)";
    seed(&mut db, number, &ids, SENT).await;
    let pick = "INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, \
                picked_at, confirm_due) VALUES (($1::uuid[])[1], ($1::uuid[])[2], \
                ($1::uuid[])[3], 1, 'page', 'awaiting_confirm', '2026-11-10T20:00Z', \
                $2::timestamptz)";
    seed(&mut db, pick, &ids, "2026-11-11T02:00:00Z").await;

    run(&pool, FIVE).await;
    let sent = emails(&mut db).await;
    assert_eq!(
        sent.len(),
        2,
        "the owner and the cards member; never the billing-only one"
    );
    let owner = sent
        .iter()
        .find(|e| e.1 == boxes[0])
        .expect("owner's email");
    let fill = brand().fill(
        "{brand_short}: Pick waiting: confirm job 1042 by 7 p.m.",
        false,
    );
    assert_eq!(owner.2, fill.expect("the brand fills"));
    let confirm = format!("https://{}/b/a/confirm/{}/", brand().domain(), ids[0]);
    assert!(owner.3.starts_with("Job 1042:") && owner.3.contains(&confirm));
    assert!(owner.3.find("Job 1042:") < owner.3.find("plumbing in Fake Town"));
    // BIZ-037: a "Cards and quotes" member gets no picks, so no pick line and the plain subject.
    let member = sent
        .iter()
        .find(|e| e.1 == boxes[1])
        .expect("member's email");
    assert_eq!(member.2, still_open());
    assert!(!member.3.contains("Job 1042") && member.3.contains("plumbing in Fake Town"));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_card_18_above_90_percent_of_the_limit_it_is_bulk_and_waits_for_the_reset(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (business, _) = team(&mut db).await;
    card(&mut db, business, ("open", "plumbing"), SENT).await;
    // OPS-038: 91 of the day's 100 emails already went.
    let earlier = "INSERT INTO messaging.email_outbox (idempotency_key, template_id, \
                   recipient_id, recipient_kind, subject_ids, class, status, not_before, \
                   queued_at, sent_at, provider_id) SELECT encode(sha256(n::text::bytea), 'hex'), 'fake.sent', \
                   gen_random_uuid(), 'business', $1, 'notice', 'sent', $2::timestamptz, \
                   $2::timestamptz, $2::timestamptz, 're_fake' FROM generate_series(1, 91) n";
    seed(&mut db, earlier, &[], "2026-11-10T01:00:00Z").await;
    run(&pool, EIGHT).await;
    let sent = emails(&mut db).await;
    assert_eq!((sent.len(), sent[0].4.as_str()), (1, "bulk"));

    let now = at("2026-11-10T15:06:00Z");
    let limit = DailyLimit::new(100, 90).expect("limit");
    let to = EmailTo::Business("lee.member@example.test");
    let outcome = send_email(&mut db, (&secret(), &Provider, limit), sent[0].0, to, now).await;
    assert_eq!(outcome, Ok(EmailSent::Waiting(next_reset(now))));
    let status: String =
        sqlx::query_scalar("SELECT status FROM messaging.email_outbox WHERE id = $1")
            .bind(sent[0].0)
            .fetch_one(&mut *db)
            .await
            .expect("status");
    assert_eq!(status, "queued", "held, not sent: it goes after the reset");
}
