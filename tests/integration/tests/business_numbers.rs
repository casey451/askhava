//! The "Your numbers" readers and the saved-card list on PostgreSQL (BIZ-025, BIZ-015, FEE-014;
//! D-193, D-250, D-284, D-331; T451).
//!
//! Proves that `quotes::api::quote_numbers`, `picks::api::pick_numbers`,
//! `jobs::api::finished_counts` and `payments::api::saved_cards` count one business's own rows
//! in one Arizona month only (last month's and another business's never); that the typical
//! quotes per job is the median, in tenths, for an odd and an even number of jobs; that a move out
//! of Connected to Done, Quotes ready and Closed lands in DONE, NO and the third figure, in the
//! month of that move, credited only to the business whose pick made that connection; and that the
//! billing page lists a saved card's brand and last four only, never a removed card or a `pm_` ID.
//! Rows are seeded straight into each module's tables (fake IDs only); the readers run as the app
//! role.

use askhava_integration::set_app_role;
use askhava_jobs::api::{Finished, finished_counts};
use askhava_payments::api::{business_statement, saved_cards};
use askhava_picks::api::pick_numbers;
use askhava_portals::api::{PaymentMethod, billing_page};
use askhava_quotes::api::{median_tenths, quote_numbers};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// November 2026 in Arizona so far: from the 1st at midnight (07:00 UTC) to the 20th, noon.
fn november() -> (Timestamp, Timestamp) {
    (at("2026-11-01T07:00:00Z"), at("2026-11-20T19:00:00Z"))
}

/// October 2026 in Arizona.
fn october() -> (Timestamp, Timestamp) {
    (at("2026-10-01T07:00:00Z"), at("2026-11-01T07:00:00Z"))
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

/// Runs one seeding statement.
async fn run(db: &mut PgConnection, sql: &'static str, args: (Uuid, Uuid, &str)) {
    sqlx::query(sql)
        .bind(args.0)
        .bind(args.1)
        .bind(args.2.to_owned())
        .execute(db)
        .await
        .expect("seed");
}

/// A job card for `business` on `job`, sent at `sent`.
async fn card(db: &mut PgConnection, job: Uuid, business: Uuid, sent: &str) {
    let sql = "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
               VALUES ($1, $2, false, $3::timestamptz)";
    run(db, sql, (job, business, sent)).await;
}

/// A firm quote by `business` on `job`, received at `received`.
async fn quote(db: &mut PgConnection, job: Uuid, business: Uuid, received: &str) {
    let sql = "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents,
                 confirm_on_site, notes, original, ai_read, confirmed, received_at)
               VALUES ($1, $2, 'firm', 10000, false, '', 'Fake quote', false, true,
                 $3::timestamptz)";
    run(db, sql, (job, business, received)).await;
}

/// The sent card of `job` in trade `category`, sent at `sent`.
async fn shown(db: &mut PgConnection, job: Uuid, category: &str, sent: &str) {
    let sql = "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place,
                 photo_ids, category, candidates, emergency, recorded_at, sent_at)
               VALUES ($1, $2, '0123456789abcdef0123456789abcdef', 'Fake card', 'Fake Town',
                 'house', '{}', split_part($3, '|', 1), '{}', false,
                 split_part($3, '|', 2)::timestamptz, split_part($3, '|', 2)::timestamptz)";
    let both = format!("{category}|{sent}");
    run(db, sql, (Uuid::new_v4(), job, &both)).await;
}

/// A page pick of `business` on `job` with `status`, picked at `picked`; returns its ID.
async fn pick(db: &mut PgConnection, job: Uuid, business: Uuid, status_at: (&str, &str)) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
         VALUES ($1, $2, $3, 1, 'page', $4, $5::timestamptz)",
    )
    .bind(id)
    .bind(job)
    .bind(business)
    .bind(status_at.0)
    .bind(status_at.1)
    .execute(db)
    .await
    .expect("pick");
    id
}

/// A paid pick of `business` on `job` released (the job connected) at `released`.
async fn connect(db: &mut PgConnection, job: Uuid, business: Uuid, released: &str) {
    let id = pick(db, job, business, ("paid", released)).await;
    let sql = "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at,
                 contact_id, mail_chosen)
               SELECT $1, job_id, business_id, 'paid', $3::timestamptz, gen_random_uuid(), false
                 FROM picks.pick WHERE id = $1 AND $2 = business_id";
    run(db, sql, (id, business, released)).await;
}

