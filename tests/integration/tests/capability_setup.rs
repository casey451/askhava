//! Sign-up capability setup across `businesses`, `quotes` and `portals` (TD-BIZ-29: BIZ-049,
//! BIZ-004, CARD-001; D-393, D-492). An R-11 business gets every electrical job type pre-ticked;
//! unticking one stops its cards; a licensed job type outside the license is refused and a
//! no-license one can be added; a $1,500 largest job skips a card whose band floor is $5,000; a
//! recurring "no" removes the recurring job types; the guided screens reach the same answers as
//! the one-page form; the quote form warns above the largest job. With `CAPABILITY_HTML_DIR` set,
//! `td_biz_29_write_pages_for_axe` writes every screen for `tests/e2e/capability_setup.spec.ts`.
//! Fake data only.

use std::collections::BTreeMap;

use askhava_businesses::api::{
    AccountStatus, BusinessesError, Capability, CapabilityAnswers, JobTypeChoice, LargestJob,
    TradeOffer, capabilities, checklist_offer, create, is_recurring, job_types, pre_ticked,
    save_capability,
};
use askhava_catalog::api::{Catalog, parse};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_portals::api::{
    CapabilityPage, CapabilityStep, CapabilityValues, FactsForm, QuoteFormFacts, capability_page,
    largest_job_warning, quote_form_page_warned,
};
use askhava_quotes::api::rules::match_job;
use askhava_quotes::api::{
    CapabilitySkip, CardBusiness, CardJob, JobFit, OfferTown, OwnerPauses, Pool, Pooled, Price,
    band_floor_cents, capability_skip, capable_pool,
};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const TOWN: &str = "lake_havasu_city";

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog parses")
}

fn r11_offer() -> Vec<TradeOffer> {
    let map = ClassMap::shipped().expect("license map loads");
    checklist_offer(map, &catalog(), &["R-11"])
}

fn towns() -> Vec<(String, String)> {
    vec![(TOWN.to_owned(), "Lake Havasu City".to_owned())]
}

fn now() -> Timestamp {
    "2026-11-10T17:00:00Z".parse().expect("fake time")
}

fn pick(category: &str, job_type: &str) -> JobTypeChoice {
    JobTypeChoice::new(category, job_type)
}

fn answers(ticked: Vec<JobTypeChoice>, largest: LargestJob, recurring: bool) -> CapabilityAnswers {
    CapabilityAnswers {
        ticked,
        towns: vec![TOWN.to_owned()],
        capability: Capability { largest, recurring },
        after_hours: false,
    }
}

/// The stored business as matching sees it: active, verified, R-11, its stored ticks.
async fn card_business(db: &mut PgConnection, id: Uuid) -> CardBusiness {
    let ticked = job_types(db, id).await.expect("ticks read");
    CardBusiness {
        id,
        status: AccountStatus::Active,
        verified: true,
        towns: [TOWN.to_owned()].into(),
        ticked: ticked
            .into_iter()
            .map(|c| (c.category, c.job_type))
            .collect(),
        classes: vec!["R-11".to_owned()],
        licenses: Holder::default(),
    }
}

