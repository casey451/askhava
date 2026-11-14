//! The quote rules read from the stored job, catalog and card (PICK-053, LIC-019, LIC-025;
//! T453, D-3675). Fake IDs and made-up quotes only; the categories are the shipped catalog's.
//!
//! `quote_rules` takes the job's type and category from its latest facts row, the format,
//! construction flag and license rule from the stored catalog, the handyman flag from the card the
//! business was sent and the certification tick from the caller; it reads none when the job has
//! no facts, the category or job type is not in the catalog, or no card went to the business.

use askhava_jobs::api::{create, record_job_facts};
use askhava_licensing::api::{LicenseKind, QuoteRefusal};
use askhava_ops::api::Actor;
use askhava_quotes::api::{Bounce, Format, Price, QuoteForm, QuoteRules, parse_quote, quote_rules};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const SENT: &str = "2026-11-03T08:00:00-07:00";

/// The shipped catalog, stored.
async fn catalog(db: &mut Db) {
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
}

/// A job with facts `category` and `job_type`, or no facts row when `job_type` is none.
async fn job(db: &mut Db, category: &str, job_type: Option<&str>) -> Uuid {
    let when: Timestamp = SENT.parse().expect("time");
    let id = create(db, when, false, Actor::System).await.expect("job");
    if let Some(t) = job_type {
        record_job_facts(db, id, category, &[t.to_owned()])
            .await
            .expect("job facts");
    }
    id
}

/// Records business `n`'s card for `job`, with its handyman flag.
async fn card(db: &mut Db, job: Uuid, n: u128, handyman: bool) -> Uuid {
    let business = Uuid::from_u128(n);
    let sent: Timestamp = SENT.parse().expect("time");
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, $3, $4)")
        .bind(job)
        .bind(business)
        .bind(handyman)
        .bind(sent.to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    business
}

/// The rules read for `business` on `job`, which must exist.
async fn read(db: &mut Db, job: Uuid, business: Uuid, certified: bool) -> QuoteRules {
    quote_rules(db, job, business, certified)
        .await
        .expect("read")
        .expect("rules")
}

fn firm(cents: u64) -> QuoteForm {
    QuoteForm {
        price: Price::Firm(cents),
        notes: "Parts and labor included.".to_owned(),
        original: "fake firm quote".to_owned(),
        ai_read: false,
        earliest_visit: None,
    }
}

/// PICK-053: a job type quoted `firm` inside a `diagnostic_visit` category reads `firm`, with the
/// category's construction flag and license rule and a licensed card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_firm_job_reads_its_job_types_format_and_the_categorys_rules(mut db: Db) {
    catalog(&mut db).await;
    let id = job(&mut db, "plumbing_repair", Some("drain clearing")).await;
    let business = card(&mut db, id, 1, false).await;
    let rules = read(&mut db, id, business, false).await;
    assert_eq!(rules.format, Format::Firm);
    assert!(rules.construction_type);
    assert_eq!(rules.license, LicenseKind::Roc);
    assert!(!rules.handyman);
    assert!(!rules.certified);
    assert!(parse_quote(firm(150_000), &rules).is_ok());
}

/// PICK-053: a `range` job type inside a `diagnostic_visit` category reads `range`, so a firm price
/// bounces as the wrong format.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_range_job_type_in_a_category_of_another_format_reads_range(mut db: Db) {
    catalog(&mut db).await;
    let id = job(&mut db, "pool_repair", Some("equipment replacement")).await;
    let business = card(&mut db, id, 2, false).await;
    let rules = read(&mut db, id, business, false).await;
    assert_eq!(rules.format, Format::Range);
    assert_eq!(rules.license, LicenseKind::Roc);
    assert_eq!(
        parse_quote(firm(50_000), &rules).err(),
        Some(Bounce::WrongFormat)
    );
}

/// LIC-019, LIC-025: a handyman card on a construction-type category reads `handyman` and the
/// caller's tick; a quote of $1,000.00 or more then bounces, $999.99 passes, and no tick bounces.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_handyman_card_bounces_a_quote_at_the_handyman_limit(mut db: Db) {
    catalog(&mut db).await;
    let id = job(&mut db, "plumbing_repair", Some("faucet replacement")).await;
    let business = card(&mut db, id, 3, true).await;
    let rules = read(&mut db, id, business, true).await;
    assert!(rules.handyman && rules.certified && rules.construction_type);
    assert_eq!(
        parse_quote(firm(100_000), &rules).err(),
        Some(Bounce::Handyman(QuoteRefusal::OverHandymanLimit))
    );
    assert!(parse_quote(firm(99_999), &rules).is_ok());
    let unticked = read(&mut db, id, business, false).await;
    assert!(!unticked.certified);
    assert_eq!(
        parse_quote(firm(5_000), &unticked).err(),
        Some(Bounce::Handyman(QuoteRefusal::NotCertified))
    );
}

/// A business that was sent no card for the job reads none, while the card holder reads rules.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_business_with_no_card_reads_none(mut db: Db) {
    catalog(&mut db).await;
    let id = job(&mut db, "plumbing_repair", Some("drain clearing")).await;
    let holder = card(&mut db, id, 4, false).await;
    let other = Uuid::from_u128(5);
    let read = quote_rules(&mut db, id, other, false).await.expect("read");
    assert!(read.is_none());
    let held = quote_rules(&mut db, id, holder, false).await.expect("read");
    assert!(held.is_some());
}

/// An unknown job type, an unknown category and a job with no facts row each read none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_unknown_job_type_category_or_no_facts_reads_none(mut db: Db) {
    catalog(&mut db).await;
    let cases = [
        ("plumbing_repair", Some("fake roof repair")),
        ("fake_category", Some("drain clearing")),
        ("plumbing_repair", None),
    ];
    for (n, (category, job_type)) in (6_u128..).zip(cases) {
        let id = job(&mut db, category, job_type).await;
        let business = card(&mut db, id, n, false).await;
        let read = quote_rules(&mut db, id, business, false)
            .await
            .expect("read");
        assert!(read.is_none(), "{category} {job_type:?}");
    }
}