/// A job whose history is `moves` (from, to, at), version 1 upward.
async fn job_with(db: &mut PgConnection, moves: &[(&str, &str, &str)]) -> Uuid {
    let job = Uuid::new_v4();
    let (_, last, when) =
        moves
            .last()
            .copied()
            .unwrap_or(("intake", "intake", "2026-10-01T00:00:00Z"));
    let sql = "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from)
               VALUES ($1, split_part($3, '|', 1), split_part($3, '|', 2)::timestamptz,
                 split_part($3, '|', 2)::timestamptz, split_part($3, '|', 2)::timestamptz)
               ";
    run(db, sql, (job, job, &format!("{last}|{when}"))).await;
    for (version, (from, to, moved)) in (1..).zip(moves) {
        sqlx::query(
            "INSERT INTO jobs.status_history (job_id, version, from_status, to_status, at)
             VALUES ($1, $2, $3, $4, $5::timestamptz)",
        )
        .bind(job)
        .bind(version)
        .bind(from)
        .bind(to)
        .bind(moved)
        .execute(&mut *db)
        .await
        .expect("history");
    }
    job
}

/// Up to Connected at `connected`, then out to `to` at `out`.
fn connected_then<'a>(
    connected: &'a str,
    to: &'a str,
    out: &'a str,
) -> Vec<(&'a str, &'a str, &'a str)> {
    vec![
        ("intake", "open", "2026-10-01T00:00:00Z"),
        ("open", "quotes_ready", "2026-10-01T01:00:00Z"),
        ("quotes_ready", "awaiting_confirm", "2026-10-01T02:00:00Z"),
        ("awaiting_confirm", "payment_pending", connected),
        ("payment_pending", "connected", connected),
        ("connected", to, out),
    ]
}

/// The panel's figures for `business` in `month`.
async fn finished(pool: &PgPool, business: Uuid, month: (Timestamp, Timestamp)) -> Finished {
    let mut db = app(pool).await;
    let picks = pick_numbers(&mut db, business, month).await.expect("picks");
    finished_counts(&mut db, &picks.connected, month)
        .await
        .expect("finished")
}

/// BIZ-025, D-331: cards, cards ended without a quote and quotes count this business's own rows
/// in this month only; a card still inside its 24 hours has not ended.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn cards_and_quotes_count_this_month_and_this_business_only(pool: PgPool) {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let jobs: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let mut db = pool.acquire().await.expect("connection");
    card(&mut db, jobs[0], a, "2026-11-02T15:00:00Z").await;
    quote(&mut db, jobs[0], a, "2026-11-03T15:00:00Z").await;
    card(&mut db, jobs[1], a, "2026-11-10T15:00:00Z").await;
    card(&mut db, jobs[2], a, "2026-11-20T08:00:00Z").await;
    card(&mut db, jobs[3], a, "2026-10-20T15:00:00Z").await;
    quote(&mut db, jobs[3], a, "2026-10-20T16:00:00Z").await;
    card(&mut db, jobs[1], b, "2026-11-10T15:00:00Z").await;
    quote(&mut db, jobs[1], b, "2026-11-10T16:00:00Z").await;
    let mut reader = app(&pool).await;
    let mine = quote_numbers(&mut reader, a, november())
        .await
        .expect("numbers");
    assert_eq!(
        (mine.cards, mine.cards_without_quote, mine.quotes),
        (3, 1, 1)
    );
    let last = quote_numbers(&mut reader, a, october())
        .await
        .expect("numbers");
    assert_eq!(
        (last.cards, last.cards_without_quote, last.quotes),
        (1, 0, 1)
    );
    let other = quote_numbers(&mut reader, b, november())
        .await
        .expect("numbers");
    assert_eq!(
        (other.cards, other.cards_without_quote, other.quotes),
        (1, 0, 1)
    );
}

/// D-331: the median quotes per job is pure: odd count the middle, even the mean of the two.
#[test]
fn the_median_in_tenths_for_odd_and_even_counts() {
    assert_eq!(median_tenths(vec![5, 0, 2]), Some(20));
    assert_eq!(median_tenths(vec![4, 1]), Some(25));
    assert_eq!(median_tenths(vec![0, 1, 3, 4]), Some(20));
    assert_eq!(median_tenths(vec![]), None);
}

