//! TD-BIZ-56 (T690, D-4361; BIZ-026, BIZ-002, QH-001): a sign-up parked by "That number is old"
//! resumes when a new ROC list is recorded and its record shows a new contact.
//!
//! Fake records are written to a temporary `file://` directory and read through the real
//! `adapters::api::LicenseLists` (the license-record stand-in), counted by a thin wrapper; a fake
//! Verify records where each code went. The worker is played by [`drain`], which runs every due
//! `businesses.record_resume_one` task and marks it done at the fake time. All data is fake.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_adapters::api::{
    AdapterError, CodeChannel, IdempotencyKey, LicenseLists, LicenseRecords, VerifyAdapter,
};
use askhava_businesses::api::{
    RESUME_BATCH, RESUME_BATCH_KEY, RESUME_KIND, RESUME_ONE_KIND, SignupRecord, create,
    number_is_old, resume_batch, run_record_resume, run_resume_one, schedule_record_resume,
    signup_record, waiting_on_record,
};
use askhava_core::api::{Claimed, MIGRATOR};
use askhava_licensing::api::{DailyListSettings, RECORD_HEADER, check_license, import_roc_list};
use jiff::Timestamp;
use jiff::civil::date;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OLD_PHONE: &str = "928-555-0142";
const NEW_PHONE: &str = "928-555-0199";
const PACIFIC_PHONE: &str = "213-555-0177";
const OLD_MAIL: &str = "old@example.test";
const NEW_MAIL: &str = "new@example.test";
const OLD_STREET: &str = "100 Sample Way";
const NEW_STREET: &str = "200 Fake Road";

/// The fake record source: `file://` fixtures through `LicenseLists`, with every read counted.
struct Records {
    dir: PathBuf,
    lists: LicenseLists,
    reads: AtomicUsize,
}

impl Records {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("askhava-resume-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("record").join("roc")).expect("fixture dir");
        let lists = LicenseLists::new(&format!("file://{}", dir.display())).expect("source");
        let reads = AtomicUsize::new(0);
        Self { dir, lists, reads }
    }

    fn write(&self, number: &str, (phone, mail, street): (&str, &str, &str)) {
        let row = format!(
            "{number},Example Plumbing Co,,{phone},mobile,{mail},{street},Lake Havasu City,86403"
        );
        self.raw(number, &format!("{RECORD_HEADER}\n{row}\n"));
    }

