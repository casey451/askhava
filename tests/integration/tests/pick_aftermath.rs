//! After the pick: the PICK-026 loss notice with the PICK-038 comparison, "This job is no longer
//! active." once and the PICK-027 live-pick reply, in the database (TD-PICK-11, TD-PICK-17,
//! TD-PICK-20, TD-PICK-33; PICK-026, PICK-027, PICK-038). Fake IDs and amounts only.
//!
//! A job's card goes to a few fake businesses, some quote, one is picked and its pick marked paid;
//! the loss notices are the `messaging.business_notice` tasks queued for the others.

use askhava_businesses::api::{add_owner, create};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_messaging::api::{Language, render};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    BUSINESS_NOTICE_KIND, Comparison, INACTIVE_TEMPLATE, LIVE_PICK_TEMPLATE, LossSettings, compare,
    live_pick_reply, loss_template, lost_notices, no_longer_active, refused_after_confirm,
};
use askhava_quotes::api::{Period, Price};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// A fake quote's price columns: format, amount, low and high, tier prices.
type Columns = (
    &'static str,
    Option<i64>,
    (Option<i64>, Option<i64>),
    Option<Vec<i64>>,
);

const fn firm(cents: i64) -> Columns {
    ("firm", Some(cents), (None, None), None)
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

/// Fake business `n` with an owner whose opaque mailbox ID is `0xa00 + n`.
async fn business(db: &mut PgConnection, n: u128) -> Uuid {
    let business = create(db, false, at(0)).await.expect("business");
    add_owner(db, business, Uuid::from_u128(0xa00 + n), at(0))
        .await
        .expect("owner");
    business
}

/// A job moved through `steps`, its card sent to `sent` fake businesses (returned in order).
async fn job(
    db: &mut PgConnection,
    sent: u128,
    steps: &[(JobStatus, JobStatus)],
) -> (Uuid, Vec<Uuid>) {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    for step in steps {
        let moved = move_to(db, job, *step, at(1), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    let mut holders = Vec::new();
    for n in 1..=sent {
        let b = business(db, n).await;
        sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
            .bind(job)
            .bind(b)
            .bind(at(2).to_sqlx())
            .execute(&mut *db)
            .await
            .expect("card");
        holders.push(b);
    }
    (job, holders)
}

const TO_AWAITING: [(JobStatus, JobStatus); 3] = [
    (JobStatus::Intake, JobStatus::Open),
    (JobStatus::Open, JobStatus::QuotesReady),
    (JobStatus::QuotesReady, JobStatus::AwaitingConfirm),
];

async fn quote(db: &mut PgConnection, (job, business): (Uuid, Uuid), c: Columns, minute: i64) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, low_cents, \
         high_cents, tier_names, tier_cents, confirm_on_site, notes, original, ai_read, confirmed, \
         received_at) VALUES ($1, $2, $3, $4, $5, $6, \
         CASE WHEN $7::bigint[] IS NULL THEN NULL ELSE array_fill('Tier'::text, \
         ARRAY[cardinality($7)]) END, $7, false, '', 'Fake quote', false, true, $8)",
    )
    .bind(job)
    .bind(business)
    .bind(c.0)
    .bind(c.1)
    .bind(c.2.0)
    .bind(c.2.1)
    .bind(c.3)
    .bind(at(minute).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

async fn pick(db: &mut PgConnection, (job, business): (Uuid, Uuid), status: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, $4) RETURNING id",
    )
    .bind(job)
    .bind(business)
    .bind(status)
    .bind(at(10).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("pick")
}

/// The queued business notices: (business, template, payload).
async fn notices(pool: &PgPool) -> Vec<(Uuid, String, Value)> {
    let rows: Vec<(Value,)> =
        sqlx::query_as("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(BUSINESS_NOTICE_KIND)
            .fetch_all(pool)
            .await
            .expect("tasks");
    rows.into_iter()
        .map(|(p,)| {
            let b = p["business"].as_str().and_then(|b| Uuid::parse_str(b).ok());
            (
                b.expect("business"),
                p["template"].as_str().expect("t").to_owned(),
                p,
            )
        })
        .collect()
}

/// TD-PICK-11, TD-PICK-17 (PICK-026, D-150, D-193): with the pick paid, each other quoter gets the
/// loss notice once, the picked business none; a card sent to 5 says "3 businesses quoted", lower
/// for the higher quote and higher for the lower one; the payload holds no amount or name.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_11_every_other_quoter_hears_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (job, b) = job(&mut db, 5, &TO_AWAITING).await;
    for (minute, business, cents) in [(3, b[0], 10_000), (4, b[1], 12_000), (5, b[2], 8_000)] {
        quote(&mut db, (job, business), firm(cents), minute).await;
    }
    let waiting = pick(&mut db, (job, b[0]), "awaiting_confirm").await;
    assert_eq!(
        lost_notices(&mut db, waiting, None, LossSettings::default()).await,
        Ok(0)
    );
    sqlx::query("UPDATE picks.pick SET status = 'paid' WHERE id = $1")
        .bind(waiting)
        .execute(&mut *db)
        .await
        .expect("paid");
    assert_eq!(
        lost_notices(&mut db, waiting, None, LossSettings::default()).await,
        Ok(2)
    );
    assert_eq!(
        lost_notices(&mut db, waiting, None, LossSettings::default()).await,
        Ok(0)
    );
    let sent = notices(&pool).await;
    let got: Vec<(Uuid, &str)> = sent.iter().map(|(b, t, _)| (*b, t.as_str())).collect();
    assert_eq!(got, [(b[1], "pick_lost_lower"), (b[2], "pick_lost_higher")]);
    for (_, _, payload) in &sent {
        assert_eq!(payload["values"]["count"], 3);
        let values = payload["values"].as_object().expect("values");
        assert_eq!(
            values.len(),
            2,
            "job number and count only, never an amount"
        );
        assert_eq!(payload["to"].as_array().map(Vec::len), Some(1));
    }
}

/// TD-PICK-20, TD-PICK-17 (PICK-026, D-263): a card sent to 4 businesses with 3 quotes says
/// "Several businesses quoted"; 2 quotes give the first sentence only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_20_several_businesses_on_a_card_sent_to_four(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (job, b) = job(&mut db, 4, &TO_AWAITING).await;
    for (business, cents) in [(b[0], 10_000), (b[1], 10_500), (b[2], 9_500)] {
        quote(&mut db, (job, business), firm(cents), 3).await;
    }
    let paid = pick(&mut db, (job, b[0]), "free").await;
    assert_eq!(
        lost_notices(&mut db, paid, None, LossSettings::default()).await,
        Ok(2)
    );
    let templates: Vec<String> = notices(&pool).await.into_iter().map(|n| n.1).collect();
    assert_eq!(
        templates,
        ["pick_lost_several_same", "pick_lost_several_same"]
    );
    let two = LossSettings::default();
    assert_eq!(loss_template((2, 9), Comparison::Lower, two), "pick_lost");
    assert_eq!(
        loss_template((3, 5), Comparison::Lower, two),
        "pick_lost_lower"
    );
    assert_eq!(
        loss_template((3, 4), Comparison::Higher, two),
        "pick_lost_several_higher"
    );
}