/// Adds `n` quotes by fresh fake businesses on `job` in November.
async fn quotes_on(db: &mut PgConnection, job: Uuid, n: usize) {
    for _ in 0..n {
        quote(db, job, Uuid::new_v4(), "2026-11-05T15:00:00Z").await;
    }
}

/// D-331: per trade of this business's cards, the median over every job of that trade sent this
/// month: plumbing [0, 2, 5] (odd) is 2.0, roofing [1, 4] (even) is 2.5; a trade it got no card
/// in and last month's jobs are not counted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn typical_quotes_per_trade_is_the_median(pool: PgPool) {
    let a = Uuid::new_v4();
    let mut db = pool.acquire().await.expect("connection");
    let sent = "2026-11-04T15:00:00Z";
    let plan = [
        ("plumbing", 2, true),
        ("plumbing", 0, true),
        ("plumbing", 5, false),
        ("roofing", 1, true),
        ("roofing", 4, false),
        ("electrical", 3, false),
    ];
    for (trade, n, mine) in plan {
        let job = Uuid::new_v4();
        shown(&mut db, job, trade, sent).await;
        if mine {
            card(&mut db, job, a, sent).await;
        }
        quotes_on(&mut db, job, n).await;
    }
    let old = Uuid::new_v4();
    shown(&mut db, old, "plumbing", "2026-10-10T15:00:00Z").await;
    quotes_on(&mut db, old, 9).await;
    let mut reader = app(&pool).await;
    let mine = quote_numbers(&mut reader, a, november())
        .await
        .expect("numbers");
    let want = vec![("plumbing".to_owned(), 20), ("roofing".to_owned(), 25)];
    assert_eq!(mine.typical_quotes, want);
}

/// BIZ-025, D-284: picks, confirms and lapsed picks of this month and business only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn picks_confirms_and_lapsed_picks(pool: PgPool) {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut db = pool.acquire().await.expect("connection");
    let rows = [
        (a, "paid", "2026-11-05T15:00:00Z"),
        (a, "free", "2026-11-06T15:00:00Z"),
        (a, "expired", "2026-11-07T15:00:00Z"),
        (a, "awaiting_confirm", "2026-11-08T15:00:00Z"),
        (a, "paid", "2026-10-08T15:00:00Z"),
        (a, "expired", "2026-10-09T15:00:00Z"),
        (b, "paid", "2026-11-05T15:00:00Z"),
        (b, "expired", "2026-11-05T16:00:00Z"),
    ];
    for (business, status, picked) in rows {
        pick(&mut db, Uuid::new_v4(), business, (status, picked)).await;
    }
    let mut reader = app(&pool).await;
    let mine = pick_numbers(&mut reader, a, november())
        .await
        .expect("numbers");
    assert_eq!((mine.picks, mine.confirms, mine.lapsed), (4, 2, 1));
    let last = pick_numbers(&mut reader, a, october())
        .await
        .expect("numbers");
    assert_eq!((last.picks, last.confirms, last.lapsed), (2, 1, 1));
}

/// D-250, D-143: Connected to Done, Quotes ready and Closed land in DONE, NO and the third
/// figure; a job another business's pick connected lands in none of this business's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_move_out_of_connected_lands_in_its_figure(pool: PgPool) {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut db = pool.acquire().await.expect("connection");
    let connected = "2026-11-03T15:00:00Z";
    for (to, business) in [("done", a), ("quotes_ready", a), ("closed", a), ("done", b)] {
        let job = job_with(
            &mut db,
            &connected_then(connected, to, "2026-11-09T15:00:00Z"),
        )
        .await;
        connect(&mut db, job, business, connected).await;
    }
    let mine = finished(&pool, a, november()).await;
    assert_eq!((mine.done, mine.no, mine.other), (1, 1, 1));
    let other = finished(&pool, b, november()).await;
    assert_eq!((other.done, other.no, other.other), (1, 0, 0));
}

