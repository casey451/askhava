//! The owner's "away until" date, the Google and Yelp links on the business home, and a won
//! dispute on the statement (TD-OPS-07; OPS-026, CON-018, BIZ-052, BIZ-015; D-207, D-439, D-2625,
//! D-3753, D-3844, D-3890).
//!
//! Proves that with the away date stored through `ops::api`, the business home's review and
//! dispute lines and the HELP reply (read through `messaging::api::reply_to_help` with settings
//! that carry no date) say "away until <date>" and the 911 line instead of the reply-time line;
//! that a stored date before today in Arizona counts as not away (`still_away`, D-4493); that
//! real timers queued through their own scheduling functions are due and fire exactly as without
//! it; that a Google or Yelp link on another
//! domain is refused and stores nothing, and only the owner changes them; and that a won
//! dispute's two rows read as a won dispute on the page and in the CSV. All data are fake.

use askhava_businesses::api::{
    Acting, LinkOpener, LinkRefusal, OpenCardsTimes, ReviewSite, Roles, add_member, add_owner,
    create, review_links,
};
use askhava_core::api::{WorkerId, claim};
use askhava_jobs::api::{BACKSTOP_KIND, schedule_backstop};
use askhava_messaging::api::{
    BODY_SCAN_KIND, HelpSettings, InboundText, ProviderKeyword, Relation, reply_to_help,
    schedule_body_scan, still_away,
};
use askhava_ops::api::set_away_until;
use askhava_payments::api::{DISPUTE_FEE_CENTS, EntryKind, MoneyView, Statement, StatementLine};
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, BusinessPrincipal, CookieJar, HomeExtras, InboxSwitch, LinksChangeError,
    Realm, YourNumbers, billing_page, business_home_page_with, business_principal, save_home_links,
    start_session, statement_csv,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::{Date, date};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Far enough ahead that the HELP reply (which reads the system clock's today) still finds it.
const AWAY: Date = date(2099, 11, 20);
const DAY_SECS: i64 = 86_400;
const GOOGLE: &str = "https://g.page/fake-plumbing-co";
const YELP: &str = "https://www.yelp.com/biz/fake-plumbing-co";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_secs(secs)
}

/// A fake opener: every link opens; no network.
struct Opens;

impl LinkOpener for Opens {
    async fn opens(&self, _site: ReviewSite, _url: &str) -> bool {
        true
    }
}

async fn away(db: &mut PgConnection) {
    away_on(db, AWAY).await;
}

async fn away_on(db: &mut PgConnection, day: Date) {
    set_away_until(db, Some(day), Uuid::from_u128(0x0a), at(0))
        .await
        .expect("away set");
}

fn home(extras: &HomeExtras<'_>) -> String {
    let times = OpenCardsTimes::new(&[]).expect("no times");
    let inbox = InboxSwitch::default();
    let numbers = YourNumbers::default();
    business_home_page_with(&numbers, (times, false), (&[], &inbox), extras, "csrf").expect("page")
}

/// TD-OPS-07 (OPS-026, D-207): with the date set the home's review and dispute lines show it.
#[test]
fn td_ops_07_the_home_shows_the_away_date() {
    let page = home(&HomeExtras {
        away_until: Some(AWAY),
        ..HomeExtras::default()
    });
    for (id, words) in [
        ("away-line", "away until November 20"),
        ("review-line", "reply by November 20"),
        ("dispute-line", "reply by November 20"),
    ] {
        let line = page.split(&format!("id=\"{id}\"")).nth(1).expect(id);
        let line = line.split("</p>").next().expect(id);
        assert!(line.contains(words), "{id}");
    }
    assert!(!home(&HomeExtras::default()).contains("away until"));
}

/// A fake homeowner contact.
async fn contact(db: &mut PgConnection) -> Uuid {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x52), &key(0x62), &url).expect("vault");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550142".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault, &new).await.expect("contact")
}

/// The HELP reply queued for "HELP" from `her` with settings that carry no date.
async fn help_body(db: &mut PgConnection, her: Uuid, inbox: u128) -> String {
    let msg = InboundText {
        inbox_id: Uuid::from_u128(inbox),
        contact: her,
        relation: Relation::OpenRequest,
        phone: "+19285550142",
        text: "HELP",
        provider: ProviderKeyword::None,
        emergency: false,
    };
    let settings = HelpSettings::new((2, 24), None, (None, "+19285550100")).expect("settings");
    let secret = SecretString::from("fake-id-secret-for-tests".to_owned());
    let row = reply_to_help(db, &secret, &msg, &settings)
        .await
        .expect("reply");
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(row.expect("queued"))
        .fetch_one(db)
        .await
        .expect("body")
}

