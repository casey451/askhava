//! TD-PICK-53 (T318, D-2206, fourth audit F23): `service::find` gives the view of the job asked
//! for or none, never another job's view (another job's request number), and an empty list gives
//! none without a panic. Also TD-TXT-29's release wiring (TXT-027, D-3508): `release`'s
//! `follow_up` queues one `calls.whos_calling` task for a claimed pick, shown or failed after the
//! claim, none for a repeat, an unclaimed pick or a quiet outcome. The pick and release rows are
//! seeded in this crate's own schema (no test may mint a proof, D-1563). Fake IDs and numbers.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_calls::api::{WHOS_CALLING_KIND, job_had_line};
use askhava_core::api::{Settings, WorkerId, claim};
use askhava_ops::api::Actor;
use askhava_privacy::api::ReleaseBasis;
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::release::{Shown, follow_up};
use crate::rules::{JobView, Stage};
use crate::service::find;
use crate::status::PickStatus;

/// A ready job's view with request number `number`.
fn view(job: Uuid, number: i64) -> JobView {
    JobView {
        job,
        number,
        stage: Stage::Ready,
        paused: false,
        terms_sent: true,
        quotes: vec![true],
        waiting: None,
    }
}

/// TD-PICK-53: the job asked for is found among several.
#[test]
fn td_pick_53_the_job_asked_for_is_found() {
    let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let views = [view(a, 1042), view(b, 1043)];
    assert_eq!(find(&views, b).map(|v| (v.job, v.number)), Some((b, 1043)));
    assert_eq!(find(&views, a).map(|v| v.number), Some(1042));
}

/// TD-PICK-53: a job not among the views gives none, never the first view's number.
#[test]
fn td_pick_53_a_missing_job_gives_no_view_never_another_jobs() {
    let views = [view(Uuid::from_u128(1), 1042)];
    assert!(find(&views, Uuid::from_u128(9)).is_none());
}

/// TD-PICK-53: an empty list gives none and does not panic.
#[test]
fn td_pick_53_an_empty_list_gives_no_view_without_a_panic() {
    assert!(find(&[], Uuid::from_u128(1)).is_none());
}

/// A masked pool of one fake number.
fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

/// A paid pick, with its `picks.release` row (the claim) when `claimed`: `(pick, job)`.
async fn paid_pick(db: &mut PgConnection, claimed: bool) -> (Uuid, Uuid) {
    let business = askhava_businesses::api::create(db, false, Timestamp::now())
        .await
        .expect("business");
    let job = askhava_jobs::api::create(db, Timestamp::now(), false, Actor::System)
        .await
        .expect("job");
    let pick = sqlx::query_scalar!(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, now()) RETURNING id",
        job,
        business,
        PickStatus::Paid.code(),
    )
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    if claimed {
        sqlx::query!(
            "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
             contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', now(), $4, false)",
            pick,
            job,
            business,
            Uuid::from_u128(0x0254_0b01),
        )
        .execute(&mut *db)
        .await
        .expect("release");
    }
    (pick, job)
}

/// The `calls.whos_calling` tasks for `pick` newly due within two days, claimed through `core`'s
/// API, so each is counted once and a later call counts only tasks queued since.
async fn new_whos_calling_tasks(pool: &PgPool, pick: Uuid) -> usize {
    let owner = WorkerId::new(pool).await.expect("worker");
    let by = Timestamp::now() + SignedDuration::from_hours(48);
    let claimed = claim(pool, owner, by, SignedDuration::from_hours(1), 100)
        .await
        .expect("claim");
    let pick = pick.to_string();
    claimed
        .iter()
        .filter(|t| t.kind == WHOS_CALLING_KIND && t.payload["pick"].as_str() == Some(&pick))
        .count()
}

/// Runs `release`'s follow-up for `pick` as `release` does after `show`.
async fn follow(pool: &PgPool, pick: Uuid, shown: Shown) {
    let ids = (pick, Uuid::from_u128(0x0254_0b01), ReleaseBasis::PaidCharge);
    follow_up(pool, &masked(), (ids, false), shown, Timestamp::now()).await;
}

/// TD-TXT-29 (TXT-027, D-3508): a shown release opens the line and queues exactly one
/// who's-calling task; a repeated follow-up queues no second.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_a_shown_release_queues_one_whos_calling_task(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (pick, job) = paid_pick(&mut db, true).await;
    follow(&pool, pick, Shown::Yes).await;
    assert!(job_had_line(&mut db, job).await.expect("line"));
    assert_eq!(new_whos_calling_tasks(&pool, pick).await, 1);
    follow(&pool, pick, Shown::Yes).await;
    assert_eq!(
        new_whos_calling_tasks(&pool, pick).await,
        0,
        "no second task"
    );
}

/// TD-TXT-29 (TXT-027, D-3508): a release that failed after its claim committed (the vault read)
/// still queues the one task, since a later proof gets `Already`; before the claim it queues none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_a_failed_release_queues_the_task_only_once_claimed(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (claimed, _) = paid_pick(&mut db, true).await;
    follow(&pool, claimed, Shown::Failed).await;
    assert_eq!(new_whos_calling_tasks(&pool, claimed).await, 1);
    let (unclaimed, _) = paid_pick(&mut db, false).await;
    follow(&pool, unclaimed, Shown::Failed).await;
    assert_eq!(new_whos_calling_tasks(&pool, unclaimed).await, 0);
}

/// TD-TXT-29 (TXT-027): `Already`, `WaitingForAddress` and `MovedOn` queue nothing, so a repeated
/// release never queues a second text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_29_a_quiet_release_queues_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (pick, _) = paid_pick(&mut db, true).await;
    follow(&pool, pick, Shown::No).await;
    assert_eq!(new_whos_calling_tasks(&pool, pick).await, 0);
}
