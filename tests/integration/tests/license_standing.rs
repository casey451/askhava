//! The unknown-license clock on PostgreSQL (TD-LIC-05: LIC-008, CARD-015, OPS-030, BIZ-018; T655,
//! D-4310, D-5184 to D-5186): the warning, the pause at the instant the card pool stops ROC cards,
//! one email per spell, and the resume with no review. Fake data only: invented license numbers,
//! no names, phones or addresses. The clock is set and lies before the day this runs, because the
//! check a run writes itself is stamped by the database and must be later than the pause.

use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::{
    AccountMailer, Capability, CapabilityAnswers, JobTypeChoice, Known, LICENSE_UNKNOWN_KIND,
    LargestJob, NewComplaint, NoticeParts, STANDING_KIND, STANDING_LAG, WARNED_ACTION, add_owner,
    apply_lists, apply_standing, business, checklist_offer, create, license_unknown_words,
    pause_self, pre_ticked, record_complaint, run_notice_email, run_standing, save_capability,
    save_job_types, schedule_standing,
};
use askhava_catalog::api::{Catalog, parse};
use askhava_core::api::{Claimed, HandlerError, SystemClock, brand};
use askhava_licensing::api::{
    ClassMap, DailyListSettings, Freshness, PestAttestation, arizona_day, check_license,
    import_roc_list, latest_check_allows_card, record_pest_attestation, report_pest_license_wrong,
};
use askhava_messaging::api::{DailyLimit, MailboxKind};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::types::Uuid;

const NUMBER: &str = "ROC900100";
/// The day of the latest list recorded before it fails.
const LIST_DAY: &str = "2026-11-02";
/// The check, made ten minutes after the 6 a.m. Arizona run on the list's day.
const CHECKED: &str = "2026-11-02T13:10:00Z";
/// The failure start: the run hour on the day after the latest recorded list.
const START: &str = "2026-11-03T13:00:00Z";
/// The pause moment: the check plus 72 hours.
const PAUSE: &str = "2026-11-05T13:10:00Z";
const SECOND: SignedDuration = SignedDuration::from_secs(1);

fn at(text: &str) -> Timestamp {
    text.parse().expect("fake time")
}

fn lists() -> DailyListSettings {
    DailyListSettings::default()
}

/// Records the list of `day` holding `rows` (`class`, `status`) of [`NUMBER`], plus ten filler
/// numbers so a shorter list is never refused as truncated (D-905).
async fn record(db: &mut PgConnection, day: &str, rows: &[(&str, &str)]) {
    let mut text = "license_number,class,status,issued_on\n".to_owned();
    for n in 0..10 {
        text.push_str(&format!("ROC9990{n:02},R-11,Active,2015-01-01\n"));
    }
    for (class, status) in rows {
        text.push_str(&format!("{NUMBER},{class},{status},2015-01-01\n"));
    }
    let day = day.parse().expect("fake date");
    import_roc_list(db, day, &text)
        .await
        .expect("list recorded");
}

/// Plants a check of [`NUMBER`] for business `id` made at `when` against the list of [`LIST_DAY`].
async fn plant(db: &mut PgConnection, id: Uuid, status: &str, when: Timestamp) {
    sqlx::query(
        "INSERT INTO licensing.license_check (business_id, source, license_number, list_date, \
         status, checked_at) VALUES ($1, 'roc', $2, $3::date, $4, $5::timestamptz)",
    )
    .bind(id)
    .bind(NUMBER)
    .bind(LIST_DAY)
    .bind(status)
    .bind(when.to_string())
    .execute(db)
    .await
    .expect("check planted");
}

/// A business in `status` (with `reason`), with an Active check at `checked` when one is given.
async fn biz(db: &mut PgConnection, state: (&str, Option<&str>), checked: Option<&str>) -> Uuid {
    let id = create(db, false, at(CHECKED)).await.expect("business");
    sqlx::query("UPDATE businesses.business SET status = $2, pause_reason = $3 WHERE id = $1")
        .bind(id)
        .bind(state.0)
        .bind(state.1)
        .execute(&mut *db)
        .await
        .expect("status set");
    if let Some(checked) = checked {
        plant(db, id, "active", at(checked)).await;
    }
    id
}