/// TD-OPS-07 (OPS-026, CON-018, D-3890): the stored date turns the HELP reply into the away text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_07_the_help_reply_says_away_until(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let her = contact(&mut db).await;
    let before = help_body(&mut db, her, 0x1b1).await;
    assert!(before.contains("usually replies within") && !before.contains("away until"));
    away(&mut db).await;
    let body = help_body(&mut db, her, 0x1b2).await;
    assert!(body.contains("away until Nov 20"), "{body}");
    assert!(
        body.contains("call 911") && !body.contains("usually replies"),
        "{body}"
    );
}

/// TD-OPS-07 (OPS-026, D-207, D-4493): a stored date before today in Arizona is not away, so the
/// HELP reply keeps its reply-time line instead of naming a day already gone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_07_a_past_away_date_is_not_shown(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let her = contact(&mut db).await;
    away_on(&mut db, date(2020, 11, 20)).await;
    let body = help_body(&mut db, her, 0x1b3).await;
    assert!(
        body.contains("usually replies within") && !body.contains("away until"),
        "{body}"
    );
}

/// TD-OPS-07 (OPS-026, D-4493): the shared reader the HELP reply and the business home both use
/// keeps the date through its own day in Arizona and drops it from the next day on.
#[test]
fn td_ops_07_still_away_ends_after_the_date_in_arizona() {
    let day = date(2026, 11, 20);
    let noon = |d: Date| {
        d.at(12, 0, 0, 0)
            .to_zoned(askhava_core::api::arizona())
            .expect("zoned")
            .timestamp()
    };
    assert_eq!(still_away(Some(day), noon(date(2026, 11, 19))), Some(day));
    assert_eq!(still_away(Some(day), noon(day)), Some(day));
    assert_eq!(still_away(Some(day), noon(date(2026, 11, 25))), None);
    let late_in_arizona = Timestamp::from_second(1_795_237_200).expect("time");
    assert_eq!(still_away(Some(day), late_in_arizona), Some(day));
    assert_eq!(still_away(None, noon(day)), None);
}

/// Real timers queued at `now` through their own scheduling functions (the timer backstop,
/// JOB-006; the text-body scan, PRIV-027) on an emptied queue: each kind with its due time in
/// seconds after `now`, and the kinds claimed a second before and at that due time.
async fn fired(pool: &PgPool, now: Timestamp) -> (Vec<(String, i64)>, Vec<Vec<String>>) {
    let mut db = pool.acquire().await.expect("db");
    sqlx::query("DELETE FROM core.task")
        .execute(&mut *db)
        .await
        .expect("empty queue");
    schedule_backstop(&mut db, now).await.expect("backstop");
    schedule_body_scan(&mut db, now).await.expect("body scan");
    let due: Vec<(String, i64)> = sqlx::query_as(
        "SELECT kind, EXTRACT(EPOCH FROM run_at)::bigint - $1 FROM core.task ORDER BY kind",
    )
    .bind(now.as_second())
    .fetch_all(&mut *db)
    .await
    .expect("due");
    let worker = WorkerId::new(pool).await.expect("worker");
    let mut seen = Vec::new();
    for t in [DAY_SECS - 1, DAY_SECS] {
        let when = now + SignedDuration::from_secs(t);
        let claimed = claim(pool, worker, when, SignedDuration::from_hours(1), 10).await;
        let mut kinds: Vec<_> = claimed
            .expect("claim")
            .into_iter()
            .map(|c| c.kind)
            .collect();
        kinds.sort();
        seen.push(kinds);
    }
    (due, seen)
}

/// TD-OPS-07 (D-207): real timers are due and fire exactly as without the away date.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_07_every_timer_fires_as_without_it(pool: PgPool) {
    let without = fired(&pool, at(1_000)).await;
    away(&mut pool.acquire().await.expect("db")).await;
    let with = fired(&pool, at(1_000)).await;
    assert_eq!(with, without);
    let mut kinds = vec![BACKSTOP_KIND.to_owned(), BODY_SCAN_KIND.to_owned()];
    kinds.sort();
    let due: Vec<_> = kinds.iter().map(|k| (k.clone(), DAY_SECS)).collect();
    assert_eq!(without, (due, vec![vec![], kinds]));
}

