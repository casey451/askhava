//! TD-FEE-70 (PICK-021, PICK-053; D-2332, D-3497, D-3626, D-3627): on a fresh database, the quote
//! page's Pick for a tiers quote offers one choice per package; a page pick with no package or a
//! stale one shows the choices again and stores nothing; a page pick naming package 2 reaches the
//! Confirm page, which shows package 2 with the fee basis. The pick routes run with the page
//! pick's keys over a Verify stand-in that passes only [`CODE`]. Fake numbers, IDs and words only.

use std::sync::Arc;

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_ops::api::Actor;
use askhava_payments::api::{Fee, Tier, pick_fee};
use askhava_picks::api::ChosenTier;
use askhava_portals::api::{
    ConfirmFacts, PRE_SESSION_COOKIE, PickCodeKeys, confirm_page_with_tier, pick_flow_routes,
    quote_page_routes,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const PHONE: &str = "+19285550162";
const CODE: &str = "123456";
const COOKIE: &str = "fake-pre-session-cookie-value-0440";
const DAY: &str = "2026-11-02T17:00:00Z";
const DONE: &str = "Done. The business has your pick.";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x44), &key(0x40), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-440".to_owned())
}

/// A Verify stand-in that sends nothing and passes only [`CODE`].
#[derive(Clone, Default)]
struct FakeVerify;

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        _channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        Ok("VEfake".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// The quote page and the pick routes with keys; `tier` is the package cookie the browser holds.
struct Page {
    app: Router,
    token: String,
    tier: Option<String>,
}

impl Page {
    fn new(pool: &PgPool, token: String) -> Self {
        let terms = (
            "2026-11-01".to_owned(),
            "Sample homeowner terms for tests only.",
        );
        let keys = PickCodeKeys::new(FakeVerify, vault(), secret(), terms);
        let clock = Arc::new(ManualClock::new(ts(DAY)));
        let app = pick_flow_routes(pool.clone(), clock.clone(), Some(keys))
            .merge(quote_page_routes(pool.clone(), clock));
        Self {
            app,
            token,
            tier: None,
        }
    }

    /// Posts `pairs` to `/q/<token><tail>` as the browser would, keeping a package cookie set.
    async fn post(&mut self, tail: &str, pairs: &[(&str, &str)]) -> (StatusCode, String) {
        let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let body = pairs.iter().fold(format!("csrf={csrf}"), |body, (k, v)| {
            format!("{body}&{k}={v}")
        });
        let mut cookies = format!("{PRE_SESSION_COOKIE}={COOKIE}");
        if let Some(tier) = &self.tier {
            cookies.push_str(&format!("; {tier}"));
        }
        let request = Request::post(format!("/q/{}{tail}", self.token))
            .header(header::HOST, "askhava.test")
            .header(header::ORIGIN, "https://askhava.test")
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookies)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .expect("request");
        let response = self.app.clone().oneshot(request).await.expect("response");
        let set = response.headers().get(header::SET_COOKIE);
        if let Some(set) = set.and_then(|v| v.to_str().ok()) {
            self.tier = set.split(';').next().map(str::to_owned);
        }
        read(response).await
    }

    /// The quote page as she sees it, with `query`.
    async fn quote_page(&self, query: &str) -> String {
        let request = Request::get(format!("/q/{}{query}", self.token))
            .header(header::HOST, "askhava.test")
            .body(Body::empty())
            .expect("request");
        read(self.app.clone().oneshot(request).await.expect("response"))
            .await
            .1
    }

    /// The card's Pick with `tier`, the final Pick, then "Finish my pick": the last answer.
    async fn pick(&mut self, tier: &str) -> (StatusCode, String) {
        let (status, body) = self.post("", &[("quote", "1"), ("tier", tier)]).await;
        if status != StatusCode::OK {
            return (status, body);
        }
        let (status, body) = self
            .post("/pick", &[("quote", "1"), ("share", "yes")])
            .await;
        if status != StatusCode::OK {
            return (status, body);
        }
        let form = [
            ("quote", "1"),
            ("code", CODE),
            ("reach", "call"),
            ("send", "pick"),
        ];
        self.post("/code", &form).await
    }
}

async fn read(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

/// A texted-in plumbing job in Quotes ready with quote #1 a tiers quote (Basic $100, Plus $800,
/// Premium $2,000) and #2 a firm $190; returns the job and its quote-page token.
async fn ready(db: &mut PgConnection) -> (Uuid, String) {
    let (conversation, words) = request(db).await;
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let kinds = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &kinds)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    quotes(db, job).await;
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    (job, token.as_str().to_owned())
}

/// A homeowner with a fake phone texts in: her open request and its words.
async fn request(db: &mut PgConnection) -> (Uuid, &'static str) {
    let phone = Some(SecretString::from(PHONE.to_owned()));
    let new = NewContact {
        name: None,
        phone,
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: ts(DAY),
    };
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    (conversation, words)
}

