//! The daily ROC check and the card pool's reading of it, in the database (TD-LIC-33: CARD-015,
//! LIC-008, LIC-007, PICK-020; T654, D-4310, D-5117). Fake IDs, numbers and words only; the towns
//! are public place names; the clock is set (each check row carries its own `checked_at`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use askhava_adapters::api::LicenseLists;
use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, active_license_numbers, add_owner, apply, create,
    request_welcome, set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Claimed, Guarded};
use askhava_integration::set_app_role;
use askhava_jobs::api::{JobStatus, move_to};
use askhava_licensing::api::{
    ActiveLicenses, Agency, CardFacts as LicenseCard, DAILY_LISTS_KIND, DailyListSettings,
    ImportError, ROC_HEADER, card_facts, import_roc_list, record_agency_license, run_daily_list,
};
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardFacts, CardParts, DEFAULT_MIN_WORDS, Place, build_card, card_digest, card_pool,
    card_pool_at, keep_card, late_cards, mark_card_sent,
};
use jiff::civil::{Date, date};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const TOWN: &str = "lake_havasu_city";
const PIPE: (&str, &str) = ("plumbing_repair", "faucet replacement");
const NUMBER: &str = "ROC900037";
/// The set clock's start: 10:00 Arizona time on November 5.
const T0: &str = "2026-11-05T17:00:00Z";

struct Active;

impl ActiveLicenses for Active {
    async fn active_after(
        &self,
        conn: &mut PgConnection,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<(Uuid, String)>, ImportError> {
        let found = active_license_numbers(conn, after, limit).await;
        found.map_err(|_| ImportError::Database)
    }
}

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn hours(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n)
}

fn roc_list() -> String {
    format!("{ROC_HEADER}\n{NUMBER},R-37,Active,2012-05-01\n")
}

