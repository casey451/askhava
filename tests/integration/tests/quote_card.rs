//! The quote card from a stored quote (TD-PICK-18, TD-PICK-29, TD-PICK-44 in the database;
//! PICK-003, PICK-005, PICK-040, PICK-041, PICK-044). Fake IDs, names and quotes only.
//!
//! The card shows the business's stored original wording and earliest visit and no AI summary,
//! the business's preview equals the homeowner's card character for character, and only a
//! business that ticked after-hours emergencies gets "Gets emergency requests at night".

use askhava_businesses::api::{Ticks, create as create_business, set_ticks};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::{CardLanguage, CardLine, LicenseKind};
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardSource, DayPart, Format, Parts, Permit, Price, QuoteFacts, QuoteForm, QuoteRules, Spanish,
    Submitted, TripFee, parse_quote, quote_card, submit_quote,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const ORIGINAL: &str = "Swap the water heater valve, 190 all in";

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn source() -> CardSource {
    CardSource {
        business_name: "Sample Valley Plumbing".to_owned(),
        license_lines: vec![CardLine {
            text: "ROC #000000 showed Active on the ROC website when we checked on November 1, 2026. Check it yourself.".to_owned(),
            link: Some("https://example.test/roc".to_owned()),
        }],
        facts: QuoteFacts {
            trip_fee: TripFee::Counts,
            parts: Parts::Included,
            permit: Permit::NotNeeded,
            may_change: false,
            earliest_visit: (date(2026, 11, 9), DayPart::Evening),
            spanish: Spanish::No,
        },
    }
}

/// A business with or without the after-hours tick, its card on `job` and a confirmed quote.
async fn quoted(db: &mut Db, job: Uuid, after_hours: bool) -> Uuid {
    let id = create_business(db, false, at("02T09:00:00"))
        .await
        .expect("business");
    let ticks = Ticks {
        overflow_on: false,
        speaks_spanish: false,
        insurance: None,
        after_hours,
    };
    set_ticks(db, id, ticks, Actor::System)
        .await
        .expect("ticks");
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(id)
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: ORIGINAL.to_owned(),
        ai_read: false,
        earliest_visit: Some((date(2026, 11, 4), DayPart::Morning)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(db, (job, id), &quote, at("03T09:00:00")).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    id
}

/// TD-PICK-18, TD-PICK-29, TD-PICK-44: the stored wording and visit show with no summary; the
/// business preview equals the homeowner card; only the ticked business gets the night line.
/// PICK-005 rests on `QuoteCard::lines` taking no viewer argument, so there is no business-only
/// path to differ; this test only shows two reads of the stored quote give the same lines.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_44_card_from_the_stored_quote(mut db: Db) {
    let job = create(&mut db, at("02T21:00:00"), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, at("02T21:00:00"), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    let night = quoted(&mut db, job, true).await;
    let day = quoted(&mut db, job, false).await;
    let homeowner = quote_card(&mut db, (job, night), source())
        .await
        .expect("card")
        .expect("shown");
    let preview = quote_card(&mut db, (job, night), source())
        .await
        .expect("card")
        .expect("shown");
    assert_eq!(homeowner, preview);
    let (h, p) = (
        homeowner.lines(CardLanguage::Spanish),
        preview.lines(CardLanguage::Spanish),
    );
    assert_eq!(h, p);
    let en: Vec<String> = homeowner
        .lines(CardLanguage::English)
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert_eq!(en[4], "Gets emergency requests at night");
    assert!(en.contains(&"Earliest visit: November 4, morning".to_owned()));
    assert_eq!(
        en.last(),
        Some(&format!("The business's own words: {ORIGINAL}"))
    );
    assert!(homeowner.summary.is_none());
    let plain = quote_card(&mut db, (job, day), source())
        .await
        .expect("card")
        .expect("shown");
    let lines = plain.lines(CardLanguage::English);
    assert!(lines.iter().all(|l| !l.text.contains("emergency")));
    let stranger = quote_card(&mut db, (job, Uuid::from_u128(9)), source()).await;
    assert_eq!(stranger, Ok(None));
}
