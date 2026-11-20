//! The monthly intake accuracy report and the owner money view pages on PostgreSQL (OPS-040,
//! FEE-046, FEE-028; T434, D-3155, D-3753, D-3890, D-493).
//!
//! TD-OPS-16: `quotes::api::intake_report_ids` lists the month's cards that got no quote and the
//! "need more info" quotes (confirm-on-site ticked or the diagnostic-visit format count, one with
//! neither does not, a withdrawn one counts in neither list, D-3155); `jobs::api::report_facts`
//! gives each job's latest facts row by `seq`; `portals::api::intake_report_page` groups them by
//! category and question set and shows the latest stored golden-set run against the 90% target
//! setting. The money view page shows T210's four lines and the month's expenses; its expense
//! `POST` stores a good entry and refuses a bad one with nothing stored; the CSV is T210's.
//! Every ID, vendor and note is fake.

use std::collections::BTreeSet;

use askhava_catalog::api::{load_embedded, question_bank};
use askhava_core::api::{DEFAULT_GOLDEN_TARGET_PERCENT, Settings};
use askhava_integration::set_app_role;
use askhava_jobs::api::report_facts;
use askhava_payments::api::CSV_HEADER;
use askhava_portals::api::{
    ExpenseAnswer, ExpenseForm, OWNER_MONEY_ROUTE, intake_report_page, money_view_csv_file,
    money_view_page, post_expense,
};
use askhava_quotes::api::intake_report_ids;
use jiff::Timestamp;
use jiff::civil::Date;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The pages' "now": Friday Nov 20, 2026, noon in Arizona.
const NOW: &str = "2026-11-20T19:00:00Z";

fn now() -> Timestamp {
    NOW.parse().expect("now")
}

fn november() -> Date {
    Date::new(2026, 11, 1).expect("date")
}

/// Runs one seeding statement with text arguments; the SQL casts each.
async fn seed(db: &mut PgConnection, sql: &'static str, args: &[&str]) {
    let mut q = sqlx::query(sql);
    for a in args {
        q = q.bind((*a).to_owned());
    }
    q.execute(db).await.expect("seed");
}

/// A job in `category` whose facts narrowed from `first` to `latest` job types, with one card sent
/// at `sent`; returns the job ID.
async fn job(db: &mut PgConnection, category: &str, types: (&str, &str), sent: &str) -> Uuid {
    let id = Uuid::new_v4().to_string();
    seed(
        db,
        "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from)
         VALUES ($1::uuid, 'quotes_ready', $2::timestamptz, $2::timestamptz, $2::timestamptz)",
        &[&id, sent],
    )
    .await;
    for (seq, list) in [("1", types.0), ("2", types.1)] {
        seed(
            db,
            "INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
             VALUES ($1::uuid, $2::int, $3, string_to_array($4, ','), $5::timestamptz)",
            &[&id, seq, category, list, sent],
        )
        .await;
    }
    seed(
        db,
        "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
         VALUES ($1::uuid, gen_random_uuid(), false, $2::timestamptz)",
        &[&id, sent],
    )
    .await;
    id.parse().expect("uuid")
}

/// A quote by the card's business on `job`: `format`, the confirm-on-site box, withdrawn or not.
async fn quote(db: &mut PgConnection, job: Uuid, format: &str, on_site: &str, withdrawn: &str) {
    seed(
        db,
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site,
           notes, original, ai_read, confirmed, received_at, withdrawn_at)
         SELECT job_id, business_id, $2, 10000, $3::bool, 'Fake note: need more info', 'Fake quote',
           false, true, sent_at + interval '1 hour',
           CASE WHEN $4::bool THEN sent_at + interval '2 hours' END
           FROM quotes.job_card WHERE job_id = $1::uuid",
        &[&job.to_string(), format, on_site, withdrawn],
    )
    .await;
}

/// The planted month: what each list must hold.
struct Planted {
    no_quote: Vec<Uuid>,
    more_info: Vec<Uuid>,
}

