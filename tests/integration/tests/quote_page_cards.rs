//! The served quote page's cards (T393, IC-71; TD-PICK-62: PICK-041, PICK-010, LIC-024, LIC-026,
//! LIC-034, LIC-038, LIC-047, BIZ-052; D-3080, D-3134, D-3642 to D-3645). Fake businesses,
//! records, names and wording only.
//!
//! Each shown quote with stored facts carries its card on `/q/{token}`: the trade name (else the
//! business name), the license lines of the job's category with their links, the Google and Yelp
//! links only when given as plain links, the PICK-040 facts and "doesn't rank businesses; check it
//! yourself" once per card under the page's one top sentence; a business with no usable name
//! shows the logged fallback; `?lang=es` gives the Spanish lines; nothing names the homeowner or
//! gives a business phone or address.

use std::sync::Arc;

use askhava_businesses::api::{create as create_business, save_business_name};
use askhava_core::api::{IdSecret, LinkToken, ManualClock, brand};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_portals::api::{CARD_FALLBACK_NAME, quote_page_routes};
use askhava_privacy::api::{Vault, mailbox_for, store_staff_phone};
use askhava_quotes::api::{
    DayPart, Format, Parts, Permit, Price, QuoteFacts, QuoteForm, QuoteRules, Spanish, Submitted,
    TripFee, parse_quote, submit_quote_with_facts,
};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const CHECK_LINE: &str = "doesn't rank businesses; check it yourself.";
const ROC_LINE: &str = "ROC #312345 showed Active on the ROC website when we checked on November 1, 2026. Check it yourself.";

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

/// A request from fake homeowner `seed` opening a job of `category`, moved to Open.
async fn open_job(db: &mut Db, seed: u128, category: &str) -> Uuid {
    let words = "my kitchen sink is leaking under the cabinet";
    let first = askhava_intake::api::TextIn {
        inbox_id: Uuid::from_u128((0x393 << 64) + seed),
        contact: Uuid::from_u128(0x393_0000 + seed),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at("02T20:00:00"),
    };
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: at("02T20:01:00"),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["sample job type".to_owned()];
    record_job_facts(db, job, category, &types)
        .await
        .expect("job facts");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, at("02T20:02:00"), Actor::System)
        .await
        .expect("open");
    job
}

fn facts() -> QuoteFacts {
    QuoteFacts {
        trip_fee: TripFee::Counts,
        parts: Parts::Included,
        permit: Permit::NotNeeded,
        may_change: false,
        earliest_visit: (date(2026, 11, 9), DayPart::Evening),
        spanish: Spanish::No,
    }
}

/// A new business quoting on `job` at 9:<n> a.m. with "Sample wording <n>" and stored facts.
async fn quote(db: &mut Db, job: Uuid, n: usize) -> Uuid {
    let id = create_business(db, false, at("01T09:00:00"))
        .await
        .expect("business");
    sql(db, "INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, '2026-11-03T08:00:00-07:00')", &[job, id]).await;
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: format!("Sample wording {n}"),
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
    let quote = parse_quote(form, &rules).expect("quote");
    let when = at(&format!("03T09:{n:02}:00"));
    let saved = submit_quote_with_facts(db, (job, id), &quote, &facts(), when).await;
    assert_eq!(saved, Ok(Submitted::Saved));
    id
}

async fn sql(db: &mut Db, text: &'static str, ids: &[Uuid]) {
    let mut query = sqlx::query(text);
    for id in ids {
        query = query.bind(*id);
    }
    query.execute(&mut **db).await.expect("fixture row");
}

/// Business `id`'s ROC license 312345, Active on the November 1 list and check, issued in 2015.
async fn roc_license(db: &mut Db, id: Uuid) {
    let list = Uuid::from_u128(0x393_0001);
    sql(db, "INSERT INTO licensing.list_import (id, source, list_date, rows_recorded, rows_skipped) VALUES ($1, 'roc', '2026-11-01', 1, 0) ON CONFLICT DO NOTHING", &[list]).await;
    sql(db, "INSERT INTO licensing.agency_record (import_id, source, list_date, license_number, class_code, status, issued_on) VALUES ($1, 'roc', '2026-11-01', '312345', 'R-37', 'active', '2015-04-02') ON CONFLICT DO NOTHING", &[list]).await;
    sql(db, "INSERT INTO licensing.license_check (business_id, source, license_number, list_date, status, checked_at) VALUES ($1, 'roc', '312345', '2026-11-01', 'active', '2026-11-01T09:00:00-07:00')", &[id]).await;
}

