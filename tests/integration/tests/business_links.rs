//! The business's own names and its Google and Yelp links (TD-BIZ-32 name and link cases,
//! TD-PICK-18 link lines, TD-SEC-20 review-link host rule; BIZ-022, BIZ-052, PICK-041, SEC-041,
//! LIC-011; D-2943, D-2954). Fake names, numbers and links only.
//!
//! As the app role: a good name, trade name and city license number are stored, a second save
//! replaces them, and a name holding a phone number, a link or a channel direction is refused with
//! its reason and stores nothing; each save writes one audit row naming the field only. A Google
//! link on another domain and a dead Yelp link are refused; saved links show on the stored quote
//! card as plain "Google" and "Yelp" link lines right after the license lines. A city license
//! number holding a phone number is refused; a Google search, Sites, Docs or `/url?q=` link and a
//! Yelp search link are refused (D-3037). The guarded fetch
//! itself is `adapters`' `ssrf_guard.rs` (TD-SEC-20); here a fake opener answers.

use askhava_businesses::api::{
    BusinessNames, BusinessesError, LinkOpener, LinkRefusal, LinksError, NameRefusal, ReviewSite,
    business_names, check_links, create, on_site, review_links, save_business_name,
    save_review_links,
};
use askhava_core::api::Guarded;
use askhava_integration::set_app_role;
use askhava_jobs::api::{JobStatus, create as create_job, move_to};
use askhava_licensing::api::{CardLanguage, CardLine, LicenseKind};
use askhava_ops::api::Actor;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    Entered, PageLanguage, Parts as PageParts, Screen, fee_numbers, signup_page,
};
use askhava_quotes::api::{
    CardSource, DayPart, Format, Parts, Permit, Price, QuoteFacts, QuoteForm, QuoteRules, Spanish,
    TripFee, parse_quote, quote_card, submit_quote,
};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

/// A name and trade name, a city license number and the refusal expected.
type NameCase<'a> = ((&'a str, Option<&'a str>), Option<&'a str>, NameRefusal);

const GOOGLE: &str = "https://g.page/example-sample-plumbing";
const YELP: &str = "https://www.yelp.com/biz/example-sample-plumbing";

/// Answers every link with `opens`, recording nothing.
struct FakeOpener(bool);

impl LinkOpener for FakeOpener {
    async fn opens(&self, _site: ReviewSite, _url: &str) -> bool {
        self.0
    }
}

fn now() -> Timestamp {
    "2026-11-11T17:00:00Z".parse().expect("fake time")
}

async fn app(pool: &sqlx::PgPool) -> Db {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    conn
}

async fn field_rows(conn: &mut PgConnection, id: Uuid, field: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM ops.audit_log WHERE subject_id = $1 AND field = $2",
    )
    .bind(id)
    .bind(field)
    .fetch_one(conn)
    .await
    .expect("audit count")
}

/// TD-BIZ-32 (BIZ-022, D-2943, D-2954): a good name, trade name and city license number are
/// stored; a second save replaces them; each save has one audit row naming the field only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_32_names_are_stored_then_replaced(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = create(&mut conn, false, now()).await.expect("business");
    assert_eq!(business_names(&mut conn, id).await, Ok(None));
    let first = ("  Example Fixit LLC ", Some("Sample Odd Jobs"));
    save_business_name(&mut conn, id, first, Some("CL-7001"))
        .await
        .expect("saved");
    let stored = business_names(&mut conn, id).await.expect("read");
    let want = BusinessNames {
        business_name: "Example Fixit LLC".to_owned(),
        trade_name: Some("Sample Odd Jobs".to_owned()),
        city_license_number: Some("CL-7001".to_owned()),
    };
    assert_eq!(stored, Some(want));
    save_business_name(&mut conn, id, ("Sample Valley Plumbing", Some("  ")), None)
        .await
        .expect("replaced");
    let stored = business_names(&mut conn, id).await.expect("read");
    let want = BusinessNames {
        business_name: "Sample Valley Plumbing".to_owned(),
        trade_name: None,
        city_license_number: None,
    };
    assert_eq!(stored, Some(want));
    assert_eq!(field_rows(&mut conn, id, "business_name").await, 2);
}