    fn raw(&self, number: &str, text: &str) {
        let path = self.dir.join("record").join("roc").join(number);
        std::fs::write(path, text).expect("record");
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl LicenseRecords for Records {
    fn lookup_record(
        &self,
        list: &str,
        number: &str,
    ) -> impl Future<Output = Result<Vec<u8>, AdapterError>> + Send {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.lists.lookup_record(list, number)
    }
}

impl Drop for Records {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A fake Verify recording each code's destination and whether it went by text.
#[derive(Default)]
struct FakeVerify(Mutex<Vec<(String, bool)>>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let sms = matches!(channel, CodeChannel::Sms);
        self.0.lock().expect("lock").push((to.to_owned(), sms));
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, _: &str) -> Result<bool, AdapterError> {
        Ok(false)
    }
}

impl FakeVerify {
    fn sent(&self) -> Vec<(String, bool)> {
        self.0.lock().expect("lock").clone()
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned())
}

fn at(text: &str) -> Timestamp {
    text.parse().expect("fake time")
}

/// 10 a.m. Arizona on the second list's day.
fn day_time() -> Timestamp {
    at("2026-11-20T17:00:00Z")
}

fn number(i: usize) -> String {
    format!("ROC{:06}", 100_000 + i)
}

async fn import(conn: &mut PgConnection, day: i8, count: usize) {
    let mut text = "license_number,class,status,issued_on\n".to_owned();
    for i in 0..count {
        text.push_str(&format!("{},R-37,Active,2015-06-01\n", number(i)));
    }
    import_roc_list(conn, date(2026, 11, day), &text)
        .await
        .expect("roc list");
}

/// `count` sign-ups bound to their ROC numbers on the first list, each parked on its old record.
async fn parked(pool: &PgPool, records: &Records, count: usize) -> Vec<Uuid> {
    let mut conn = pool.acquire().await.expect("connection");
    import(&mut conn, 19, count).await;
    let mut ids = Vec::new();
    for i in 0..count {
        records.write(&number(i), (OLD_PHONE, OLD_MAIL, OLD_STREET));
        let id = create(&mut conn, false, at("2026-11-19T17:00:00Z")).await;
        let id = id.expect("business");
        sqlx::query(
            "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
             VALUES ($1, gen_random_uuid(), now(), ARRAY['plumbing_repair'])",
        )
        .bind(id)
        .execute(&mut *conn)
        .await
        .expect("sign-up row");
        check_license(&mut conn, id, &number(i))
            .await
            .expect("check");
        let SignupRecord::Read(found) = signup_record(&mut conn, records, id).await.expect("read")
        else {
            panic!("record");
        };
        let s = secret();
        let wait = at("2026-11-19T17:00:00Z") + jiff::SignedDuration::from_secs(i as i64);
        number_is_old(&mut conn, &s, (id, found.seen()), wait)
            .await
            .expect("parked");
        ids.push(id);
    }
    let seeded = schedule_record_resume(&mut conn, at("2026-11-19T17:00:00Z"), lists()).await;
    seeded.expect("seeded");
    ids
}

fn lists() -> DailyListSettings {
    DailyListSettings::default()
}

/// Runs every due lookup task as the worker would and marks it done at `now`.
async fn drain(pool: &PgPool, records: &Records, verify: &FakeVerify, now: Timestamp) {
    let due: Vec<(i64, serde_json::Value)> = sqlx::query_as(
        "SELECT id, payload FROM core.task WHERE kind = $1 AND done_at IS NULL \
         AND run_at <= $2::timestamptz ORDER BY id",
    )
    .bind(RESUME_ONE_KIND)
    .bind(now.to_string())
    .fetch_all(pool)
    .await
    .expect("due tasks");
    let s = secret();
    for (id, payload) in due {
        let kind = RESUME_ONE_KIND.to_owned();
        let task = Claimed {
            id,
            kind,
            payload,
            attempts: 1,
        };
        let ran = run_resume_one(pool, &task, (records, verify, &s), now).await;
        ran.expect("handler");
        sqlx::query("UPDATE core.task SET done_at = $2::timestamptz WHERE id = $1")
            .bind(id)
            .bind(now.to_string())
            .execute(pool)
            .await
            .expect("done");
    }
}

async fn step(pool: &PgPool, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT identity_step FROM businesses.signup WHERE business_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("step")
}

async fn waiting(pool: &PgPool, id: Uuid) -> bool {
    let mut conn = pool.acquire().await.expect("connection");
    waiting_on_record(&mut conn, id).await.expect("waiting")
}

/// Lookup tasks queued for a list (held ones left out).
async fn lookups(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = $1 AND NOT starts_with(dedupe_key, $2)",
    )
    .bind(RESUME_ONE_KIND)
    .bind(format!("{RESUME_ONE_KIND}.held."))
    .fetch_one(pool)
    .await
    .expect("lookups")
}

/// Runs the `id` run task at `now` as the worker would, marks it done and returns how many
/// lookups it queued.
async fn run_task(pool: &PgPool, (id, payload): (i64, serde_json::Value), now: Timestamp) -> i64 {
    let before = lookups(pool).await;
    let kind = RESUME_KIND.to_owned();
    let task = Claimed {
        id,
        kind,
        payload,
        attempts: 1,
    };
    let ran = run_record_resume(pool, &task, now, (lists(), 25)).await;
    ran.expect("run");
    sqlx::query("UPDATE core.task SET done_at = $2::timestamptz WHERE id = $1")
        .bind(id)
        .bind(now.to_string())
        .execute(pool)
        .await
        .expect("done");
    lookups(pool).await - before
}

/// Runs the next run of the chain (finished `false`) or again the last finished one (`true`), at
/// `now` or its due time when later, as the worker would.
async fn run(pool: &PgPool, now: Timestamp, again: bool) -> i64 {
    let (id, payload, due): (i64, serde_json::Value, String) = sqlx::query_as(
        "SELECT id, payload, to_char(GREATEST(run_at, $3::timestamptz) AT TIME ZONE 'UTC', \
           'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') FROM core.task \
         WHERE kind = $1 AND (done_at IS NOT NULL) = $2 ORDER BY id DESC LIMIT 1",
    )
    .bind(RESUME_KIND)
    .bind(again)
    .bind(now.to_string())
    .fetch_one(pool)
    .await
    .expect("run task");
    run_task(pool, (id, payload), at(&due)).await
}

async fn queue(pool: &PgPool, now: Timestamp) -> i64 {
    run(pool, now, false).await
}

/// Rows in any table whose text holds `value`.
async fn rows_holding(pool: &PgPool, value: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum((xpath('/row/n/text()', query_to_xml(format( \
           'SELECT count(*) AS n FROM %I.%I t WHERE t::text LIKE %L', table_schema, table_name, \
           '%' || $1 || '%'), false, true, '')))[1]::text::bigint), 0)::bigint \
         FROM information_schema.tables WHERE table_type = 'BASE TABLE' \
         AND table_schema NOT IN ('pg_catalog', 'information_schema')",
    )
    .bind(value)
    .fetch_one(pool)
    .await
    .expect("scan")
}

