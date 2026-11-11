//! One quote format per job type and the per-visit schedule (T164; TD-PICK-35's format cases:
//! PICK-053; TD-PICK-57: REC-010; D-441, D-2333, D-1407). Fake IDs and made-up prices only.
//!
//! Every quote for a job type uses the catalog's format; a range job's form has no visit-fee
//! field and a visit fee on it bounces; a per-visit price needs its visit schedule, stored with
//! the quote, and "Other: enter the monthly amount" stores a monthly price with no schedule.

use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_portals::api::{FactsForm, PriceForm, QuoteFormFacts, quote_form_page_priced};
use askhava_quotes::api::{
    Bounce, Format, OTHER_SCHEDULE, Period, Price, PriceFields, PriceProblem, QuoteForm,
    QuoteRules, Saved, Submitted, VisitSchedule, format_for, parse_price, parse_quote, save_quote,
    visit_schedule_of,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn rules(format: Format) -> QuoteRules {
    QuoteRules {
        format,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    }
}

fn form(price: Price) -> QuoteForm {
    QuoteForm {
        price,
        notes: "Chemicals included.".to_owned(),
        original: String::new(),
        ai_read: false,
        earliest_visit: None,
    }
}

/// An Open job with business 1's card sent at 08:00 on the 3rd.
async fn open_job(db: &mut Db) -> (Uuid, Uuid) {
    let id = create(db, at("02T21:00:00"), false, Actor::System)
        .await
        .expect("job");
    let step = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, id, step, at("02T21:00:00"), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    let business = Uuid::from_u128(1);
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(id)
        .bind(business)
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    (id, business)
}

fn per_visit(schedule: &str) -> PriceFields {
    PriceFields {
        amount: "45.00".to_owned(),
        per: "visit".to_owned(),
        schedule: schedule.to_owned(),
        ..PriceFields::default()
    }
}

fn form_facts() -> QuoteFormFacts {
    QuoteFormFacts {
        job_number: "1001".to_owned(),
        sent: 3,
        quoted: 0,
        fee_lines: Vec::new(),
        values: FactsForm::default(),
        same_as_last: false,
        read_back: None,
        problems: Vec::new(),
    }
}

/// TD-PICK-35 (PICK-053): each job type takes its catalog format: the category's, a job type's
/// own, and `firm` for a recurring category's one-time job type.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_35_every_quote_for_a_job_type_uses_one_format(mut db: Db) {
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let cases = [
        ("plumbing_repair", "leak repair", Format::DiagnosticVisit),
        ("plumbing_repair", "faucet replacement", Format::Firm),
        ("hvac_replacement", "full system replacement", Format::Range),
        ("pool_cleaning", "weekly pool service", Format::Monthly),
        ("pool_cleaning", "green pool cleanup", Format::Firm),
    ];
    for (code, job_type, want) in cases {
        let category = askhava_catalog::api::category(&mut db, code)
            .await
            .expect("read")
            .expect("category");
        assert_eq!(format_for(&category, job_type), Some(want), "{job_type}");
    }
}

/// TD-PICK-35 (PICK-053): a range job's quote form offers no visit-fee type, a visit fee sent
/// for it bounces, and each form shows its format's plain definition.
#[test]
fn td_pick_35_a_range_form_offers_no_visit_fee() {
    let fields = PriceFields::default();
    let page = |format| {
        let price = PriceForm {
            format,
            fields: &fields,
            note: "",
            problem: None,
            note_problem: None,
        };
        let out = quote_form_page_priced(&form_facts(), "csrf-fake", (None, None), &price);
        out.expect("render")
    };
    let range = page(Format::Range);
    assert!(range.contains("name=\"low\"") && range.contains("name=\"high\""));
    assert!(range.contains("Range to be confirmed on site"));
    assert!(!range.contains("Visit fee") && !range.contains("name=\"amount\""));
    assert!(range.contains("Price range: the lowest and highest"));
    assert!(page(Format::DiagnosticVisit).contains("Visit fee ($)"));
    let visit = parse_quote(form(Price::VisitFee(8_900)), &rules(Format::Range));
    assert_eq!(visit, Err(Bounce::WrongFormat));
}

