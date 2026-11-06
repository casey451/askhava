//! TD-FEE-17, TD-FEE-34 and TD-FEE-32's receipt case (FEE-031, FEE-053; D-225, D-491, D-492): on
//! a fresh database, a charged pick raises one IDs-only receipt event, `picks` turns it into one
//! receipt email request naming the charge "Ask Hava introduction service, job <n>", and
//! `messaging` stores one receipt per recipient saying "Tax included." with the pick's "Why was I
//! charged?" link; the page shows the job number, quote, tier, basis and refund rules. Stripe's own
//! receipt is a Dashboard setting (payments brain section 13), so only the receipt's line saying it
//! comes is checked here. All IDs, amounts and addresses are fake.

use askhava_core::api::{Claimed, MIGRATOR};
use askhava_messaging::api::{
    Audience, Language, MailboxKind, Notice, PostalAddress, RECEIPT_EMAIL_KIND, ReceiptSender,
    Recipient, WHY_CHARGED_PREFIX, render_email, render_receipt, run_receipt_email,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    MoneyEventError, RECEIPT_KIND, Tier, queue_receipt, receipt_for, tier_of_fee,
};
use askhava_picks::api::{RECEIPT_NO_RECIPIENT_ALERT, run_receipt_notice};
use askhava_portals::api::{Basis, WHY_CHARGED_ROUTE, WhyCharged, why_charged_page};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Uuid;
use std::sync::Arc;

const PICK: Uuid = Uuid::from_u128(0x7068_0001);
const BUSINESS: Uuid = Uuid::from_u128(0x7068_0002);
const ADDRESS: Uuid = Uuid::from_u128(0x7068_0003);
const POSTAL: &str = "PO Box 0000, Lake Havasu City, AZ 86403";

fn sender() -> ReceiptSender {
    let secret = SecretString::from("fake-id-secret-for-tests-only-t068".to_owned());
    ReceiptSender::new(secret, POSTAL, MailboxKind::PoBox).expect("postal")
}

fn claimed(payload: Value) -> Claimed {
    Claimed {
        id: 1,
        kind: String::new(),
        payload,
        attempts: 1,
    }
}

/// A job with one succeeded $25 charge for `PICK`, written as T062 and T189 store it.
async fn charged(pool: &PgPool) -> Uuid {
    let mut conn = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut conn, Timestamp::now(), false, Actor::System)
        .await
        .expect("job");
    let press = Uuid::from_u128(0x7068_0004);
    let label = format!("pick-{PICK}-try1");
    sqlx::query(
        "INSERT INTO payments.pick_intent VALUES ($1, $2, $3, 2500, 'pi_FakeT068', false, now(), NULL)",
    )
    .bind(PICK)
    .bind(BUSINESS)
    .bind(job)
    .execute(&mut *conn)
    .await
    .expect("intent");
    sqlx::query(
        "INSERT INTO payments.charge_attempt VALUES ($1, 1, $2, 'succeeded', now(), now())",
    )
    .bind(PICK)
    .bind(press)
    .execute(&mut *conn)
    .await
    .expect("attempt");
    sqlx::query(
        "INSERT INTO payments.charge_evidence VALUES ($1, $2, $3, $4, 1, $5, 'cus_FakeT068', \
         'pi_FakeT068', NULL, 2500, now(), now())",
    )
    .bind(PICK)
    .bind(BUSINESS)
    .bind(job)
    .bind(press)
    .bind(label)
    .execute(&mut *conn)
    .await
    .expect("charge");
    job
}

/// `BUSINESS` as an active business whose owner's mailbox is the fake `ADDRESS`.
async fn with_owner(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
         VALUES ($1, 'active', now(), now())",
    )
    .bind(BUSINESS)
    .execute(pool)
    .await
    .expect("business");
    sqlx::query(
        "INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, \
         cards_and_quotes, confirm_and_billing, added_at) VALUES ($1, $2, true, true, true, now())",
    )
    .bind(BUSINESS)
    .bind(ADDRESS)
    .execute(pool)
    .await
    .expect("owner");
}

