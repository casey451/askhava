//! The business job page's data in the database (CARD-023, CARD-019, D-434). Fake IDs, wording,
//! phones (928 555 01xx) and emails (example.com) only.
//!
//! A card holder's page holds the approved card, every answer and the later details, each through
//! the final gate so no contact detail she typed reaches a business, and the current number sent
//! and quotes so far; a business with no card for the job gets no page. The photo and concept-image
//! cases of TD-CARD-12 are T212's (D-491).

use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardParts, DEFAULT_MIN_WORDS, Format, PageParts, Place, Price, QuoteForm, QuoteRules,
    Submitted, Timing, build_card, job_page, parse_quote, submit_quote,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn open_job(db: &mut Db) -> Uuid {
    let job = create(db, at("02T21:00:00"), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, at("02T21:00:00"), Actor::System)
        .await
        .expect("open");
    job
}

async fn card(db: &mut Db, job: Uuid, n: u128) {
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(business(n))
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
}

async fn quote(db: &mut Db, job: Uuid, n: u128) {
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: "Sample wording".to_owned(),
        ai_read: false,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let checked = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(db, (job, business(n)), &checked, at("03T09:00:00")).await;
    assert_eq!(saved, Ok(Submitted::Saved));
}

fn parts() -> PageParts {
    let card = build_card(
        &CardParts {
            category: "Plumbing".to_owned(),
            town: "Lake Havasu City".to_owned(),
            place: Place::House,
            description: "Kitchen sink drips under the cabinet all day".to_owned(),
            answers: vec![("Fixture".to_owned(), "Kitchen sink".to_owned())],
            timing: Some(Timing::NextFewDays),
            danger: None,
            photos: Vec::new(),
            concept: None,
        },
        DEFAULT_MIN_WORDS,
    );
    PageParts {
        card,
        answers: vec![
            ("Fixture".to_owned(), "Kitchen sink".to_owned()),
            (
                "Anything else".to_owned(),
                "Gate code is 4 call 928-555-0142".to_owned(),
            ),
        ],
        later: vec!["Water is on the floor now, write sample@example.com".to_owned()],
    }
}

/// TD-CARD-12's data cases (CARD-023, CARD-019, D-434): a card holder's page holds the approved
/// card, every answer and the later details with no contact detail, and the current counts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_023_page_holds_card_answers_details_and_counts(mut db: Db) {
    let job = open_job(&mut db).await;
    for n in 1..=3 {
        card(&mut db, job, n).await;
    }
    quote(&mut db, job, 1).await;
    quote(&mut db, job, 2).await;
    let parts = parts();
    let page = job_page(&mut db, (job, business(3)), &parts)
        .await
        .expect("read")
        .expect("page");
    assert_eq!(page.card(), &parts.card);
    assert!(page.card().text().starts_with("Kitchen sink drips"));
    assert_eq!(page.answers().len(), 2);
    assert_eq!(page.answers()[0], ("Fixture".into(), "Kitchen sink".into()));
    assert_eq!(page.later().len(), 1);
    let shown = format!("{:?} {:?}", page.answers(), page.later());
    for contact in ["928", "0142", "example.com", "sample@"] {
        assert!(!shown.contains(contact), "{contact} reached the page");
    }
    assert_eq!((page.sent(), page.quoted()), (3, 2));
    let debug = format!("{page:?} {parts:?}");
    assert!(!debug.contains("Kitchen") && !debug.contains("Water"));
}

/// CARD-023: the page opens only for a business holding the job's card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_023_no_page_without_a_card(mut db: Db) {
    let job = open_job(&mut db).await;
    card(&mut db, job, 1).await;
    let page = job_page(&mut db, (job, business(9)), &parts()).await;
    assert_eq!(page.map(|p| p.is_none()), Ok(true));
}

/// CARD-019: the counts are read when the page is, so a later quote shows on the next read.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_023_counts_are_current(mut db: Db) {
    let job = open_job(&mut db).await;
    card(&mut db, job, 1).await;
    card(&mut db, job, 2).await;
    let parts = parts();
    let before = job_page(&mut db, (job, business(1)), &parts).await;
    let before = before.expect("read").expect("page");
    assert_eq!((before.sent(), before.quoted()), (2, 0));
    quote(&mut db, job, 2).await;
    let after = job_page(&mut db, (job, business(1)), &parts).await;
    let after = after.expect("read").expect("page");
    assert_eq!((after.sent(), after.quoted()), (2, 1));
}