/// An active, welcomed plumber in the town, ticking faucet replacement, with no check yet.
async fn plumber(db: &mut PgConnection) -> Uuid {
    let at = ts("2026-11-01T17:00:00Z");
    let b = create(db, false, at).await.expect("business");
    add_owner(db, b, Uuid::new_v4(), at).await.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let active = apply(db, b, Event::Checked(ok), Actor::System, at).await;
    active.expect("active");
    request_welcome(db, b, at).await.expect("welcome");
    welcome_sent(db, b, at).await.expect("sent");
    let actor = Actor::Business(b);
    set_towns(db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new(PIPE.0, PIPE.1);
    let allowed: BTreeSet<_> = [choice.clone()].into();
    let ticked = set_job_types(db, b, &[choice], &allowed, actor).await;
    ticked.expect("job types");
    b
}

/// Stores a check of `b` that read `status` against the list dated `day`, made at `at`.
async fn check(db: &mut PgConnection, b: Uuid, status: &str, day: &str, at: Timestamp) {
    sqlx::query(
        "INSERT INTO licensing.license_check (business_id, source, license_number, list_date, \
         status, checked_at) VALUES ($1, 'roc', $2, $3::date, $4, $5::timestamptz)",
    )
    .bind(b)
    .bind(NUMBER)
    .bind(day)
    .bind(status)
    .bind(at.to_string())
    .execute(db)
    .await
    .expect("check");
}

fn facts() -> CardFacts {
    CardFacts {
        category: PIPE.0.to_owned(),
        candidates: vec![PIPE.1.to_owned()],
        emergency: false,
    }
}

/// Whether `b` holds R-37 in the plumbing job's pool: at `now` through `card_pool_at`, or through
/// plain `card_pool` for `None`. `b` is pooled either way, with its agency license.
async fn holds(db: &mut PgConnection, b: Uuid, now: Option<Timestamp>) -> bool {
    let job = ((TOWN, None), &facts());
    let pool = match now {
        Some(now) => card_pool_at(db, job, String::new(), None, now).await,
        None => card_pool(db, job, String::new(), None).await,
    };
    let pool = pool.expect("pool");
    let found = pool.businesses.iter().find(|p| p.card.id == b);
    let card = &found.expect("pooled whatever its ROC check").card;
    assert!(card.licenses.agencies.contains(&Agency::HomeInspector));
    assert!(card.classes.is_empty() || card.classes == ["R-37"]);
    !card.classes.is_empty()
}

async fn count(db: &mut PgConnection, sql: &'static str, b: Uuid) -> i64 {
    let n = sqlx::query_scalar(sql).bind(b).fetch_one(db).await;
    n.expect("count")
}

/// One ROC list run at `now` with `payload`, one business a page and no budget for a second.
async fn run(pool: &PgPool, lists: &LicenseLists, payload: Value, now: &str) -> Timestamp {
    let settings = DailyListSettings {
        batch: 1,
        sweep_budget: SignedDuration::ZERO,
        ..DailyListSettings::default()
    };
    let task = Claimed {
        id: 0,
        kind: DAILY_LISTS_KIND.to_owned(),
        payload,
        attempts: 1,
    };
    let done = run_daily_list(pool, (lists, &Active), &task, (ts(now), settings)).await;
    done.expect("run").next_run
}

/// TD-LIC-33 (CARD-015, LIC-007): a recorded ROC list writes one check per active ROC business,
/// a page a run and the rest in the next queued run; a re-run of the same list day writes none;
/// the quote card's ROC line then dates from that day's check, not from sign-up's.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_33_a_recorded_list_checks_every_active_business_once(pool: PgPool) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("t654-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(dir.join("roc"), roc_list()).expect("roc fixture");
    let lists = LicenseLists::new(&format!("file://{}", dir.display())).expect("file source");
    let mut db = pool.acquire().await.expect("conn");
    load_embedded(&mut db).await.expect("catalog");
    let (one, two) = (plumber(&mut db).await, plumber(&mut db).await);
    for b in [one, two] {
        check(
            &mut db,
            b,
            "active",
            "2026-11-01",
            ts("2026-11-01T17:00:00Z"),
        )
        .await;
    }
    let today = "SELECT count(*) FROM licensing.license_check WHERE list_date = '2026-11-02' \
                 AND business_id = $1";
    let next = run(
        &pool,
        &lists,
        json!({"list": "roc"}),
        "2026-11-02T13:00:00Z",
    )
    .await;
    assert_eq!(next, ts("2026-11-02T14:00:00Z"), "checks are left");
    let first = count(&mut db, today, one).await + count(&mut db, today, two).await;
    assert_eq!(first, 1, "one page a run");
    let queued = "SELECT payload FROM core.task WHERE kind = 'licensing.daily_lists' \
                  AND done_at IS NULL ORDER BY run_at DESC LIMIT 1";
    for (now, next_run) in [
        ("2026-11-02T14:00:00Z", "2026-11-02T15:00:00Z"),
        ("2026-11-02T15:00:00Z", "2026-11-03T13:00:00Z"),
    ] {
        let payload: Value = sqlx::query_scalar(queued)
            .fetch_one(&mut *db)
            .await
            .expect("next");
        assert_eq!(run(&pool, &lists, payload, now).await, ts(next_run));
    }
    for _ in 0..2 {
        let fresh = json!({"list": "roc"});
        run(&pool, &lists, fresh, "2026-11-02T16:00:00Z").await;
        for b in [one, two] {
            assert_eq!(count(&mut db, today, b).await, 1, "one check a list day");
        }
    }
    let day: String = sqlx::query_scalar(
        "SELECT (checked_at AT TIME ZONE 'America/Phoenix')::date::text \
         FROM licensing.license_check WHERE business_id = $1 AND list_date = '2026-11-02'",
    )
    .bind(one)
    .fetch_one(&mut *db)
    .await
    .expect("the daily check");
    let day: Date = day.parse().expect("day");
    assert_ne!(day, date(2026, 11, 1));
    let card = card_facts(&mut db, one, PIPE.0, None).await.expect("facts");
    let Some(LicenseCard::Licensed(lines)) = card else {
        panic!("a licensed card");
    };
    assert_eq!(
        lines[0].checked_on, day,
        "the ROC line dates from the check"
    );
}