async fn tasks(pool: &PgPool, kind: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(kind)
        .fetch_all(pool)
        .await
        .expect("tasks")
}

/// TD-FEE-17 (FEE-031, D-492): one IDs-only receipt event per charge, one email request to the
/// money recipients naming the request number, and one stored receipt per recipient.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_17_a_receipt_email_per_charge_names_the_request_number(pool: PgPool) {
    let job = charged(&pool).await;
    with_owner(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    assert!(
        queue_receipt(&mut conn, PICK, Timestamp::now())
            .await
            .expect("queue")
    );
    assert!(
        !queue_receipt(&mut conn, PICK, Timestamp::now())
            .await
            .expect("again")
    );
    let events = tasks(&pool, RECEIPT_KIND).await;
    assert_eq!(events.len(), 1);
    let ids = json!({"pick": PICK, "business": BUSINESS, "job": job});
    assert_eq!(events[0], ids, "the event carries IDs only");
    run_receipt_notice(pool.clone(), claimed(events[0].clone()))
        .await
        .expect("notice");
    run_receipt_notice(pool.clone(), claimed(events[0].clone()))
        .await
        .expect("twice");
    let asks = tasks(&pool, RECEIPT_EMAIL_KIND).await;
    assert_eq!(asks.len(), 1, "one email request per pick");
    let number = asks[0]["values"]["job"].as_i64().expect("job number");
    let line = format!("Ask Hava introduction service, job {number}");
    assert_eq!(asks[0]["values"]["line"], line.as_str());
    assert_eq!(asks[0]["values"]["fee_cents"], 2500);
    assert_eq!(
        asks[0]["to"],
        json!([ADDRESS]),
        "the owner's mailbox ID only"
    );
    let ask = asks[0].clone();
    let sender = Arc::new(sender());
    run_receipt_email(pool.clone(), Arc::clone(&sender), claimed(ask.clone()))
        .await
        .expect("email");
    run_receipt_email(pool.clone(), sender, claimed(ask))
        .await
        .expect("email again");
    let rows: Vec<(String, String, Uuid)> = sqlx::query_as(
        "SELECT subject, plain, recipient_id FROM messaging.email_outbox WHERE template_id = 'charge_receipt'",
    )
    .fetch_all(&pool)
    .await
    .expect("outbox");
    assert_eq!(rows.len(), 1, "one receipt per recipient and charge");
    assert_eq!(rows[0].0, format!("Ask Hava: Receipt for job {number}"));
    assert!(rows[0].1.contains(&format!("{line}: $25.00")));
    assert_eq!(rows[0].2, ADDRESS);
}

/// TD-FEE-17, FEE-053: no receipt for a pick with no charge, and another business reads nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_17_no_charge_no_receipt(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("conn");
    let none = queue_receipt(&mut conn, PICK, Timestamp::now()).await;
    assert_eq!(none, Err(MoneyEventError::NothingCharged));
    charged(&pool).await;
    let other = Uuid::from_u128(0x7068_0009);
    assert_eq!(receipt_for(&mut conn, PICK, other).await, Ok(None));
    let event = json!({"pick": PICK, "business": other, "job": PICK});
    run_receipt_notice(pool.clone(), claimed(event))
        .await
        .expect("notice");
    assert!(tasks(&pool, RECEIPT_EMAIL_KIND).await.is_empty());
    let bad = run_receipt_notice(pool.clone(), claimed(json!({"pick": "x"}))).await;
    assert!(bad.is_err());
    assert!(
        run_receipt_email(pool.clone(), Arc::new(sender()), claimed(json!({})))
            .await
            .is_err()
    );
    assert_eq!(tier_of_fee(7_500), Some(Tier::Medium));
    assert_eq!(tier_of_fee(19_900), Some(Tier::Large));
    assert_eq!(tier_of_fee(100), None);
}

