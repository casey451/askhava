//! The scorecard's OPS-046 email panel and OPS-047 figures (TD-OPS-21's panel and scorecard
//! halves; D-490, D-4280, D-4639, T622). Every row is planted and fake: provider IDs, random
//! recipient IDs, numbered job, business and pick IDs, no address or name. The panel shows the
//! four daily counts, opens its alert above 5% deferrals and not at 5%, writes no alert itself and
//! is gone before the first send and after six weeks; each OPS-047 figure comes from its rows.

use askhava_messaging::api::WINDOW_DAYS;
use askhava_ops::api::{NotAFit, NotAFitReason, record_not_a_fit};
use askhava_portals::api::{BusinessQuotes, email_panel, trade_figures};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DAY: SignedDuration = SignedDuration::from_hours(24);

/// Arizona midnight (07:00 UTC) of fake day 0, plus `days` days and `hours` hours.
fn at(days: i64, hours: i64) -> Timestamp {
    let start: Timestamp = "2026-10-01T07:00:00Z".parse().expect("time");
    start + DAY * i32::try_from(days).expect("days") + SignedDuration::from_hours(hours)
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// Plants `count` sent emails at `when`, provider IDs `fake-<tag>-<n>`.
async fn sends(db: &mut PgConnection, tag: &str, count: i32, when: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id,
           recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at)
         SELECT md5($1 || n) || md5(n || $1), 'fake_template', gen_random_uuid(), 'business',
           '{}', 'notice', 'sent', $3, 'fake-' || $1 || '-' || n, $3, $3
         FROM generate_series(1, $2) AS n",
    )
    .bind(tag)
    .bind(count)
    .bind(when.to_sqlx())
    .execute(db)
    .await
    .expect("sends");
}

/// Plants `count` provider events of `kind` on `tag`'s emails 1 to `count` at `when`.
async fn events(db: &mut PgConnection, tag: &str, kind: &str, count: i32, when: Timestamp) {
    sqlx::query(
        "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at)
         SELECT 'fake-' || $1 || '-' || n, $2, $4, $4 FROM generate_series(1, $3) AS n",
    )
    .bind(tag)
    .bind(kind)
    .bind(count)
    .bind(when.to_sqlx())
    .execute(db)
    .await
    .expect("events");
}

async fn alerts(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert")
        .fetch_one(db)
        .await
        .expect("alerts")
}

/// TD-OPS-21 (OPS-046): the panel's four daily counts from planted rows; exactly 5% deferrals
/// leave the alert closed, 6% open it; the panel raises nothing itself; no panel before the first
/// send or after the six weeks.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_21_the_email_panel_shows_four_daily_counts_and_alerts_above_five_percent(
    pool: PgPool,
) {
    let db = &mut *pool.acquire().await.expect("conn");
    assert_eq!(email_panel(db, at(0, 12)).await.expect("panel"), None);
    sends(db, "d0", 40, at(0, 9)).await;
    sends(db, "d1", 100, at(1, 9)).await;
    events(db, "d1", "email.delivery_delayed", 5, at(1, 10)).await;
    events(db, "d1", "email.bounced", 2, at(1, 11)).await;
    events(db, "d1", "email.complained", 1, at(1, 12)).await;
    let panel = email_panel(db, at(2, 8))
        .await
        .expect("panel")
        .expect("shown");
    let counts: Vec<_> = panel
        .days
        .iter()
        .map(|d| (d.sends, d.deferrals, d.bounces, d.complaints))
        .collect();
    assert_eq!(counts, [(40, 0, 0, 0), (100, 5, 2, 1), (0, 0, 0, 0)]);
    assert!(!panel.alert_open, "exactly 5% opens no alert");
    assert!(!panel.days[1].deferrals_too_high());
    sends(db, "d2", 100, at(2, 9)).await;
    events(db, "d2", "email.delivery_delayed", 6, at(2, 10)).await;
    let panel = email_panel(db, at(3, 8))
        .await
        .expect("panel")
        .expect("shown");
    assert_eq!(panel.days.len(), 4);
    assert_eq!(panel.days[2].deferrals, 6);
    assert!(panel.alert_open, "6% opens the alert");
    assert_eq!(alerts(db).await, 0, "the panel raises no alert itself");
    let last = i64::from(WINDOW_DAYS) - 1;
    let panel = email_panel(db, at(last, 8))
        .await
        .expect("panel")
        .expect("shown");
    assert_eq!(panel.days.len(), 42);
    assert!(!panel.alert_open);
    let after = email_panel(db, at(last + 1, 8)).await.expect("panel");
    assert_eq!(after, None, "no panel after six weeks");
}

