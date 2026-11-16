//! The readers behind the owner health page's figures on PostgreSQL (OPS-008, OPS-004, OPS-009,
//! OPS-037, BIZ-044; T475, D-3753, D-3844, D-3877, D-3155).
//!
//! Proves that `messaging::api::{text_figures, email_figures, emails_sent, notice_timing}`,
//! `quotes::api::{card_figures, spot_check_cards}`, `payments::api::payment_errors` and
//! `ai::api::ai_figures` count each figure from planted rows inside their window only; that
//! TD-OPS-12's card (sent 10:00, first opened 10:20, quoted 11:00) gives 20- and 60-minute
//! medians; that an empty window gives `None` for every share and median; that the spot-check
//! draws 10 distinct cards of the month, and fewer when fewer exist; and that no figure carries
//! the planted fake body, subject or card words. Rows are seeded straight into each module's
//! tables (fake IDs and fake words only); the readers run as the app role.

use askhava_ai::api::{AiFigures, ai_figures};
use askhava_integration::set_app_role;
use askhava_messaging::api::{
    EmailFigures, NoticeTiming, PerAudience, TextFigures, email_figures, emails_sent,
    notice_timing, text_figures,
};
use askhava_payments::api::{PaymentErrors, payment_errors};
use askhava_quotes::api::{CardFigures, SpotCard, card_figures, spot_check_cards};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The fake words planted in bodies, subjects and cards, which no figure may carry (PRIV-005).
const FAKE_WORDS: &str = "Fake private words";

/// A week: Monday Nov 9, 2026 to Monday Nov 16, Arizona midnight.
fn week() -> (Timestamp, Timestamp) {
    (at("2026-11-09T07:00:00Z"), at("2026-11-16T07:00:00Z"))
}

/// A window with nothing in it.
fn empty() -> (Timestamp, Timestamp) {
    (at("2025-01-01T07:00:00Z"), at("2025-02-01T07:00:00Z"))
}

/// A fixed instant.
fn at(text: &str) -> Timestamp {
    text.parse().expect("timestamp")
}

