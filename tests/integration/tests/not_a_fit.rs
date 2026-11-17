//! "Not a fit for us" on the job card page and the ops store for its answers and the weekly
//! minutes entry (T479; TD-WORD-30's not-a-fit half; PICK-057, OPS-039, CARD-020, D-3753, D-3883,
//! D-3889). Fake IDs, trades and wording only.
//!
//! Every job card page offers the four reasons as one-tap buttons posting to `NOT_A_FIT_ROUTE`,
//! under the WORD-002 business statement; an answer changes no row outside its own
//! `ops.not_a_fit_answer` row, so no routing or fee; a second answer by the same business for the
//! same job adds nothing; the monthly count per trade and reason follows the Arizona calendar
//! month; a minutes entry is stored and read back per Arizona week; and the app role can only
//! read and add rows in both tables.

use askhava_businesses::api::business_statement;
use askhava_integration::{is_permission_denied, set_app_role};
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_ops::api::{
    Actor, NotAFitCount, NotAFitReason, WeekMinutes, minutes_for_week, not_a_fit_counts,
    record_minutes,
};
use askhava_portals::api::{
    JobPageLinks, NOT_A_FIT_ROUTE, NotAFitChoice, NotAFitOutcome, NotAFitState, answer_not_a_fit,
    job_details_page_choice, not_a_fit_action, not_a_fit_page, parse_reason,
};
use askhava_quotes::api::{
    CardParts, DEFAULT_MIN_WORDS, PageParts, Place, Timing, build_card, job_page,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const OWNER: Uuid = Uuid::from_u128(0x0000_7e57_0479_0000_0000_0000_0000_0001);

fn at(text: &str) -> Timestamp {
    text.parse().expect("time")
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(0x0000_7e57_0479_0000_0000_0000_0000_1000 + n)
}

/// An open job whose card in `trade` was sent to businesses 1 and 2.
async fn carded_job(db: &mut Db, trade: &str) -> Uuid {
    let start = at("2026-11-02T21:00:00Z");
    let job = create(db, start, false, Actor::System).await.expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, start, Actor::System)
        .await
        .expect("open");
    sqlx::query(
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place, \
         photo_ids, category, candidates, emergency, recorded_at, sent_at) \
         VALUES ($1, $2, repeat('a', 32), 'Sample card', 'Lake Havasu City', 'house', '{}', $3, \
         '{}', false, $4, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(job)
    .bind(trade)
    .bind(start.to_sqlx())
    .execute(&mut **db)
    .await
    .expect("shown card");
    for n in 1..=2 {
        sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
            .bind(job)
            .bind(business(n))
            .bind(start.to_sqlx())
            .execute(&mut **db)
            .await
            .expect("card");
    }
    job
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
        answers: Vec::new(),
        later: Vec::new(),
    }
}

/// Every row of every table outside `ops.not_a_fit_answer`, hashed per table: equal before and
/// after an answer means the answer changed no routing, quote, pick, fee or task row.
async fn everything_else(db: &mut Db) -> String {
    sqlx::query_scalar(
        "SELECT string_agg(table_schema || '.' || table_name || '=' || (xpath('/row/h/text()', \
           query_to_xml(format('SELECT md5(coalesce(string_agg(t::text, %L ORDER BY t::text), %L)) \
           AS h FROM %I.%I t', ',', '', table_schema, table_name), false, true, '')))[1]::text, \
           ';' ORDER BY table_schema, table_name) \
         FROM information_schema.tables WHERE table_type = 'BASE TABLE' \
         AND table_schema NOT IN ('pg_catalog', 'information_schema') \
         AND (table_schema, table_name) <> ('ops', 'not_a_fit_answer')",
    )
    .fetch_one(&mut **db)
    .await
    .expect("snapshot")
}

async fn answer(
    db: &mut Db,
    job: Uuid,
    n: u128,
    reason: NotAFitReason,
    when: &str,
) -> NotAFitOutcome {
    answer_not_a_fit(db, (job, business(n)), reason, at(when))
        .await
        .expect("answer")
}