/// D-250: A's pick connects, NO returns the job to Quotes ready, B's pick connects it again and
/// it finishes: A gets NO, B gets DONE.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_reconnect_under_another_pick_is_never_credited_to_the_first(pool: PgPool) {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut db = pool.acquire().await.expect("connection");
    let (first, second) = ("2026-11-03T15:00:00Z", "2026-11-07T15:00:00Z");
    let mut moves = connected_then(first, "quotes_ready", "2026-11-05T15:00:00Z");
    moves.extend([
        ("quotes_ready", "awaiting_confirm", "2026-11-06T15:00:00Z"),
        ("awaiting_confirm", "payment_pending", second),
        ("payment_pending", "connected", second),
        ("connected", "done", "2026-11-12T15:00:00Z"),
    ]);
    let job = job_with(&mut db, &moves).await;
    connect(&mut db, job, a, first).await;
    connect(&mut db, job, b, second).await;
    let mine = finished(&pool, a, november()).await;
    assert_eq!((mine.done, mine.no, mine.other), (0, 1, 0));
    let other = finished(&pool, b, november()).await;
    assert_eq!((other.done, other.no, other.other), (1, 0, 0));
}

/// D-250: a job picked on the last day of October and finished on November 2nd counts in
/// November only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_finish_counts_in_the_month_it_happened(pool: PgPool) {
    let a = Uuid::new_v4();
    let mut db = pool.acquire().await.expect("connection");
    let connected = "2026-10-31T18:00:00Z";
    let moves = connected_then(connected, "done", "2026-11-02T18:00:00Z");
    let job = job_with(&mut db, &moves).await;
    connect(&mut db, job, a, connected).await;
    let now = finished(&pool, a, november()).await;
    assert_eq!((now.done, now.no, now.other), (1, 0, 0));
    let before = finished(&pool, a, october()).await;
    assert_eq!(before, Finished::default());
}

/// Saves a fake card (brand, last four) for `business`, removed when `removed`.
async fn save_card(db: &mut PgConnection, business: Uuid, card: (&str, &str), removed: bool) {
    sqlx::query(
        "INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(business)
    .bind(format!("cus_fake{}", business.simple()))
    .execute(&mut *db)
    .await
    .expect("customer");
    let auth: Uuid = sqlx::query_scalar(
        "INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
         VALUES ($1, 1, 'Fake authorization', '2026-11-01T15:00:00Z') RETURNING id",
    )
    .bind(business)
    .fetch_one(&mut *db)
    .await
    .expect("authorization");
    sqlx::query(
        "INSERT INTO payments.payment_method (stripe_payment_method_id, business_id,
           authorization_id, method_type, usage, brand, last4, saved_at, removed_at)
         VALUES ($1, $2, $3, 'card', 'off_session', $4, $5, '2026-11-01T15:00:00Z',
           CASE WHEN $6 THEN '2026-11-02T15:00:00Z'::timestamptz END)",
    )
    .bind(format!("pm_fake{}", Uuid::new_v4().simple()))
    .bind(business)
    .bind(auth)
    .bind(card.0)
    .bind(card.1)
    .bind(removed)
    .execute(db)
    .await
    .expect("card");
}

/// FEE-014, BIZ-015: only the business's own saved cards not removed, brand and last four only,
/// and the billing page never shows a `pm_` ID.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn saved_cards_show_brand_and_last_four_only(pool: PgPool) {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut db = pool.acquire().await.expect("connection");
    save_card(&mut db, a, ("visa", "4242"), false).await;
    save_card(&mut db, a, ("mastercard", "4444"), true).await;
    save_card(&mut db, b, ("amex", "0005"), false).await;
    let mut reader = app(&pool).await;
    let cards = saved_cards(&mut reader, a).await.expect("cards");
    let shown: Vec<(&str, &str)> = cards
        .iter()
        .map(|c| (c.brand.as_str(), c.last4.as_str()))
        .collect();
    assert_eq!(shown, vec![("visa", "4242")]);
    let methods: Vec<PaymentMethod> = cards
        .into_iter()
        .map(|c| PaymentMethod {
            brand: c.brand,
            last4: c.last4,
        })
        .collect();
    let statement = business_statement(&pool, a, november())
        .await
        .expect("statement");
    let html = billing_page(&statement, &methods, None).expect("page");
    assert!(html.contains("4242"));
    assert!(!html.contains("4444") && !html.contains("0005") && !html.contains("pm_"));
}
