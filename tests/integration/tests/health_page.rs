//! The owner health page from planted data on PostgreSQL (OPS-008, OPS-009, OPS-037, BIZ-044,
//! SEC-002, SEC-036; T476, D-3753, D-3844, D-3882, D-4280, D-492).
//!
//! TD-OPS-02's page half: `portals::api::owner_health_page` shows every OPS-008 figure from rows
//! planted straight into each module's tables (fake IDs and fake words only): text and email rates
//! per audience, sending-limit use, TD-OPS-12's 20- and 60-minute medians and the 6-hour share,
//! the share of this month's cards that ended without a quote, payment and AI errors, uptime, the
//! last backup and each secret's three flags beside the SEC-036 identifiers, with no business-call
//! rate (TD-OPS-10's page figures); the OPS-009 spot-check shows 10 distinct cards of the window and
//! words a window of fewer or none; and a sentinel secret read through `Settings::from_pairs`
//! reaches the page as its name and flags only, its value, length and hash nowhere.

use std::collections::BTreeSet;

use askhava_core::api::{DEFAULT_SPOT_CHECK_EVERY_DAYS, Settings};
use askhava_integration::set_app_role;
use askhava_portals::api::{
    AccountIds, DEFAULT_QUOTED_WITHIN, HealthInputs, SecretLine, UPTIME_HISTORY_URL,
    owner_health_page,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// A fake 32-character secret value set as `ASKHAVA_STRIPE_SECRET_KEY`; nothing on the page may reveal it.
const SENTINEL: &str = "sk_test_FAKE_SENTINEL_value_0042";

/// The page's "now": Friday Nov 20, 2026, noon in Arizona.
const NOW: &str = "2026-11-20T19:00:00Z";

/// Runs one seeding statement with text arguments; the SQL casts each.
async fn seed(db: &mut PgConnection, sql: &'static str, args: &[&str]) {
    let mut q = sqlx::query(sql);
    for a in args {
        q = q.bind((*a).to_owned());
    }
    q.execute(db).await.expect("seed");
}

/// A sent text to `kind` at `when` whose delivery report is `event`.
async fn text(db: &mut PgConnection, kind: &str, when: &str, event: &str) {
    let sid = format!("SM{}", Uuid::new_v4().simple());
    seed(
        db,
        "INSERT INTO messaging.outbox (idempotency_key, template_id, recipient_id, recipient_kind,
           subject_ids, status, provider_sid, created_at, sent_at)
         VALUES (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'fake_template',
           gen_random_uuid(), $1, '{}', 'sent', $2, $3::timestamptz, $3::timestamptz)",
        &[kind, &sid, when],
    )
    .await;
    seed(
        db,
        "INSERT INTO messaging.delivery_event (provider_sid, status, received_at)
         VALUES ($1, $2, $3::timestamptz)",
        &[&sid, event, when],
    )
    .await;
}

/// An email to a business sent at `sent` with one provider event `event`.
async fn email(db: &mut PgConnection, sent: &str, event: &str) {
    let provider = format!("fake-{}", Uuid::new_v4());
    seed(
        db,
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id,
           recipient_kind, subject_ids, class, status, not_before, provider_id, queued_at, sent_at)
         VALUES (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), 'fake_template',
           gen_random_uuid(), 'business', '{}', 'notice', 'sent', $2::timestamptz, $1,
           $2::timestamptz, $2::timestamptz)",
        &[&provider, sent],
    )
    .await;
    seed(
        db,
        "INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at)
         VALUES ($1, $2, $3::timestamptz, $3::timestamptz)",
        &[&provider, event, sent],
    )
    .await;
}

/// A job card for a new business sent at `sent`, with a quote when `quoted`; returns its ID.
async fn card(db: &mut PgConnection, sent: &str, quoted: bool) -> Uuid {
    let id = Uuid::new_v4().to_string();
    seed(
        db,
        "INSERT INTO quotes.job_card (id, job_id, business_id, handyman, sent_at)
         VALUES ($1::uuid, gen_random_uuid(), gen_random_uuid(), false, $2::timestamptz)",
        &[&id, sent],
    )
    .await;
    if quoted {
        seed(
            db,
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site,
               notes, original, ai_read, confirmed, received_at)
             SELECT job_id, business_id, 'firm', 10000, false, '', 'Fake quote', false, true,
               sent_at FROM quotes.job_card WHERE id = $1::uuid",
            &[&id],
        )
        .await;
    }
    id.parse().expect("uuid")
}