/// TD-WORD-30, PICK-057, CARD-020: every job card page offers the four not-a-fit reasons, each one
/// button posting to the not-a-fit route, under the business statement.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_word_30_every_job_card_page_offers_the_four_reasons(mut db: Db) {
    let job = carded_job(&mut db, "plumbing").await;
    let page = job_page(&mut db, (job, business(1)), &parts())
        .await
        .expect("read")
        .expect("page");
    let links = JobPageLinks {
        job_number: "J-4790",
        photos: &[],
        concept: None,
    };
    let action = not_a_fit_action(job, "sample-token");
    assert_eq!(action, format!("/b/not-a-fit/{job}/sample-token"));
    assert!(NOT_A_FIT_ROUTE.contains("{object}") && NOT_A_FIT_ROUTE.contains("{token}"));
    let choice = NotAFitChoice {
        action: &action,
        csrf: "sample-csrf",
    };
    let html = job_details_page_choice(&page, &links, None, choice).expect("render");
    assert!(html.contains(&format!("action=\"{action}\"")));
    assert!(html.contains("value=\"sample-csrf\""));
    assert!(html.contains("Not a fit for us"));
    for (code, label) in [
        ("not_my_trade", "Not my trade"),
        ("not_my_town", "Not my town"),
        ("too_big_or_small", "Too big or too small"),
        ("other", "Other reason"),
    ] {
        let button =
            format!("<button type=\"submit\" name=\"reason\" value=\"{code}\">{label}</button>");
        assert!(html.contains(&button), "{code} missing");
        assert_eq!(
            parse_reason(Some(code)).map(NotAFitReason::code),
            Some(code)
        );
    }
    let statement = askama_escape(&business_statement());
    let card_end = html.find("id=\"job-card\"").expect("card");
    let shown = html.find(&statement).expect("statement");
    assert!(shown > card_end && shown < html.find("Not a fit for us").expect("choice"));
    assert_eq!(parse_reason(Some("sample")), None);
    assert_eq!(parse_reason(None), None);
}

/// The statement as Askama escapes it (its apostrophes).
fn askama_escape(text: &str) -> String {
    text.replace('\'', "&#39;")
}

/// TD-WORD-30, PICK-057: an answer writes its own row and nothing else, so no routing or fee
/// changes; a second answer by the same business for the same job adds nothing; a business with no
/// card for the job stores nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_word_30_an_answer_changes_no_routing_or_fee_and_counts_once(mut db: Db) {
    let job = carded_job(&mut db, "plumbing").await;
    let before = everything_else(&mut db).await;
    let first = answer(
        &mut db,
        job,
        1,
        NotAFitReason::TooBigOrSmall,
        "2026-11-03T16:00:00Z",
    )
    .await;
    assert_eq!(first, NotAFitOutcome::Saved);
    assert_eq!(everything_else(&mut db).await, before);
    let again = answer(
        &mut db,
        job,
        1,
        NotAFitReason::Other,
        "2026-11-03T16:05:00Z",
    )
    .await;
    assert_eq!(again, NotAFitOutcome::AlreadyAnswered);
    let stranger = answer(
        &mut db,
        job,
        9,
        NotAFitReason::NotMyTown,
        "2026-11-03T16:06:00Z",
    )
    .await;
    assert_eq!(stranger, NotAFitOutcome::NoCard);
    assert_eq!(everything_else(&mut db).await, before);
    let counts = not_a_fit_counts(&mut db, at("2026-11-20T12:00:00Z")).await;
    let want = vec![NotAFitCount {
        trade: "plumbing".to_owned(),
        reason: NotAFitReason::TooBigOrSmall,
        count: 1,
    }];
    assert_eq!(counts.ok(), Some(want));
}

/// TD-WORD-30, PICK-057, OPS-039: the monthly count per trade and reason covers the Arizona
/// calendar month only (an answer at 11:30 p.m. on October 31, Arizona time, is October's).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_word_30_the_monthly_count_by_trade_and_reason_is_right(mut db: Db) {
    let pipes = carded_job(&mut db, "plumbing").await;
    let roof = carded_job(&mut db, "roofing").await;
    let lawn = carded_job(&mut db, "landscaping").await;
    let town = NotAFitReason::NotMyTown;
    assert_eq!(
        answer(&mut db, pipes, 1, town, "2026-11-01T07:00:00Z").await,
        NotAFitOutcome::Saved
    );
    assert_eq!(
        answer(&mut db, pipes, 2, town, "2026-11-30T23:00:00Z").await,
        NotAFitOutcome::Saved
    );
    let trade = NotAFitReason::NotMyTrade;
    assert_eq!(
        answer(&mut db, roof, 1, trade, "2026-11-10T15:00:00Z").await,
        NotAFitOutcome::Saved
    );
    assert_eq!(
        answer(&mut db, lawn, 1, trade, "2026-11-01T06:30:00Z").await,
        NotAFitOutcome::Saved
    );
    assert_eq!(
        answer(&mut db, lawn, 2, town, "2026-12-01T07:00:00Z").await,
        NotAFitOutcome::Saved
    );
    let count = |trade: &str, reason, count| NotAFitCount {
        trade: trade.to_owned(),
        reason,
        count,
    };
    let november = not_a_fit_counts(&mut db, at("2026-11-15T12:00:00Z")).await;
    let want = vec![count("plumbing", town, 2), count("roofing", trade, 1)];
    assert_eq!(november.ok(), Some(want));
    let october = not_a_fit_counts(&mut db, at("2026-10-15T12:00:00Z")).await;
    assert_eq!(october.ok(), Some(vec![count("landscaping", trade, 1)]));
    let december = not_a_fit_counts(&mut db, at("2026-12-15T12:00:00Z")).await;
    assert_eq!(december.ok(), Some(vec![count("landscaping", town, 1)]));
}

