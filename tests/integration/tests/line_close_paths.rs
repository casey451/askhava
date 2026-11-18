//! TD-CALL-27 (CALL-009, CALL-010, JOB-005, JOB-015; D-1694, D-4338): a masked call line closes
//! through the real paths, not `close_line` directly: her text cancel, her page cancel and the
//! INT-038 cancel queue `jobs`' `calls.close_line` task in the move's transaction, and the task
//! (`calls::api::run_close_line`) closes the line with one `line_closed_cancelled` notice and no
//! strike, after which the business's call on the pool number connects nothing; a check-in
//! PROBLEM closes it as a complaint and a NO as a no-show, with no notice; OTHER and DONE leave it
//! open; a rolled-back move queues nothing; a late line retry opens nothing for a final job; the
//! daily sweep closes 30-day-old lines and queues its next run. Fake numbers (555-01xx and the
//! 500-555 test range), fake IDs and fake words only.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{Acting, Roles, SenderLink, TeamError, add_member, add_owner};
use askhava_calls::api::{
    Answer as CallAnswer, Attestation, CLOSE_EXPIRED_KIND, CallTurn, CloseReason, Connected,
    IncomingCall, Step, answer_call, assign_number, close_line, run_close_expired_at,
    run_close_line,
};
use askhava_core::api::{Claimed, Guarded};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, Opened, RawReply, Reply, TextIn, begin_job, close_from_page, receive_raw_reply,
};
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{
    Answer, Inbound as CheckInbound, JobStatus, LINE_CLOSE_KIND, answer_check_in, current,
    job_number, move_to,
};
use askhava_messaging::api::{OutboxError, sender_add};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

/// The first case's business owner main phone; a later case's ends in its index.
const OWNER_PHONE: &str = "+19285550180";
const POOL_NUMBER: &str = "+15005550100";
const TO_RELEASE: [JobStatus; 3] = [
    JobStatus::Open,
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
];
const TO_CONNECTED: [JobStatus; 2] = [JobStatus::PaymentPending, JobStatus::Connected];

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x62), &key(0x63), &url).expect("vault")
}

/// A masked pool of one fake number, so every line is on it.
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
        ("ASKHAVA_MASKED_NUMBERS", POOL_NUMBER),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let settings = askhava_core::api::Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

/// The composition's sender-directory link (D-963), with the test vault.
struct Directory(Vault);

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &self.0, business_line, number.expose_secret())
            .await
            .map_err(|e| match e {
                OutboxError::BadPhone => TeamError::BadPhone,
                _ => TeamError::Database,
            })
    }
}

/// Case `n`'s business: its owner (mailbox 0xa0 for case 0, main phone [`OWNER_PHONE`] for case
/// 0) and a "Confirm and billing" member (0xa1 for case 0).
async fn business(db: &mut PgConnection, n: u8) -> Uuid {
    let mailbox = |m: u128| Uuid::from_u128(m + u128::from(n) * 0x100);
    let b = askhava_businesses::api::create(db, false, at(0))
        .await
        .expect("business");
    let owner = add_owner(db, b, mailbox(0xa0), at(0)).await.expect("owner");
    let by = Acting {
        business: b,
        member: owner,
    };
    let billing = Roles::new(false, true).expect("roles");
    add_member(db, by, mailbox(0xa1), billing, at(0))
        .await
        .expect("member");
    let phone = SecretString::from(format!("+1928555018{n}"));
    askhava_businesses::api::link_phone(db, &Directory(vault()), (by, true), &phone, at(0))
        .await
        .expect("main phone");
    b
}

