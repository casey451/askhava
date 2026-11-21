//! The pilot scorecard's refund and dispute rates as a cohort (OPS-039, D-5081, T780). Of the
//! charges recorded in a window, the share with a refund or a dispute recorded by now: a refund in
//! week 2 of a week-1 charge counts against week 1, never week 2; no figure passes 100%; a week
//! with refunds and no charge shows their plain count and no rate. Every row is planted and fake:
//! numbered job and business IDs, no name, phone or address.

use askhava_core::api::Settings;
use askhava_payments::api::{charge_outcomes, ledger_totals};
use askhava_portals::api::{ScorecardInputs, ScorecardLines, owner_scorecard_page};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const HOUR: SignedDuration = SignedDuration::from_hours(1);
const WEEK: i64 = 24 * 7;

/// Fake Monday 08:00 Arizona time of week 1, plus `hours`.
fn at(hours: i64) -> Timestamp {
    let base: Timestamp = "2026-10-05T15:00:00Z".parse().expect("time");
    base + HOUR * i32::try_from(hours).expect("hours")
}

/// Thursday noon of week 2, when the page is read.
fn now() -> Timestamp {
    at(WEEK + 76)
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// One ledger entry of `kind` on fake job `job` and business `job + 100`, recorded at `h` hours.
async fn entry(db: &mut PgConnection, kind: &str, job: u128, h: i64) {
    sqlx::query(
        "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, \
         idempotency_key, recorded_at) VALUES ($1, $2, $3, 7500, $4, $5)",
    )
    .bind(kind)
    .bind(id(job + 100))
    .bind(id(job))
    .bind(format!("t780.{kind}.{job}"))
    .bind(at(h).to_sqlx())
    .execute(db)
    .await
    .expect("entry");
}

/// Fake plumbing cards for `jobs`, all sent at the start of week 1.
async fn cards(db: &mut PgConnection, jobs: &[u128]) {
    for job in jobs {
        sqlx::query(
            "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
             photo_ids, category, candidates, emergency, recorded_at, sent_at) VALUES ($1, $1, \
             repeat('a', 32), 'Fake card.', 'Lake Havasu City', 'house', '{}', 'plumbing', '{}', \
             false, $2, $2)",
        )
        .bind(id(*job))
        .bind(at(0).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("card");
    }
}

fn inputs() -> ScorecardInputs {
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
    ];
    let settings =
        Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned()))).expect("settings");
    ScorecardInputs {
        lines: ScorecardLines::from_pairs(settings.scorecard_lines()).expect("lines"),
        set_aside_basis_points: settings.sales_tax_set_aside_basis_points(),
        pilot_start: at(-24),
        pushed: vec![],
    }
}

/// The row of `label` under "All trades".
fn row<'a>(html: &'a str, label: &str) -> &'a str {
    let start = html.find("<h2>All trades</h2>").expect("total");
    let table = &html[start..start + html[start..].find("</table>").expect("table")];
    let head = format!("<tr><th scope=\"row\">{label}</th>");
    table.lines().find(|l| l.starts_with(&head)).expect(label)
}

/// Every percent figure on the page.
fn percents(html: &str) -> Vec<f64> {
    let parts: Vec<&str> = html.split("% (").collect();
    parts[..parts.len() - 1]
        .iter()
        .filter_map(|p| {
            let digits = p.rsplit(|c: char| !(c.is_ascii_digit() || c == '.'));
            digits.into_iter().next().and_then(|d| d.parse().ok())
        })
        .collect()
}

const REFUNDS: &str = "Paid introductions refunded so far (recent weeks can still rise)";
const DISPUTES: &str =
    "Paid introductions with a bank dispute so far (recent weeks can still rise)";

/// D-5081: a week-2 refund and dispute of a week-1 charge count against week 1, not week 2.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_later_refund_counts_against_its_charges_window(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    entry(&mut db, "charge", 1, 2).await;
    entry(&mut db, "charge", 2, 3).await;
    entry(&mut db, "refund", 1, WEEK + 5).await;
    entry(&mut db, "dispute", 1, WEEK + 6).await;
    let jobs = [id(1), id(2)];
    let (week1, week2) = ((at(0), at(WEEK)), (at(WEEK), at(2 * WEEK)));
    let cohort1 = charge_outcomes(&mut db, &jobs, week1.0, week1.1)
        .await
        .expect("read");
    assert_eq!(
        (cohort1.charges, cohort1.refunded, cohort1.disputed),
        (2, 1, 1)
    );
    let cohort2 = charge_outcomes(&mut db, &jobs, week2.0, week2.1)
        .await
        .expect("read");
    assert_eq!(
        (cohort2.charges, cohort2.refunded, cohort2.disputed),
        (0, 0, 0)
    );
    let plain2 = ledger_totals(&mut db, &jobs, week2.0, week2.1)
        .await
        .expect("read")
        .expect("rows");
    assert_eq!((plain2.refunds, plain2.disputes), (1, 1));
}

/// D-5081: two refunds of earlier charges and one new charge read 0 of 1 this week, never 200%,
/// and no percent on the page passes 100.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn no_scorecard_figure_passes_100_percent(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    cards(&mut db, &[1, 2, 3]).await;
    entry(&mut db, "charge", 1, 2).await;
    entry(&mut db, "charge", 2, 3).await;
    entry(&mut db, "refund", 1, WEEK + 5).await;
    entry(&mut db, "refund", 2, WEEK + 6).await;
    entry(&mut db, "charge", 3, WEEK + 7).await;
    let html = owner_scorecard_page(&mut db, &inputs(), ("fake-csrf", None), now())
        .await
        .expect("page");
    let refunds = row(&html, REFUNDS);
    assert!(refunds.contains("<td>0.0% (0 of 1)"), "{refunds}");
    assert!(refunds.contains("66.7% (2 of 3)"), "{refunds}");
    let all = percents(&html);
    assert!(
        !all.is_empty() && all.iter().all(|p| *p <= 100.0),
        "{all:?}"
    );
}

/// D-5081: a week with refunds and a dispute but no charge shows their counts and no rate.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_week_with_refunds_and_no_charge_shows_the_count(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    cards(&mut db, &[1, 2]).await;
    entry(&mut db, "charge", 1, 2).await;
    entry(&mut db, "charge", 2, 3).await;
    entry(&mut db, "refund", 1, WEEK + 5).await;
    entry(&mut db, "refund", 2, WEEK + 6).await;
    entry(&mut db, "dispute", 2, WEEK + 7).await;
    let html = owner_scorecard_page(&mut db, &inputs(), ("fake-csrf", None), now())
        .await
        .expect("page");
    let refunds = row(&html, REFUNDS);
    assert!(
        refunds.contains("<td>No data yet (2 recorded in this period)</td>"),
        "{refunds}"
    );
    assert!(refunds.contains("100.0% (2 of 2)"), "{refunds}");
    let disputes = row(&html, DISPUTES);
    assert!(
        disputes.contains("<td>No data yet (1 recorded in this period)</td>"),
        "{disputes}"
    );
    assert!(disputes.contains("50.0% (1 of 2)"), "{disputes}");
}
