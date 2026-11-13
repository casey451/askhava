//! TD-FEE-62 (FEE-045, FEE-005, FEE-004, FEE-007; T340, D-2331, D-3547): on a fresh database with
//! the reviewed catalog and a small catalog fixture, the introduction fee by job size fixed at the
//! pick uses the job's latest stored category and candidate job types, resolved from the catalog,
//! through the real page-pick (`picks::api::page_pick`) and text-pick
//! (`intake::api::answer_text_pick`) paths. A fee-free job type is never charged and uses no free
//! first win; a non-emergency job with unsettled facts is refused before any pick or charge; an
//! emergency job is never refused for its facts and raises one High alert, by pick ID only.
//!
//! No Stripe call is made: a fee-free Confirm writes its $0 entry with no charge. Fake numbers,
//! IDs and wording only.

use askhava_catalog::api::{load, load_embedded, parse};
use askhava_core::api::Guarded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, PublishedTerms, TextIn, answer_text_pick};
use askhava_intake::api::{begin_job, receive_text};
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, Confirmed, FeeMode, FeeQuote, FeeTerms,
    JobFeeFacts, JoinDates, PickedFee, VisitTier, Waiver, WinCheck, accept_authorization,
    fee_at_pick, may_confirm, record_fee_at_confirm, waive_at_confirm,
};
use askhava_picks::api::{FEE_FACTS_ALERT, PagePick, PicksError, page_pick};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TERMS: PublishedTerms<'static> = PublishedTerms {
    version: "2026-11-01",
    text: "Sample homeowner terms for tests only.",
};
const EMBEDDED: &str = include_str!("../../../docs/catalog/categories.yaml");
/// The catalog fixture: a diagnostic-visit category whose set tier is $75, and one whose stored
/// tier ($50) is not a visit tier, so its facts never settle.
const FIXTURE: &str = "
not_offered_at_launch: []
categories:
  - id: sample_visit_trade
    name: Sample visit trade
    group: home systems/repair
    enabled: true
    license: {kind: none}
    handyman_allowed: false
    construction_type: false
    recurring: false
    trip_fee_tier: 75
    after_hours_emergency: true
    job_types: [sample visit job]
    quote_format: diagnostic_visit
  - id: sample_odd_tier
    name: Sample odd tier
    group: home systems/repair
    enabled: true
    license: {kind: none}
    handyman_allowed: false
    construction_type: false
    recurring: false
    trip_fee_tier: 50
    after_hours_emergency: false
    job_types: [sample odd job]
    quote_format: firm
";
const BUYER: &str = "buyer inspection";
const CONDITION: &str = "home condition inspection (no sale or refinance)";

/// One stored quote's price.
#[derive(Clone, Copy)]
enum Quoted {
    /// A firm price in cents.
    Firm(i64),
    /// A price per month in cents.
    Monthly(i64),
    /// A diagnostic-visit fee in cents.
    Visit(i64),
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-340".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_200_000 + minute * 60).expect("time")
}

fn quoter(n: i64) -> Uuid {
    Uuid::from_u128(0xb340_0100 + u128::try_from(n).expect("small"))
}

fn her(n: u128) -> Uuid {
    Uuid::from_u128(0x0340_0000 + n)
}

/// The reviewed catalog with the fixture categories added (the not-offered list kept).
async fn catalog(db: &mut PgConnection) {
    load_embedded(db).await.expect("catalog");
    let mut fixture = parse(FIXTURE).expect("fixture parses");
    fixture.not_offered_at_launch = parse(EMBEDDED).expect("catalog").not_offered_at_launch;
    load(db, &fixture).await.expect("fixture");
}

