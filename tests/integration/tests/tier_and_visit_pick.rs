//! TD-FEE-63 (PICK-053, PICK-038, REC-010, REC-011; D-2332, D-2333, D-3580): on a fresh database,
//! through `picks::api`, a page pick of a tiers quote naming its package fixes the introduction fee
//! by job size from that package's price and stores the package on the pick (the tier the loss
//! comparison of PICK-038 takes); no package or a stale one stores nothing and names the choices; a
//! text pick of a tiers quote stores nothing and replies with her quote-page link, once, even when
//! the inbound is handled twice; a per-visit price is priced on its stored visit schedule ($160
//! weekly gives $75, REC-011) and one with no schedule is refused. Fake IDs and wording only.

use askhava_core::api::{Guarded, LinkToken, brand};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_messaging::api::Language;
use askhava_ops::api::Actor;
use askhava_payments::api::{
    FeeJob, FeeMode, FeeQuote, Period, PeriodJob, RecurringCap, Service, VisitTier, fee_at_pick,
};
use askhava_picks::api::rules::Plan;
use askhava_picks::api::{
    ChosenTier, PACKAGE_TEMPLATE, PagePick, PicksError, TextPickIn, TierPick, page_pick,
    page_pick_tier, text_pick,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: Uuid = Uuid::from_u128(0xb341_0001);

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-341".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_200_000 + minute * 60).expect("time")
}

/// A job by `her` in `category` (job type `kind`) moved to Quotes ready, with no quotes yet.
async fn job_ready(db: &mut PgConnection, her: Uuid, (category, kind): (&str, &str)) -> Uuid {
    let words = "sample request words for a test job";
    let first = TextIn {
        inbox_id: Uuid::from_u128(her.as_u128() << 32),
        contact: her,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(0),
    };
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    record_job_facts(db, job, category, &[kind.to_owned()])
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(2), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    job
}

/// A confirmed tiers quote by [`BUSINESS`]: Basic $100, Plus $200, Premium $300.
async fn tiers_quote(db: &mut PgConnection, job: Uuid) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, tier_names, tier_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'tiers', ARRAY['Basic', 'Plus', 'Premium'], \
         ARRAY[10000, 20000, 30000]::bigint[], false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(BUSINESS)
    .bind(at(3).to_sqlx())
    .execute(db)
    .await
    .expect("tiers quote");
}

/// A confirmed $160 per-visit quote by [`BUSINESS`] with visit schedule `schedule`.
async fn visit_quote(db: &mut PgConnection, job: Uuid, schedule: Option<&str>) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, period, \
         visit_schedule, confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'monthly', 16000, 'visit', $3, false, '', 'Sample wording', false, true, \
         $4)",
    )
    .bind(job)
    .bind(BUSINESS)
    .bind(schedule)
    .bind(at(3).to_sqlx())
    .execute(db)
    .await
    .expect("per-visit quote");
}

/// The stored picks (tier index, tier name) and fee amounts.
async fn stored(pool: &PgPool) -> (Vec<(Option<i16>, Option<String>)>, Vec<i64>) {
    let picks = sqlx::query_as("SELECT tier_index, tier_name FROM picks.pick")
        .fetch_all(pool)
        .await
        .expect("picks");
    let fees = sqlx::query_scalar("SELECT amount_cents FROM payments.pick_fee")
        .fetch_all(pool)
        .await
        .expect("fees");
    (picks, fees)
}

/// Package `index` named `name` on quote 1, in its own transaction, committed only when it picked.
async fn tier_picked(pool: &PgPool, job: Uuid, (index, name): (usize, &str)) -> TierPick {
    let mut tx = pool.begin().await.expect("tx");
    let tier = ChosenTier {
        index,
        name: name.to_owned(),
    };
    let picked = page_pick_tier(&mut tx, (job, false), (1, &tier), Actor::System, at(20)).await;
    let picked = picked.expect("tier pick");
    if matches!(picked, TierPick::Page(PagePick::Picked(_))) {
        tx.commit().await.expect("commit");
    }
    picked
}

/// The fee `fee_at_pick` gives `cents` with `period` on a priced job of `service`, in tiers mode.
fn expected(cents: u64, period: Option<Period>, service: Service) -> i64 {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(cents), period, job, FeeMode::Tiers, None);
    i64::try_from(fee.expect("fee").cents()).expect("cents")
}

/// TD-FEE-63 (PICK-053, PICK-038, D-2332): naming package 2 fixes the fee from its $200 and stores
/// the package; the same package again is the same pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_63_a_named_package_fixes_its_fee_and_is_stored(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let job = job_ready(
        &mut db,
        Uuid::from_u128(0x0341_0001),
        ("plumbing_repair", "leak repair"),
    )
    .await;
    tiers_quote(&mut db, job).await;
    let picked = tier_picked(&pool, job, (1, "Plus")).await;
    assert!(matches!(picked, TierPick::Page(PagePick::Picked(_))));
    let fee = expected(20_000, None, Service::NotRecurring);
    assert_eq!(
        stored(&pool).await,
        (vec![(Some(1), Some("Plus".to_owned()))], vec![fee])
    );
    let again = tier_picked(&pool, job, (1, "Plus")).await;
    assert!(matches!(again, TierPick::Page(PagePick::Same(_))));
}