/// A job's first sent card in `trade` on day `d`.
async fn card(db: &mut PgConnection, job: u128, trade: &str, d: i64) {
    sqlx::query(
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
         photo_ids, category, candidates, emergency, recorded_at, sent_at) VALUES ($1, $1, \
         repeat('a', 32), 'Fake card.', 'Lake Havasu City', 'house', '{}', $2, '{}', false, $3, $3)",
    )
    .bind(id(job))
    .bind(trade)
    .bind(at(d, 8).to_sqlx())
    .execute(db)
    .await
    .expect("card");
}

/// A confirmed quote by `business` on `job` on day `d`, with its optional minutes.
async fn quote(db: &mut PgConnection, (job, business): (u128, u128), d: i64, minutes: Option<i32>) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at, quote_minutes) \
         VALUES ($1, $2, 'firm', 20000, false, '', '', false, true, $3, $4)",
    )
    .bind(id(job))
    .bind(id(business))
    .bind(at(d, 10).to_sqlx())
    .bind(minutes)
    .execute(db)
    .await
    .expect("quote");
}

/// A pick `pick` of `business`'s quote on `job` on day `d`, in `status`.
async fn pick(
    db: &mut PgConnection,
    (pick, job, business): (u128, u128, u128),
    d: i64,
    status: &str,
) {
    sqlx::query(
        "INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, \
         picked_at) VALUES ($1, $2, $3, 1, 'page', $4, $5)",
    )
    .bind(id(pick))
    .bind(id(job))
    .bind(id(business))
    .bind(status)
    .bind(at(d, 12).to_sqlx())
    .execute(db)
    .await
    .expect("pick");
}

/// A charged $75 introduction fee on `pick` (business 1, `job`), its refund asked on day `d`.
async fn refund(db: &mut PgConnection, (pick, job): (u128, u128), trigger: &str, d: i64) {
    let (pick, job, intent) = (id(pick), id(job), format!("pi_FakeT622{}", pick % 1_000));
    let charged = at(d - 1, 9).to_sqlx();
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, \
         stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, $5)",
    )
    .bind(pick)
    .bind(id(1))
    .bind(job)
    .bind(&intent)
    .bind(charged)
    .execute(&mut *db)
    .await
    .expect("intent");
    sqlx::query(
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, \
         decided_at) VALUES ($1, 1, $2, 'succeeded', $3, $3)",
    )
    .bind(pick)
    .bind(id(1))
    .bind(charged)
    .execute(&mut *db)
    .await
    .expect("attempt");
    sqlx::query(
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, \
         idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version, \
         amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1, \
         'pick-' || $1::text || '-try1', 'cus_FakeT622', $4, 1, 7500, $5, $5)",
    )
    .bind(pick)
    .bind(id(1))
    .bind(job)
    .bind(&intent)
    .bind(charged)
    .execute(&mut *db)
    .await
    .expect("evidence");
    sqlx::query(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at) \
         VALUES ($1, $2, 'succeeded', 1, $3)",
    )
    .bind(pick)
    .bind(trigger)
    .bind(at(d, 9).to_sqlx())
    .execute(db)
    .await
    .expect("refund");
}

/// One not-a-fit answer by `business` on `job` in `trade` on day `d`.
async fn not_a_fit(
    db: &mut PgConnection,
    (job, business): (u128, u128),
    trade: &str,
    reason: NotAFitReason,
    d: i64,
) {
    let answer = NotAFit {
        job: id(job),
        business: id(business),
        trade,
        reason,
    };
    assert!(
        record_not_a_fit(db, &answer, at(d, 9))
            .await
            .expect("answer")
    );
}