/// A pooled connection running as the app role.
async fn app(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// Runs one seeding statement with text arguments; the SQL casts each.
async fn seed(db: &mut PgConnection, sql: &'static str, args: &[&str]) {
    let mut q = sqlx::query(sql);
    for a in args {
        q = q.bind((*a).to_owned());
    }
    q.execute(db).await.expect("seed");
}

/// A text to `kind` with `status`, sent (or, when failed, created) at `when`, then its delivery
/// events in order.
async fn text(db: &mut PgConnection, kind: &str, status: &str, when: &str, events: &[&str]) {
    let sid = format!("SM{}", Uuid::new_v4().simple());
    let sent = status == "sent";
    seed(
        db,
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, recipient_kind,
           subject_ids, body, status, provider_sid, created_at, sent_at)
         VALUES (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'fake_template',
           gen_random_uuid(), $1, '{}', NULLIF($2, ''), $3, NULLIF($4, ''), $5::timestamptz,
           NULLIF($6, '')::timestamptz)",
        &[
            kind,
            if sent { "" } else { FAKE_WORDS },
            status,
            if sent { &sid } else { "" },
            when,
            if sent { when } else { "" },
        ],
    )
    .await;
    for e in events {
        seed(
            db,
            "INSERT INTO messaging.delivery_event (provider_sid, status, received_at)
             VALUES ($1, $2, $3::timestamptz)",
            &[&sid, e, when],
        )
        .await;
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn texts_give_delivery_rates_and_failures_per_audience(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let d = &["sent", "delivered"];
    for when in [
        "2026-11-10T17:00:00Z",
        "2026-11-11T17:00:00Z",
        "2026-11-12T17:00:00Z",
    ] {
        text(&mut db, "homeowner", "sent", when, d).await;
    }
    text(
        &mut db,
        "homeowner",
        "sent",
        "2026-11-13T17:00:00Z",
        &["sent", "undelivered"],
    )
    .await;
    text(&mut db, "homeowner", "failed", "2026-11-13T18:00:00Z", &[]).await;
    text(&mut db, "homeowner", "sent", "2026-11-02T17:00:00Z", d).await;
    text(&mut db, "homeowner", "failed", "2026-11-17T17:00:00Z", &[]).await;
    text(&mut db, "homeowner", "queued", "2026-11-12T17:00:00Z", &[]).await;
    text(&mut db, "business", "sent", "2026-11-10T18:00:00Z", d).await;
    text(&mut db, "business", "sent", "2026-11-10T19:00:00Z", &[]).await;
    let (from, to) = week();
    let got = text_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("texts");
    let home = TextFigures {
        sent: 4,
        delivered: 3,
        delivered_per_mille: Some(750),
        failed: 2,
    };
    let biz = TextFigures {
        sent: 2,
        delivered: 1,
        delivered_per_mille: Some(500),
        failed: 0,
    };
    assert_eq!(
        got,
        PerAudience {
            homeowner: home,
            business: biz
        }
    );
    assert!(!format!("{got:?}").contains(FAKE_WORDS));
    let (from, to) = empty();
    let none = text_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("texts");
    assert_eq!(none, PerAudience::default());
    assert_eq!(none.homeowner.delivered_per_mille, None);
}

/// An email to `kind` sent at `sent` (or queued, with fake words, when `sent` is empty), then
/// its provider events.
async fn email(db: &mut PgConnection, kind: &str, sent: &str, events: &[&str]) {
    let provider = format!("fake-{}", Uuid::new_v4());
    let (status, words, id) = match sent {
        "" => ("queued", FAKE_WORDS, ""),
        _ => ("sent", "", provider.as_str()),
    };
    seed(
        db,
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id,
           recipient_kind, subject_ids, class, status, subject, html, plain, not_before,
           provider_id, queued_at, sent_at)
         VALUES (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'fake_template',
           gen_random_uuid(), $1, '{}', 'notice', $2, NULLIF($3, ''), NULLIF($3, ''),
           NULLIF($3, ''), '2026-11-01T07:00:00Z', NULLIF($4, ''), '2026-11-01T07:00:00Z',
           NULLIF($5, '')::timestamptz)",
        &[kind, status, words, id, sent],
    )
    .await;
    for e in events {
        seed(
            db,
            "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at)
             VALUES ($1, $2, $3::timestamptz, $3::timestamptz)",
            &[&provider, e, sent],
        )
        .await;
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn emails_give_delivered_bounced_and_complained_rates_and_sent_counts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let day = "2026-11-15T17:00:00Z";
    email(&mut db, "business", day, &["email.sent", "email.delivered"]).await;
    email(
        &mut db,
        "business",
        day,
        &["email.delivered", "email.complained"],
    )
    .await;
    email(
        &mut db,
        "business",
        "2026-11-10T17:00:00Z",
        &["email.bounced"],
    )
    .await;
    email(&mut db, "business", "2026-11-10T18:00:00Z", &[]).await;
    email(
        &mut db,
        "business",
        "2026-11-03T17:00:00Z",
        &["email.bounced"],
    )
    .await;
    email(&mut db, "homeowner", day, &["email.delivered"]).await;
    email(&mut db, "business", "", &[]).await;
    let (from, to) = week();
    let got = email_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("emails");
    let biz = EmailFigures {
        sent: 4,
        delivered: 2,
        bounced: 1,
        complained: 1,
        per_mille: Some((500, 250, 250)),
    };
    let home = EmailFigures {
        sent: 1,
        delivered: 1,
        bounced: 0,
        complained: 0,
        per_mille: Some((1000, 0, 0)),
    };
    assert_eq!(
        got,
        PerAudience {
            homeowner: home,
            business: biz
        }
    );
    assert!(!format!("{got:?}").contains(FAKE_WORDS));
    let today = (at("2026-11-15T07:00:00Z"), at("2026-11-16T07:00:00Z"));
    let month = (at("2026-11-01T07:00:00Z"), at("2026-11-16T07:00:00Z"));
    let mut conn = app(&pool).await;
    assert_eq!(
        emails_sent(&mut conn, today.0, today.1)
            .await
            .expect("today"),
        3
    );
    assert_eq!(
        emails_sent(&mut conn, month.0, month.1)
            .await
            .expect("month"),
        6
    );
    let (from, to) = empty();
    assert_eq!(emails_sent(&mut conn, from, to).await.expect("empty"), 0);
    let none = email_figures(&mut conn, from, to).await.expect("emails");
    assert_eq!(none, PerAudience::default());
    assert_eq!(none.business.per_mille, None);
}

/// A notice of `kind` for one business and `job`, sent (held when empty), first opened and acted
/// on (none when empty) at the given November 2026 days and UTC times (`DDTHH:MM`).
async fn notice(db: &mut PgConnection, (kind, job): (&str, &str), times: [&str; 3]) {
    seed(
        db,
        "INSERT INTO messaging.notice_log (business_id, job_id, kind, mailbox_id, sent_at,
           held_until, first_opened_at, acted_at)
         VALUES ('00000000-0000-4000-8000-000000000001', md5($1)::uuid, $2, gen_random_uuid(),
           ('2026-11-' || NULLIF($3, '') || ':00Z')::timestamptz, '2026-11-12T15:00:00Z',
           ('2026-11-' || NULLIF($4, '') || ':00Z')::timestamptz,
           ('2026-11-' || NULLIF($5, '') || ':00Z')::timestamptz)",
        &[job, kind, times[0], times[1], times[2]],
    )
    .await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_12_weekly_medians_include_twenty_and_sixty_minutes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    // (kind, job, [sent, first opened, acted]); TD-OPS-12's card has a second address.
    let rows = [
        (("card", "td-ops-12"), ["10T17:00", "10T17:20", "10T18:00"]),
        (("card", "td-ops-12"), ["10T17:00", "10T17:50", "10T18:00"]),
        (("card", "b"), ["11T17:00", "11T17:10", "11T17:30"]),
        (("card", "c"), ["12T17:00", "12T17:40", "13T01:00"]),
        (("card", "d"), ["13T17:00", "", ""]),
        (("pick", "e"), ["14T17:00", "14T17:20", "14T17:30"]),
        (("card", "last-week"), ["02T17:00", "02T17:01", "02T17:02"]),
        (("card", "held"), ["", "", ""]),
    ];
    for (what, times) in rows {
        notice(&mut db, what, times).await;
    }
    let six_hours = SignedDuration::from_hours(6);
    let (from, to) = week();
    let got = notice_timing(&mut *app(&pool).await, from, to, six_hours)
        .await
        .expect("timing");
    let want = NoticeTiming {
        to_first_open: Some(SignedDuration::from_mins(20)),
        to_quote: Some(SignedDuration::from_mins(60)),
        to_confirm: Some(SignedDuration::from_mins(30)),
        quoted_within_per_mille: Some(500),
    };
    assert_eq!(got, want);
    let (from, to) = empty();
    let none = notice_timing(&mut *app(&pool).await, from, to, six_hours)
        .await
        .expect("timing");
    assert_eq!(none, NoticeTiming::default());
}

/// A job card for a new business on `job`, sent at `sent`; its sent card text names `trade`.
async fn job_card(db: &mut PgConnection, job: Uuid, sent: &str, trade: &str) -> Uuid {
    let business = Uuid::new_v4().to_string();
    let job = job.to_string();
    seed(
        db,
        "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
         VALUES ($1::uuid, $2::uuid, false, $3::timestamptz)",
        &[&job, &business, sent],
    )
    .await;
    seed(
        db,
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place,
           photo_ids, category, candidates, emergency, recorded_at, sent_at)
         VALUES (gen_random_uuid(), $1::uuid, md5(gen_random_uuid()::text), $2, 'Fake Town',
           'house', '{}', $3, '{}', false, $4::timestamptz, $4::timestamptz)",
        &[&job, FAKE_WORDS, trade, sent],
    )
    .await;
    business.parse().expect("uuid")
}