/// TD-BIZ-56 (BIZ-026, BIZ-002, D-1712): a new phone resumes at the text step with the code to
/// the new number only; an email-only change at the email step with no text; an address-only
/// change at the letter with no text or email; the same contacts or a removed one keep waiting;
/// one lookup each per list (a rerun makes none); no record contact is written anywhere.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_56_a_new_record_contact_resumes_at_its_step(pool: PgPool) {
    let (records, verify) = (Records::new(), FakeVerify::default());
    let ids = parked(&pool, &records, 5).await;
    records.write(&number(0), (NEW_PHONE, OLD_MAIL, OLD_STREET));
    records.write(&number(1), (OLD_PHONE, NEW_MAIL, OLD_STREET));
    records.write(&number(2), (OLD_PHONE, OLD_MAIL, NEW_STREET));
    records.write(&number(4), ("", OLD_MAIL, OLD_STREET));
    let mut conn = pool.acquire().await.expect("connection");
    import(&mut conn, 20, 5).await;
    let before = records.reads();
    assert_eq!(queue(&pool, day_time()).await, 5);
    drain(&pool, &records, &verify, day_time()).await;
    assert_eq!(records.reads() - before, 5, "one lookup each");
    let mut sent = verify.sent();
    sent.sort();
    let want = vec![(NEW_PHONE.to_owned(), true), (NEW_MAIL.to_owned(), false)];
    assert_eq!(sent, want, "codes go to the new contacts only");
    for (id, want) in ids.iter().zip(["text", "email", "letter"]) {
        assert!(!waiting(&pool, *id).await);
        assert_eq!(step(&pool, *id).await.as_deref(), Some(want));
    }
    assert!(
        waiting(&pool, ids[3]).await,
        "the same contacts keep waiting"
    );
    assert!(
        waiting(&pool, ids[4]).await,
        "a removed contact keeps waiting"
    );
    assert_eq!(queue(&pool, day_time()).await, 0, "a rerun queues none");
    drain(&pool, &records, &verify, day_time()).await;
    assert_eq!(records.reads() - before, 5);
    for value in [
        NEW_PHONE, "5550199", NEW_MAIL, NEW_STREET, OLD_MAIL, OLD_STREET,
    ] {
        assert_eq!(
            rows_holding(&pool, value).await,
            0,
            "no record contact stored"
        );
    }
}