fn text(contact: Uuid, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(contact.as_u128() ^ (0x662_0000 + minute.unsigned_abs() as u128)),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// The reply to `msg` through the route's entry.
async fn reply(db: &mut PgConnection, msg: &TextIn<'_>) -> Reply {
    let raw = SecretString::from(msg.inbound.words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

/// One fake homeowner's job, its request open: (her contact, job, job number, business).
struct Case {
    her: Uuid,
    job: Uuid,
    number: i64,
    business: Uuid,
}

/// Case `n` (0 to 9): her request and one plumbing job, walked along `path` by hand; her fake
/// phone ends in 9 and `n`.
async fn case(db: &mut PgConnection, path: &[JobStatus], n: u8) -> Case {
    let new = NewContact {
        phone: Some(SecretString::from(format!("+192855501{}", 90 + n))),
        ..NewContact::default()
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let first = text(her, "my kitchen sink is leaking under the cabinet", 1);
    let Reply::Handled(handled) = reply(db, &first).await else {
        panic!("a first text opens a request");
    };
    let start = JobStart {
        conversation: handled.conversation.expect("request"),
        words: "my kitchen sink is leaking under the cabinet",
        emergency: false,
        trade: "plumbing",
        at: at(2),
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    walk(db, job, path).await;
    let number = job_number(db, job).await.expect("number");
    let business = business(db, n).await;
    Case {
        her,
        job,
        number,
        business,
    }
}

/// Moves `job` along `path` by hand, each move applied.
async fn walk(db: &mut PgConnection, job: Uuid, path: &[JobStatus]) {
    for &to in path {
        let from = current(db, job).await.expect("job").status;
        let moved = move_to(db, job, (from, to), at(3), Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())), "{from:?} to {to:?}");
    }
}

/// Opens the case's masked line at `when`; its ID.
async fn open_line(db: &mut PgConnection, c: &Case, when: Timestamp) -> Uuid {
    let connected = Connected {
        job: c.job,
        homeowner: c.her,
        business: c.business,
    };
    assign_number(db, &masked(), connected, when)
        .await
        .expect("line")
        .id()
}

/// The business owner's call on the pool number, with no PIN entered.
async fn business_call(db: &mut PgConnection) -> Step {
    let caller = SecretString::from(OWNER_PHONE.to_owned());
    let incoming = IncomingCall {
        pool_number: POOL_NUMBER,
        caller: &caller,
    };
    let turn = CallTurn {
        attestation: Attestation::A,
        tries_call: 0,
        tries_today: 0,
        entered: None,
    };
    match answer_call(db, &vault(), &incoming, turn)
        .await
        .expect("answer")
    {
        CallAnswer::Step(step) => step,
        CallAnswer::ReadBack(..) => panic!("no read-back without a wrong PIN"),
    }
}

/// The queued `calls.close_line` tasks of `job`: (payload, dedupe key).
async fn close_tasks(db: &mut PgConnection, job: Uuid) -> Vec<(Value, String)> {
    sqlx::query_as(
        "SELECT payload, dedupe_key FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         ORDER BY id",
    )
    .bind(LINE_CLOSE_KIND)
    .bind(job.to_string())
    .fetch_all(db)
    .await
    .expect("tasks")
}

/// Runs each not yet done close of `job` once, as the worker would, and marks it done.
async fn run_closes(pool: &PgPool, job: Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let due: Vec<(i64, Value)> = sqlx::query_as(
        "SELECT id, payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         AND done_at IS NULL ORDER BY id",
    )
    .bind(LINE_CLOSE_KIND)
    .bind(job.to_string())
    .fetch_all(&mut *db)
    .await
    .expect("due");
    for (id, payload) in due {
        let task = Claimed {
            id,
            kind: LINE_CLOSE_KIND.to_owned(),
            payload,
            attempts: 1,
        };
        run_close_line(pool.clone(), task, at(60))
            .await
            .expect("ran");
        sqlx::query("UPDATE core.task SET done_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *db)
            .await
            .expect("done");
    }
}

async fn scalar(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id.to_string())
        .fetch_one(db)
        .await
        .expect("count")
}

const OPEN: &str =
    "SELECT count(*) FROM calls.number_pool WHERE id::text = $1 AND closed_at IS NULL";
const NOTICES: &str = "SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice' \
     AND payload->>'template' = 'line_closed_cancelled' AND payload->>'line' = $1";
const STRIKES: &str =
    "SELECT count(*) FROM picks.pick WHERE job_id::text = $1 AND status = 'no_show'";

/// The line's close audit actions.
async fn closes(db: &mut PgConnection, line: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT action FROM ops.audit_log WHERE subject_id = $1 \
         AND action LIKE 'calls.line_closed_%' ORDER BY id",
    )
    .bind(line)
    .fetch_all(db)
    .await
    .expect("audit")
}

/// After a cancel and its task: the line closed once as a cancel, one notice to the owner and
/// the billing member with the job number, no strike, and the business's call connects nothing.
async fn assert_cancel_closed(db: &mut PgConnection, c: &Case, line: Uuid) {
    assert_eq!(scalar(db, OPEN, line).await, 0, "the line is closed");
    assert_eq!(closes(db, line).await, [CloseReason::Cancelled.action()]);
    assert_eq!(scalar(db, NOTICES, line).await, 1, "one cancel notice");
    let sent: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'line' = $1",
    )
    .bind(line.to_string())
    .fetch_one(&mut *db)
    .await
    .expect("notice");
    assert_eq!(sent["values"]["job"], c.number);
    let to = [Uuid::from_u128(0xa0), Uuid::from_u128(0xa1)].map(|u| u.to_string());
    assert_eq!(sent["to"], json!(to));
    assert_eq!(scalar(db, STRIKES, c.job).await, 0, "no strike");
    assert!(
        !matches!(business_call(db).await, Step::Connect(_)),
        "a closed line connects nothing"
    );
}

/// TD-CALL-27 (CALL-009, CALL-010): her text cancel, through `intake`, of a job at the end of
/// `path` with an open line queues one close with the move; the task closes the line.
async fn text_cancel_closes(pool: &PgPool, path: &[JobStatus]) {
    let mut db = pool.acquire().await.expect("db");
    let c = case(&mut db, path, 0).await;
    let line = open_line(&mut db, &c, at(5)).await;
    assert_eq!(business_call(&mut db).await, Step::Connect(line));
    let words = format!("CLOSE {}", c.number);
    reply(&mut db, &text(c.her, &words, 10)).await;
    assert_eq!(
        current(&mut db, c.job).await.expect("job").status.code(),
        "cancelled"
    );
    assert_eq!(
        close_tasks(&mut db, c.job).await.len(),
        1,
        "one close queued"
    );
    assert_eq!(
        scalar(&mut db, OPEN, line).await,
        1,
        "open until the task runs"
    );
    run_closes(pool, c.job).await;
    let (payload, _) = close_tasks(&mut db, c.job).await.remove(0);
    let again = Claimed {
        id: 2,
        kind: LINE_CLOSE_KIND.to_owned(),
        payload,
        attempts: 2,
    };
    run_close_line(pool.clone(), again, at(61))
        .await
        .expect("a repeat closes nothing");
    assert_cancel_closed(&mut db, &c, line).await;
}

/// TD-CALL-27: a released job left in Awaiting confirm (where production leaves it), cancelled by
/// her text, has its line closed with the notice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_a_text_cancel_in_awaiting_confirm_closes_the_line(pool: PgPool) {
    text_cancel_closes(&pool, &TO_RELEASE).await;
}