/// A quote by `business` on `job`, withdrawn when `withdrawn`.
async fn quote(db: &mut PgConnection, job: Uuid, business: Uuid, withdrawn: bool) {
    let (job, business) = (job.to_string(), business.to_string());
    seed(
        db,
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site,
           notes, original, ai_read, confirmed, received_at, withdrawn_at)
         VALUES ($1::uuid, $2::uuid, 'firm', 10000, false, '', 'Fake quote', false, true,
           '2026-11-08T17:00:00Z', CASE WHEN $3 = 'yes' THEN now() END)",
        &[&job, &business, if withdrawn { "yes" } else { "no" }],
    )
    .await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_044_cards_ended_without_a_quote_count_a_withdrawn_quote_as_none(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (quoted, unquoted, withdrawn, open) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let b = job_card(&mut db, quoted, "2026-11-05T17:00:00Z", "plumbing").await;
    quote(&mut db, quoted, b, false).await;
    job_card(&mut db, unquoted, "2026-11-06T17:00:00Z", "plumbing").await;
    let b = job_card(&mut db, withdrawn, "2026-11-07T17:00:00Z", "painting").await;
    quote(&mut db, withdrawn, b, true).await;
    job_card(&mut db, open, "2026-11-20T10:00:00Z", "painting").await;
    job_card(&mut db, Uuid::new_v4(), "2026-10-20T17:00:00Z", "painting").await;
    let (from, to) = (at("2026-11-01T07:00:00Z"), at("2026-11-20T19:00:00Z"));
    let got = card_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("cards");
    let want = CardFigures {
        cards: 4,
        ended_without_quote: 2,
        without_quote_per_mille: Some(500),
    };
    assert_eq!(got, want);
    let (from, to) = empty();
    let none = card_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("cards");
    assert_eq!(none, CardFigures::default());
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_009_spot_check_draws_ten_distinct_cards_of_the_month_or_fewer(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    for day in 1..=12 {
        let sent = format!("2026-11-{:02}T17:00:00Z", day + 1);
        job_card(&mut db, Uuid::new_v4(), &sent, "plumbing").await;
    }
    job_card(&mut db, Uuid::new_v4(), "2026-10-20T17:00:00Z", "plumbing").await;
    for day in [
        "2026-09-02T17:00:00Z",
        "2026-09-03T17:00:00Z",
        "2026-09-04T17:00:00Z",
    ] {
        job_card(&mut db, Uuid::new_v4(), day, "painting").await;
    }
    let (from, to) = (at("2026-11-01T07:00:00Z"), at("2026-12-01T07:00:00Z"));
    let got = spot_check_cards(&mut *app(&pool).await, from, to, 10)
        .await
        .expect("spot");
    let ids: std::collections::BTreeSet<Uuid> = got.iter().map(|c| c.card).collect();
    assert_eq!((got.len(), ids.len()), (10, 10));
    for SpotCard {
        card: _,
        job: _,
        trade,
        sent_at,
    } in &got
    {
        assert_eq!(trade.as_deref(), Some("plumbing"));
        assert!(*sent_at >= from && *sent_at < to);
    }
    assert!(!format!("{got:?}").contains(FAKE_WORDS));
    let (from, to) = (at("2026-09-01T07:00:00Z"), at("2026-10-01T07:00:00Z"));
    let few = spot_check_cards(&mut *app(&pool).await, from, to, 10)
        .await
        .expect("spot");
    assert_eq!(few.len(), 3);
    let (from, to) = empty();
    let none = spot_check_cards(&mut *app(&pool).await, from, to, 10)
        .await
        .expect("spot");
    assert!(none.is_empty());
}

/// A charge attempt in `status` started at `started`; with `refund`, a refund in that status
/// requested then.
async fn charge(db: &mut PgConnection, status: &str, started: &str, refund: &str) {
    let pick = Uuid::new_v4().to_string();
    seed(
        db,
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1::uuid, gen_random_uuid(), gen_random_uuid(), 7500,
           'pi_' || md5($1), false, $2::timestamptz)",
        &[&pick, started],
    )
    .await;
    seed(
        db,
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
         VALUES ($1::uuid, 1, gen_random_uuid(), $2, $3::timestamptz)",
        &[&pick, status, started],
    )
    .await;
    if !refund.is_empty() {
        seed(
            db,
            "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
               idempotency_label, stripe_customer_id, stripe_intent_id, amount_cents, pressed_at,
               charged_at)
             SELECT pick_id, business_id, job_id, gen_random_uuid(), 1, 'pick-' || pick_id || '-try1',
               'cus_fake', stripe_intent_id, 7500, created_at, created_at
               FROM payments.pick_intent WHERE pick_id = $1::uuid",
            &[&pick],
        )
        .await;
        seed(
            db,
            "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at)
             VALUES ($1::uuid, 'unreachable', $2, 1, $3::timestamptz)",
            &[&pick, refund, started],
        )
        .await;
    }
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn payment_errors_count_failed_and_undecided_charges_and_failed_refunds(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let inside = "2026-11-12T17:00:00Z";
    for status in [
        "declined",
        "needs_action",
        "unreachable",
        "sent",
        "processing",
        "succeeded",
    ] {
        charge(&mut db, status, inside, "").await;
    }
    charge(&mut db, "declined", "2026-11-02T17:00:00Z", "").await;
    charge(&mut db, "succeeded", inside, "failed").await;
    charge(&mut db, "succeeded", inside, "succeeded").await;
    charge(&mut db, "succeeded", "2026-11-02T17:00:00Z", "failed").await;
    let (from, to) = week();
    let got = payment_errors(&mut *app(&pool).await, from, to)
        .await
        .expect("payments");
    let want = PaymentErrors {
        failed_charges: 3,
        undecided_charges: 2,
        failed_refunds: 1,
    };
    assert_eq!(got, want);
    let (from, to) = empty();
    let none = payment_errors(&mut *app(&pool).await, from, to)
        .await
        .expect("payments");
    assert_eq!(none, PaymentErrors::default());
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ai_figures_count_calls_and_errors_but_not_daily_cap_refusals(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let inside = "2026-11-12T17:00:00Z";
    for (outcome, when) in [
        ("ok", inside),
        ("ok", inside),
        ("timed_out", inside),
        ("schema", inside),
        ("daily_cap", inside),
        ("http_error", "2026-11-02T17:00:00Z"),
    ] {
        seed(
            &mut db,
            "INSERT INTO ai.call_stat (at, subject_id, job, instruction_version, model, outcome,
               attempts)
             VALUES ($1::timestamptz, gen_random_uuid(), 'intake_reading', 'fake_v1',
               'claude-fake-model', $2, 1)",
            &[when, outcome],
        )
        .await;
    }
    let (from, to) = week();
    let got = ai_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("ai");
    assert_eq!(
        got,
        AiFigures {
            calls: 4,
            errors: 2
        }
    );
    let (from, to) = empty();
    let none = ai_figures(&mut *app(&pool).await, from, to)
        .await
        .expect("ai");
    assert_eq!(none, AiFigures::default());
}