fn electrical_job(job_type: &str) -> CardJob {
    CardJob {
        towns: vec![OfferTown {
            code: TOWN.to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![Job {
            category: "electrical".to_owned(),
            job_type: job_type.to_owned(),
            license: LicenseKind::Roc,
            handyman_allowed: false,
            listed: true,
        }],
        text: "Fake request about the electrical work".to_owned(),
    }
}

fn reached(pool: &[Pooled], job: &CardJob) -> usize {
    let map = ClassMap::shipped().expect("license map loads");
    let pauses = OwnerPauses::default();
    match_job(
        map,
        job,
        &Pool {
            businesses: pool,
            pauses: &pauses,
        },
    )
    .len()
}

/// TD-BIZ-29 (BIZ-049, BIZ-004, LIC-043): an R-11 sign-up shows every electrical job type ticked,
/// licensed trades first, the no-license extras unticked, and no other licensed trade.
#[test]
fn td_biz_29_r11_shows_every_electrical_job_type_pre_ticked() {
    let offer = r11_offer();
    let electrical = catalog()
        .categories
        .into_iter()
        .find(|c| c.id == "electrical")
        .expect("electrical");
    let expected: Vec<JobTypeChoice> = electrical
        .job_types
        .iter()
        .map(|t| pick("electrical", t))
        .collect();
    assert_eq!(pre_ticked(&offer), expected);
    assert_eq!(offer[0].category, "electrical");
    assert!(offer.iter().all(|t| t.category != "plumbing_repair"));
    let extras = offer
        .iter()
        .find(|t| t.category == "pool_cleaning")
        .expect("extra");
    assert!(extras.rows.iter().all(|r| !r.licensed));
    let values = CapabilityValues::fresh(&offer);
    let html = page(CapabilityStep::All, &offer, &values);
    assert!(html.contains(r#"value="electrical:panel upgrade" checked>"#));
    assert!(html.contains(r#"value="pool_cleaning:weekly pool service">"#));
}

fn page(step: CapabilityStep, offer: &[TradeOffer], values: &CapabilityValues) -> String {
    let towns = towns();
    let p = CapabilityPage {
        step,
        offer,
        towns: &towns,
        values,
        problems: &[],
        csrf: "fake-csrf",
    };
    capability_page(&p).expect("page renders")
}

/// TD-BIZ-29 (BIZ-004, CARD-001, LIC-043): unticking panel upgrade stops those cards; a licensed
/// job type outside the license is refused with nothing stored; a no-license one is added.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_29_untick_stops_cards_and_only_no_license_extras_add(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = create(&mut db, false, now()).await.expect("business");
    let offer = r11_offer();
    let mut ticked = pre_ticked(&offer);
    ticked.retain(|c| c.job_type != "panel upgrade");
    let plumbing = [ticked.clone(), vec![pick("plumbing_repair", "leak repair")]].concat();
    let refused = save_capability(
        &mut db,
        id,
        &offer,
        &answers(plumbing, LargestJob::NoLimit, true),
        (Actor::System, now()),
    )
    .await;
    assert_eq!(refused, Err(BusinessesError::NotCovered));
    assert!(job_types(&mut db, id).await.expect("read").is_empty());
    ticked.push(pick("window_cleaning", "inside and outside windows"));
    let all = answers(ticked, LargestJob::NoLimit, true);
    save_capability(&mut db, id, &offer, &all, (Actor::System, now()))
        .await
        .expect("saved");
    let stored = job_types(&mut db, id).await.expect("read");
    assert!(stored.contains(&pick("window_cleaning", "inside and outside windows")));
    let business = Pooled {
        card: card_business(&mut db, id).await,
        overflow_on: false,
    };
    assert_eq!(
        reached(
            std::slice::from_ref(&business),
            &electrical_job("panel upgrade")
        ),
        0
    );
    assert_eq!(reached(&[business], &electrical_job("outlet repair")), 1);
}

/// TD-BIZ-29 (BIZ-049, CARD-001): a $1,500 largest job skips a card whose band floor is $5,000,
/// not one whose floor is $1,500; a recurring "no" removes the recurring job types and skips a
/// recurring job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_29_largest_job_and_recurring_answers_skip(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = create(&mut db, false, now()).await.expect("business");
    let offer = r11_offer();
    let weekly = pick("pool_cleaning", "weekly pool service");
    let green = pick("pool_cleaning", "green pool cleanup");
    let ticked = [pre_ticked(&offer), vec![weekly.clone(), green.clone()]].concat();
    let a = answers(ticked, LargestJob::Cents(150_000), false);
    save_capability(&mut db, id, &offer, &a, (Actor::System, now()))
        .await
        .expect("saved");
    let stored = job_types(&mut db, id).await.expect("read");
    assert!(!stored.contains(&weekly) && stored.contains(&green));
    assert!(is_recurring(
        &catalog(),
        "pool_cleaning",
        "weekly pool service"
    ));
    let answers = capabilities(&mut db, &[id]).await.expect("answers");
    let cap = answers.get(&id).copied();
    assert_eq!(cap.map(|c| c.largest), Some(LargestJob::Cents(150_000)));
    let fit = |floor: &str, recurring| JobFit {
        band_floor_cents: band_floor_cents(floor),
        recurring,
    };
    let big = fit("5000.00", false);
    assert_eq!(
        capability_skip(big, cap.as_ref()),
        Some(CapabilitySkip::AboveLargestJob)
    );
    assert_eq!(capability_skip(fit("1500.00", false), cap.as_ref()), None);
    let weekly_job = fit("0", true);
    assert_eq!(
        capability_skip(weekly_job, cap.as_ref()),
        Some(CapabilitySkip::NoRecurringWork)
    );
    let business = Pooled {
        card: card_business(&mut db, id).await,
        overflow_on: true,
    };
    let job = electrical_job("outlet repair");
    assert_eq!(
        reached(
            &capable_pool(std::slice::from_ref(&business), big, &answers),
            &job
        ),
        0
    );
    let small = capable_pool(
        std::slice::from_ref(&business),
        fit("1500.00", false),
        &answers,
    );
    assert_eq!(reached(&small, &job), 1);
    let unanswered = capable_pool(&[business], big, &BTreeMap::new());
    assert_eq!(reached(&unanswered, &job), 1);
}

/// The pairs one screen posts: its own answer plus the hidden fields it carries.
fn post(
    step: CapabilityStep,
    carried: &CapabilityValues,
    own: &[(&str, &str)],
) -> CapabilityValues {
    let mut pairs: Vec<(String, String)> = carried
        .carried(step)
        .into_iter()
        .map(|(n, v)| (n.to_owned(), v))
        .collect();
    pairs.extend(own.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())));
    CapabilityValues::from_pairs(&pairs)
}

/// TD-BIZ-29 (BIZ-049, A11Y-012): the guided one-question-per-screen flow reaches the same
/// answers as the one-page form, each guided screen asks one question and carries the others, and
/// the check screen reads them back.
#[test]
fn td_biz_29_guided_flow_gives_the_same_answers() {
    let offer = r11_offer();
    let mut one_page: Vec<(String, String)> = pre_ticked(&offer)
        .iter()
        .filter(|c| c.job_type != "panel upgrade")
        .map(|c| {
            (
                "job_type".to_owned(),
                format!("{}:{}", c.category, c.job_type),
            )
        })
        .collect();
    let rest = [
        ("town", TOWN),
        ("largest", "typed"),
        ("largest_amount", "$2,500"),
        ("recurring", "no"),
        ("after_hours", "yes"),
    ];
    one_page.extend(rest.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())));
    let expected = CapabilityValues::from_pairs(&one_page)
        .answers()
        .expect("complete");
    let jobs: Vec<(&str, &str)> = one_page[..5]
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let mut v = CapabilityValues::fresh(&offer);
    let screens: [(CapabilityStep, Vec<(&str, &str)>); 5] = [
        (CapabilityStep::JobTypes, jobs),
        (CapabilityStep::Towns, vec![rest[0]]),
        (CapabilityStep::Largest, vec![rest[1], rest[2]]),
        (CapabilityStep::Recurring, vec![rest[3]]),
        (CapabilityStep::AfterHours, vec![rest[4]]),
    ];
    for (step, own) in screens {
        let html = page(step, &offer, &v);
        let questions = ["job-types", "towns", "largest", "recurring", "after_hours"];
        let asked = questions
            .iter()
            .filter(|q| html.contains(&format!(r#"<fieldset id="{q}""#)));
        assert_eq!(asked.count(), 1, "{step:?} asks one question");
        assert_eq!(html.matches("<h1>").count(), 1);
        v = post(step, &v, &own);
    }
    assert_eq!(v.answers(), Ok(expected));
    assert_eq!(expected_largest(&v), Some(250_000));
    assert_check_screen(&offer, &v);
}

/// The check screen reads the answers back, panel upgrade unticked.
fn assert_check_screen(offer: &[TradeOffer], v: &CapabilityValues) {
    let check = page(CapabilityStep::Check, offer, v);
    assert!(
        check.contains("Largest job you&#x27;ll quote: $2,500")
            || check.contains("Largest job you&#39;ll quote: $2,500")
    );
    assert!(check.contains("Recurring work: No") && check.contains("Lake Havasu City"));
    assert!(!check.contains("panel upgrade"));
}

fn expected_largest(v: &CapabilityValues) -> Option<u64> {
    v.answers().ok()?.capability.largest.cents()
}

/// TD-BIZ-29 (BIZ-049, A11Y-030): missing answers give an "Error: " title and the summary; the
/// typed amount must be money.
#[test]
fn td_biz_29_missing_answers_are_listed() {
    let offer = r11_offer();
    let v = CapabilityValues::from_pairs(&[
        ("largest".into(), "typed".into()),
        ("largest_amount".into(), "lots".into()),
    ]);
    let problems = v.answers().expect_err("incomplete");
    assert_eq!(problems.len(), 4);
    let towns = towns();
    let p = CapabilityPage {
        step: CapabilityStep::All,
        offer: &offer,
        towns: &towns,
        values: &v,
        problems: &problems,
        csrf: "fake-csrf",
    };
    let html = capability_page(&p).expect("renders");
    assert!(html.contains("<title>Error: ") && html.contains(r#"id="error-summary""#));
    assert!(html.contains(r#"id="largest-error""#) && html.contains(r#"value="lots""#));
}

/// TD-BIZ-29 (BIZ-049): the quote form warns before a quote above the largest job is sent, and
/// lets it through once confirmed.
#[test]
fn td_biz_29_quote_form_warns_above_the_largest_job() {
    let limit = LargestJob::Cents(150_000);
    let warning = largest_job_warning(&Price::Firm(200_000), limit, false).expect("warns");
    assert!(warning.contains("$1,500"));
    assert_eq!(
        largest_job_warning(&Price::Firm(200_000), limit, true),
        None
    );
    assert_eq!(
        largest_job_warning(&Price::Firm(150_000), limit, false),
        None
    );
    assert_eq!(
        largest_job_warning(&Price::Firm(900_000), LargestJob::NoLimit, false),
        None
    );
    let range = Price::Range {
        low: 100_000,
        high: 160_000,
        on_site: false,
    };
    assert!(largest_job_warning(&range, limit, false).is_some());
    let facts = QuoteFormFacts {
        job_number: "1001".to_owned(),
        sent: 3,
        quoted: 0,
        fee_lines: Vec::new(),
        values: FactsForm::default(),
        same_as_last: false,
        read_back: None,
        problems: Vec::new(),
    };
    let html = quote_form_page_warned(&facts, "fake-csrf", Some(&warning)).expect("renders");
    assert!(html.contains(r#"id="largest-warning""#));
    assert!(html.contains(r#"name="over_largest" value="yes""#));
}

/// The e2e axe scan's pages: written only when `CAPABILITY_HTML_DIR` is set.
#[test]
fn td_biz_29_write_pages_for_axe() {
    let Some(dir) = std::env::var_os("CAPABILITY_HTML_DIR") else {
        return;
    };
    let offer = r11_offer();
    let fresh = CapabilityValues::fresh(&offer);
    let done = CapabilityValues::from_pairs(&[
        ("town".into(), TOWN.into()),
        ("largest".into(), "1500".into()),
        ("recurring".into(), "no".into()),
        ("after_hours".into(), "no".into()),
    ]);
    let steps = [CapabilityStep::All]
        .into_iter()
        .chain(CapabilityStep::GUIDED);
    for step in steps {
        let values = if step == CapabilityStep::Check {
            &done
        } else {
            &fresh
        };
        let path = std::path::Path::new(&dir).join(format!("{}.html", step.slug()));
        std::fs::write(path, page(step, &offer, values)).expect("page written");
    }
}