/// TD-CALL-27: a released job the confirm timer moved back to Quotes ready, cancelled, too.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_a_text_cancel_back_in_quotes_ready_closes_the_line(pool: PgPool) {
    let path = [TO_RELEASE.as_slice(), &[JobStatus::QuotesReady]].concat();
    text_cancel_closes(&pool, &path).await;
}

/// TD-CALL-27: a Connected job (set by hand), cancelled by her text, too.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_a_text_cancel_of_a_connected_job_closes_the_line(pool: PgPool) {
    let path = [TO_RELEASE.as_slice(), &TO_CONNECTED].concat();
    text_cancel_closes(&pool, &path).await;
}

/// TD-CALL-27: her page cancel (`close_from_page`) of a Connected job closes its line once the
/// task runs; a rolled-back page cancel first queues nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_a_page_cancel_closes_the_line_and_a_rollback_queues_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let c = case(&mut db, &[TO_RELEASE.as_slice(), &TO_CONNECTED].concat(), 0).await;
    let line = open_line(&mut db, &c, at(5)).await;
    let mut tx = db.begin().await.expect("tx");
    assert_eq!(close_from_page(&mut tx, c.job, at(10)).await, Ok(true));
    tx.rollback().await.expect("rollback");
    assert!(
        close_tasks(&mut db, c.job).await.is_empty(),
        "a rolled-back move queues nothing"
    );
    assert_eq!(
        current(&mut db, c.job).await.expect("job").status.code(),
        "connected"
    );
    assert_eq!(close_from_page(&mut db, c.job, at(11)).await, Ok(true));
    assert_eq!(close_tasks(&mut db, c.job).await.len(), 1);
    run_closes(&pool, c.job).await;
    assert_cancel_closed(&mut db, &c, line).await;
}

/// TD-CALL-27: the INT-038 cancel ("I'm 15") of a Connected job closes its line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_the_int_038_cancel_closes_the_line(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let c = case(&mut db, &[TO_RELEASE.as_slice(), &TO_CONNECTED].concat(), 0).await;
    let line = open_line(&mut db, &c, at(5)).await;
    let Reply::UnderAge(..) = reply(&mut db, &text(c.her, "I'm 15", 10)).await else {
        panic!("I'm 15 stops");
    };
    assert_eq!(close_tasks(&mut db, c.job).await.len(), 1);
    run_closes(&pool, c.job).await;
    assert_cancel_closed(&mut db, &c, line).await;
}

