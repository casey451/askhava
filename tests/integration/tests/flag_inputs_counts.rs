//! TD-OPS-08's crowded-trade, lapsed-pick and repeat-request inputs (T619; OPS-027, OPS-034,
//! PICK-019; D-284, D-464, D-4280, D-4311) on a real database: each reader counts its planted
//! rows inside the half-open window and none outside it, at its boundary (a median of 4 quotes
//! against 3, 3 lapsed picks against 2, 3 requests against 2), and returns IDs and counts only.
//! Fake IDs and times only; no contact detail is planted.

use askhava_intake::api::repeat_requesters;
use askhava_picks::api::{lapsed_businesses, picked_jobs, wins};
use askhava_quotes::api::{MonthQuotes, TradeJobs, jobs_by_trade, month_quotes};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// November 2026 starts at 00:00 Arizona time (UTC-7).
const FROM: &str = "2026-11-01T07:00:00Z";
/// December 2026 starts at 00:00 Arizona time.
const TO: &str = "2026-12-01T07:00:00Z";

fn from() -> Timestamp {
    FROM.parse().expect("instant")
}

fn to() -> Timestamp {
    TO.parse().expect("instant")
}

/// Day `d` of the month at hour `h` (a negative day is before it).
fn day(d: i64, h: i64) -> Timestamp {
    from() + SignedDuration::from_hours(d * 24 + h)
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// A result's debug text holds no contact field (PRIV-028).
fn ids_and_counts_only(result: &str) {
    for word in ["phone", "name", "address", "email", "+1"] {
        assert!(!result.contains(word), "{word} in {result}");
    }
}

/// A job's sent card in `trade` at `at`.
async fn card(db: &mut PgConnection, job: u128, trade: &str, at: Timestamp) {
    sqlx::query(
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
         photo_ids, category, candidates, emergency, recorded_at, sent_at) VALUES ($1, $1, \
         repeat('a', 32), 'Fake card.', 'Lake Havasu City', 'house', '{}', $2, '{}', false, $3, \
         $3)",
    )
    .bind(id(job))
    .bind(trade)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("card");
}

/// A quote by `business` on `job` received at `at`, confirmed or not.
async fn quote(db: &mut PgConnection, (job, business): (u128, u128), at: Timestamp, ok: bool) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 20000, false, '', '', false, $3, $4)",
    )
    .bind(id(job))
    .bind(id(business))
    .bind(ok)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("quote");
}

/// `n` confirmed quotes on `job` on day 3, by businesses 1 to `n`.
async fn quotes(db: &mut PgConnection, job: u128, n: u128) {
    for business in 1..=n {
        quote(db, (job, business), day(3, 9), true).await;
    }
}

/// A pick of `business`'s quote on `job` at `at`, in `status`.
async fn pick(db: &mut PgConnection, (job, business): (u128, u128), at: Timestamp, status: &str) {
    sqlx::query(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4)",
    )
    .bind(id(job))
    .bind(id(business))
    .bind(status)
    .bind(at.to_sqlx())
    .execute(db)
    .await
    .expect("pick");
}

/// Plumbing jobs 1 to 3 (4, 4 and 5 confirmed quotes: median 4) and 5 (carded the month before,
/// 9 quotes). Roofing job 11 (3 confirmed, 1 unconfirmed), 12 (4 confirmed, a fifth received
/// exactly at the month's end), 13 (carded exactly at its start, no quote; median 3 of 0, 3 and
/// 4) and 16 (carded exactly at the month's end, 9 quotes).
async fn plant_quotes(db: &mut PgConnection) {
    for (job, trade, at) in [
        (1, "plumbing", day(2, 8)),
        (2, "plumbing", day(10, 8)),
        (3, "plumbing", day(29, 8)),
        (5, "plumbing", day(-1, 23)),
        (11, "roofing", day(1, 8)),
        (12, "roofing", day(2, 8)),
        (13, "roofing", from()),
        (16, "roofing", to()),
    ] {
        card(db, job, trade, at).await;
    }
    for (job, n) in [(1, 4), (2, 4), (3, 5), (5, 9), (11, 3), (12, 4), (16, 9)] {
        quotes(db, job, n).await;
    }
    quote(db, (11, 4), day(3, 10), false).await;
    quote(db, (12, 5), to(), true).await;
}

