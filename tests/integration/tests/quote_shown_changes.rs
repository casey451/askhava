//! What the homeowner sees after a business edits or withdraws its quote before any pick
//! (T384; TD-PICK-34's homeowner part: PICK-051; D-084, D-446, D-1310, D-2912). Fake IDs,
//! names and wording only.
//!
//! An edit shows "Updated <time>" in Arizona time on the quote page and the card (the business's
//! preview too) and keeps the quote's number. A withdrawn quote keeps its place as "Withdrawn by
//! the business" on the page and in the LIST reply (English and Spanish), with no wording, name,
//! price or Pick button, and a later quote keeps its number; the page pick, a "2 YES" text and a
//! backup naming it all refuse it, storing no pick, fee or notice. LIST lines carry the quote's
//! page number, so with #1 marked unavailable the second quote's line reads "2." and "2 YES"
//! picks that business. With every quote withdrawn the LIST is no text at all; a form
//! resubmission after a withdrawal comes back in its old place.

use std::sync::Arc;

use askhava_businesses::api::create as create_business;
use askhava_core::api::{IdSecret, LinkToken, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, PublishedTerms, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_licensing::api::{CardLanguage, CardLine, LicenseKind};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_picks::api::rules::Plan;
use askhava_picks::api::{
    Browser, PICK_MADE_KIND, PagePick, TextPickIn, choose_backup, page_pick, text_pick,
};
use askhava_portals::api::quote_page_routes;
use askhava_quotes::api::{
    CardSource, DayPart, Format, Parts, Permit, Price, QuoteFacts, QuoteForm, QuoteRules, Reader,
    Spanish, Submitted, TripFee, Withdrawal, list_texts, mark_unavailable, page_quotes,
    parse_quote, quote_card, shown_card, submit_quote, withdraw_quote,
};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const EN: Reader = Reader {
    wanted: Language::En,
    spanish_on: false,
};
const ES: Reader = Reader {
    wanted: Language::Es,
    spanish_on: true,
};
const BROWSER: Browser = [9; 32];
const TERMS: PublishedTerms<'static> = PublishedTerms {
    version: "2026-11-01",
    text: "Sample homeowner terms for tests only.",
};

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn her() -> Uuid {
    Uuid::from_u128(0x384_0001)
}

fn text(words: &str, when: Timestamp) -> TextIn<'_> {
    let second = u128::try_from(when.as_second()).expect("positive");
    TextIn {
        inbox_id: Uuid::from_u128((0x384 << 64) + second),
        contact: her(),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: when,
    }
}

/// The quote form a business sends, in its own fake words, from the job's one format.
fn form(words: &str) -> askhava_quotes::api::Quote {
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: words.to_owned(),
        ai_read: false,
        earliest_visit: Some((date(2026, 11, 9), DayPart::Evening)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    parse_quote(form, &rules).expect("quote")
}

/// Her request, one plumbing job and three businesses quoting at 9:01, 9:02 and 9:03 a.m.; the
/// first quote moves the job to Quotes ready.
async fn three_quotes(db: &mut Db) -> (Uuid, [Uuid; 3]) {
    let first = text(
        "my kitchen sink is leaking under the cabinet",
        at("02T20:00:00"),
    );
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: first.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: at("02T20:01:00"),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, at("02T20:02:00"), Actor::System)
        .await
        .expect("open");
    let mut ids = [Uuid::nil(); 3];
    for (n, id) in ids.iter_mut().enumerate() {
        *id = one_quote(db, job, n + 1).await;
    }
    (job, ids)
}

/// A new business sent `job`'s card quoting at 9:<n> a.m. with "Sample wording <n>"; its id.
async fn one_quote(db: &mut Db, job: Uuid, n: usize) -> Uuid {
    let id = create_business(db, false, at("01T09:00:00"))
        .await
        .expect("business");
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(id)
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    let when = at(&format!("03T09:{n:02}:00"));
    let words = format!("Sample wording {n}");
    let saved = submit_quote(db, (job, id), &form(&words), when).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    id
}