/// Business `id`'s fake sealed email address and main phone (PICK-010: never on the page).
async fn contacts(db: &mut Db, id: Uuid) {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    let vault = Vault::new(&key(0x31), &key(0x41), &url).expect("vault keys");
    let mailbox = mailbox_for(db, &vault, "sample.pipes@example.com")
        .await
        .expect("mailbox");
    let (member, line) = (Uuid::from_u128(0x393_0010), Uuid::from_u128(0x393_0011));
    sql(db, "INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes, confirm_and_billing, added_at) VALUES ($1, $2, $3, true, true, true, now())", &[member, id, mailbox]).await;
    sql(db, "INSERT INTO businesses.staff_phone (id, business_id, linked_by, is_main, linked_at) VALUES ($1, $2, $3, true, now())", &[line, id, member]).await;
    store_staff_phone(db, &vault, line, "(928) 555-0177")
        .await
        .expect("phone");
}

async fn token(db: &mut Db, job: Uuid) -> LinkToken {
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    issue(db, &secret, job, at("02T21:00:00"))
        .await
        .expect("token")
}

/// The served page for `job`, `query` appended, with HTML entities read back.
async fn page(pool: &PgPool, db: &mut Db, job: Uuid, query: &str) -> String {
    let token = token(db, job).await;
    let clock = Arc::new(ManualClock::new(at("03T12:00:00")));
    let request = Request::get(format!("/q/{}{query}", token.as_str()))
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let response = quote_page_routes(pool.clone(), clock)
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf8");
    body.replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2f;", "/")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// The page's list item for quote `n`.
fn item(body: &str, n: usize) -> &str {
    let start = body
        .find(&format!("<li id=\"quote-{n}\""))
        .unwrap_or_else(|| panic!("quote {n} missing"));
    let rest = &body[start + 1..];
    let len = rest.find("<li id=\"quote-").unwrap_or(rest.len());
    &body[start..=start + len]
}

fn check_line() -> String {
    format!("{} {CHECK_LINE}", brand().display_name())
}

/// TD-PICK-62 (PICK-041, LIC-026, LIC-024, LIC-038, BIZ-052, PICK-010): an ROC business's card
/// shows its trade name, the check line with the ROC link, its since-line and its given Google and
/// Yelp links as plain links; a handyman with no usable name shows the fallback, LIC-024 first and
/// no review link and, off the city list, no city line; a no-license business shows the dated
/// city line; the top sentence once, the check line once per card, no homeowner detail and no
/// business phone or email address; a later Inactive check drops the ROC line (D-3646).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_62_roc_handyman_and_city_cards(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = open_job(&mut db, 1, "plumbing_repair").await;
    let roc = quote(&mut db, job, 1).await;
    let handyman = quote(&mut db, job, 2).await;
    let city = quote(&mut db, job, 3).await;
    let names = ("Sample Desert Pipes LLC", Some("Sample Pipes"));
    save_business_name(&mut db, roc, names, None)
        .await
        .expect("name");
    roc_license(&mut db, roc).await;
    contacts(&mut db, roc).await;
    sql(&mut db, "INSERT INTO businesses.review_link (business_id, google_url, yelp_url, changed_at) VALUES ($1, 'https://g.page/sample-pipes', 'https://www.yelp.com/biz/sample-pipes', now())", &[roc]).await;
    sql(&mut db, "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) VALUES ($1, $1, now(), '{handyman}')", &[handyman]).await;
    let own = ("Sample Lake Cleaners", None);
    save_business_name(&mut db, city, own, None)
        .await
        .expect("name");
    sql(
        &mut db,
        "UPDATE businesses.business SET insured = true WHERE id = $1",
        &[city],
    )
    .await;
    let list = Uuid::from_u128(0x393_0002);
    sql(&mut db, "INSERT INTO licensing.list_import (id, source, list_date, rows_recorded, rows_skipped) VALUES ($1, 'city', '2026-11-01', 1, 0)", &[list]).await;
    sql(&mut db, "INSERT INTO licensing.city_license_entry (import_id, source, list_date, business_name, issued_on) VALUES ($1, 'city', '2026-11-01', 'SAMPLE LAKE CLEANERS', '2019-06-01')", &[list]).await;
    let body = page(&pool, &mut db, job, "").await;
    let first = item(&body, 1);
    assert!(first.contains("<li>Sample Pipes</li>"), "{first}");
    assert!(!first.contains("Sample Desert Pipes LLC"));
    assert!(
        first.contains(&format!("<a href=\"https://roc.az.gov/\">{ROC_LINE}</a>")),
        "{first}"
    );
    assert!(first.contains("<li>ROC license since 2015</li>"));
    assert!(first.contains("<a href=\"https://g.page/sample-pipes\">Google</a>"));
    assert!(first.contains("<a href=\"https://www.yelp.com/biz/sample-pipes\">Yelp</a>"));
    assert!(
        first.contains("Trip fee counts toward the repair:"),
        "{first}"
    );
    assert!(first.contains("Pick #1"));
    let second = item(&body, 2);
    assert!(
        second.contains(&format!("<li>{}</li>", CARD_FALLBACK_NAME[0])),
        "{second}"
    );
    for other in ["Sample Pipes", "Sample Lake Cleaners", "Sample Desert"] {
        assert!(!second.contains(other), "{other}: {second}");
    }
    let lic_024 = "Not a licensed contractor. Only for jobs under $1,000 total with no permit.";
    assert!(second.contains(lic_024), "{second}");
    assert!(!second.contains("City business license"), "{second}");
    assert!(!second.contains("Google") && !second.contains("Yelp") && !second.contains("ROC #"));
    let third = item(&body, 3);
    assert!(third.contains("<li>Sample Lake Cleaners</li>"));
    assert!(
        third.contains("<li>City business license on file since 2019</li>"),
        "{third}"
    );
    assert!(third.contains("Insurance (business-reported): Yes"));
    assert!(!third.contains(lic_024));
    assert_eq!(
        body.matches("doesn't rank businesses or show ratings")
            .count(),
        1
    );
    assert_eq!(body.matches(&check_line()).count(), 3);
    for banned in [
        "★",
        "stars",
        "rating:",
        "reviews)",
        "tel:",
        "mailto:",
        "555-0177",
        "5550177",
        "sample.pipes@example.com",
        "my kitchen sink",
    ] {
        assert!(!body.contains(banned), "{banned}");
    }
    sql(&mut db, "INSERT INTO licensing.license_check (business_id, source, license_number, list_date, status, checked_at) VALUES ($1, 'roc', '312345', '2026-11-10', 'inactive', '2026-11-10T09:00:00-07:00')", &[roc]).await;
    let lapsed = page(&pool, &mut db, job, "").await;
    let first = item(&lapsed, 1);
    assert!(first.contains("<li>Sample Pipes</li>"), "{first}");
    assert!(
        !first.contains("ROC #") && !first.contains("showed Active"),
        "{first}"
    );
    assert!(!first.contains("ROC license since"), "{first}");
}