async fn trade_jobs(db: &mut PgConnection, trade: &str) -> Vec<Uuid> {
    let all: Vec<TradeJobs> = jobs_by_trade(db, to()).await.expect("trades");
    all.into_iter()
        .find(|t| t.trade == trade)
        .map(|t| t.jobs)
        .unwrap_or_default()
}

/// TD-OPS-08 (OPS-027, D-464, D-4311): plumbing's median job got 4 confirmed quotes in the month
/// and roofing's 3: the unconfirmed quote, the quote at the month's end and the jobs carded
/// outside the month are left out, and a carded job with no quote counts 0.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_median_four_quotes_against_three(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_quotes(&mut db).await;
    let plumbing = trade_jobs(&mut db, "plumbing").await;
    assert_eq!(plumbing, vec![id(1), id(2), id(3), id(5)]);
    let got = month_quotes(&mut db, &plumbing, (from(), to())).await;
    let want = MonthQuotes {
        jobs: 3,
        median_tenths: 40,
        confirmed_quotes: 13,
    };
    assert_eq!(
        got.expect("read"),
        Some(want),
        "job 5 was carded before the month"
    );
    let roofing = trade_jobs(&mut db, "roofing").await;
    assert_eq!(
        roofing,
        vec![id(11), id(12), id(13)],
        "job 16 is carded at the end"
    );
    let got = month_quotes(&mut db, &roofing, (from(), to())).await;
    let want = MonthQuotes {
        jobs: 3,
        median_tenths: 30,
        confirmed_quotes: 7,
    };
    let got = got.expect("read");
    assert_eq!(got, Some(want), "counts 0, 3 and 4: median 3");
    ids_and_counts_only(&format!("{got:?}"));
    let late = month_quotes(&mut db, &[id(16)], (from(), to())).await;
    assert_eq!(late.expect("read"), None, "no job carded in the month");
}

/// Business 101: 3 lapsed picks in the window, one exactly at its start. Business 102: 2 in it,
/// one exactly at its end, one just before its start and one paid. Business 103: 2 lapsed and 1
/// cancelled in it.
async fn plant_lapses(db: &mut PgConnection) {
    for (job, business, at, status) in [
        (41, 101, from(), "expired"),
        (42, 101, day(5, 12), "expired"),
        (43, 101, day(29, 12), "expired"),
        (44, 102, day(3, 12), "expired"),
        (45, 102, day(4, 12), "expired"),
        (46, 102, to(), "expired"),
        (47, 102, from() - SignedDuration::from_secs(1), "expired"),
        (48, 102, day(6, 12), "paid"),
        (49, 103, day(7, 12), "expired"),
        (50, 103, day(8, 12), "expired"),
        (51, 103, day(9, 12), "cancelled"),
    ] {
        pick(db, (job, business), at, status).await;
    }
}

/// TD-OPS-08 (OPS-034, PICK-019, D-284): 3 lapsed picks in 30 days flag the business, 2 do not;
/// a pick at the window's end or before its start, or one not lapsed, never counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_three_lapsed_picks_against_two(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_lapses(&mut db).await;
    let window = (from(), day(30, 0));
    let got = lapsed_businesses(&mut db, window, 3).await.expect("read");
    assert_eq!(got, vec![(id(101), 3)]);
    ids_and_counts_only(&format!("{got:?}"));
    let got = lapsed_businesses(&mut db, window, 2).await.expect("read");
    assert_eq!(got, vec![(id(101), 3), (id(102), 2), (id(103), 2)]);
}