/// TD-BIZ-56 (BIZ-026, LIC-008): a failed lookup does nothing and waits for the next list.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_56_a_failed_lookup_waits_for_the_next_list(pool: PgPool) {
    let (records, verify) = (Records::new(), FakeVerify::default());
    let ids = parked(&pool, &records, 1).await;
    records.raw(&number(0), "license_number,business_name\nROC100000,Fake\n");
    let mut conn = pool.acquire().await.expect("connection");
    import(&mut conn, 20, 1).await;
    assert_eq!(queue(&pool, day_time()).await, 1);
    drain(&pool, &records, &verify, day_time()).await;
    assert!(waiting(&pool, ids[0]).await && verify.sent().is_empty());
    records.write(&number(0), (NEW_PHONE, OLD_MAIL, OLD_STREET));
    let later = at("2026-11-21T17:00:00Z");
    assert_eq!(queue(&pool, later).await, 0, "no new list, no lookup");
    import(&mut conn, 21, 1).await;
    let next_day = at("2026-11-22T17:00:00Z");
    assert_eq!(queue(&pool, next_day).await, 1);
    drain(&pool, &records, &verify, next_day).await;
    assert!(!waiting(&pool, ids[0]).await);
    assert_eq!(verify.sent(), vec![(NEW_PHONE.to_owned(), true)]);
}

/// TD-BIZ-56 (BIZ-026): at most the batch per list; with 30 waiting and a batch of 25 the 5 not
/// looked up come first on the next list (the run chain's payload carries the last one queued,
/// D-5217), with no new column; a retried run queues none; the batch setting.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_56_the_batch_reaches_every_waiting_sign_up(pool: PgPool) {
    let (records, verify) = (Records::new(), FakeVerify::default());
    let ids = parked(&pool, &records, 30).await;
    let mut conn = pool.acquire().await.expect("connection");
    import(&mut conn, 20, 30).await;
    assert_eq!(queue(&pool, day_time()).await, 25);
    assert_eq!(
        run(&pool, day_time(), true).await,
        0,
        "a retried run queues none"
    );
    assert_eq!(
        queue(&pool, day_time()).await,
        0,
        "at most the batch per list"
    );
    drain(&pool, &records, &verify, day_time()).await;
    let looked: Vec<String> = sqlx::query_scalar(
        "SELECT payload ->> 'business' FROM core.task WHERE kind = $1 AND done_at IS NOT NULL",
    )
    .bind(RESUME_ONE_KIND)
    .fetch_all(&pool)
    .await
    .expect("finished rows");
    let missed: Vec<Uuid> = ids
        .iter()
        .filter(|id| !looked.contains(&id.to_string()))
        .copied()
        .collect();
    assert_eq!(missed.len(), 5);
    import(&mut conn, 21, 30).await;
    let later = at("2026-11-21T17:00:00Z");
    assert_eq!(queue(&pool, later).await, 25);
    for id in missed {
        let key = format!("{RESUME_ONE_KIND}.{id}.2026-11-21");
        let queued: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM core.task WHERE dedupe_key = $1)")
                .bind(key)
                .fetch_one(&pool)
                .await
                .expect("queued");
        assert!(queued, "a sign-up not looked up comes first");
    }
    let columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'businesses' \
         AND table_name = 'signup' AND column_name LIKE '%look%'",
    )
    .fetch_one(&pool)
    .await
    .expect("columns");
    assert_eq!(columns, 0);
    let pair = |v: &str| vec![(RESUME_BATCH_KEY.to_owned(), v.to_owned())];
    assert_eq!(resume_batch(Vec::new()), Ok(RESUME_BATCH));
    assert_eq!(resume_batch(pair("40")), Ok(40));
    assert!(resume_batch(pair("0")).is_err() && resume_batch(pair("x")).is_err());
}