/// A check-in `answer` on the case's Connected job, as her reply at `minute`.
async fn check_in(db: &mut PgConnection, c: &Case, answer: Answer, check: CheckIn, minute: i64) {
    let inbound = CheckInbound {
        inbox: Uuid::from_u128(0x0006_6200 + minute.unsigned_abs() as u128),
        safety: false,
    };
    let actor = Actor::Homeowner(c.her);
    let answered = answer_check_in(
        db,
        c.job,
        (answer, check, Contact::Call),
        inbound,
        (at(minute), actor),
    )
    .await
    .expect("answered");
    assert!(answered.applied().is_some(), "{answer:?} applied");
}

/// TD-CALL-27 (CALL-009, JOB-015): a check-in PROBLEM closes the line as a complaint and a NO as
/// a no-show, with no notice; OTHER and DONE queue nothing and leave it open.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_check_in_problem_and_no_close_other_and_done_do_not(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let cases = [
        (Answer::Problem, Some(CloseReason::Complaint)),
        (Answer::No, Some(CloseReason::NoShow)),
        (Answer::Other, None),
        (Answer::Done, None),
    ];
    for (n, (answer, reason)) in (0..).zip(cases) {
        let c = case(&mut db, &[TO_RELEASE.as_slice(), &TO_CONNECTED].concat(), n).await;
        let line = open_line(&mut db, &c, at(5)).await;
        check_in(&mut db, &c, answer, CheckIn::Day3, 20).await;
        run_closes(&pool, c.job).await;
        let queued = close_tasks(&mut db, c.job).await.len();
        assert_eq!(queued, usize::from(reason.is_some()), "{answer:?}");
        let expected: Vec<&str> = reason.iter().map(|r| r.action()).collect();
        assert_eq!(closes(&mut db, line).await, expected, "{answer:?}");
        assert_eq!(
            scalar(&mut db, NOTICES, line).await,
            0,
            "{answer:?}: no notice"
        );
    }
}

/// TD-CALL-27 (CALL-009, D-1694): a line the no-show path closed first makes the cancel's task a
/// no-op; a cancel of a job that never had a line queues a task that closes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_a_closed_or_missing_line_makes_the_task_a_no_op(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let c = case(&mut db, &[TO_RELEASE.as_slice(), &TO_CONNECTED].concat(), 0).await;
    let line = open_line(&mut db, &c, at(5)).await;
    let closed = close_line(&mut db, (c.job, c.number), CloseReason::NoShow, at(6)).await;
    assert_eq!(closed, Ok(Some(line)));
    assert_eq!(close_from_page(&mut db, c.job, at(10)).await, Ok(true));
    run_closes(&pool, c.job).await;
    assert_eq!(closes(&mut db, line).await, [CloseReason::NoShow.action()]);
    assert_eq!(scalar(&mut db, NOTICES, line).await, 0, "no cancel notice");
    let bare = case(&mut db, &TO_RELEASE, 1).await;
    assert_eq!(close_from_page(&mut db, bare.job, at(12)).await, Ok(true));
    assert_eq!(close_tasks(&mut db, bare.job).await.len(), 1);
    run_closes(&pool, bare.job).await;
    let lines = "SELECT count(*) FROM calls.number_pool WHERE job_id::text = $1";
    assert_eq!(
        scalar(&mut db, lines, bare.job).await,
        0,
        "nothing to close"
    );
}