/// TD-FEE-17 (FEE-031, D-1561): a charge whose business has no money recipient raises one alert
/// and queues no email, an empty recipient list is refused by the email handler, and a fee that
/// is no tier's (a changed tier price) still gets its receipt, naming no tier.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_17_no_recipient_alerts_and_any_fee_gets_a_receipt(pool: PgPool) {
    let job = charged(&pool).await;
    let event = json!({"pick": PICK, "business": BUSINESS, "job": job});
    run_receipt_notice(pool.clone(), claimed(event.clone()))
        .await
        .expect("alerted");
    assert!(tasks(&pool, RECEIPT_EMAIL_KIND).await.is_empty());
    let alerts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.alert WHERE kind = $1 AND subject_kind = 'pick' AND subject_id = $2",
    )
    .bind(RECEIPT_NO_RECIPIENT_ALERT)
    .bind(PICK)
    .fetch_one(&pool)
    .await
    .expect("alerts");
    assert_eq!(alerts, 1);
    let empty =
        json!({"pick": PICK, "to": [], "values": {"job": 1, "fee_cents": 2500, "line": "x"}});
    let refused = run_receipt_email(pool.clone(), Arc::new(sender()), claimed(empty)).await;
    assert!(
        refused.is_err(),
        "an empty recipient list is never a silent success"
    );
    sqlx::query("UPDATE payments.charge_evidence SET amount_cents = 3000 WHERE pick_id = $1")
        .bind(PICK)
        .execute(&pool)
        .await
        .expect("fake changed price");
    let mut conn = pool.acquire().await.expect("conn");
    let receipt = receipt_for(&mut conn, PICK, BUSINESS).await.expect("read");
    assert_eq!(receipt.map(|r| (r.fee_cents, r.tier)), Some((3_000, None)));
    with_owner(&pool).await;
    run_receipt_notice(pool.clone(), claimed(event))
        .await
        .expect("notice");
    let asks = tasks(&pool, RECEIPT_EMAIL_KIND).await;
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["values"]["fee_cents"], 3000);
}

/// TD-FEE-34, TD-FEE-32 (FEE-031, FEE-053): the receipt says "Tax included.", has one charge
/// line, says Stripe sends its own receipt and links the pick's page, whose route matches; its
/// footer is T094's business footer, in English and draft Spanish.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_34_the_receipt_says_tax_included_and_links_why(pool: PgPool) {
    charged(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    let receipt = receipt_for(&mut conn, PICK, BUSINESS)
        .await
        .expect("read")
        .expect("charge");
    assert_eq!(
        (receipt.fee_cents, receipt.tier),
        (2_500, Some(Tier::Small))
    );
    let line = "Ask Hava introduction service, job 1001".to_owned();
    let facts = askhava_messaging::api::ReceiptFacts {
        pick: PICK,
        job_number: 1001,
        fee_cents: 2500,
        line,
    };
    let email = render_receipt(&sender(), Language::En, &facts).expect("render");
    let link = format!("{WHY_CHARGED_PREFIX}{PICK}");
    assert_eq!(
        link,
        format!(
            "https://askhava.com{}",
            WHY_CHARGED_ROUTE.replace("{pick}", &PICK.to_string())
        )
    );
    for part in [&email.html, &email.plain] {
        assert!(part.contains("Tax included."));
        assert!(part.contains("Why was I charged?"));
        assert!(part.contains(&link));
        assert!(part.contains("Stripe also emails its own receipt for this same charge."));
        assert_eq!(part.matches("Ask Hava introduction service").count(), 1);
    }
    let to = Recipient {
        id: ADDRESS,
        audience: Audience::Business,
        language: Language::En,
        manage_url: "https://askhava.com/b",
    };
    let postal = PostalAddress::new(POSTAL, MailboxKind::PoBox).expect("postal");
    let footer = render_email(&postal, &to, Notice::StillOpen(1)).expect("footer");
    let tail = |plain: &str| plain.split("\n--\n").nth(1).map(str::to_owned);
    assert_eq!(tail(&email.plain), tail(&footer.plain));
    let spanish = render_receipt(&sender(), Language::Es, &facts).expect("es");
    assert!(spanish.html.contains("lang=\"es\"") && spanish.plain.contains("Impuestos incluidos."));
    let es = Recipient {
        language: Language::Es,
        ..to
    };
    let es_footer = render_email(&postal, &es, Notice::StillOpen(1)).expect("es footer");
    assert_eq!(tail(&spanish.plain), tail(&es_footer.plain));
    let bad = ReceiptSender::new(
        SecretString::from("x".to_owned()),
        "no mailbox",
        MailboxKind::PoBox,
    );
    assert!(bad.is_err());
}