/// Plumbing jobs 21 and 22 and electrical job 25, carded on day 11; quotes with minutes 20, 40,
/// 60 and none on plumbing (one more after the window), 90 on electrical; business 1 has two
/// picks in the window and one after it, business 2 none.
async fn plant_quotes_and_picks(db: &mut PgConnection) {
    for (job, trade) in [(21, "plumbing"), (22, "plumbing"), (25, "electrical")] {
        card(db, job, trade, 11).await;
    }
    quote(db, (21, 1), 11, Some(20)).await;
    quote(db, (22, 1), 12, Some(40)).await;
    quote(db, (21, 2), 11, None).await;
    quote(db, (22, 2), 12, Some(60)).await;
    quote(db, (21, 3), 20, Some(5)).await;
    quote(db, (25, 3), 12, Some(90)).await;
    pick(db, (81, 21, 1), 12, "paid").await;
    pick(db, (82, 22, 1), 13, "expired").await;
    pick(db, (83, 25, 1), 20, "paid").await;
}

/// TD-OPS-21 (OPS-047): the scorecard's quote minutes, quotes per pick, not-a-fit counts and
/// refund reasons per pushed trade, each from planted rows; rows outside the window, the month or
/// the trade are left out, and a pushed trade with no job has empty figures.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_21_each_ops_047_figure_comes_from_planted_rows(pool: PgPool) {
    let db = &mut *pool.acquire().await.expect("conn");
    plant_quotes_and_picks(db).await;
    refund(db, (81, 21), "unreachable", 13).await;
    refund(db, (84, 22), "unreachable", 14).await;
    refund(db, (85, 21), "cancelled-before-talk", 15).await;
    refund(db, (86, 21), "unreachable", 25).await;
    refund(db, (87, 25), "unreachable", 13).await;
    not_a_fit(db, (21, 5), "plumbing", NotAFitReason::Other, 12).await;
    not_a_fit(db, (21, 6), "plumbing", NotAFitReason::NotMyTown, 12).await;
    not_a_fit(db, (22, 6), "plumbing", NotAFitReason::NotMyTown, 13).await;
    not_a_fit(db, (22, 7), "plumbing", NotAFitReason::NotMyTrade, -5).await;
    not_a_fit(db, (25, 5), "electrical", NotAFitReason::TooBigOrSmall, 12).await;
    let window = (at(10, 0), at(17, 0));
    let figures = trade_figures(db, &["plumbing", "roofing"], window)
        .await
        .expect("figures");
    assert_eq!(figures.len(), 2);
    let plumbing = &figures[0];
    assert_eq!(plumbing.trade, "plumbing");
    assert_eq!(
        plumbing.median_quote_minutes,
        Some(SignedDuration::from_mins(40))
    );
    let per_business = [
        BusinessQuotes {
            business: id(1),
            quotes: 2,
            picks: 2,
        },
        BusinessQuotes {
            business: id(2),
            quotes: 2,
            picks: 0,
        },
    ];
    assert_eq!(plumbing.quotes_per_pick, per_business);
    assert_eq!(
        plumbing.quotes_per_pick[0].quotes_per_pick_tenths(),
        Some(10)
    );
    assert_eq!(plumbing.quotes_per_pick[1].quotes_per_pick_tenths(), None);
    let not_a_fit = [(NotAFitReason::NotMyTown, 2), (NotAFitReason::Other, 1)];
    assert_eq!(plumbing.not_a_fit, not_a_fit);
    let reasons = [
        ("cancelled-before-talk".to_owned(), 1),
        ("unreachable".to_owned(), 2),
    ];
    assert_eq!(plumbing.refund_reasons, reasons);
    let roofing = &figures[1];
    assert_eq!(roofing.trade, "roofing");
    assert_eq!(roofing.median_quote_minutes, None);
    assert!(roofing.quotes_per_pick.is_empty() && roofing.not_a_fit.is_empty());
    assert!(roofing.refund_reasons.is_empty());
}

/// OPS-047: quotes per pick in tenths rounds half up and needs a pick.
#[test]
fn quotes_per_pick_rounds_half_up_in_tenths() {
    let row = |quotes, picks| BusinessQuotes {
        business: id(1),
        quotes,
        picks,
    };
    assert_eq!(row(1, 3).quotes_per_pick_tenths(), Some(3));
    assert_eq!(row(2, 3).quotes_per_pick_tenths(), Some(7));
    assert_eq!(row(1, 4).quotes_per_pick_tenths(), Some(3));
    assert_eq!(row(5, 2).quotes_per_pick_tenths(), Some(25));
    assert_eq!(row(3, 0).quotes_per_pick_tenths(), None);
}