/// A charge attempt in `status` started and decided at `when` (undecided when `status` is
/// `sent`), with a refund in `refund` when given.
async fn charge(db: &mut PgConnection, status: &str, when: &str, refund: &str) {
    let pick = Uuid::new_v4().to_string();
    let decided = if status == "sent" { "" } else { when };
    seed(
        db,
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1::uuid, gen_random_uuid(), gen_random_uuid(), 7500, 'pi_' || md5($1), false,
           $2::timestamptz)",
        &[&pick, when],
    )
    .await;
    seed(
        db,
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at)
         VALUES ($1::uuid, 1, gen_random_uuid(), $2, $3::timestamptz, NULLIF($4, '')::timestamptz)",
        &[&pick, status, when, decided],
    )
    .await;
    if !refund.is_empty() {
        seed(
            db,
            "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
               idempotency_label, stripe_customer_id, stripe_intent_id, amount_cents, pressed_at,
               charged_at)
             SELECT pick_id, business_id, job_id, gen_random_uuid(), 1, 'pick-' || pick_id || '-try1',
               'cus_fake', stripe_intent_id, 7500, created_at, created_at
               FROM payments.pick_intent WHERE pick_id = $1::uuid",
            &[&pick],
        )
        .await;
        seed(
            db,
            "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at)
             VALUES ($1::uuid, 'unreachable', $2, 1, $3::timestamptz)",
            &[&pick, refund, when],
        )
        .await;
    }
}

/// Plants every figure's rows; returns the IDs of the cards inside the spot-check window.
async fn plant(db: &mut PgConnection) -> BTreeSet<Uuid> {
    let day = "2026-11-18T17:00:00Z";
    for event in ["delivered", "delivered", "undelivered"] {
        text(db, "homeowner", day, event).await;
    }
    text(db, "business", day, "delivered").await;
    for event in ["email.delivered", "email.delivered", "email.bounced"] {
        email(db, "2026-11-20T17:00:00Z", event).await;
    }
    email(db, "2026-11-05T17:00:00Z", "email.delivered").await;
    // TD-OPS-12: sent 10:00, first opened 10:20, quoted 11:00 (Arizona).
    seed(
        db,
        "INSERT INTO messaging.notice_log (business_id, job_id, kind, mailbox_id, sent_at,
           held_until, first_opened_at, acted_at)
         VALUES (gen_random_uuid(), gen_random_uuid(), 'card', gen_random_uuid(),
           '2026-11-18T17:00:00Z', '2026-11-18T17:00:00Z', '2026-11-18T17:20:00Z',
           '2026-11-18T18:00:00Z')",
        &[],
    )
    .await;
    let mut inside = BTreeSet::new();
    for n in 1..=12 {
        inside.insert(card(db, &format!("2026-11-{n:02}T17:00:00Z"), n <= 3).await);
    }
    card(db, "2026-10-01T17:00:00Z", false).await;
    charge(db, "declined", day, "").await;
    charge(db, "sent", day, "").await;
    charge(db, "succeeded", day, "failed").await;
    for outcome in ["ok", "ok", "ok", "timed_out"] {
        seed(
            db,
            "INSERT INTO ai.call_stat (at, subject_id, job, instruction_version, model, outcome,
               attempts)
             VALUES ($1::timestamptz, gen_random_uuid(), 'intake_reading', 'fake_v1',
               'claude-fake-model', $2, 1)",
            &[day, outcome],
        )
        .await;
    }
    inside
}