/// An active business checked Active at [`CHECKED`] against the list of that day; the list then
/// fails from [`START`].
async fn unknown(db: &mut PgConnection) -> Uuid {
    record(db, LIST_DAY, &[("R-11", "Active")]).await;
    biz(db, ("active", None), Some(CHECKED)).await
}

async fn run(db: &mut PgConnection, now: Timestamp) {
    apply_standing(db, now, lists()).await.expect("run");
}

async fn standing(db: &mut PgConnection, id: Uuid) -> (String, Option<String>) {
    let b = business(db, id).await.expect("read");
    let reason = b.pause_reason.map(|r| r.code().to_owned());
    (b.status.code().to_owned(), reason)
}

fn active() -> (String, Option<String>) {
    ("active".to_owned(), None)
}

fn paused(reason: &str) -> (String, Option<String>) {
    ("paused".to_owned(), Some(reason.to_owned()))
}

async fn notices(db: &mut PgConnection) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM core.task WHERE kind = $1 ORDER BY id")
        .bind(LICENSE_UNKNOWN_KIND)
        .fetch_all(db)
        .await
        .expect("tasks")
}

/// How many audit rows by the system about `id` enter status `key` or name the field or action
/// `key`.
async fn audits(db: &mut PgConnection, id: Uuid, key: &str) -> i64 {
    let count = sqlx::query_scalar(
        "SELECT count(*) FROM ops.audit_log WHERE actor_kind = 'system' AND subject_id = $1 \
         AND (to_status = $2 OR field = $2 OR action = $2)",
    );
    count.bind(id).bind(key).fetch_one(db).await.expect("audit")
}

/// Whether the card pool would still give `id` its ROC classes at `now` (T654's reading).
async fn card_allowed(db: &mut PgConnection, id: Uuid, now: Timestamp) -> bool {
    let since = now - lists().limits.pause_after;
    let fresh = Freshness {
        checked_since: since + SignedDuration::from_micros(1),
        listed_since: arizona_day(since),
    };
    let allowed = latest_check_allows_card(db, id, fresh).await;
    allowed.expect("read")
}

/// TD-LIC-05 (LIC-008, CARD-015, D-4310): the warning at the failure start plus 24:00:00 and not
/// at 23:59:59, once per spell; kept one second before the pause moment; paused at the pause
/// moment, the instant the card pool's reading turns false; a system status audit row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_warned_at_24_hours_and_paused_when_the_cards_stop(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = unknown(&mut db).await;
    let unchecked = biz(&mut db, ("active", None), None).await;
    let warn = at(START) + SignedDuration::from_hours(24);
    run(&mut db, warn - SECOND).await;
    assert!(notices(&mut db).await.is_empty());
    run(&mut db, warn).await;
    assert_eq!(notices(&mut db).await.len(), 1, "warned at 24:00:00");
    run(&mut db, warn + SECOND).await;
    let sent = notices(&mut db).await;
    assert_eq!(sent.len(), 1, "one email per spell");
    assert_eq!(sent[0]["business"], id.to_string());
    assert_eq!(sent[0]["since"], at(START).as_second());
    assert_eq!(sent[0]["pause_at"], at(PAUSE).as_second());
    assert_eq!(sent[0]["paused"], false);
    assert_eq!(sent[0].as_object().expect("payload").len(), 4);
    assert_eq!(audits(&mut db, id, WARNED_ACTION).await, 1);
    run(&mut db, at(PAUSE) - SECOND).await;
    assert_eq!(standing(&mut db, id).await, active());
    assert!(card_allowed(&mut db, id, at(PAUSE) - SECOND).await);
    run(&mut db, at(PAUSE)).await;
    assert_eq!(standing(&mut db, id).await, paused("license"));
    assert!(!card_allowed(&mut db, id, at(PAUSE)).await);
    assert_eq!(notices(&mut db).await.len(), 1);
    assert_eq!(audits(&mut db, id, "paused").await, 1);
    assert_eq!(standing(&mut db, unchecked).await, active());
}