/// TD-BIZ-32 (BIZ-022, PICK-041, D-336): a name or trade name holding a phone number, a link or a
/// direction to another channel is refused with its reason; a blank or over-long name or city
/// number is refused too; nothing is stored and no audit row is written.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_32_a_screened_name_stores_nothing(pool: sqlx::PgPool) {
    let mut conn = app(&pool).await;
    let id = create(&mut conn, false, now()).await.expect("business");
    let long = "x".repeat(201);
    let refused: [NameCase<'_>; 7] = [
        (("Sample Plumbing 928 555 0199", None), None, contact()),
        (
            ("Sample Plumbing", Some("sampleplumbing.com")),
            None,
            contact(),
        ),
        (("Find us on Google Sample Plumbing", None), None, channel()),
        (("   ", None), None, NameRefusal::Missing),
        ((long.as_str(), None), None, NameRefusal::TooLong),
        (
            ("Sample Plumbing", None),
            Some(&"9".repeat(65)),
            NameRefusal::CityLicense,
        ),
        (
            ("Sample Plumbing", None),
            Some("928-555-0100"),
            NameRefusal::CityLicenseScreened(askhava_businesses::api::AboutUsRefusal::Contact),
        ),
    ];
    for (names, city, want) in refused {
        let got = save_business_name(&mut conn, id, names, city).await;
        assert_eq!(got, Err(LinksError::Name(want)), "{names:?}");
        assert!(!want.reason().is_empty());
    }
    assert!(contact().reason().contains("phone numbers"));
    assert!(channel().reason().contains("Google"));
    assert_eq!(business_names(&mut conn, id).await, Ok(None));
    assert_eq!(field_rows(&mut conn, id, "business_name").await, 0);
    let stranger = save_business_name(&mut conn, Uuid::from_u128(9), ("Sample", None), None).await;
    let unknown = LinksError::Businesses(BusinessesError::UnknownBusiness);
    assert_eq!(stranger, Err(unknown));
}

fn contact() -> NameRefusal {
    NameRefusal::Screened(askhava_businesses::api::AboutUsRefusal::Contact)
}

fn channel() -> NameRefusal {
    NameRefusal::Screened(askhava_businesses::api::AboutUsRefusal::Channel)
}

/// TD-BIZ-32, TD-SEC-20 (BIZ-052, SEC-041): a link is accepted only as https on its own site's
/// host, with no user name or port; another domain or a dead page is refused with its reason.
#[tokio::test]
async fn td_biz_32_links_on_another_domain_or_dead_are_refused() {
    for good in [
        GOOGLE,
        "https://www.google.com/maps/place/x",
        "https://maps.app.goo.gl/x",
        "https://g.co/kgs/x",
    ] {
        assert!(on_site(ReviewSite::Google, good), "{good}");
    }
    let bad = [
        "http://g.page/x",
        "https://g.page.example.com/x",
        "https://notgoogle.com/x",
        "https://@google.com/x",
        "https://google.com:8443/x",
        "https://.google.com/x",
        "https://www.google.com/search?q=928-555-0100",
        "https://sites.google.com/view/example-sample",
        "https://docs.google.com/document/d/example",
        "https://www.google.com/url?q=https://example.com",
    ];
    for url in bad {
        assert!(!on_site(ReviewSite::Google, url), "{url}");
    }
    assert!(!on_site(ReviewSite::Yelp, GOOGLE));
    assert!(!on_site(
        ReviewSite::Yelp,
        "https://www.yelp.com/search?find_desc=928-555-0100"
    ));
    let wrong = check_links(&FakeOpener(true), Some("https://example.com/biz"), None).await;
    assert_eq!(wrong, Err(LinkRefusal::WrongSite(ReviewSite::Google)));
    let dead = check_links(&FakeOpener(false), None, Some(YELP)).await;
    assert_eq!(dead, Err(LinkRefusal::DoesNotOpen(ReviewSite::Yelp)));
    assert!(dead.unwrap_err().reason().contains("didn't open"));
    let yelp_wrong = LinkRefusal::WrongSite(ReviewSite::Yelp).reason();
    assert!(yelp_wrong.contains("yelp.com"));
    let none = check_links(&FakeOpener(false), Some(" "), None).await;
    assert_eq!(none.map(|l| l.google().is_none()), Ok(true));
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
            earliest_visit: (date(2026, 11, 12), DayPart::Morning),
            spanish: Spanish::No,
        },
    }
}