fn text_in(contact: Uuid, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128((contact.as_u128() << 32) + u128::from(minute.unsigned_abs())),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// Opens `her` request and one job (an emergency one when `emergency`) moved to Quotes ready,
/// with no facts row, and `quotes` by businesses 1, 2, ... in that arrival order.
async fn job_ready(db: &mut PgConnection, her: Uuid, emergency: bool, quotes: &[Quoted]) -> Uuid {
    let first = text_in(her, "sample request words for a test job", 0);
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: first.inbound.words,
        emergency,
        trade: "plumbing",
        at: at(1),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(2), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    for (n, quoted) in (1_i64..).zip(quotes) {
        insert_quote(db, job, n, *quoted).await;
    }
    job
}

async fn insert_quote(db: &mut PgConnection, job: Uuid, n: i64, quoted: Quoted) {
    let (format, cents, period) = match quoted {
        Quoted::Firm(c) => ("firm", c, None),
        Quoted::Monthly(c) => ("monthly", c, Some("month")),
        Quoted::Visit(c) => ("diagnostic_visit", c, None),
    };
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, period, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, $3, $4, $5, false, '', 'Sample wording', false, true, $6)",
    )
    .bind(job)
    .bind(quoter(n))
    .bind(format)
    .bind(cents)
    .bind(period)
    .bind(at(n + 3).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

/// Records `job`'s category and candidate job types (a new row or a narrowing).
async fn facts(db: &mut PgConnection, job: Uuid, category: &str, types: &[&str]) {
    let types: Vec<String> = types.iter().map(|t| (*t).to_owned()).collect();
    record_job_facts(db, job, category, &types)
        .await
        .expect("job facts");
}

/// The page pick of quote `n` on `job` in its own transaction, committed only when it picked.
async fn page_picked(pool: &PgPool, job: Uuid, n: usize) -> Result<PagePick, PicksError> {
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), n, Actor::System, at(20)).await;
    if picked.is_ok() {
        tx.commit().await.expect("commit");
    }
    picked
}

async fn page_pick_ok(pool: &PgPool, job: Uuid, n: usize) -> Uuid {
    let Ok(PagePick::Picked(pick)) = page_picked(pool, job, n).await else {
        panic!("a page pick");
    };
    pick
}

/// `her` texts "<n> YES" twice (the Terms line first, then the pick); the pick it stored, if any.
async fn text_picked(pool: &PgPool, her: Uuid, job: Uuid, n: usize) -> Option<Uuid> {
    let mut db = pool.acquire().await.expect("connection");
    let words = format!("{n} YES");
    for minute in [10, 11] {
        let msg = text_in(her, &words, minute);
        let settings = (Settings::default(), &TERMS);
        let _ = answer_text_pick(&mut db, &secret(), &msg, settings).await;
    }
    sqlx::query_scalar("SELECT id FROM picks.pick WHERE job_id = $1")
        .bind(job)
        .fetch_optional(pool)
        .await
        .expect("pick")
}

/// The stored fee of `pick`: its kind and amount in cents.
async fn stored_fee(pool: &PgPool, pick: Uuid) -> (String, i64) {
    sqlx::query_as("SELECT kind, amount_cents FROM payments.pick_fee WHERE pick_id = $1")
        .bind(pick)
        .fetch_one(pool)
        .await
        .expect("stored fee")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// The fee alerts: (severity, subject kind, subject ID).
async fn alerts(pool: &PgPool) -> Vec<(String, Option<String>, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT severity, subject_kind, subject_id FROM ops.alert WHERE kind = $1 \
         ORDER BY raised_at, id",
    )
    .bind(FEE_FACTS_ALERT)
    .fetch_all(pool)
    .await
    .expect("alerts")
}

fn fee_free() -> (String, i64) {
    ("fee_free".to_owned(), 0)
}

fn charged(cents: i64) -> (String, i64) {
    ("charged".to_owned(), cents)
}

/// Business 1 with a Stripe customer that accepted the authorization in force.
async fn ready(db: &mut PgConnection) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(quoter(1))
        .bind("cus_FakeT340")
        .execute(&mut *db)
        .await
        .expect("customer");
    let terms = FeeTerms::new(FeeMode::Tiers, AUTHORIZATION_VERSION);
    accept_authorization(db, BusinessId(quoter(1)), terms, AUTHORIZATION_TEXT, at(0))
        .await
        .expect("accepted");
}