/// TD-PICK-57 (REC-010, D-2333): a per-visit price needs one of the five visit schedules and
/// stores it with the quote.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_57_a_per_visit_price_stores_its_schedule(mut db: Db) {
    let (job, business) = open_job(&mut db).await;
    for s in VisitSchedule::ALL {
        let (price, schedule) = parse_price(Format::Monthly, &per_visit(s.code())).expect("price");
        assert_eq!(schedule, Some(s));
        assert!(matches!(
            price,
            Price::Monthly {
                period: Period::Visit,
                cents: 4_500,
                ..
            }
        ));
    }
    let (price, schedule) =
        parse_price(Format::Monthly, &per_visit("every_two_weeks")).expect("price");
    let quote = parse_quote(form(price), &rules(Format::Monthly)).expect("quote");
    let saved = save_quote(
        &mut db,
        (job, business),
        &quote,
        schedule,
        at("03T09:00:00"),
    )
    .await;
    assert_eq!(saved, Ok(Saved::Stored(Submitted::Saved)));
    let stored = visit_schedule_of(&mut db, (job, business)).await;
    assert_eq!(stored, Ok(Some(VisitSchedule::EveryTwoWeeks)));
}

/// TD-PICK-57 (D-1407): "Other: enter the monthly amount" stores a monthly price with no
/// schedule, and an edit to it clears the earlier schedule.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_57_other_stores_a_monthly_price(mut db: Db) {
    let (job, business) = open_job(&mut db).await;
    let ids = (job, business);
    let (first, weekly) = parse_price(Format::Monthly, &per_visit("weekly")).expect("price");
    let quote = parse_quote(form(first), &rules(Format::Monthly)).expect("quote");
    let saved = save_quote(&mut db, ids, &quote, weekly, at("03T09:00:00")).await;
    assert_eq!(saved, Ok(Saved::Stored(Submitted::Saved)));
    let (price, schedule) = parse_price(Format::Monthly, &per_visit(OTHER_SCHEDULE)).expect("p");
    assert_eq!(schedule, None);
    assert!(matches!(
        price,
        Price::Monthly {
            period: Period::Month,
            cents: 4_500,
            ..
        }
    ));
    let quote = parse_quote(form(price), &rules(Format::Monthly)).expect("quote");
    let saved = save_quote(&mut db, ids, &quote, schedule, at("03T10:00:00")).await;
    assert_eq!(saved, Ok(Saved::Stored(Submitted::Saved)));
    assert_eq!(visit_schedule_of(&mut db, ids).await, Ok(None));
    let period: Option<String> =
        sqlx::query_scalar("SELECT period FROM quotes.quote WHERE job_id = $1")
            .bind(job)
            .fetch_one(&mut *db)
            .await
            .expect("period");
    assert_eq!(period.as_deref(), Some("month"));
}

/// TD-PICK-57: a per-visit price with neither a schedule nor "Other" is refused with the
/// reason, shown by the schedule field and in the summary.
#[test]
fn td_pick_57_no_schedule_is_refused_with_the_reason() {
    for bad in ["", "twice_a_week"] {
        let got = parse_price(Format::Monthly, &per_visit(bad));
        assert_eq!(got, Err(PriceProblem::Schedule));
    }
    let fields = per_visit("");
    let price = PriceForm {
        format: Format::Monthly,
        fields: &fields,
        note: "",
        problem: Some(PriceProblem::Schedule),
        note_problem: None,
    };
    let page =
        quote_form_page_priced(&form_facts(), "csrf-fake", (None, None), &price).expect("render");
    let reason = PriceProblem::Schedule.message();
    assert!(reason.contains("how often") && reason.contains("Other"));
    assert!(page.contains("<title>Error: "));
    assert!(page.contains("id=\"schedule-error\""));
    assert_eq!(page.matches(&reason.replace('\'', "&#39;")).count(), 2);
    assert!(page.contains("Other: enter the monthly amount"));
}