/// The ROC list of November 1, one plumber with a home-inspector license too, then the app role.
async fn licensed(db: &mut PgConnection) -> Uuid {
    load_embedded(db).await.expect("catalog");
    let listed = import_roc_list(db, date(2026, 11, 1), &roc_list()).await;
    listed.expect("roc list");
    let b = plumber(db).await;
    let agency = (Agency::HomeInspector, "HI-9001");
    let recorded = record_agency_license(db, b, agency, Actor::System).await;
    recorded.expect("agency license");
    set_app_role(db).await.expect("SET ROLE askhava_app");
    b
}

/// TD-LIC-33 (CARD-015, LIC-008, LIC-007, PICK-020): an R-37 business gets the plumbing card only
/// while its latest check is Active, under 72 hours old and against a list inside the window; a
/// later Inactive lookup stops it and the next Active check gives it again; a fresh Active lookup
/// while the list fails keeps it; plain `card_pool` tests the status only; its agency license
/// stands throughout.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_33_roc_classes_need_a_fresh_active_latest_check(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn").detach();
    let b = licensed(&mut db).await;
    let t0 = ts(T0);
    assert!(!holds(&mut db, b, Some(t0)).await, "no check, no ROC card");
    check(&mut db, b, "active", "2026-11-05", t0).await;
    let second = SignedDuration::from_secs(1);
    assert!(holds(&mut db, b, Some(t0 + hours(72) - second)).await);
    assert!(!holds(&mut db, b, Some(t0 + hours(72))).await);
    assert!(holds(&mut db, b, None).await, "no clock, no age test");
    check(&mut db, b, "active", "2026-11-01", t0 + hours(10)).await;
    assert!(
        !holds(&mut db, b, Some(t0 + hours(11))).await,
        "an old list"
    );
    check(&mut db, b, "inactive", "2026-11-06", t0 + hours(20)).await;
    assert!(!holds(&mut db, b, Some(t0 + hours(21))).await);
    assert!(
        !holds(&mut db, b, None).await,
        "Inactive stops it clockless"
    );
    check(&mut db, b, "active", "2026-11-07", t0 + hours(30)).await;
    assert!(
        holds(&mut db, b, Some(t0 + hours(31))).await,
        "the next list"
    );
    assert!(
        !holds(&mut db, b, Some(t0 + hours(200))).await,
        "list failing"
    );
    check(&mut db, b, "active", "2026-11-13", t0 + hours(199)).await;
    assert!(holds(&mut db, b, Some(t0 + hours(200))).await, "a lookup");
}

/// A plumbing job, Open, with its card stored and sent at [`T0`].
async fn open_job(db: &mut PgConnection) -> Uuid {
    let at = ts(T0);
    let job = askhava_jobs::api::create(db, at, false, Actor::System).await;
    let job = job.expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, at, Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))));
    let parts = CardParts {
        category: "Plumbing repair".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "Kitchen faucet drips and needs a new one fitted".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let digest = card_digest(card.text());
    let kept = keep_card(db, (Uuid::new_v4(), job), &digest, &card, &facts(), at).await;
    kept.expect("kept");
    assert!(mark_card_sent(db, job, &digest, at).await.expect("sent"));
    job
}

/// TD-LIC-33 (CARD-015, LIC-008, D-5117): at the sender, the late-cards run gives a business
/// whose latest Active check is over 72 hours old no card, and one when the check is fresh.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_33_late_cards_need_the_fresh_check(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn").detach();
    let b = licensed(&mut db).await;
    let job = open_job(&mut db).await;
    check(&mut db, b, "active", "2026-11-05", ts(T0)).await;
    let cards = "SELECT count(*) FROM quotes.job_card WHERE business_id = $1";
    let stale = late_cards(&mut db, b, ts(T0) + hours(73)).await;
    assert_eq!(stale.expect("run"), Vec::<Uuid>::new());
    assert_eq!(count(&mut db, cards, b).await, 0);
    let fresh = late_cards(&mut db, b, ts(T0) + hours(1)).await;
    assert_eq!(fresh.expect("run"), vec![job]);
    assert_eq!(count(&mut db, cards, b).await, 1);
}