/// Business 1's Confirm of `pick` on `job` up to the free-win decision: the proof, then
/// `waive_at_confirm` with `picked`; a fee-free pick that is not waived writes its $0 entry.
async fn confirm(db: &mut PgConnection, (pick, job): (Uuid, Uuid), picked: &PickedFee) -> Waiver {
    let business = BusinessId(quoter(1));
    let authorized = may_confirm(db, business, pick).await.expect("may confirm");
    let dates = JoinDates {
        joined: at(0),
        launch: at(0),
        trades_opened: Vec::new(),
    };
    let check = WinCheck {
        business,
        job,
        picked,
        dates: &dates,
        same_customer_jobs: &[],
    };
    let waiver = waive_at_confirm(db, &secret(), check)
        .await
        .expect("waiver");
    if waiver == Waiver::NotWaived && picked.cents() == 0 {
        let done = Confirmed {
            business,
            job,
            charge: None,
        };
        record_fee_at_confirm(db, &secret(), (pick, authorized), done)
            .await
            .expect("fee-free entry");
    }
    waiver
}

/// The fee `fee_at_pick` gives a firm quote of `cents` with `facts` in tiers mode.
fn firm_fee(cents: u64, facts: JobFeeFacts) -> PickedFee {
    let job = facts.period_job(false, false);
    fee_at_pick(FeeQuote::Fixed(cents), None, job, FeeMode::Tiers, None).expect("fee")
}

/// TD-FEE-62 (FEE-045, D-403, D-2331): a buyer inspection at any price is fee-free: $0 stored at
/// the pick, a $0 `fee-free job type` ledger entry at Confirm and no free first win used; the
/// same business's next ordinary job still takes its free first win.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_a_buyer_inspection_is_fee_free_and_keeps_the_free_first_win(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    ready(&mut db).await;
    let free = JobFeeFacts {
        visit_tier: VisitTier::DEFAULT,
        fee_free: true,
        one_time: false,
    };
    for (n, cents) in [(1, 19_000_i64), (2, 900_000)] {
        let job = job_ready(&mut db, her(n), false, &[Quoted::Firm(cents)]).await;
        facts(&mut db, job, "home_inspection", &[BUYER]).await;
        let pick = page_pick_ok(&pool, job, 1).await;
        assert_eq!(stored_fee(&pool, pick).await, fee_free());
        let picked = firm_fee(cents.unsigned_abs(), free);
        assert_eq!(picked.cents(), 0);
        let waiver = confirm(&mut db, (pick, job), &picked).await;
        assert_eq!(waiver, Waiver::NotWaived);
        let entry: (String, i64) = sqlx::query_as(
            "SELECT kind, amount_cents FROM payments.ledger_entry WHERE job_id = $1",
        )
        .bind(job)
        .fetch_one(&pool)
        .await
        .expect("one ledger entry");
        assert_eq!(entry, ("fee-free-job-type".to_owned(), 0));
    }
    let wins = "SELECT count(*) FROM payments.free_win";
    assert_eq!(count(&pool, wins).await, 0);
    let job = job_ready(&mut db, her(3), false, &[Quoted::Firm(90_000)]).await;
    facts(&mut db, job, "plumbing_repair", &["leak repair"]).await;
    let pick = text_picked(&pool, her(3), job, 1)
        .await
        .expect("a text pick");
    assert_eq!(stored_fee(&pool, pick).await, charged(7_500));
    let picked = firm_fee(90_000, JobFeeFacts::EMERGENCY_DEFAULT);
    let waiver = confirm(&mut db, (pick, job), &picked).await;
    assert!(
        matches!(
            waiver,
            Waiver::FreeFirstWin {
                waived_cents: 7_500,
                ..
            }
        ),
        "the free first win is still there: {waiver:?}"
    );
    assert_eq!(count(&pool, wins).await, 1);
}