/// TD-PICK-17 (PICK-038, D-193): a range compares on its midpoint, a tiers quote on the tier
/// picked, a firm quote against a diagnostic visit is a different type, in the database.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_17_ranges_tiers_and_other_types(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (job, b) = job(&mut db, 6, &TO_AWAITING).await;
    let quotes: [(usize, Columns); 4] = [
        (
            0,
            (
                "tiers",
                None,
                (None, None),
                Some(vec![5_000, 9_000, 20_000]),
            ),
        ),
        (1, ("range", None, (Some(6_000), Some(14_000)), None)),
        (2, ("diagnostic_visit", Some(9_000), (None, None), None)),
        (3, firm(12_000)),
    ];
    for (i, c) in quotes {
        quote(&mut db, (job, b[i]), c, 3 + i64::try_from(i).expect("i")).await;
    }
    let paid = pick(&mut db, (job, b[0]), "paid").await;
    assert_eq!(
        lost_notices(&mut db, paid, Some(1), LossSettings::default()).await,
        Ok(3)
    );
    let got: Vec<(Uuid, String)> = notices(&pool)
        .await
        .into_iter()
        .map(|n| (n.0, n.1))
        .collect();
    let want = [
        (b[1], "pick_lost_same"),
        (b[2], "pick_lost_other_type"),
        (b[3], "pick_lost_lower"),
    ];
    assert_eq!(got.len(), 3);
    for ((gb, gt), (wb, wt)) in got.iter().zip(want) {
        assert_eq!((gb, gt.as_str()), (&wb, wt));
    }
}

/// TD-PICK-17 (PICK-038, D-193, D-1572): exactly 90% and 110% are about the same, 89.99% lower and
/// 110.01% higher; a range on its midpoint; tiers on the tier picked; recurring with recurring of
/// one period; a firm quote against a diagnostic visit is a different type.
#[test]
fn td_pick_17_the_comparison_edges() {
    use Comparison::{AboutSame, Higher, Lower, OtherType};
    let range = Price::Range {
        low: 1_000,
        high: 3_001,
        on_site: false,
    };
    let tiers = Price::Tiers(vec![("A".into(), 1_000), ("B".into(), 5_000)]);
    let monthly = |cents, period| Price::Monthly {
        cents,
        period,
        setup: None,
    };
    let hourly = |hours| Price::Hourly { rate: 5_000, hours };
    let (firm, visit) = (Price::Firm, Price::VisitFee);
    let cases = [
        (firm(900_000), None, firm(1_000_000), 10, AboutSame),
        (firm(1_100_000), None, firm(1_000_000), 10, AboutSame),
        (firm(899_900), None, firm(1_000_000), 10, Lower),
        (firm(1_100_100), None, firm(1_000_000), 10, Higher),
        (range, None, firm(2_000), 0, AboutSame),
        (tiers.clone(), Some(1), firm(9_000), 10, Lower),
        (tiers.clone(), None, firm(9_000), 10, OtherType),
        (tiers.clone(), Some(0), tiers, 10, AboutSame),
        (
            monthly(4_000, Period::Month),
            None,
            monthly(3_000, Period::Month),
            10,
            Higher,
        ),
        (
            monthly(4_000, Period::Month),
            None,
            monthly(4_000, Period::Week),
            10,
            OtherType,
        ),
        (firm(9_000), None, visit(9_000), 10, OtherType),
        (hourly(Some((2, 4))), None, firm(15_000), 0, AboutSame),
        (hourly(None), None, firm(15_000), 10, OtherType),
        (visit(9_000), None, visit(5_000), 10, Higher),
    ];
    for (picked, tier, yours, pct, want) in cases {
        assert_eq!(
            compare(&picked, tier, &yours, pct),
            want,
            "{picked:?} vs {yours:?}"
        );
    }
}