/// TD-LIC-05 (LIC-008, D-4310): a first run only at the pause moment pauses and queues the one
/// email with it; a check so old that its pause moment precedes the failure start is warned and
/// paused the moment the list fails, not before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_never_a_pause_without_a_warning(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = unknown(&mut db).await;
    let old = biz(&mut db, ("active", None), Some("2026-10-30T13:00:00Z")).await;
    run(&mut db, at(START) - SECOND).await;
    assert!(notices(&mut db).await.is_empty());
    assert_eq!(standing(&mut db, old).await, active());
    run(&mut db, at(START)).await;
    assert_eq!(standing(&mut db, old).await, paused("license"));
    assert_eq!(standing(&mut db, id).await, active());
    let sent = notices(&mut db).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["business"], old.to_string());
    assert_eq!(sent[0]["paused"], true);
    run(&mut db, at(PAUSE)).await;
    assert_eq!(standing(&mut db, id).await, paused("license"));
    let sent = notices(&mut db).await;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1]["business"], id.to_string());
    assert_eq!(sent[1]["paused"], true);
    assert_eq!(audits(&mut db, id, WARNED_ACTION).await, 1);
}

/// TD-LIC-05 (LIC-008, D-4310 B3): a check 60 hours before the failure start has its pause moment
/// 12 hours into the failure, before the failure start plus 24 hours. The warning moment is then
/// the pause moment: nothing at the failure start or one second before the pause moment, and at
/// the pause moment the one email and the pause together, with no second email at 24 hours.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_a_pause_inside_the_first_24_hours_carries_the_warning(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    record(&mut db, LIST_DAY, &[("R-11", "Active")]).await;
    let id = biz(&mut db, ("active", None), Some("2026-11-01T01:00:00Z")).await;
    let pause = at("2026-11-04T01:00:00Z");
    run(&mut db, at(START)).await;
    run(&mut db, pause - SECOND).await;
    assert!(notices(&mut db).await.is_empty());
    assert_eq!(audits(&mut db, id, WARNED_ACTION).await, 0);
    assert_eq!(standing(&mut db, id).await, active());
    assert!(card_allowed(&mut db, id, pause - SECOND).await);
    run(&mut db, pause).await;
    assert_eq!(standing(&mut db, id).await, paused("license"));
    assert!(!card_allowed(&mut db, id, pause).await);
    run(&mut db, at(START) + SignedDuration::from_hours(24)).await;
    let sent = notices(&mut db).await;
    assert_eq!(sent.len(), 1, "one email per spell");
    assert_eq!(sent[0]["business"], id.to_string());
    assert_eq!(sent[0]["since"], at(START).as_second());
    assert_eq!(sent[0]["pause_at"], pause.as_second());
    assert_eq!(sent[0]["paused"], true);
    assert_eq!(audits(&mut db, id, WARNED_ACTION).await, 1);
    assert_eq!(audits(&mut db, id, "paused").await, 1);
}

/// TD-LIC-05 (LIC-008, D-199, D-4310): a fresh list showing the license Active gives the paused
/// business a check written by the run itself, and it resumes with no review, with a system
/// status audit row; a second run writes no second check.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_the_pause_clears_when_the_list_loads(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = unknown(&mut db).await;
    run(&mut db, at(PAUSE)).await;
    assert_eq!(standing(&mut db, id).await, paused("license"));
    record(&mut db, "2026-11-06", &[("R-11", "Active")]).await;
    let later = at("2026-11-06T13:05:00Z");
    run(&mut db, later).await;
    run(&mut db, later + SECOND).await;
    assert_eq!(standing(&mut db, id).await, active());
    let written: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM licensing.license_check WHERE business_id = $1 \
         AND list_date = '2026-11-06' AND status = 'active'",
    )
    .bind(id)
    .fetch_one(&mut *db)
    .await
    .expect("checks");
    assert_eq!(written, 1);
    assert_eq!(audits(&mut db, id, "active").await, 1);
    assert_eq!(notices(&mut db).await.len(), 1);
}