/// TD-FEE-34 (FEE-031, FEE-039, FEE-044, D-225, D-1561): the "Why was I charged?" page shows the
/// job number, the quote, the tier, how the basis was set and the refund rules. Every basis case
/// pairs a basis with the tier its range really gives, or checks the capped-tier sentence: an
/// earlier version of this test paired a $1,425.00 basis with the large tier and accepted "over
/// $1,500.00", a false line (reviewer finding, D-1561).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_34_the_why_page_shows_job_quote_tier_basis_and_refunds(pool: PgPool) {
    charged(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    let receipt = receipt_for(&mut conn, PICK, BUSINESS)
        .await
        .expect("read")
        .expect("charge");
    let mut facts = WhyCharged {
        receipt,
        job_number: 1001,
        quote: "$480.00, fixed price".to_owned(),
        basis: Basis::Quote(48_000),
    };
    let page = why_charged_page(&facts).expect("page");
    for words in [
        "Why was I charged for job 1001?",
        "Ask Hava introduction service, job 1001: $25.00. Tax included.",
        "Your quote: $480.00, fixed price",
        "Introduction fee by job size: $25.00 (small job).",
        "The fee basis from your quote is $480.00. That is under $500.00, so this is a small job.",
        "This charge is for this one pick only.",
        "2 call attempts at least 3 hours apart",
    ] {
        assert!(page.contains(words), "missing: {words}");
    }
    let line = |facts: &WhyCharged| why_charged_page(facts).expect("page");
    facts.basis = Basis::OnSite;
    assert!(line(&facts).contains("That makes it a small job."));
    (facts.receipt.tier, facts.basis) = (Some(Tier::Medium), Basis::Quote(142_500));
    assert!(
        line(&facts)
            .contains("is $1,425.00. That is from $500.00 to $1,500.00, so this is a medium job.")
    );
    (facts.receipt.tier, facts.basis) = (Some(Tier::Large), Basis::Quote(150_001));
    assert!(line(&facts).contains("is $1,500.01. That is over $1,500.00, so this is a large job."));
    (facts.receipt.tier, facts.basis) = (Some(Tier::Medium), Basis::FourMonths(240_000));
    let capped = line(&facts);
    assert!(capped.contains("4 months of your regular price: $2,400.00. That is over $1,500.00, but the fee for this kind of job is set at the medium job fee."));
    assert!(
        !capped.contains("so this is a"),
        "a capped basis never claims its own tier"
    );
    (facts.receipt.tier, facts.basis) = (None, Basis::Quote(48_000));
    let untiered = line(&facts);
    assert!(untiered.contains("Introduction fee by job size: $25.00.</li>"));
    assert!(untiered.contains("That is under $500.00.</li>"));
    facts.basis = Basis::OnSite;
    assert!(line(&facts).contains("for this kind of work.</li>"));
    facts.job_number = i64::MAX;
    assert!(why_charged_page(&facts).is_err());
}