async fn held_keys(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT dedupe_key, to_char(run_at AT TIME ZONE 'UTC', 'HH24:MI') FROM core.task \
         WHERE kind = $1 AND starts_with(dedupe_key, $2) ORDER BY id",
    )
    .bind(RESUME_ONE_KIND)
    .bind(format!("{RESUME_ONE_KIND}.held."))
    .fetch_all(pool)
    .await
    .expect("held")
}

/// TD-BIZ-56 (QH-001, BIZ-026): a run at 6 a.m. Arizona makes no lookup until 8 a.m. and then
/// exactly one, and the code goes at 8 a.m.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_56_an_early_run_looks_up_at_eight_arizona(pool: PgPool) {
    let (records, verify) = (Records::new(), FakeVerify::default());
    let ids = parked(&pool, &records, 1).await;
    records.write(&number(0), (NEW_PHONE, OLD_MAIL, OLD_STREET));
    let mut conn = pool.acquire().await.expect("connection");
    import(&mut conn, 20, 1).await;
    let (six, eight) = (at("2026-11-20T13:00:00Z"), at("2026-11-20T15:00:00Z"));
    let before = records.reads();
    assert_eq!(queue(&pool, six).await, 1);
    drain(&pool, &records, &verify, six).await;
    drain(&pool, &records, &verify, at("2026-11-20T14:59:00Z")).await;
    assert_eq!(records.reads(), before, "no lookup before 8 a.m.");
    let key = format!(
        "{RESUME_ONE_KIND}.held.{}.2026-11-20.{}",
        ids[0],
        eight.as_second()
    );
    assert_eq!(held_keys(&pool).await, vec![(key, "15:00".to_owned())]);
    drain(&pool, &records, &verify, eight).await;
    assert_eq!(records.reads() - before, 1);
    assert_eq!(verify.sent(), vec![(NEW_PHONE.to_owned(), true)]);
    assert!(!waiting(&pool, ids[0]).await);
}

/// TD-BIZ-56 (QH-001, BIZ-026): a 6 a.m. run with a winter Pacific record phone looks up once at
/// 8 a.m. Arizona, re-queues under `.held.<sign-up>.<list date>.<until>` for 9 a.m. Arizona (8
/// a.m. Pacific), sends nothing and resumes nothing before then, and the re-queued task sends.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_56_a_pacific_phone_waits_for_its_own_eight(pool: PgPool) {
    let (records, verify) = (Records::new(), FakeVerify::default());
    let ids = parked(&pool, &records, 1).await;
    records.write(&number(0), (PACIFIC_PHONE, OLD_MAIL, OLD_STREET));
    let mut conn = pool.acquire().await.expect("connection");
    import(&mut conn, 20, 1).await;
    let before = records.reads();
    let (six, eight, nine) = (
        at("2026-11-20T13:00:00Z"),
        at("2026-11-20T15:00:00Z"),
        at("2026-11-20T16:00:00Z"),
    );
    assert_eq!(queue(&pool, six).await, 1);
    drain(&pool, &records, &verify, six).await;
    assert_eq!(records.reads(), before);
    drain(&pool, &records, &verify, eight).await;
    assert_eq!(records.reads() - before, 1, "one lookup at 8 a.m. Arizona");
    assert!(verify.sent().is_empty() && waiting(&pool, ids[0]).await);
    let key = format!(
        "{RESUME_ONE_KIND}.held.{}.2026-11-20.{}",
        ids[0],
        nine.as_second()
    );
    let held = held_keys(&pool).await;
    assert_eq!(held.last(), Some(&(key, "16:00".to_owned())));
    drain(&pool, &records, &verify, at("2026-11-20T15:59:00Z")).await;
    assert!(verify.sent().is_empty());
    drain(&pool, &records, &verify, nine).await;
    assert_eq!(verify.sent(), vec![(PACIFIC_PHONE.to_owned(), true)]);
    assert!(!waiting(&pool, ids[0]).await);
    assert_eq!(step(&pool, ids[0]).await.as_deref(), Some("text"));
}