async fn plant(db: &mut PgConnection) -> Planted {
    load_embedded(db).await.expect("catalog");
    let at = "2026-11-05T17:00:00Z";
    let silent = job(db, "plumbing_repair", ("leak,drain", "leak"), at).await;
    let withdrawn = job(db, "plumbing_repair", ("drain", "drain"), at).await;
    quote(db, withdrawn, "firm", "true", "true").await;
    let ticked = job(db, "hvac_repair", ("no_cool", "no_cool"), at).await;
    quote(db, ticked, "firm", "true", "false").await;
    let visit = job(db, "hvac_repair", ("no_heat", "no_heat"), at).await;
    quote(db, visit, "diagnostic_visit", "false", "false").await;
    let plain = job(db, "hvac_repair", ("no_cool", "no_cool"), at).await;
    quote(db, plain, "firm", "false", "false").await;
    let october = job(
        db,
        "plumbing_repair",
        ("leak", "leak"),
        "2026-10-20T17:00:00Z",
    )
    .await;
    quote(db, october, "diagnostic_visit", "false", "false").await;
    job(
        db,
        "plumbing_repair",
        ("leak", "leak"),
        "2026-11-20T10:00:00Z",
    )
    .await;
    Planted {
        no_quote: vec![silent, withdrawn],
        more_info: vec![ticked, visit],
    }
}

async fn app_conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

/// TD-OPS-16: the readers list the right IDs, and the latest facts row by `seq` gives the types.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_16_the_readers_list_no_quote_cards_and_more_info_quotes(pool: PgPool) {
    let planted = plant(&mut pool.acquire().await.expect("connection")).await;
    let mut conn = app_conn(&pool).await;
    let start: Timestamp = "2026-11-01T07:00:00Z".parse().expect("start");
    let end: Timestamp = "2026-12-01T07:00:00Z".parse().expect("end");
    let ids = intake_report_ids(&mut conn, (start, end), now())
        .await
        .expect("ids");
    let set = |v: &[Uuid]| v.iter().copied().collect::<BTreeSet<Uuid>>();
    assert_eq!(ids.no_quote_jobs.len(), 2);
    assert_eq!(set(&ids.no_quote_jobs), set(&planted.no_quote));
    let more: Vec<Uuid> = ids.more_info.iter().map(|q| q.job).collect();
    assert_eq!((more.len(), set(&more)), (2, set(&planted.more_info)));
    let facts = report_facts(&mut conn, &planted.no_quote)
        .await
        .expect("facts");
    let first = &facts[&planted.no_quote[0]];
    assert_eq!(
        (first.category.as_str(), first.job_types.clone()),
        ("plumbing_repair", vec!["leak".to_owned()])
    );
}

/// TD-OPS-16, D-4858: a card sent in the last hours of a month with no quote is counted in its own
/// month once its 24 hours are over, never dropped at the month edge.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_16_a_card_at_the_month_edge_counts_in_its_own_month(pool: PgPool) {
    let late = job(
        &mut pool.acquire().await.expect("connection"),
        "plumbing_repair",
        ("leak", "leak"),
        "2026-11-01T01:00:00Z",
    )
    .await;
    let mut conn = app_conn(&pool).await;
    let start: Timestamp = "2026-10-01T07:00:00Z".parse().expect("start");
    let end: Timestamp = "2026-11-01T07:00:00Z".parse().expect("end");
    let ids = intake_report_ids(&mut conn, (start, end), now())
        .await
        .expect("ids");
    assert_eq!(ids.no_quote_jobs, vec![late]);
}

/// TD-OPS-16: the page groups by category and question set and shows the latest golden run.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_16_the_page_groups_by_category_and_question_set(pool: PgPool) {
    plant(&mut pool.acquire().await.expect("connection")).await;
    let mut conn = app_conn(&pool).await;
    let html = intake_report_page(&mut conn, november(), 90, now())
        .await
        .expect("page");
    let bank = question_bank().expect("bank");
    let set = |c: &str| format!("Version {}", bank.sets[c].version);
    let plumbing = format!(
        "<tr><th scope=\"row\">Plumbing repair</th><td>{}</td><td>drain, leak</td><td>2</td><td>0</td></tr>",
        set("plumbing_repair")
    );
    let hvac = format!(
        "<tr><th scope=\"row\">Heating and cooling repair</th><td>{}</td><td>no_cool, no_heat</td><td>0</td><td>2</td></tr>",
        set("hvac_repair")
    );
    assert!(html.contains(&plumbing), "{html}");
    assert!(html.contains(&hvac), "{html}");
    assert!(html.contains("checked yet. The goal is 90%."), "{html}");
}