/// The inputs the serving route would pass: fake limits, uptime, secret flags and identifiers.
fn inputs() -> HealthInputs {
    let line = |name: &str, env_matched: bool| SecretLine {
        name: name.to_owned(),
        present: true,
        parsed: true,
        env_matched,
    };
    HealthInputs {
        email_daily_limit: 100,
        email_monthly_limit: 3000,
        quoted_within: DEFAULT_QUOTED_WITHIN,
        spot_check_every_days: DEFAULT_SPOT_CHECK_EVERY_DAYS,
        web_up_for: SignedDuration::from_mins(2 * 1440 + 3 * 60 + 4),
        worker_up: true,
        secrets: vec![
            line("ASKHAVA_STRIPE_SECRET_KEY", false),
            line("ASKHAVA_TWILIO_AUTH_TOKEN", true),
        ],
        accounts: AccountIds {
            twilio_account_sid: Some("ACfake0000000000000000000000000000".to_owned()),
            stripe_account_id: Some("acct_fake000000".to_owned()),
            resend_domain: Some("mail.example.test".to_owned()),
            storage_bucket: None,
        },
    }
}

/// Renders the page as the app role.
async fn render(pool: &PgPool, inputs: &HealthInputs) -> String {
    let mut conn = pool.acquire().await.expect("connection");
    set_app_role(&mut conn).await.expect("SET ROLE askhava_app");
    let now: Timestamp = NOW.parse().expect("now");
    owner_health_page(&mut conn, inputs, now)
        .await
        .expect("page")
}

/// Plants the rows and renders the page.
async fn page(pool: &PgPool) -> (String, BTreeSet<Uuid>) {
    let inside = plant(&mut pool.acquire().await.expect("connection")).await;
    (render(pool, &inputs()).await, inside)
}

/// TD-OPS-02, TD-OPS-12, TD-OPS-10: every OPS-008 figure from planted rows, no business-call rate.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_02_the_page_shows_every_ops_008_figure(pool: PgPool) {
    let (html, _) = page(&pool).await;
    for want in [
        "Texts to homeowners delivered</dt><dd>2 of 3 (66.6%)",
        "Failed texts to homeowners</dt><dd>1",
        "Texts to businesses delivered</dt><dd>1 of 1 (100.0%)",
        "Emails to businesses</dt><dd>3 sent; 2 delivered (66.6%), 1 bounced (33.3%), 0 complaints (0.0%)",
        "Emails to homeowners</dt><dd>0 sent; 0 delivered (none sent)",
        "Emails sent today</dt><dd>3 of the daily limit of 100",
        "Emails sent this month</dt><dd>4 of the monthly limit of 3000",
        "first opened by the business</dt><dd>20 minutes",
        "card to quote</dt><dd>60 minutes",
        "Job cards quoted within 6 hours</dt><dd>100.0%",
        "ended without a quote</dt><dd>9 of 12 (75.0%)",
        "Failed charges, last 7 days</dt><dd>1",
        "Charges still pending</dt><dd>1",
        "Failed refunds, last 7 days</dt><dd>1",
        "AI errors, last 7 days</dt><dd>1 of 4 calls",
        "Web app running for</dt><dd>2 days, 3 hours, 4 minutes",
        "Background worker</dt><dd>Up",
        "Last backup</dt><dd>Not yet recorded",
        UPTIME_HISTORY_URL,
        "Twilio Account SID</dt><dd>ACfake0000000000000000000000000000",
        "Stripe account ID</dt><dd>acct_fake000000",
        "Resend sending domain</dt><dd>mail.example.test",
        "Storage bucket</dt><dd>Not set",
        "ASKHAVA_STRIPE_SECRET_KEY</th><td>Yes</td><td>Yes</td><td>No</td>",
        "ASKHAVA_TWILIO_AUTH_TOKEN</th><td>Yes</td><td>Yes</td><td>Yes</td>",
    ] {
        assert!(html.contains(want), "missing {want:?}");
    }
    let lower = html.to_lowercase();
    assert!(!lower.contains("call rate") && !lower.contains("pick call"));
}

/// OPS-009, TD-OPS-02: the spot-check shows 10 distinct cards, each from the last 30 days.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_009_spot_check_shows_ten_distinct_cards_of_the_month(pool: PgPool) {
    let (html, inside) = page(&pool).await;
    let shown: BTreeSet<Uuid> = html
        .split("<tr><td>")
        .skip(1)
        .map(|row| row[..36].parse().expect("card id"))
        .collect();
    assert_eq!(shown.len(), 10, "{shown:?}");
    assert!(shown.is_subset(&inside), "a card outside the window");
    assert!(html.contains("Every 30 days, open these 10 job cards, picked at random"));
}