/// TD-FEE-62 (FEE-045, FEE-005): a home condition inspection (no sale or refinance) is charged,
/// through the text pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_a_home_condition_inspection_is_charged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let job = job_ready(&mut db, her(1), false, &[Quoted::Firm(90_000)]).await;
    facts(&mut db, job, "home_inspection", &[CONDITION]).await;
    let pick = text_picked(&pool, her(1), job, 1)
        .await
        .expect("a text pick");
    assert_eq!(stored_fee(&pool, pick).await, charged(7_500));
    assert!(alerts(&pool).await.is_empty());
}

/// TD-FEE-62 (FEE-005, REC-001, D-2331): with a $1,000 monthly quote, a one-time job type in a
/// recurring category is 4 months, uncapped ($199); a recurring job type, or candidates that are
/// not all one-time, is capped at $75.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_a_one_time_job_type_in_a_recurring_category_is_priced_one_time(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let cases: [(&[&str], i64); 3] = [
        (&["deep cleaning"], 19_900),
        (&["regular cleaning"], 7_500),
        (&["deep cleaning", "regular cleaning"], 7_500),
    ];
    for (n, (types, cents)) in (1_u128..).zip(cases) {
        let job = job_ready(&mut db, her(n), false, &[Quoted::Monthly(100_000)]).await;
        facts(&mut db, job, "house_cleaning", types).await;
        let pick = page_pick_ok(&pool, job, 1).await;
        assert_eq!(stored_fee(&pool, pick).await, charged(cents), "{types:?}");
    }
}

/// TD-FEE-62 (FEE-045, D-2331): a job whose remaining candidates include a fee-free type is
/// fee-free, on the page and by text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_a_fee_free_candidate_makes_the_pick_fee_free(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let paged = job_ready(&mut db, her(1), false, &[Quoted::Firm(90_000)]).await;
    facts(&mut db, paged, "home_inspection", &[CONDITION, BUYER]).await;
    let pick = page_pick_ok(&pool, paged, 1).await;
    assert_eq!(stored_fee(&pool, pick).await, fee_free());
    let texted = job_ready(&mut db, her(2), false, &[Quoted::Firm(90_000)]).await;
    facts(&mut db, texted, "home_inspection", &[CONDITION, BUYER]).await;
    let pick = text_picked(&pool, her(2), texted, 1)
        .await
        .expect("a text pick");
    assert_eq!(stored_fee(&pool, pick).await, fee_free());
}

/// TD-FEE-62 (FEE-004, FEE-007): a diagnostic-visit quote pays its category's set visit tier from
/// the catalog ($75 in the fixture), whatever its visit fee.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_the_set_visit_tier_comes_from_the_catalog(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let job = job_ready(&mut db, her(1), false, &[Quoted::Visit(9_900)]).await;
    facts(&mut db, job, "sample_visit_trade", &["sample visit job"]).await;
    let pick = page_pick_ok(&pool, job, 1).await;
    assert_eq!(stored_fee(&pool, pick).await, charged(7_500));
}

/// TD-FEE-62 (FEE-005, INT-018, D-2331): the pick reads the latest row: a job narrowed from a
/// fee-free candidate to a home condition inspection is charged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_the_pick_reads_the_latest_row(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let job = job_ready(&mut db, her(1), false, &[Quoted::Firm(90_000)]).await;
    facts(&mut db, job, "home_inspection", &[CONDITION, BUYER]).await;
    facts(&mut db, job, "home_inspection", &[CONDITION]).await;
    let pick = page_pick_ok(&pool, job, 1).await;
    assert_eq!(stored_fee(&pool, pick).await, charged(7_500));
}