/// TD-LIC-05 (LIC-008, PICK-020): an Inactive check keeps the pause; a later Active lookup clears
/// it with no review; a known Inactive check never pauses an active business here.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_an_inactive_check_keeps_it_and_an_active_lookup_clears_it(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = unknown(&mut db).await;
    let lapsed = biz(&mut db, ("active", None), None).await;
    plant(&mut db, lapsed, "inactive", at(CHECKED)).await;
    run(&mut db, at(PAUSE)).await;
    assert_eq!(standing(&mut db, lapsed).await, active());
    record(&mut db, "2026-11-06", &[("R-11", "Inactive")]).await;
    run(&mut db, at("2026-11-06T13:05:00Z")).await;
    assert_eq!(standing(&mut db, id).await, paused("license"));
    plant(
        &mut db,
        id,
        "active",
        Timestamp::now() + SignedDuration::from_mins(1),
    )
    .await;
    run(&mut db, at("2026-11-07T15:00:00Z")).await;
    assert_eq!(standing(&mut db, id).await, active());
    assert_eq!(audits(&mut db, id, "active").await, 1);
}

/// TD-LIC-05 (LIC-008, LIC-047, BIZ-012): with the list back, a pest license reported wrong keeps
/// the `license` pause and three complaints in the window keep the business paused for
/// `complaints`; a paused business with no check, and every business before any list, is left.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_a_pest_report_or_a_complaint_hold_keeps_it_paused(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let bare = biz(&mut db, ("paused", Some("license")), None).await;
    let early = biz(
        &mut db,
        ("paused", Some("license")),
        Some("2026-11-01T13:10:00Z"),
    )
    .await;
    let steady = biz(&mut db, ("active", None), Some(CHECKED)).await;
    run(&mut db, at(PAUSE)).await;
    assert_eq!(standing(&mut db, steady).await, active());
    assert_eq!(standing(&mut db, early).await, paused("license"));
    let (pest, loud) = (unknown(&mut db).await, early);
    plant(&mut db, loud, "active", at(CHECKED)).await;
    sqlx::query(
        "UPDATE businesses.business SET status = 'active', pause_reason = NULL WHERE id = $1",
    )
    .bind(loud)
    .execute(&mut *db)
    .await
    .expect("active again");
    let attested = PestAttestation::new(true, "PM-4242", true, false).expect("attestation");
    record_pest_attestation(&mut db, pest, &attested, Actor::Business(pest))
        .await
        .expect("recorded");
    let reported = report_pest_license_wrong(&mut db, pest, at(CHECKED)).await;
    reported.expect("reported");
    run(&mut db, at(PAUSE)).await;
    let later = at("2026-11-06T13:05:00Z");
    for n in 1..=3u128 {
        let complaint = NewComplaint {
            business: loud,
            job: Uuid::from_u128(n),
            kind: "no_show",
            words: "did not arrive",
        };
        let recorded = record_complaint(&mut db, &complaint, later).await;
        recorded.expect("recorded");
    }
    record(&mut db, "2026-11-06", &[("R-11", "Active")]).await;
    run(&mut db, later).await;
    assert_eq!(standing(&mut db, pest).await, paused("license"));
    assert_eq!(standing(&mut db, loud).await, paused("complaints"));
    assert_eq!(standing(&mut db, bare).await, paused("license"));
    assert_eq!(standing(&mut db, steady).await, active());
    assert_eq!(audits(&mut db, loud, "pause_reason").await, 1);
}

fn catalog() -> Catalog {
    parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog")
}

fn map() -> &'static ClassMap {
    ClassMap::shipped().expect("license map loads")
}

/// The pre-ticked job types `class` gives in `category`.
fn covered(class: &str, category: &str) -> Vec<JobTypeChoice> {
    let offer = checklist_offer(map(), &catalog(), &[class]);
    let ticks = pre_ticked(&offer).into_iter();
    ticks.filter(|c| c.category == category).collect()
}

/// An active R-11 and R-37 business ticking only plumbing whose R-37 then goes (paused for
/// `no_covered_job_type` at the real time, as the worker would), self-paused first when `own`;
/// its latest check then turns Inactive and it ticks electrical, so `lift_no_cover` holds it for
/// `license`.
async fn held_for_license(db: &mut PgConnection, own: bool) -> Uuid {
    let id = biz(db, ("pending", None), None).await;
    check_license(db, id, NUMBER).await.expect("checked");
    let answers = CapabilityAnswers {
        ticked: covered("R-37", "plumbing_repair"),
        towns: vec!["lake_havasu_city".to_owned()],
        capability: Capability {
            largest: LargestJob::NoLimit,
            recurring: true,
        },
        after_hours: false,
    };
    let offer = checklist_offer(map(), &catalog(), &["R-11", "R-37"]);
    let saved = save_capability(db, id, &offer, &answers, (Actor::System, Timestamp::now())).await;
    saved.expect("saved");
    sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("active");
    if own {
        pause_self(db, id, Timestamp::now())
            .await
            .expect("self-paused");
    }
    id
}