/// TD-FEE-63 (PICK-053, D-2332): no package, a stale one and one past the end store nothing; the
/// last two name the quote's packages to choose again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_63_no_or_a_stale_package_stores_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let job = job_ready(
        &mut db,
        Uuid::from_u128(0x0341_0002),
        ("plumbing_repair", "leak repair"),
    )
    .await;
    tiers_quote(&mut db, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let bare = page_pick(&mut tx, (job, false), 1, Actor::System, at(20)).await;
    assert_eq!(bare, Err(PicksError::Quotes));
    drop(tx);
    let names = vec!["Basic".to_owned(), "Plus".to_owned(), "Premium".to_owned()];
    for stale in [(1, "Gold"), (3, "Plus")] {
        assert_eq!(
            tier_picked(&pool, job, stale).await,
            TierPick::Choose(names.clone())
        );
    }
    assert_eq!(stored(&pool).await, (vec![], vec![]));
}

/// The text pick of `words` from `her` on `job` as inbound `inbox`, committed.
async fn texted(pool: &PgPool, (her, job): (Uuid, Uuid), inbox: u128) -> Plan {
    let jobs = [(job, false)];
    let input = TextPickIn {
        contact: her,
        inbox: Uuid::from_u128(inbox),
        words: "1 YES",
        jobs: &jobs,
        language: (Language::En, false),
        actor: Actor::System,
        at: at(30),
    };
    let mut tx = pool.begin().await.expect("tx");
    let done = text_pick(&mut tx, &secret(), &input).await;
    let plan = done.expect("text pick").expect("a text pick").plan;
    tx.commit().await.expect("commit");
    plan
}

/// TD-FEE-63 (PICK-053, PICK-015, D-2332, D-3580): a text pick of a tiers quote stores nothing and
/// queues one package reply whose link opens her quote page; the same inbound handled again
/// queues no second reply and leaves that link live.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_63_a_text_pick_of_a_tiers_quote_replies_with_the_page_link(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let her = Uuid::from_u128(0x0341_0003);
    let job = job_ready(&mut db, her, ("plumbing_repair", "leak repair")).await;
    tiers_quote(&mut db, job).await;
    assert_eq!(
        texted(&pool, (her, job), 0x0341_0a01).await,
        Plan::Terms(job)
    );
    for _ in 0..2 {
        assert_eq!(
            texted(&pool, (her, job), 0x0341_0a02).await,
            Plan::Pick(job)
        );
    }
    assert_eq!(stored(&pool).await, (vec![], vec![]));
    let bodies: Vec<String> =
        sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = $1")
            .bind(PACKAGE_TEMPLATE)
            .fetch_all(&pool)
            .await
            .expect("replies");
    assert_eq!(bodies.len(), 1);
    let page = format!("https://{}/q/", brand().domain());
    let (_, raw) = bodies[0].split_once(&page).expect("her quote-page link");
    let token = LinkToken::parse(raw.trim()).expect("token");
    assert_eq!(resolve(&mut db, &token).await, Ok(Some(job)));
}

/// TD-FEE-63 (REC-010, REC-011, D-2333): $160 a visit, weekly, gives a $75 fee (W33); every two
/// weeks gives its own fee; a per-visit quote with no stored schedule stores nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_63_a_per_visit_price_is_priced_on_its_schedule(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let cases = [
        (Some("weekly"), Some(7_500)),
        (
            Some("every_two_weeks"),
            Some(expected(
                16_000,
                Some(Period::EveryTwoWeeks),
                Service::RecurringJobType,
            )),
        ),
        (None, None),
    ];
    for (n, (schedule, fee)) in (1_u128..).zip(cases) {
        let her = Uuid::from_u128(0x0341_0100 + n);
        let job = job_ready(&mut db, her, ("house_cleaning", "regular cleaning")).await;
        visit_quote(&mut db, job, schedule).await;
        let mut tx = pool.begin().await.expect("tx");
        let picked = page_pick(&mut tx, (job, false), 1, Actor::System, at(20)).await;
        let stored_fee: Option<i64> = match picked {
            Ok(PagePick::Picked(pick)) => {
                sqlx::query_scalar("SELECT amount_cents FROM payments.pick_fee WHERE pick_id = $1")
                    .bind(pick)
                    .fetch_optional(&mut *tx)
                    .await
                    .expect("fee")
            }
            other => {
                assert_eq!(other, Err(PicksError::Quotes));
                None
            }
        };
        assert_eq!(stored_fee, fee, "schedule {schedule:?}");
    }
    assert_eq!(
        expected(16_000, Some(Period::Weekly), Service::RecurringJobType),
        7_500
    );
}