/// An open job with a confirmed quote from `business`.
async fn quoted(conn: &mut PgConnection, business: Uuid) -> Uuid {
    let job = create_job(conn, now(), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(conn, job, open, now(), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(business)
        .bind(now().to_sqlx())
        .execute(&mut *conn)
        .await
        .expect("card");
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: "Swap the valve".to_owned(),
        ai_read: false,
        earliest_visit: Some((date(2026, 11, 12), DayPart::Morning)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    submit_quote(conn, (job, business), &quote, now())
        .await
        .expect("quote saved");
    job
}

/// TD-PICK-18, TD-BIZ-32 (BIZ-052, PICK-041): saved links show on the stored quote card as plain
/// "Google" and "Yelp" links right after the license lines; a business with none shows none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_18_saved_links_show_on_the_quote_card(pool: sqlx::PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    let id = create(&mut conn, false, now()).await.expect("business");
    let job = quoted(&mut conn, id).await;
    let bare = quote_card(&mut conn, (job, id), source())
        .await
        .expect("card");
    assert_eq!(bare.expect("shown").license_lines, source().license_lines);
    let links = check_links(&FakeOpener(true), Some(GOOGLE), Some(YELP))
        .await
        .expect("checked");
    save_review_links(&mut conn, id, &links)
        .await
        .expect("saved");
    assert_eq!(review_links(&mut conn, id).await.as_ref(), Ok(&links));
    let card = quote_card(&mut conn, (job, id), source())
        .await
        .expect("card");
    let lines = card.expect("shown").lines(CardLanguage::English);
    let link = |text: &str, url: &str| CardLine {
        text: text.to_owned(),
        link: Some(url.to_owned()),
    };
    assert_eq!(lines[2], link("Google", GOOGLE));
    assert_eq!(lines[3], link("Yelp", YELP));
    assert!(lines[4].text.contains("check it yourself"));
    assert_eq!(field_rows(&mut conn, id, "review_links").await, 1);
    let stranger = save_review_links(&mut conn, Uuid::from_u128(9), &links).await;
    assert_eq!(stranger, Err(BusinessesError::UnknownBusiness));
}

/// TD-BIZ-32 (BIZ-022, BIZ-052, LIC-011, A11Y-030): the form asks the names, the license numbers
/// and both links with no `required`; a refused name comes back with what was typed, the reason
/// by its field and a focused summary linking to it.
#[test]
fn td_biz_32_the_form_shows_a_refused_name_again() {
    let parts = PageParts {
        texting_on: false,
        language: PageLanguage::En,
        csrf: "fake-csrf",
        fees: fee_numbers(ConfirmSettings::default(), WindowRules::default()),
    };
    let cats = vec![("plumbing_repair".to_owned(), "Open now")];
    let page = signup_page(Screen::Form(&cats), &parts).expect("page");
    for name in [
        "business_name",
        "trade_name",
        "roc_number",
        "btr_number",
        "city_license_number",
        "google_link",
        "yelp_link",
    ] {
        assert!(page.contains(&format!("name=\"{name}\"")), "{name}");
    }
    assert!(!page.contains("required") && !page.contains("error-summary"));
    let reason = contact().reason();
    let entered = Entered {
        business_name: "Sample Plumbing 928 555 0199",
        google_link: GOOGLE,
        problem: Some(("business_name", reason.as_str())),
        ..Entered::default()
    };
    let again = signup_page(Screen::FormAgain(&cats, &entered), &parts).expect("page");
    assert!(again.contains("<title>Error: Sign up your business"));
    assert!(again.contains("href=\"#business-name\""));
    assert!(again.contains("id=\"business-name-error\""));
    assert!(again.contains("aria-invalid=\"true\""));
    assert!(again.contains("value=\"Sample Plumbing 928 555 0199\""));
    assert!(again.contains(&format!("value=\"{GOOGLE}\"")));
}
