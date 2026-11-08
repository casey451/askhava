//! The business action pages, the quote form's fee lines and quote facts, "Ask about a charge"
//! and email replies (TD-BIZ-15, TD-BIZ-16, TD-BIZ-08 portal cases, TD-PICK-37, TD-PICK-22,
//! TD-SEC-09 email cases; BIZ-028, BIZ-032, BIZ-033, BIZ-038, CALL-011, PICK-006, PICK-039,
//! PICK-040, FEE-043, A11Y-023, A11Y-024, SEC-025).
//!
//! Every job number, price, ID and date is fake.

use askhava_payments::api::{FeeError, FeeJob, PeriodJob, RecurringCap, Service, Tier, VisitTier};
use askhava_portals::api::{
    ActionFacts, ActionKind, ChargeAsked, FactProblem, FactsForm, HELP_REQUEST_KIND,
    OPEN_IN_BROWSER_LINE, QuoteFormFacts, action_email_text, action_page, ask_about_charge,
    busy_days, busy_until_line, charge_answer, email_reply, fee_lines, parse_facts, prefill,
    quote_form_page, quote_minutes,
};
use askhava_quotes::api::{
    DayPart, Parts, Period, Permit, Price, QuoteFacts, Spanish, TripFee, record_quote_minutes,
};
use jiff::Timestamp;
use jiff::civil::date;
use sqlx::PgPool;
use sqlx::types::Uuid;