/// TD-OPS-16, D-493: the latest stored run's share is shown against the target, not an older one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_16_the_latest_golden_share_is_shown_against_the_target(pool: PgPool) {
    let mut seed_conn = pool.acquire().await.expect("connection");
    for (version, right, at) in [
        ("v-old", "50", "2026-11-01T10:00:00Z"),
        ("v-new", "93", "2026-11-10T10:00:00Z"),
    ] {
        seed(
            &mut seed_conn,
            "INSERT INTO intake.golden_result (catalog_version, rows_total, rows_right, ran_at)
             VALUES ($1, 100, $2::int, $3::timestamptz)",
            &[version, right, at],
        )
        .await;
    }
    let mut conn = app_conn(&pool).await;
    let met = intake_report_page(&mut conn, november(), 90, now())
        .await
        .expect("page");
    assert!(
        met.contains("93% of the sample requests") && met.contains("93 of 100"),
        "{met}"
    );
    assert!(
        met.contains("The goal is 90%.") && met.contains("That's on target."),
        "{met}"
    );
    assert!(!met.contains("50%"), "{met}");
    let below = intake_report_page(&mut conn, november(), 95, now())
        .await
        .expect("page");
    assert!(below.contains("That's short of the goal."), "{below}");
}

/// OPS-040: the target is a setting, 90 when unset, refused by name outside 1 to 100.
#[test]
fn the_golden_target_is_a_typed_setting() {
    let parse = |v: Option<&str>| {
        let mut pairs = vec![
            ("ASKHAVA_APP_ENV".to_owned(), "dev".to_owned()),
            (
                "ASKHAVA_DATABASE_URL".to_owned(),
                "postgres://fake@localhost/fake".to_owned(),
            ),
        ];
        pairs.extend(v.map(|d| ("ASKHAVA_GOLDEN_TARGET_PERCENT".to_owned(), d.to_owned())));
        Settings::from_pairs(pairs)
    };
    assert_eq!(parse(None).expect("default").golden_target_percent(), 90);
    assert_eq!(DEFAULT_GOLDEN_TARGET_PERCENT, 90);
    assert_eq!(parse(Some("85")).expect("85").golden_target_percent(), 85);
    for bad in ["0", "101", "abc"] {
        let err = parse(Some(bad)).expect_err(bad).to_string();
        assert!(err.contains("ASKHAVA_GOLDEN_TARGET_PERCENT"), "{err}");
    }
}

fn form(amount: &str) -> ExpenseForm {
    ExpenseForm {
        spent_on: "2026-11-04".to_owned(),
        vendor: "Fake Hosting Co".to_owned(),
        amount: amount.to_owned(),
        category: "Software".to_owned(),
        note: "Fake monthly plan".to_owned(),
    }
}

async fn expenses(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM payments.expense")
        .fetch_one(pool)
        .await
        .expect("count")
}

/// FEE-046, FEE-028: a good entry is stored and shown with the four lines; no ledger row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn fee_046_the_expense_post_stores_and_the_page_shows_it(pool: PgPool) {
    let answer = post_expense(&pool, &form("$1,234.5"), (860, "fake-csrf"), now())
        .await
        .expect("post");
    assert_eq!(
        answer,
        ExpenseAnswer::Added(format!("{OWNER_MONEY_ROUTE}?month=2026-11"))
    );
    let html = money_view_page(&pool, november(), 860, "fake-csrf")
        .await
        .expect("page");
    for want in [
        "Money, November 2026",
        "Fake Hosting Co",
        "$1234.50",
        "<dt>Left after tax and expenses</dt><dd>-$1234.50</dd>",
        "fake-csrf",
    ] {
        assert!(html.contains(want), "{want}: {html}");
    }
    let ledger: i64 = sqlx::query_scalar("SELECT count(*) FROM payments.ledger_entry")
        .fetch_one(&pool)
        .await
        .expect("ledger");
    assert_eq!((expenses(&pool).await, ledger), (1, 0));
    let csv = money_view_csv_file(&pool, november(), 860)
        .await
        .expect("csv");
    assert!(
        csv.starts_with(CSV_HEADER) && csv.contains("Fake Hosting Co"),
        "{csv}"
    );
}

/// FEE-046: a bad amount or a future date stores nothing and shows the field's error, values kept.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn fee_046_a_bad_entry_is_refused_with_its_error(pool: PgPool) {
    let ExpenseAnswer::Refused(html) = post_expense(&pool, &form("12.345"), (860, "t"), now())
        .await
        .expect("post")
    else {
        panic!("a bad amount was stored");
    };
    assert!(
        html.contains("id=\"error-summary\"") && html.contains("href=\"#amount\""),
        "{html}"
    );
    assert!(html.contains("value=\"Fake Hosting Co\""), "{html}");
    let mut late = form("10");
    late.spent_on = "2026-11-21".to_owned();
    let answer = post_expense(&pool, &late, (860, "t"), now())
        .await
        .expect("post");
    assert!(matches!(answer, ExpenseAnswer::Refused(ref h) if h.contains("href=\"#spent_on\"")));
    assert_eq!(expenses(&pool).await, 0);
}