/// Fake names for the LIST: business `k` of `ids` (from 1) is "Sample Business <k>", for each
/// `k` in `keep` only.
fn names(ids: &[Uuid; 3], keep: &[usize]) -> Vec<(Uuid, String)> {
    keep.iter()
        .map(|&k| (ids[k - 1], format!("Sample Business {k}")))
        .collect()
}

async fn list(db: &mut Db, job: Uuid, reader: Reader, named: &[(Uuid, String)]) -> String {
    list_texts(db, (job, "1234"), reader, named)
        .await
        .expect("list")
        .concat()
}

async fn token(db: &mut Db, job: Uuid) -> LinkToken {
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    issue(db, &secret, job, at("02T21:00:00"))
        .await
        .expect("token")
}

async fn page(pool: &PgPool, token: &LinkToken) -> String {
    let clock = Arc::new(ManualClock::new(at("03T12:00:00")));
    let request = Request::get(format!("/q/{}", token.as_str()))
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let response = quote_page_routes(pool.clone(), clock)
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

/// The page's list item for quote `n`.
fn item(body: &str, n: usize) -> &str {
    let start = body
        .find(&format!("<li id=\"quote-{n}\""))
        .unwrap_or_else(|| panic!("quote {n} missing"));
    let len = body[start..].find("</li>").expect("item end");
    &body[start..start + len]
}

fn source() -> CardSource {
    CardSource {
        business_name: "Sample Valley Plumbing".to_owned(),
        license_lines: vec![CardLine {
            text: "No license needed for this job.".to_owned(),
            link: None,
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

async fn count(db: &mut Db, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut **db)
        .await
        .expect("count")
}

/// TD-PICK-34 (PICK-051, PICK-005): an edit before any pick shows "Updated <time>" in Arizona
/// time on the quote page and on the card, the same on the business's preview, and the quote
/// keeps #1; an unedited quote and card are unchanged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_an_edit_shows_updated_and_keeps_its_number(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, ids) = three_quotes(&mut db).await;
    let plain_before = quote_card(&mut db, (job, ids[1]), source()).await;
    let edit = form("Sample wording 1, now with the new valve");
    let saved = submit_quote(&mut db, (job, ids[0]), &edit, at("03T09:15:00")).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    let shown = page_quotes(&mut db, job).await.expect("quotes");
    assert_eq!(shown[0].business, ids[0]);
    assert_eq!(shown[0].received_at, at("03T09:01:00"));
    assert_eq!(shown[0].updated_at, Some(at("03T09:15:00")));
    assert_eq!(shown[1].updated_at, None);
    let body = page(&pool, &token(&mut db, job).await).await;
    let first = item(&body, 1);
    assert!(first.contains("Updated Nov 3, 9:15 a.m. Arizona time"));
    assert!(first.contains("Sample wording 1, now with the new valve"));
    assert!(first.contains(&format!("data-updated=\"{}\"", at("03T09:15:00"))));
    assert!(first.contains("Pick #1"));
    assert!(!item(&body, 2).contains("Updated"));
    assert!(item(&body, 2).contains("data-updated=\"\""));
    let card = shown_card(&mut db, (job, ids[0]), source()).await;
    let card = card.expect("read").expect("card");
    let lines: Vec<String> = card
        .lines(CardLanguage::English)
        .into_iter()
        .map(|l| l.text)
        .collect();
    let at_words = lines
        .iter()
        .position(|l| l.ends_with("Sample wording 1, now with the new valve"))
        .expect("own words");
    assert_eq!(
        lines[at_words + 1],
        "Updated Nov 3, 9:15 a.m. Arizona time."
    );
    let spanish = card.lines(CardLanguage::Spanish);
    assert!(
        spanish
            .iter()
            .any(|l| l.text == "Actualizada el 3 nov, 9:15 a.m. hora de Arizona.")
    );
    let preview = shown_card(&mut db, (job, ids[0]), source()).await;
    assert_eq!(preview.expect("read").expect("card"), card);
    let plain = shown_card(&mut db, (job, ids[1]), source()).await;
    let plain = plain.expect("read").expect("card");
    assert_eq!(plain.updated_at, None);
    let unchanged = plain_before.expect("read").expect("card");
    assert_eq!(
        plain.lines(CardLanguage::English),
        unchanged.lines(CardLanguage::English)
    );
}

/// TD-PICK-34 (PICK-051, D-084): with #2 withdrawn, the page and the LIST (English and Spanish,
/// no name given for #2) keep its place with "Withdrawn by the business" and no wording, name,
/// price or Pick button; #3 stays #3; the page pick, a "2 YES" text and a backup naming #2 all
/// refuse it, and no pick, fee or pick notice is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_a_withdrawn_quote_keeps_its_place_and_cannot_be_picked(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, ids) = three_quotes(&mut db).await;
    let gone = withdraw_quote(&mut db, (job, ids[1]), at("03T10:00:00")).await;
    assert_eq!(gone, Ok(Withdrawal::Withdrawn));
    let shown = page_quotes(&mut db, job).await.expect("quotes");
    assert_eq!(shown.len(), 3);
    assert!(shown[1].withdrawn && shown[1].unavailable);
    assert_eq!(shown[1].original, "");
    assert_eq!(shown[1].updated_at, None);
    assert_eq!(shown[2].business, ids[2]);
    let body = page(&pool, &token(&mut db, job).await).await;
    let second = item(&body, 2);
    assert!(second.contains("Quote #2"));
    assert!(second.contains("Withdrawn by the business."));
    assert!(second.contains("data-available=\"false\""));
    assert!(!second.contains("Pick #2") && !second.contains("2 YES"));
    assert!(!body.contains("Sample wording 2"));
    assert!(item(&body, 3).contains("Pick #3"));
    let named = names(&ids, &[1, 3]);
    let english = list(&mut db, job, EN, &named).await;
    assert!(english.contains("\n1. Sample Business 1: $190."));
    assert!(english.contains("\n2. Withdrawn by the business.\n"));
    assert!(english.contains("\n3. Sample Business 3: $190."));
    let spanish = list(&mut db, job, ES, &named).await;
    assert!(spanish.contains("\n2. Retirada por el negocio.\n"));
    assert!(spanish.contains("\n3. Sample Business 3: $190."));
    assert!(!english.contains("Sample Business 2") && !english.contains("Sample wording"));
    let picked = page_pick(&mut db, (job, false), 2, Actor::System, at("03T11:00:00")).await;
    assert_eq!(picked, Ok(PagePick::Taken));
    let jobs = [(job, false)];
    let input = TextPickIn {
        contact: her(),
        inbox: text("", at("03T11:01:00")).inbox_id,
        words: "2 YES",
        jobs: &jobs,
        language: (Language::En, false),
        actor: Actor::System,
        at: at("03T11:01:00"),
    };
    let done = text_pick(&mut db, &secret(), &input).await;
    assert_eq!(
        done.expect("text pick").map(|d| d.plan),
        Some(Plan::NoQuote(job))
    );
    let backup = choose_backup(&mut db, (job, 1), (Some(2), &BROWSER), at("03T11:02:00")).await;
    assert_eq!(backup, Ok(None), "No backup");
    let picks = "SELECT count(*) FROM picks.pick WHERE job_id = $1";
    assert_eq!(count(&mut db, picks, job).await, 0);
    let fees = "SELECT count(*) FROM payments.pick_fee WHERE job_id = $1";
    assert_eq!(count(&mut db, fees, job).await, 0);
    let made: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(PICK_MADE_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(made, 0);
}

/// TD-PICK-34 (PICK-051, D-084): LIST numbers are page numbers: with #1 marked unavailable after
/// a failed pick its line is left out, the second quote's line reads "2." and "2 YES" picks that
/// business (after the Terms line).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_list_numbers_are_the_page_numbers(mut db: Db) {
    let (job, ids) = three_quotes(&mut db).await;
    let marked = mark_unavailable(&mut db, (job, ids[0]), at("03T10:00:00")).await;
    assert!(marked.is_ok());
    let english = list(&mut db, job, EN, &names(&ids, &[2, 3])).await;
    assert!(!english.contains("\n1. "));
    assert!(english.contains("\n2. Sample Business 2: $190."));
    assert!(english.contains("\n3. Sample Business 3: $190."));
    for minute in ["01", "02"] {
        let msg = text("2 YES", at(&format!("03T11:{minute}:00")));
        let done = askhava_intake::api::answer_text_pick(
            &mut db,
            &secret(),
            &msg,
            (Settings::default(), &TERMS),
        )
        .await;
        assert!(done.expect("text pick").is_some());
    }
    let picked: (Uuid, i32) =
        sqlx::query_as("SELECT business_id, quote_number FROM picks.pick WHERE job_id = $1")
            .bind(job)
            .fetch_one(&mut *db)
            .await
            .expect("pick");
    assert_eq!(picked, (ids[1], 2));
}

/// TD-PICK-34 (PICK-051): with every quote withdrawn the LIST is no text (no "2 YES" line with
/// nothing to pick); a form resubmission after a withdrawal, confirmed at once, is #2 again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_all_withdrawn_is_no_list_and_a_resubmission_returns(mut db: Db) {
    let (job, ids) = three_quotes(&mut db).await;
    for id in ids {
        let gone = withdraw_quote(&mut db, (job, id), at("03T10:00:00")).await;
        assert_eq!(gone, Ok(Withdrawal::Withdrawn));
    }
    let none = list_texts(&mut db, (job, "1234"), EN, &[]).await;
    assert_eq!(none, Ok(vec![]));
    let back = form("Sample wording 2, sent again");
    let saved = submit_quote(&mut db, (job, ids[1]), &back, at("03T10:30:00")).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    let shown = page_quotes(&mut db, job).await.expect("quotes");
    assert_eq!(shown[1].business, ids[1]);
    assert!(!shown[1].withdrawn && !shown[1].unavailable);
    assert_eq!(shown[1].original, "Sample wording 2, sent again");
    let english = list(&mut db, job, EN, &names(&ids, &[2])).await;
    assert!(english.contains("\n1. Withdrawn by the business.\n"));
    assert!(english.contains("\n2. Sample Business 2: $190."));
    assert!(english.contains("\n3. Withdrawn by the business.\n"));
}

/// TD-PICK-34 (PICK-051, TXT-031): a LIST text whose lines are all withdrawn is not sent. With 11
/// quotes and #11 withdrawn the LIST is one text, lines 1 to 10, with its "2 YES" ending; the
/// second text would hold only "11. Withdrawn by the business." and a "2 YES" line with nothing
/// to pick (English and Spanish). Once #12 arrives the second text returns with both lines.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_a_text_of_only_withdrawn_lines_is_not_sent(mut db: Db) {
    let (job, three) = three_quotes(&mut db).await;
    let mut ids = three.to_vec();
    for n in 4..=11 {
        ids.push(one_quote(&mut db, job, n).await);
    }
    let gone = withdraw_quote(&mut db, (job, ids[10]), at("03T10:00:00")).await;
    assert_eq!(gone, Ok(Withdrawal::Withdrawn));
    let named: Vec<(Uuid, String)> = ids[..10]
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, format!("Sample Business {}", i + 1)))
        .collect();
    for (reader, gone_words) in [
        (EN, "11. Withdrawn by the business."),
        (ES, "11. Retirada por el negocio."),
    ] {
        let texts = list_texts(&mut db, (job, "1234"), reader, &named)
            .await
            .expect("list");
        assert_eq!(texts.len(), 1);
        assert!(texts[0].contains("\n10. Sample Business 10: $190."));
        assert!(!texts[0].contains(gone_words));
    }
    let twelfth = one_quote(&mut db, job, 12).await;
    let mut more = named.clone();
    more.push((twelfth, "Sample Business 12".to_owned()));
    let texts = list_texts(&mut db, (job, "1234"), EN, &more)
        .await
        .expect("list");
    assert_eq!(texts.len(), 2);
    assert!(texts[1].contains("\n11. Withdrawn by the business.\n"));
    assert!(texts[1].contains("\n12. Sample Business 12: $190."));
    assert_eq!(texts[0].lines().last(), texts[1].lines().last());
}