async fn principal(db: &mut PgConnection, who: Acting) -> BusinessPrincipal {
    let login = (Realm::Business, who.member);
    let jar = start_session(db, CookieJar::new(), login, BUSINESS_SESSION_LIFE, at(0)).await;
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(who.member)
    .bind(who.business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let jar = jar.expect("session");
    business_principal(db, &jar, at(1))
        .await
        .expect("read")
        .expect("signed in")
}

/// BIZ-052, D-439: a link on another domain is refused and stores nothing; only the owner saves.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_052_a_link_on_another_domain_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let business = create(&mut db, false, at(0)).await.expect("business");
    let member = add_owner(&mut db, business, Uuid::from_u128(0xe1), at(0)).await;
    let owner = Acting {
        business,
        member: member.expect("owner"),
    };
    let roles = Roles::new(true, false).expect("roles");
    let cards = add_member(&mut db, owner, Uuid::from_u128(0xe3), roles, at(0)).await;
    let cards = Acting {
        business,
        member: cards.expect("member"),
    };
    let boss = principal(&mut db, owner).await;
    for (google, yelp, site) in [
        (
            "https://fake-reviews.example/biz/fake",
            YELP,
            ReviewSite::Google,
        ),
        (
            GOOGLE,
            "https://www.yelp.com.fake.example/biz/fake",
            ReviewSite::Yelp,
        ),
        (YELP, "", ReviewSite::Google),
    ] {
        let tried = save_home_links(&mut db, &Opens, &boss, (Some(google), Some(yelp))).await;
        let refused = LinksChangeError::Refused(LinkRefusal::WrongSite(site));
        assert_eq!(tried.expect_err("refused"), refused, "{google} {yelp}");
    }
    let stored = review_links(&mut db, business).await.expect("links");
    assert_eq!((stored.google(), stored.yelp()), (None, None));
    let staff = principal(&mut db, cards).await;
    let tried = save_home_links(&mut db, &Opens, &staff, (Some(GOOGLE), None)).await;
    assert_eq!(tried.expect_err("not the owner"), LinksChangeError::Role);
    save_home_links(&mut db, &Opens, &boss, (Some(GOOGLE), Some(YELP)))
        .await
        .expect("saved");
    let stored = review_links(&mut db, business).await.expect("links");
    assert_eq!((stored.google(), stored.yelp()), (Some(GOOGLE), Some(YELP)));
    let refusal = LinkRefusal::WrongSite(ReviewSite::Yelp);
    let page = home(&HomeExtras {
        refused: Some(refusal),
        ..HomeExtras::default()
    });
    assert!(page.starts_with("<!") && page.contains("Error: ") && page.contains(refusal.reason()));
}

/// A statement of one $199 fee disputed (the dispute line carries Stripe's fee), and, when `won`,
/// the countering-fee return and the dispute-reversal of the disputed amount (D-2625).
fn disputed(won: bool) -> Statement {
    let mut entries = vec![
        (EntryKind::Charge, 19_900),
        (EntryKind::Dispute, 19_900 + DISPUTE_FEE_CENTS),
    ];
    if won {
        entries.extend([
            (EntryKind::CounteringFeeReturn, DISPUTE_FEE_CENTS),
            (EntryKind::Dispute, -19_900),
        ]);
    }
    let lines = (entries.iter())
        .map(|&(kind, amount_cents)| StatementLine {
            kind,
            job: Some(Uuid::from_u128(0x10b)),
            amount_cents,
            recorded_at: at(0),
        })
        .collect();
    let totals = MoneyView::from_entries(entries);
    Statement { lines, totals }
}

/// D-3844, BIZ-015: a won dispute's two rows read as a won dispute on the page and in the CSV,
/// the fee back on the account; a lost dispute is shown as before.
#[test]
fn d_3844_a_won_dispute_reads_as_won_on_the_page_and_in_the_csv() {
    let won = statement_csv(&disputed(true));
    let rows: Vec<_> = won
        .lines()
        .skip(1)
        .map(|l| l.split(',').collect::<Vec<_>>())
        .collect();
    let shown: Vec<_> = rows.iter().map(|r| (r[1], r[3])).collect();
    assert_eq!(
        shown,
        [
            ("Introduction fee by job size", "199.00"),
            ("You disputed this charge with your bank", "-199.00"),
            ("Bank dispute closed: no cost to you", "0.00"),
            ("Bank dispute closed: fee owed again", "199.00"),
        ]
    );
    let page = billing_page(&disputed(true), &[], None).expect("page");
    for (what, _) in &shown {
        assert!(page.contains(what), "{what}");
    }
    let lost = statement_csv(&disputed(false));
    assert_eq!(lost.lines().count(), 3);
    assert!(lost.contains("You disputed this charge with your bank,") && !lost.contains("closed"));
}