/// TD-OPS-08 (OPS-027, D-464): wins are the paid or free picks on the given jobs in the month;
/// a lapse, a pick exactly at the month's end, one just before its start and one on another
/// trade's job do not count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_wins_count_paid_and_free_picks_in_the_month(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    for (job, business, at, status) in [
        (1, 1, from(), "paid"),
        (2, 1, day(12, 12), "free"),
        (3, 1, day(14, 12), "expired"),
        (3, 2, to(), "paid"),
        (5, 1, from() - SignedDuration::from_secs(1), "paid"),
        (11, 1, day(12, 12), "paid"),
    ] {
        pick(&mut db, (job, business), at, status).await;
    }
    let plumbing = [id(1), id(2), id(3), id(5)];
    let got = wins(&mut db, &plumbing, (from(), to()))
        .await
        .expect("read");
    assert_eq!(got, 2);
    let none = wins(&mut db, &[id(4)], (from(), to())).await.expect("read");
    assert_eq!(none, 0);
}

/// TD-OPS-08 (OPS-034): `picked_jobs` names the given jobs with any pick row, whatever its status
/// or time, and no job it was not given.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_picked_jobs_are_those_with_any_pick_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    pick(&mut db, (61, 1), day(1, 12), "cancelled").await;
    pick(&mut db, (62, 1), day(-40, 12), "expired").await;
    pick(&mut db, (63, 1), day(2, 12), "awaiting_confirm").await;
    pick(&mut db, (64, 1), day(2, 12), "paid").await;
    let asked = [id(61), id(62), id(63), id(65)];
    let got = picked_jobs(&mut db, &asked).await.expect("read");
    assert_eq!(
        got,
        vec![id(61), id(62), id(63)],
        "job 64 was not asked, 65 has no pick"
    );
}

/// A request by the homeowner whose vault contact ID is `contact`, opened at `at` and closed an
/// hour later, on `job` when carded.
async fn request(db: &mut PgConnection, contact: u128, at: Timestamp, job: Option<u128>) {
    let home: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, $2) \
         ON CONFLICT (contact_id) DO UPDATE SET contact_id = EXCLUDED.contact_id RETURNING id",
    )
    .bind(id(contact))
    .bind(day(-60, 0).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("homeowner");
    sqlx::query(
        "INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at, closed_at) \
         VALUES ($1, $2, 'en', $3, 'consent_first', $4, $4, $5)",
    )
    .bind(home)
    .bind(job.map(id))
    .bind(Uuid::new_v4())
    .bind(at.to_sqlx())
    .bind((at + SignedDuration::from_hours(1)).to_sqlx())
    .execute(db)
    .await
    .expect("request");
}

/// Homeowner 201: 3 requests, one exactly at the window's start, one carded with no pick.
/// 202: 2 in the window, one exactly at its end and one just before its start. 203: 3, one of
/// them picked (and lapsed).
async fn plant_requests(db: &mut PgConnection) {
    for (contact, at, job) in [
        (201, from(), None),
        (201, day(4, 9), Some(71)),
        (201, day(20, 9), None),
        (202, day(2, 9), None),
        (202, day(3, 9), None),
        (202, day(30, 0), None),
        (202, from() - SignedDuration::from_secs(1), None),
        (203, day(5, 9), None),
        (203, day(6, 9), Some(72)),
        (203, day(7, 9), None),
    ] {
        request(db, contact, at, job).await;
    }
    pick(db, (72, 1), day(6, 12), "expired").await;
}

/// TD-OPS-08 (OPS-034, D-284, D-4311): 3 requests in 30 days that never picked flag the
/// homeowner by her vault ID, 2 do not, and 3 with a pick on one of them do not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_three_requests_with_no_pick_against_two(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    plant_requests(&mut db).await;
    let window = (from(), day(30, 0));
    let got = repeat_requesters(&mut db, window, 3).await.expect("read");
    assert_eq!(got, vec![(id(201), 3)]);
    ids_and_counts_only(&format!("{got:?}"));
    let got = repeat_requesters(&mut db, window, 2).await.expect("read");
    assert_eq!(got, vec![(id(201), 3), (id(202), 2)], "203 picked");
}