/// TD-FEE-62 (FEE-005, FEE-045, D-2331): a non-emergency job with no row, an unknown category or
/// job type, candidates in two categories or a set tier that is no visit tier is refused at the
/// pick (`PicksError::Quotes`) on the page, and by text, before any pick, fee, move or alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_unsettled_facts_refuse_a_non_emergency_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let cases: [Option<(&str, &[&str])>; 5] = [
        None,
        Some(("sample_unknown_trade", &["leak repair"])),
        Some(("plumbing_repair", &["sample unknown job"])),
        Some(("plumbing_repair", &["leak repair", BUYER])),
        Some(("sample_odd_tier", &["sample odd job"])),
    ];
    let mut jobs = Vec::new();
    for (n, case) in (1_u128..).zip(cases) {
        let job = job_ready(&mut db, her(n), false, &[Quoted::Firm(90_000)]).await;
        if let Some((category, types)) = case {
            facts(&mut db, job, category, types).await;
        }
        assert_eq!(page_picked(&pool, job, 1).await, Err(PicksError::Quotes));
        jobs.push((n, job));
    }
    for (n, job) in jobs {
        assert_eq!(text_picked(&pool, her(n), job, 1).await, None);
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM picks.pick").await, 0);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM payments.pick_fee").await,
        0
    );
    let moved = "SELECT count(*) FROM jobs.job WHERE status <> 'quotes_ready'";
    assert_eq!(count(&pool, moved).await, 0);
    assert!(alerts(&pool).await.is_empty());
}

/// TD-FEE-62 (FEE-004, FEE-005, D-2331): an emergency job released before the trade question
/// whose type settled later is priced on the settled type ($75 set visit tier), with no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_an_emergency_type_settled_later_uses_its_fee(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let job = job_ready(&mut db, her(1), true, &[Quoted::Visit(9_900)]).await;
    facts(&mut db, job, "sample_visit_trade", &["sample visit job"]).await;
    let pick = text_picked(&pool, her(1), job, 1)
        .await
        .expect("a text pick");
    assert_eq!(stored_fee(&pool, pick).await, charged(7_500));
    assert!(alerts(&pool).await.is_empty());
}

/// TD-FEE-62 (FEE-045, D-2331): an emergency job whose type never settled is priced under the
/// tie rules on its category's candidates: a fee-free candidate makes it fee-free, with no alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_an_emergency_type_never_settled_uses_the_tie_rules(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let job = job_ready(&mut db, her(1), true, &[Quoted::Firm(90_000)]).await;
    facts(&mut db, job, "home_inspection", &[CONDITION, BUYER]).await;
    let pick = page_pick_ok(&pool, job, 1).await;
    assert_eq!(stored_fee(&pool, pick).await, fee_free());
    assert!(alerts(&pool).await.is_empty());
}

/// TD-FEE-62 (FEE-005, D-2331): an emergency job with no row (page pick) or candidates in two
/// categories (text pick) is picked on the default one-time facts (a $1,000 monthly quote is 4
/// months, uncapped: $199) and raises exactly one High `picks.fee_facts_unsettled` alert per
/// pick, whose only subject is the pick's ID: no homeowner detail.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_62_an_emergency_with_unsettled_facts_proceeds_with_one_alert(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    catalog(&mut db).await;
    let paged = job_ready(&mut db, her(1), true, &[Quoted::Monthly(100_000)]).await;
    let first = page_pick_ok(&pool, paged, 1).await;
    let texted = job_ready(&mut db, her(2), true, &[Quoted::Monthly(100_000)]).await;
    facts(&mut db, texted, "plumbing_repair", &["leak repair", BUYER]).await;
    let second = text_picked(&pool, her(2), texted, 1)
        .await
        .expect("a text pick");
    for pick in [first, second] {
        assert_eq!(stored_fee(&pool, pick).await, charged(19_900));
    }
    let high = |pick| ("high".to_owned(), Some("picks.pick".to_owned()), Some(pick));
    assert_eq!(alerts(&pool).await, vec![high(first), high(second)]);
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = 'ops' AND table_name = 'alert' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("columns");
    let ids_only = [
        "id",
        "kind",
        "raised_at",
        "resolved_at",
        "severity",
        "subject_id",
        "subject_kind",
    ];
    assert_eq!(columns, ids_only, "an alert holds IDs and codes only");
}