/// TD-LIC-05 (LIC-008, D-4310, D-5185): `lift_no_cover`'s `license` hold clears the same way, with
/// no review: a business a rule paused from active resumes, and one that had paused itself first
/// returns to its own pause.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_the_no_cover_license_hold_clears_on_an_active_check(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    askhava_catalog::api::load(&mut db, &catalog())
        .await
        .expect("catalog stored");
    record(
        &mut db,
        "2026-11-01",
        &[("R-11", "Active"), ("R-37", "Active")],
    )
    .await;
    let ruled = held_for_license(&mut db, false).await;
    let own = held_for_license(&mut db, true).await;
    record(&mut db, "2026-11-02", &[("R-11", "Active")]).await;
    let applied = apply_lists(&mut db, (map(), &catalog()), Timestamp::now()).await;
    applied.expect("applied");
    for id in [ruled, own] {
        assert_eq!(standing(&mut db, id).await, paused("no_covered_job_type"));
        plant(&mut db, id, "inactive", Timestamp::now()).await;
        let at = (Actor::Business(id), Timestamp::now());
        let electrical = covered("R-11", "electrical");
        let maps = (map(), &catalog());
        let saved = save_job_types(&mut db, id, Known::default(), maps, &electrical, at).await;
        assert_eq!(saved.expect("saved").code(), "paused");
        assert_eq!(standing(&mut db, id).await, paused("license"));
    }
    record(&mut db, "2026-11-03", &[("R-11", "Active")]).await;
    run(&mut db, Timestamp::now()).await;
    assert_eq!(standing(&mut db, ruled).await, active());
    assert_eq!(audits(&mut db, ruled, "active").await, 1);
    assert_eq!(standing(&mut db, own).await, paused("self"));
}

/// TD-LIC-05 (LIC-008, JOB-006): the chain is queued once per list slot, five minutes after it,
/// and a run queues the next slot's before its work.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_the_run_is_queued_once_per_list_slot(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = at("2026-11-03T12:30:00Z");
    for _ in 0..2 {
        schedule_standing(&mut db, now, lists())
            .await
            .expect("queued");
    }
    run_standing(&pool, at(START) + STANDING_LAG, lists())
        .await
        .expect("run");
    let due: Vec<String> = sqlx::query_scalar(
        "SELECT to_char(run_at AT TIME ZONE 'UTC', 'HH24:MI') FROM core.task WHERE kind = $1 \
         ORDER BY run_at",
    )
    .bind(STANDING_KIND)
    .fetch_all(&mut *db)
    .await
    .expect("tasks");
    assert_eq!(due, ["13:05", "14:05"]);
}

/// TD-LIC-05 (LIC-008, BIZ-018): the email names the day the checks stopped and the pause moment
/// in Arizona time, in both languages, with the brand from the brand file and no license number,
/// link or endorsement word.
#[test]
fn td_lic_05_the_email_names_the_pause_moment_in_both_languages() {
    let spell = (at(START), at(PAUSE));
    let [subject, body] = license_unknown_words(false, spell, false);
    assert!(subject.starts_with(&format!("{}: ", brand().short_name())));
    assert!(body.contains("since Nov 3."), "{body}");
    assert!(
        body.contains("by 6:10 a.m. on Nov 5, Arizona time, we'll pause your job cards"),
        "{body}"
    );
    assert!(body.contains("If your license is Active, you don't need to do anything."));
    let [_, held] = license_unknown_words(false, spell, true);
    assert!(
        held.contains("So your job cards are paused until we can."),
        "{held}"
    );
    let [subject, es] = license_unknown_words(true, spell, false);
    assert!(subject.starts_with(&format!("{}: ", brand().es().short_name())));
    assert!(es.contains("desde el 3 de noviembre."), "{es}");
    assert!(
        es.contains("Puede que el 5 de noviembre, a las 6:10 a. m., hora de Arizona"),
        "{es}"
    );
    let [_, es_held] = license_unknown_words(true, spell, true);
    assert!(
        es_held.contains("El 5 de noviembre, a las 6:10 a. m., hora de Arizona, todavía"),
        "{es_held}"
    );
    assert!(es_held.contains("están en pausa"), "{es_held}");
    for text in [body, held, es, es_held] {
        let lower = text.to_lowercase();
        let banned = ["verified", "vetted", "verificad", "http", NUMBER];
        assert!(
            !banned.iter().any(|w| lower.contains(&w.to_lowercase())),
            "{text}"
        );
    }
}