/// The two confirmed quotes of [`ready`] (fake wording).
async fn quotes(db: &mut PgConnection, job: Uuid) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, tier_names, tier_cents, \
         amount_cents, confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'tiers', ARRAY['Basic', 'Plus', 'Premium'], \
         ARRAY[10000, 80000, 200000]::bigint[], NULL, false, '', 'Sample wording', false, true, $4), \
         ($1, $3, 'firm', NULL, NULL, 19000, false, '', 'Sample wording', false, true, $5)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0xb440_0001))
    .bind(Uuid::from_u128(0xb440_0002))
    .bind((ts(DAY) + jiff::SignedDuration::from_mins(1)).to_sqlx())
    .bind((ts(DAY) + jiff::SignedDuration::from_mins(2)).to_sqlx())
    .execute(db)
    .await
    .expect("quotes");
}

/// The radio value of package `n` (from 1) on quote #1's card.
fn radio(page: &str, n: usize) -> String {
    let at = format!("id=\"quote-1-tier-{n}\" name=\"tier\" value=\"");
    let rest = &page[page.find(&at).expect("radio") + at.len()..];
    rest[..rest.find('"').expect("value end")].to_owned()
}

async fn picks(db: &mut PgConnection, job: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM picks.pick WHERE job_id = $1")
        .bind(job)
        .fetch_one(db)
        .await
        .expect("picks")
}

/// TD-FEE-70 (PICK-053): quote #1's Pick offers its three packages, none preselected; the firm
/// quote #2 offers none. No package, a forged one or one the business changed since is the card
/// again with its error, and no pick is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_70_no_or_stale_package_shows_the_choices_and_stores_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let mut page = Page::new(&pool, token.clone());
    let html = page.quote_page("").await;
    assert_eq!(html.matches("name=\"tier\"").count(), 3, "{html}");
    for label in ["Basic, $100.00", "Plus, $800.00", "Premium, $2,000.00"] {
        assert!(html.contains(label), "{label}");
    }
    assert!(!html.contains("checked"));
    let plus = radio(&html, 2);
    for tier in ["", "1.80000.506c7573ff", "1.70000.506c7573"] {
        let (status, _) = page.post("", &[("quote", "1"), ("tier", tier)]).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{tier}");
    }
    assert!(
        page.tier.is_none(),
        "no package cookie for a refused package"
    );
    let shown = page.quote_page("?choose=1").await;
    assert!(shown.contains("id=\"quote-1-tier-error\""), "{shown}");
    assert!(!page.quote_page("").await.contains("tier-error"));
    for (tail, form) in [("", ("tier", plus.as_str())), ("/pick", ("share", "yes"))] {
        let (status, _) = page.post(tail, &[("quote", "1"), form]).await;
        assert_eq!(status, StatusCode::OK, "{tail}");
    }
    sqlx::query("UPDATE quotes.quote SET tier_cents = ARRAY[10000, 90000, 200000]::bigint[] WHERE job_id = $1 AND format = 'tiers'")
        .bind(job)
        .execute(&mut *db)
        .await
        .expect("edit");
    let finish = [
        ("quote", "1"),
        ("code", CODE),
        ("reach", "call"),
        ("send", "pick"),
    ];
    let (status, _) = page.post("/code", &finish).await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "a passed code with a stale package"
    );
    let (status, _) = page
        .post("/pick", &[("quote", "1"), ("share", "yes")])
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(page.pick(&plus).await.0, StatusCode::SEE_OTHER);
    assert_eq!(picks(&mut db, job).await, 0);
}

/// TD-FEE-70 (PICK-053, PICK-021): picking package 2 (Plus, $800) stores it on the pick with the
/// $75 introduction fee by job size, and the Confirm page shows that package with the fee basis.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_70_package_two_reaches_the_confirm_page(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, token) = ready(&mut db).await;
    let mut page = Page::new(&pool, token);
    let plus = radio(&page.quote_page("").await, 2);
    let (status, body) = page.pick(&plus).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(DONE), "{body}");
    let (pick, index, name, cents): (Uuid, i16, String, i64) = sqlx::query_as(
        "SELECT id, tier_index, tier_name, tier_cents FROM picks.pick WHERE job_id = $1",
    )
    .bind(job)
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    assert_eq!((index, name.as_str(), cents), (1, "Plus", 80_000));
    let fee = pick_fee(&mut db, pick).await.expect("fee").expect("stored");
    assert_eq!(fee.cents, 7_500);
    let tier = ChosenTier {
        index: 1,
        name,
        cents: 80_000,
    };
    let facts = ConfirmFacts {
        pick,
        job_number: 1,
        fee: Fee::Charged {
            basis: Some(80_000),
            tier: Tier::Medium,
        },
        waived: None,
        terms_version: fee.terms_version,
        paused: false,
        press: Uuid::from_u128(0x440),
        csrf: "fake-csrf".to_owned(),
        stripe_key: "pk_test_fake".to_owned(),
    };
    let confirm = confirm_page_with_tier(&facts, None, Some(&tier)).expect("page");
    assert!(
        confirm.contains("The homeowner chose: Plus, $800.00."),
        "{confirm}"
    );
    assert!(confirm.contains("Your $800.00 quote makes this a medium job."));
    assert!(confirm.contains("Confirm and pay $75"));
}