fn job(diagnostic_visit: bool, service: Service) -> PeriodJob {
    PeriodJob {
        job: FeeJob {
            diagnostic_visit,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service,
        cap: RecurringCap::DEFAULT,
    }
}

fn plain() -> PeriodJob {
    job(false, Service::NotRecurring)
}

#[test]
fn td_pick_37_every_quote_type_shows_its_fee_and_basis_before_submit() {
    let range = Price::Range {
        low: 100_000,
        high: 120_000,
        on_site: false,
    };
    assert_eq!(
        fee_lines(&range, plain()).expect("range"),
        ["If picked, this quote's fee is $75 (range midpoint $1,100)."]
    );
    assert_eq!(
        fee_lines(&Price::Firm(30_000), plain()).expect("firm"),
        ["If picked, this quote's fee is $25 (price $300)."]
    );
    let hourly = Price::Hourly {
        rate: 10_000,
        hours: Some((5, 7)),
    };
    assert_eq!(
        fee_lines(&hourly, plain()).expect("hourly"),
        ["If picked, this quote's fee is $75 (your rate times the hours, midpoint $600)."]
    );
    let no_hours = Price::Hourly {
        rate: 10_000,
        hours: None,
    };
    assert_eq!(
        fee_lines(&no_hours, plain()),
        Err(FeeError::NeedsHours),
        "FEE-043"
    );
    let visit = job(true, Service::NotRecurring);
    let line =
        "If picked, this quote's fee is $25 (the set fee for a diagnostic visit in this category).";
    assert_eq!(
        fee_lines(&Price::VisitFee(0), visit).expect("visit"),
        [line]
    );
    assert_eq!(
        fee_lines(&no_hours, visit).expect("no hours is a visit"),
        [line]
    );
}

#[test]
fn td_pick_37_tiers_monthly_and_fee_free_quotes_show_their_fees_before_submit() {
    let tiers = Price::Tiers(vec![
        ("Basic".to_owned(), 30_000),
        ("Full".to_owned(), 200_000),
    ]);
    assert_eq!(
        fee_lines(&tiers, plain()).expect("tiers"),
        [
            "If the homeowner picks Basic, this quote's fee is $25 (price $300).",
            "If the homeowner picks Full, this quote's fee is $199 (price $2,000).",
        ]
    );
    let monthly = |cents| Price::Monthly {
        cents,
        period: Period::Month,
        setup: Some(50_000),
    };
    let recurring = job(false, Service::RecurringJobType);
    assert_eq!(
        fee_lines(&monthly(10_000), recurring).expect("monthly"),
        ["If picked, this quote's fee is $25 (4 months of the price $400)."]
    );
    assert_eq!(
        fee_lines(&monthly(60_000), recurring).expect("capped"),
        ["If picked, this quote's fee is $75 (4 months of the price $2,400, capped at $75)."]
    );
    let free = PeriodJob {
        job: FeeJob {
            fee_free: true,
            ..plain().job
        },
        ..plain()
    };
    assert_eq!(
        fee_lines(&Price::Firm(30_000), free).expect("free"),
        ["No introduction fee for this job."]
    );
}

fn full_form() -> FactsForm {
    FactsForm {
        trip_fee: "none".to_owned(),
        parts: "extra".to_owned(),
        permit: "not_needed".to_owned(),
        may_change: "yes".to_owned(),
        visit_date: "2026-11-12".to_owned(),
        visit_part: "afternoon".to_owned(),
        spanish: "both".to_owned(),
        minutes: String::new(),
    }
}

#[test]
fn td_pick_37_quote_facts_are_required_and_minutes_optional() {
    let (facts, minutes) = parse_facts(&full_form()).expect("complete");
    assert_eq!(
        facts,
        QuoteFacts {
            trip_fee: TripFee::None,
            parts: Parts::Extra,
            permit: Permit::NotNeeded,
            may_change: true,
            earliest_visit: (date(2026, 11, 12), DayPart::Afternoon),
            spanish: Spanish::Both,
        }
    );
    assert_eq!(minutes, None, "minutes may be left empty");
    let all = parse_facts(&FactsForm::default()).expect_err("nothing chosen");
    assert_eq!(all.len(), 7, "mutant 'quote fact optional' turns this red");
    let no_part = FactsForm {
        visit_part: String::new(),
        ..full_form()
    };
    assert_eq!(parse_facts(&no_part), Err(vec![FactProblem::VisitPart]));
    let timed = FactsForm {
        minutes: "25".to_owned(),
        ..full_form()
    };
    assert_eq!(parse_facts(&timed).expect("timed").1, Some(25));
    assert_eq!(quote_minutes("0"), Err(FactProblem::Minutes));
    assert_eq!(quote_minutes("601"), Err(FactProblem::Minutes));
    assert_eq!(quote_minutes("ten"), Err(FactProblem::Minutes));
    for problem in all {
        assert!(problem.message().ends_with('.'), "{problem:?}");
        assert!(!problem.field().is_empty());
    }
}

#[test]
fn td_pick_37_same_job_type_prefills_every_fact_but_the_earliest_visit() {
    let (last, _) = parse_facts(&full_form()).expect("last");
    let filled = prefill(Some(&last));
    assert!(filled.visit_date.is_empty() && filled.visit_part.is_empty());
    let confirmed = FactsForm {
        visit_date: "2026-11-20".to_owned(),
        visit_part: "morning".to_owned(),
        ..filled.clone()
    };
    let (again, _) = parse_facts(&confirmed).expect("Same as last time plus a visit");
    assert_eq!(
        (again.trip_fee, again.parts, again.permit),
        (last.trip_fee, last.parts, last.permit)
    );
    assert_eq!(
        (again.may_change, again.spanish),
        (last.may_change, last.spanish)
    );
    let other = prefill(None);
    assert!(
        other.trip_fee.is_empty() && other.spanish.is_empty(),
        "a different job type opens empty"
    );
    for (t, p, m, s) in [
        ("counts", "included", "included", "phone"),
        ("does_not_count", "on_site", "not_included", "on_site"),
    ] {
        let form = FactsForm {
            trip_fee: t.to_owned(),
            parts: p.to_owned(),
            permit: m.to_owned(),
            spanish: s.to_owned(),
            may_change: "no".to_owned(),
            ..full_form()
        };
        let (facts, _) = parse_facts(&form).expect("parsed");
        let round = prefill(Some(&facts));
        assert_eq!((round.trip_fee.as_str(), round.parts.as_str()), (t, p));
        assert_eq!((round.permit.as_str(), round.spanish.as_str()), (m, s));
        assert_eq!(round.may_change, "no");
    }
    assert_eq!(
        prefill(Some(&QuoteFacts {
            spanish: Spanish::No,
            ..last
        }))
        .spanish,
        "no"
    );
}

fn form_facts() -> QuoteFormFacts {
    QuoteFormFacts {
        job_number: "1042".to_owned(),
        sent: 4,
        quoted: 2,
        fee_lines: vec!["If picked, this quote's fee is $75 (range midpoint $1,100).".to_owned()],
        values: FactsForm::default(),
        same_as_last: false,
        read_back: None,
        problems: Vec::new(),
    }
}

#[test]
fn td_biz_15_the_quote_form_shows_sent_count_quotes_and_the_fee_before_submit() {
    let page = quote_form_page(&form_facts(), "csrf-fake").expect("page");
    assert!(
        page.contains("Sent to 4 businesses. 2 quotes so far."),
        "moved from TD-CARD-07"
    );
    assert!(page.contains("range midpoint $1,100"));
    assert!(page.contains("Minutes this quote took (optional)"));
    assert!(!page.contains("<script"), "works with JavaScript off");
    assert!(!page.contains("Same as last time"));
    let one = QuoteFormFacts {
        sent: 1,
        quoted: 1,
        ..form_facts()
    };
    assert_eq!(one.status_line(), "Sent to 1 business. 1 quote so far.");
}

#[test]
fn td_pick_22_a_text_quote_reads_back_with_the_facts_prefilled() {
    let (last, _) = parse_facts(&full_form()).expect("last");
    let facts = QuoteFormFacts {
        values: prefill(Some(&last)),
        same_as_last: true,
        read_back: Some("$633".to_owned()),
        ..form_facts()
    };
    let page = quote_form_page(&facts, "csrf-fake").expect("page");
    assert!(page.contains("We read $633 from your text."));
    assert!(page.contains("Same as last time"));
    assert!(
        page.contains(r#"value="none" checked"#),
        "trip fee pre-filled"
    );
    assert!(
        !page.contains(r#"value="afternoon" checked"#),
        "earliest visit never pre-filled"
    );
}

#[test]
fn td_pick_37_a_failed_submit_names_each_missing_fact_and_keeps_values() {
    let values = FactsForm {
        visit_part: String::new(),
        ..full_form()
    };
    let problems = parse_facts(&values).expect_err("no part of day");
    let facts = QuoteFormFacts {
        values,
        problems,
        ..form_facts()
    };
    assert!(facts.has(&FactProblem::VisitPart));
    let page = quote_form_page(&facts, "csrf-fake").expect("page");
    assert!(page.contains("<title>Error: Quote job 1042"));
    assert!(page.contains(r##"href="#visit_part""##));
    assert!(page.contains(r#"value="2026-11-12""#), "values kept");
}

#[test]
fn td_biz_15_every_action_page_shows_what_it_does_before_its_button() {
    for kind in ActionKind::ALL {
        let facts = ActionFacts {
            kind,
            job_number: "1042",
            ends_at: Some("3:15 p.m."),
            csrf: "csrf-fake",
        };
        let page = action_page(&facts).expect("page");
        let does = page.find(r#"id="action-does""#).expect("says what it does");
        let button = page.find("<button").expect("one button");
        assert!(
            does < button,
            "{kind:?}: what it does comes first (SEC-026)"
        );
        assert!(page.contains(r#"method="post""#) && page.contains("csrf-fake"));
        assert!(
            page.contains("Ends at 3:15 p.m., Arizona time."),
            "an end time, never a countdown"
        );
        assert!(!page.contains("<script"), "works with JavaScript off");
        assert_eq!(page.contains(r#"id="days""#), kind == ActionKind::Busy);
    }
    let hold = ActionFacts {
        kind: ActionKind::Hold,
        job_number: "1042",
        ends_at: None,
        csrf: "csrf-fake",
    };
    let page = action_page(&hold).expect("page");
    assert!(page.contains("<h1>Hold job 1042 for 2 more business hours</h1>"));
    assert!(!page.contains("Ends at"));
}

#[test]
fn td_biz_08_busy_for_1_and_60_days_and_0_and_61_refused() {
    assert_eq!(busy_days("1"), Some(1));
    assert_eq!(busy_days(" 60 "), Some(60));
    assert_eq!(busy_days("0"), None);
    assert_eq!(busy_days("61"), None);
    assert_eq!(busy_days("soon"), None);
    assert_eq!(
        busy_until_line(date(2026, 11, 2), 10),
        "Busy until Thursday, November 12."
    );
}

#[test]
fn td_biz_15_an_email_reply_never_acts() {
    for body in ["CONFIRM 1042", "HOLD", "PAUSE", "BUSY 5", "YES 2"] {
        let reply = email_reply(body);
        assert!(
            !reply.opens_help && !reply.charge_details,
            "{body}: only the automatic reply"
        );
    }
    let question = email_reply("Why was I charged $75?");
    assert!(question.opens_help && question.charge_details);
    let spanish = email_reply("tengo un problema con el cobro");
    assert!(spanish.opens_help && spanish.charge_details);
    let fee = email_reply("about the fee");
    assert!(!fee.opens_help && fee.charge_details);
}

#[test]
fn td_sec_09_action_emails_carry_the_open_in_browser_line_and_name_the_job() {
    let link = "https://askhava.com/b/a/confirm/00000000-0000-0000-0000-000000001042/abc";
    let text = action_email_text(ActionKind::Confirm, "1042", link);
    assert!(text.starts_with("Confirm job 1042: https://askhava.com/"));
    assert!(text.contains(OPEN_IN_BROWSER_LINE), "D-333");
    assert!(!text.to_lowercase().contains("click here"));
    assert!(text.len() < 100 * 1024);
    assert_eq!(
        ActionKind::LinkPhone.link_text("1042"),
        "Link this phone",
        "CALL-011"
    );
    assert_eq!(
        ActionKind::Approve.link_text("1042"),
        "Approve this phone",
        "BIZ-033"
    );
}

#[test]
fn td_biz_15_ask_about_a_charge_answers_with_the_charge_details() {
    let answer = charge_answer("1042", Tier::Medium, "https://askhava.com/b/why/fake");
    assert!(answer.contains("$75 for job 1042, the introduction fee for a medium job."));
    assert!(answer.contains("https://askhava.com/b/why/fake"));
    assert!(answer.ends_with(
        "The Ask Hava owner gets charge questions right away and usually replies within 2 days."
    ));
    assert!(
        charge_answer("7", Tier::Small, "x")
            .contains("$25 for job 7, the introduction fee for a small job.")
    );
    assert!(
        charge_answer("7", Tier::Large, "x")
            .contains("$199 for job 7, the introduction fee for a large job.")
    );
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_15_ask_about_a_charge_enqueues_ids_only(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let asked = ChargeAsked {
        business: Uuid::from_u128(0xb1),
        charge: Uuid::from_u128(0xc1),
        job: Uuid::from_u128(0x1042),
    };
    let now: Timestamp = "2026-11-02T17:00:00Z".parse().expect("time");
    ask_about_charge(&mut db, asked, now).await.expect("queued");
    let (kind, payload): (String, serde_json::Value) =
        sqlx::query_as("SELECT kind, payload FROM core.task")
            .fetch_one(&mut *db)
            .await
            .expect("task");
    assert_eq!(kind, HELP_REQUEST_KIND);
    let keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["business", "charge", "job"], "IDs only");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_37_minutes_this_quote_took_is_stored_for_the_scorecard(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, business) = (Uuid::from_u128(0x1042), Uuid::from_u128(0xb1));
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 30000, false, '', '', false, true, now())",
    )
    .bind(job)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("fake quote");
    assert!(
        record_quote_minutes(&mut db, (job, business), 25)
            .await
            .expect("stored")
    );
    let other = (Uuid::from_u128(0x1043), business);
    assert!(
        !record_quote_minutes(&mut db, other, 25)
            .await
            .expect("no quote")
    );
    let stored: Option<i32> = sqlx::query_scalar("SELECT quote_minutes FROM quotes.quote")
        .fetch_one(&mut *db)
        .await
        .expect("read");
    assert_eq!(stored, Some(25));
}