/// TD-PICK-62 (LIC-034, LIC-047): an inspector's card shows the Board's check line with its link;
/// a pest attester's card shows the LIC-047 line with the Department's search and no check line,
/// and no pest line once a homeowner's report paused it (D-3646).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_62_agency_and_pest_cards(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let inspection = open_job(&mut db, 2, "home_inspection").await;
    let inspector = quote(&mut db, inspection, 1).await;
    sql(&mut db, "INSERT INTO licensing.business_license (business_id, agency, license_number, verification, recorded_at) VALUES ($1, 'home_inspector', 'HI-5512', 'agency_record', '2026-10-20T10:00:00-07:00')", &[inspector]).await;
    let body = page(&pool, &mut db, inspection, "").await;
    let agency = "Board of Technical Registration license #HI-5512 showed Active on the Board of Technical Registration website when we checked on October 20, 2026. Check it yourself.";
    let card = item(&body, 1);
    assert!(
        card.contains(&format!("<a href=\"https://btr.az.gov/\">{agency}</a>")),
        "{card}"
    );
    assert_eq!(body.matches(&check_line()).count(), 1);
    let pests = open_job(&mut db, 3, "pest_control").await;
    let pest = quote(&mut db, pests, 1).await;
    sql(&mut db, "INSERT INTO licensing.business_license (business_id, agency, license_number, verification) VALUES ($1, 'pest_management', 'PMB-1234', 'self_attested')", &[pest]).await;
    let body = page(&pool, &mut db, pests, "").await;
    let card = item(&body, 1);
    let lic_047 = format!(
        "This business told us it holds Arizona Department of Agriculture pest license #PMB-1234. {} has not independently verified this. Check it yourself:",
        brand().display_name()
    );
    let search = "https://searchagriculture.az.gov/mastercontent/licsearch.aspx";
    assert!(
        card.contains(&format!("<a href=\"{search}\">{lic_047}</a>")),
        "{card}"
    );
    assert!(!card.contains("showed Active"));
    assert_eq!(body.matches(&check_line()).count(), 1);
    sql(
        &mut db,
        "UPDATE licensing.business_license SET paused_at = now() WHERE business_id = $1",
        &[pest],
    )
    .await;
    let paused = page(&pool, &mut db, pests, "").await;
    let card = item(&paused, 1);
    assert!(
        !card.contains("PMB-1234") && !card.contains(search),
        "{card}"
    );
}