/// OPS-039: a minutes entry is stored and read back for its Arizona week (Monday to Sunday); a
/// later entry for the week replaces it; a week with none reads `None`; no part over a week.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_039_a_minutes_entry_is_stored_and_read_back_per_week(mut db: Db) {
    let first = WeekMinutes::new(90, 240).expect("minutes");
    record_minutes(&mut db, OWNER, first, at("2026-11-18T17:00:00Z"))
        .await
        .expect("entry");
    let read = minutes_for_week(&mut db, at("2026-11-16T07:00:00Z")).await;
    assert_eq!(read.ok(), Some(Some(first)));
    assert_eq!(
        (first.recruiting(), first.other(), first.total()),
        (90, 240, 330)
    );
    let sunday_night = minutes_for_week(&mut db, at("2026-11-16T06:30:00Z")).await;
    assert_eq!(sunday_night.ok(), Some(None));
    let fixed = WeekMinutes::new(120, 200).expect("minutes");
    record_minutes(&mut db, OWNER, fixed, at("2026-11-23T06:59:00Z"))
        .await
        .expect("entry");
    let read = minutes_for_week(&mut db, at("2026-11-20T12:00:00Z")).await;
    assert_eq!(read.ok(), Some(Some(fixed)));
    assert_eq!(WeekMinutes::new(10_081, 0), None);
    assert_eq!(WeekMinutes::new(0, 10_081), None);
    assert!(WeekMinutes::new(10_080, 10_080).is_some());
}

/// PICK-057, OPS-039, D-493: the app role reads and adds rows in both tables but cannot change or
/// remove one, so an answer and an entry stand as given.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_039_the_app_role_only_reads_and_adds_scorecard_inputs(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let job = carded_job(&mut db, "plumbing").await;
    set_app_role(&mut db).await.expect("SET ROLE askhava_app");
    let saved = answer(
        &mut db,
        job,
        1,
        NotAFitReason::Other,
        "2026-11-03T16:00:00Z",
    )
    .await;
    assert_eq!(saved, NotAFitOutcome::Saved);
    let entry = WeekMinutes::new(30, 60).expect("minutes");
    record_minutes(&mut db, OWNER, entry, at("2026-11-18T17:00:00Z"))
        .await
        .expect("entry");
    for statement in [
        "UPDATE ops.not_a_fit_answer SET reason = 'not_my_town'",
        "DELETE FROM ops.not_a_fit_answer",
        "TRUNCATE ops.not_a_fit_answer",
        "UPDATE ops.minutes_entry SET recruiting = 0",
        "DELETE FROM ops.minutes_entry",
        "TRUNCATE ops.minutes_entry",
    ] {
        let refused = sqlx::query(statement).execute(&mut *db).await;
        assert!(is_permission_denied(&refused), "{statement} was allowed");
    }
}

/// PICK-057, A11Y-030: the page after a submit thanks and names the reason, says an earlier answer
/// stands, or shows the error with the four reasons again; every state links back to the job.
#[test]
fn pick_057_the_answer_page_says_what_happened() {
    let saved = not_a_fit_page(
        NotAFitState::Saved(NotAFitReason::NotMyTown),
        "J-4790",
        "/b/jobs/x",
    );
    let saved = saved.expect("render");
    assert!(saved.contains("Your reason: Not my town.") && saved.contains("href=\"/b/jobs/x\""));
    assert!(saved.contains("/static/portals/forms.css") && !saved.contains("Error: "));
    let again =
        not_a_fit_page(NotAFitState::AlreadyAnswered, "J-4790", "/b/jobs/x").expect("render");
    assert!(again.contains("You already told us job J-4790"));
    let choice = NotAFitChoice {
        action: "/b/not-a-fit/sample/sample-token",
        csrf: "sample-csrf",
    };
    let refused =
        not_a_fit_page(NotAFitState::Refused(choice), "J-4790", "/b/jobs/x").expect("render");
    assert!(refused.contains("<title>Error: ") && refused.contains("role=\"alert\""));
    for code in NotAFitReason::ALL.map(NotAFitReason::code) {
        assert!(
            refused.contains(&format!("value=\"{code}\"")),
            "{code} missing"
        );
    }
}