#[derive(Default)]
struct FakeMail(Mutex<Vec<(String, String)>>);

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        let mut sent = self.0.lock().expect("lock");
        sent.push((e.to.to_owned(), e.subject.to_owned()));
        Ok(format!("re_{:08}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// A fake address per mailbox, Spanish, and no name, digits or link (this email has none).
struct FakeParts;

impl NoticeParts for FakeParts {
    async fn address(
        &self,
        _: &mut PgConnection,
        mailbox: Uuid,
    ) -> Result<Option<SecretString>, HandlerError> {
        let address = format!("{}@example.test", mailbox.as_u128());
        Ok(Some(SecretString::from(address)))
    }
    async fn spanish(&self, _: &mut PgConnection, _: Uuid) -> Result<bool, HandlerError> {
        Ok(true)
    }
    async fn member_name(
        &self,
        _: &mut PgConnection,
        _: Uuid,
    ) -> Result<Option<String>, HandlerError> {
        Ok(None)
    }
    async fn line_last4(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
    ) -> Result<Option<String>, HandlerError> {
        Ok(None)
    }
    async fn approve_link(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid, Uuid),
        _: Timestamp,
    ) -> Result<SecretString, HandlerError> {
        Err(HandlerError("no link in this email"))
    }
    fn retest_link(&self) -> String {
        String::new()
    }
}

/// TD-LIC-05 (LIC-008, BIZ-018, SEC-018): the queued warning is sent once, to the owner's account
/// address, in the owner's language; a repeat of the task sends nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_lic_05_the_warning_goes_once_to_the_owner(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let id = unknown(&mut db).await;
    let mailbox = Uuid::from_u128(0xb1);
    let owner = add_owner(&mut db, id, mailbox, at(CHECKED))
        .await
        .expect("owner");
    sqlx::query(
        "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, \
         sent_at, window_at, verified_at) VALUES ($1, $2, $3, false, now(), now(), now())",
    )
    .bind(id)
    .bind(owner)
    .bind(mailbox)
    .execute(&mut *db)
    .await
    .expect("verified");
    run(&mut db, at(PAUSE) - SECOND).await;
    let (task, payload): (i64, Value) =
        sqlx::query_as("SELECT id, payload FROM core.task WHERE kind = $1")
            .bind(LICENSE_UNKNOWN_KIND)
            .fetch_one(&mut *db)
            .await
            .expect("task");
    let mailer = Arc::new(AccountMailer {
        secret: SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned()),
        postal: (
            "PO Box 100, Faketown, AZ 00000".to_owned(),
            MailboxKind::PoBox,
        ),
        provider: FakeMail::default(),
        limit: DailyLimit::new(1000, 80).expect("limit"),
        directory: FakeParts,
        clock: Arc::new(SystemClock),
    });
    for attempts in 1..=2 {
        let claimed = Claimed {
            id: task,
            kind: LICENSE_UNKNOWN_KIND.to_owned(),
            payload: payload.clone(),
            attempts,
        };
        let sent = run_notice_email(pool.clone(), Some(Arc::clone(&mailer)), claimed).await;
        sent.expect("sent");
    }
    let [subject, _] = license_unknown_words(true, (at(START), at(PAUSE)), false);
    let sent = mailer.provider.0.lock().expect("lock").clone();
    assert_eq!(
        sent,
        [(format!("{}@example.test", mailbox.as_u128()), subject)]
    );
}