/// TD-PICK-11 (PICK-026, D-044, D-150): at a final status every quoter but the connected business
/// gets "This job is no longer active." once, one that quoted after the pick included; a quote
/// refused after Confirm got it at once and not again; a job not yet final sends none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_11_no_longer_active_once_except_the_connected_business(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (job, b) = job(&mut db, 4, &TO_AWAITING).await;
    for business in &b[..3] {
        quote(&mut db, (job, *business), firm(10_000), 3).await;
    }
    pick(&mut db, (job, b[0]), "paid").await;
    quote(&mut db, (job, b[3]), firm(10_000), 20).await;
    assert_eq!(no_longer_active(&mut db, job).await, Ok(0), "not final yet");
    assert_eq!(refused_after_confirm(&mut db, job, b[1]).await, Ok(true));
    let cancel = (JobStatus::AwaitingConfirm, JobStatus::Cancelled);
    let moved = move_to(&mut db, job, cancel, at(30), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "cancel");
    assert_eq!(no_longer_active(&mut db, job).await, Ok(2));
    assert_eq!(no_longer_active(&mut db, job).await, Ok(0));
    let got: Vec<(Uuid, String)> = notices(&pool)
        .await
        .into_iter()
        .map(|n| (n.0, n.1))
        .collect();
    let inactive = INACTIVE_TEMPLATE.to_owned();
    assert_eq!(
        got,
        [
            (b[1], inactive.clone()),
            (b[2], inactive.clone()),
            (b[3], inactive)
        ]
    );
}

/// TD-PICK-11 (PICK-027): the business whose pick waits gets the Confirm link reply; another
/// business, or a job with no waiting pick, gets none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_11_a_business_asking_about_a_live_pick_gets_the_confirm_link(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (job, b) = job(&mut db, 2, &TO_AWAITING).await;
    assert_eq!(live_pick_reply(&mut db, job, b[0]).await, Ok(None));
    let waiting = pick(&mut db, (job, b[0]), "awaiting_confirm").await;
    let reply = live_pick_reply(&mut db, job, b[0])
        .await
        .expect("reply")
        .expect("some");
    assert_eq!((reply.template, reply.pick), (LIVE_PICK_TEMPLATE, waiting));
    assert!(reply.job >= 1001);
    assert_eq!(live_pick_reply(&mut db, job, b[1]).await, Ok(None));
}

/// TD-PICK-33, TD-PICK-17 (PICK-026, PICK-027, PICK-038, D-406): each template equals its spec
/// text in English, has a Spanish template, and holds no amount or business name.
#[test]
fn td_pick_33_the_loss_texts_equal_the_spec() {
    let first = "Ask Hava: Job 1042 went to another business this time.";
    let tail = " No action needed. Thanks for quoting.";
    let line =
        |k: &str, how: &str| format!("{first} {k} quoted; the picked quote was {how} yours.{tail}");
    let cases = [
        ("pick_lost", first.to_owned()),
        ("pick_lost_lower", line("3 businesses", "lower than")),
        ("pick_lost_same", line("3 businesses", "about the same as")),
        ("pick_lost_higher", line("3 businesses", "higher than")),
        (
            "pick_lost_several_lower",
            line("Several businesses", "lower than"),
        ),
        (
            "pick_lost_several_same",
            line("Several businesses", "about the same as"),
        ),
        (
            "pick_lost_several_higher",
            line("Several businesses", "higher than"),
        ),
        (
            "pick_lost_other_type",
            format!("{first} The picked quote was a different type of quote from yours.{tail}"),
        ),
        (
            "job_no_longer_active",
            "Ask Hava: Job 1042. This job is no longer active.".to_owned(),
        ),
    ];
    let values = [
        ("job", "1042"),
        ("count", "3"),
        ("link", "https://askhava.com/b/x"),
    ];
    for (id, want) in cases {
        assert_eq!(
            render(id, Language::En, false, &values).expect(id).body,
            want
        );
        let es = render(id, Language::Es, true, &values).expect(id);
        assert!(es.draft && !es.body.contains('$'), "{id}");
    }
    let live = render(LIVE_PICK_TEMPLATE, Language::En, false, &values).expect("live");
    assert!(live.body.contains("https://askhava.com/b/x") && !live.body.contains('$'));
}