/// TD-PICK-62 (PICK-041, LIC-026, LIC-024, LIC-038, D-3645, D-3646): `?lang=es` gives the
/// Spanish card lines (ROC, fallback name, city, insurance) in a line list marked `lang="es"`,
/// the LIC-024 line marked `lang="en"`, the Spanish check line once per card; a quote with no
/// stored facts keeps the plain lines and its Pick button (D-3134).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_62_spanish_page_gives_spanish_lines(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = open_job(&mut db, 4, "plumbing_repair").await;
    let roc = quote(&mut db, job, 1).await;
    roc_license(&mut db, roc).await;
    let names = ("Sample Desert Pipes LLC", None);
    save_business_name(&mut db, roc, names, None)
        .await
        .expect("name");
    let handyman = quote(&mut db, job, 2).await;
    sql(&mut db, "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) VALUES ($1, $1, now(), '{handyman}')", &[handyman]).await;
    sql(
        &mut db,
        "UPDATE businesses.business SET insured = true WHERE id = $1",
        &[handyman],
    )
    .await;
    let city = quote(&mut db, job, 3).await;
    let own = ("Sample Lake Cleaners", None);
    save_business_name(&mut db, city, own, None)
        .await
        .expect("name");
    let list = Uuid::from_u128(0x393_0003);
    sql(&mut db, "INSERT INTO licensing.list_import (id, source, list_date, rows_recorded, rows_skipped) VALUES ($1, 'city', '2026-11-01', 1, 0)", &[list]).await;
    sql(&mut db, "INSERT INTO licensing.city_license_entry (import_id, source, list_date, business_name, issued_on) VALUES ($1, 'city', '2026-11-01', 'SAMPLE LAKE CLEANERS', '2019-06-01')", &[list]).await;
    let body = page(&pool, &mut db, job, "?lang=es").await;
    let card = item(&body, 1);
    assert!(
        card.contains("<ul class=\"card-lines\" lang=\"es\">"),
        "{card}"
    );
    assert!(
        !card.contains("<article aria-labelledby=\"quote-1-title\" lang"),
        "{card}"
    );
    assert!(card.contains("<li>Sample Desert Pipes LLC</li>"));
    let roc_es = "ROC #312345 figuraba como Activa en el sitio web del ROC cuando lo revisamos el 1 de noviembre de 2026. Compruébelo usted mismo.";
    assert!(card.contains(roc_es), "{card}");
    assert!(card.contains("Palabras del negocio: Sample wording 1"));
    assert!(!card.contains(ROC_LINE));
    let es = "no califica ni ordena a los negocios; revise este negocio usted mismo.";
    assert_eq!(body.matches(es).count(), 3);
    assert!(!body.contains(CHECK_LINE));
    let second = item(&body, 2);
    let fallback = format!("<li>{}</li>", CARD_FALLBACK_NAME[1]);
    assert!(second.contains(&fallback), "{second}");
    let lic_024 = "Not a licensed contractor. Only for jobs under $1,000 total with no permit.";
    assert!(
        second.contains(&format!("<li lang=\"en\">{lic_024}</li>")),
        "{second}"
    );
    assert!(
        second.contains("<li>Seguro (informado por el negocio): Sí</li>"),
        "{second}"
    );
    assert!(!second.contains("Licencia comercial"), "{second}");
    let third = item(&body, 3);
    let city_es = "<li>Licencia comercial de la ciudad registrada desde 2019</li>";
    assert!(third.contains(city_es), "{third}");
    sql(
        &mut db,
        "DELETE FROM quotes.quote_facts WHERE business_id = $1",
        &[roc],
    )
    .await;
    let plain = page(&pool, &mut db, job, "").await;
    let card = item(&plain, 1);
    assert!(
        card.contains("In their words:") && card.contains("Pick #1"),
        "{card}"
    );
    assert!(!card.contains("Sample Desert Pipes LLC"));
    assert_eq!(plain.matches(&check_line()).count(), 3);
}