/// TD-CALL-27 (CALL-009, JOB-015): two moves out of Connected on one job (a day-3 NO under email
/// contact back to Quotes ready, a re-pick, then a PROBLEM) each queue their own close.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_two_moves_out_of_connected_each_queue_their_close(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let c = case(&mut db, &[TO_RELEASE.as_slice(), &TO_CONNECTED].concat(), 0).await;
    let first = open_line(&mut db, &c, at(5)).await;
    let inbound = CheckInbound {
        inbox: Uuid::from_u128(0x0006_6201),
        safety: false,
    };
    let answer = (Answer::No, CheckIn::Day3, Contact::Email);
    let actor = Actor::Homeowner(c.her);
    let answered = answer_check_in(&mut db, c.job, answer, inbound, (at(20), actor))
        .await
        .expect("no");
    assert!(answered.applied().is_some(), "NO applied");
    assert_eq!(
        current(&mut db, c.job).await.expect("job").status.code(),
        "quotes_ready"
    );
    run_closes(&pool, c.job).await;
    walk(
        &mut db,
        c.job,
        &[&[JobStatus::AwaitingConfirm], TO_CONNECTED.as_slice()].concat(),
    )
    .await;
    let second = open_line(&mut db, &c, at(40)).await;
    assert_ne!(first, second);
    check_in(&mut db, &c, Answer::Problem, CheckIn::Day14, 50).await;
    run_closes(&pool, c.job).await;
    let tasks = close_tasks(&mut db, c.job).await;
    assert_eq!(tasks.len(), 2, "each move queues its own close");
    assert_ne!(tasks[0].1, tasks[1].1);
    assert_eq!(closes(&mut db, first).await, [CloseReason::NoShow.action()]);
    assert_eq!(
        closes(&mut db, second).await,
        [CloseReason::Complaint.action()]
    );
}

/// Seeds a paid pick of the case's business with its release row bound to her contact; the pick.
async fn paid_pick(db: &mut PgConnection, c: &Case) -> Uuid {
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', 'paid', now()) RETURNING id",
    )
    .bind(c.job)
    .bind(c.business)
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    sqlx::query(
        "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
         contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', now(), $4, false)",
    )
    .bind(pick)
    .bind(c.job)
    .bind(c.business)
    .bind(c.her)
    .execute(&mut *db)
    .await
    .expect("release");
    pick
}

/// TD-CALL-27 (CALL-009, CALL-010, D-4338): a line retry still queued when the job is cancelled
/// opens no line when it runs, while one for a job in Awaiting confirm opens it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_a_line_retry_after_a_cancel_opens_no_line(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let lines = "SELECT count(*) FROM calls.number_pool WHERE job_id::text = $1";
    for (n, cancel) in [(0, true), (1, false)] {
        let c = case(&mut db, &TO_RELEASE, n).await;
        let pick = paid_pick(&mut db, &c).await;
        if cancel {
            assert_eq!(close_from_page(&mut db, c.job, at(10)).await, Ok(true));
            run_closes(&pool, c.job).await;
        }
        let task = Claimed {
            id: 1,
            kind: askhava_picks::api::LINE_RETRY_KIND.to_owned(),
            payload: json!({ "pick": pick }),
            attempts: 1,
        };
        askhava_picks::api::retry_line(pool.clone(), masked(), task, at(20))
            .await
            .expect("retry");
        let opened = scalar(&mut db, lines, c.job).await;
        assert_eq!(opened, i64::from(!cancel), "cancelled {cancel}");
    }
}

/// TD-CALL-27 (CALL-009): the daily sweep closes a line assigned 30 days before its run, not one
/// assigned 29 days 23 hours before, and queues its next run a day later.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_27_the_daily_sweep_closes_30_day_old_lines(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let now = at(0) + SignedDuration::from_hours(60 * 24);
    let old = case(&mut db, &[], 0).await;
    let old_line = open_line(&mut db, &old, now - SignedDuration::from_hours(30 * 24)).await;
    let young = Case {
        job: Uuid::from_u128(0x662_0bad),
        ..old
    };
    let young_at = now - SignedDuration::from_hours(30 * 24 - 1);
    let young_line = open_line(&mut db, &young, young_at).await;
    let task = Claimed {
        id: 1,
        kind: CLOSE_EXPIRED_KIND.to_owned(),
        payload: json!({}),
        attempts: 1,
    };
    run_close_expired_at(pool.clone(), task, now)
        .await
        .expect("swept");
    assert_eq!(
        closes(&mut db, old_line).await,
        [CloseReason::Expired.action()]
    );
    assert_eq!(
        scalar(&mut db, OPEN, young_line).await,
        1,
        "29 days 23 hours stays open"
    );
    let next: Vec<jiff_sqlx::Timestamp> =
        sqlx::query_scalar("SELECT run_at FROM core.task WHERE kind = $1")
            .bind(CLOSE_EXPIRED_KIND)
            .fetch_all(&mut *db)
            .await
            .expect("next run");
    let next: Vec<Timestamp> = next.into_iter().map(|t| t.to_jiff()).collect();
    assert_eq!(next, [now + SignedDuration::from_hours(24)]);
}