/// OPS-009, D-4626: an empty window says there is nothing to check; a window of 3 cards shows
/// all 3 as every card of the window, never "10".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_009_spot_check_words_a_window_of_fewer_than_ten_cards(pool: PgPool) {
    let html = render(&pool, &inputs()).await;
    assert!(
        html.contains("No job cards were sent in the last 30 days, so there are none to check.")
    );
    assert!(!html.contains("open these") && !html.contains("<tr><td>"));
    let mut db = pool.acquire().await.expect("connection");
    for _ in 0..3 {
        card(&mut db, "2026-11-18T17:00:00Z", false).await;
    }
    drop(db);
    let html = render(&pool, &inputs()).await;
    assert!(html.contains(
        "Every 30 days, open each job card below. They are all the cards sent in the last 30 days."
    ));
    assert!(!html.contains("10 job cards"));
    assert_eq!(html.matches("<tr><td>").count(), 3);
}

/// TD-OPS-02, SEC-002, D-492: a sentinel secret read through `Settings::from_pairs` reaches the
/// page only as its name and three flags; its value, length and hash appear nowhere.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_sentinel_secrets_value_length_and_hash_appear_nowhere(pool: PgPool) {
    let settings = Settings::from_pairs(
        [
            ("ASKHAVA_APP_ENV", "dev"),
            (
                "ASKHAVA_DATABASE_URL",
                "postgres://fake:fake@localhost/fake",
            ),
            ("ASKHAVA_STRIPE_SECRET_KEY", SENTINEL),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    )
    .expect("settings");
    let key = settings.stripe_secret_key().expect("the sentinel is read");
    assert_eq!(key.expose_secret(), SENTINEL);
    let line = SecretLine {
        name: "ASKHAVA_STRIPE_SECRET_KEY".to_owned(),
        present: true,
        parsed: true,
        env_matched: key.expose_secret().starts_with("sk_test_"),
    };
    // No `..`: a field added to `SecretLine` (a value, a length, a hash) stops this compiling.
    let SecretLine {
        name: _,
        present: _,
        parsed: _,
        env_matched: _,
    } = &line;
    let mut with_sentinel = inputs();
    with_sentinel.secrets = vec![line];
    let html = render(&pool, &with_sentinel).await;
    let hash: String = Sha256::digest(SENTINEL.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!html.contains(SENTINEL) && !html.contains(&hash) && !html.contains(&hash[..16]));
    let table = html
        .split("<table id=\"secrets\">")
        .nth(1)
        .and_then(|t| t.split("</table>").next())
        .expect("secrets table");
    assert!(table.contains("ASKHAVA_STRIPE_SECRET_KEY</th><td>Yes</td><td>Yes</td><td>Yes</td>"));
    assert_eq!(SENTINEL.len(), 32);
    assert!(!table.contains("32"), "the length shows: {table}");
    assert!(!html.to_lowercase().contains("length"));
}

/// OPS-009, D-4625: the spot-check cadence is 30 days when unset and refuses 0 or text by name.
#[test]
fn the_spot_check_cadence_is_a_typed_setting() {
    let parse = |days: Option<&str>| {
        let mut pairs = vec![
            ("ASKHAVA_APP_ENV".to_owned(), "dev".to_owned()),
            (
                "ASKHAVA_DATABASE_URL".to_owned(),
                "postgres://fake@localhost/fake".to_owned(),
            ),
        ];
        pairs.extend(days.map(|d| ("ASKHAVA_SPOT_CHECK_EVERY_DAYS".to_owned(), d.to_owned())));
        Settings::from_pairs(pairs)
    };
    let unset = parse(None).expect("default");
    assert_eq!(unset.spot_check_every_days(), DEFAULT_SPOT_CHECK_EVERY_DAYS);
    assert_eq!(DEFAULT_SPOT_CHECK_EVERY_DAYS, 30);
    assert_eq!(parse(Some("7")).expect("7").spot_check_every_days(), 7);
    for bad in ["0", "abc"] {
        let err = parse(Some(bad)).expect_err(bad).to_string();
        assert!(
            err.contains("ASKHAVA_SPOT_CHECK_EVERY_DAYS") && !err.contains(bad),
            "{err}"
        );
    }
}
