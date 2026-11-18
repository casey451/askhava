//! TD-OPS-08's day-3 NO-or-OTHER input (T523; OPS-031, D-3881, D-4037, D-4053) on a real
//! database: `jobs::api::connected_answers` counts the planted connections of a month and, of
//! them, only the `day3_no` and `day3_other` rows of that connection (never a later one's); 31 of
//! 100 is over 30%, 30 of 100 is not; an applied check-in answer writes one `jobs.check_in_answer`
//! row and a repeat none; a texted email-contact day-3 NO, through `intake::api::receive_reply`,
//! writes one row and queues one step. Every move is planted in the past and no clock advances,
//! since the audit row's `at` is the database's `now()`. Fake IDs, codes, names and numbers only.

use askhava_core::api::Guarded;
use askhava_intake::api::rules::{Inbound as Words, Settings};
use askhava_intake::api::{
    JobStart, Opened, Reply, TextIn, begin_job, receive_reply, receive_text,
};
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{
    Answer, CHECK_IN_ANSWER, ConnectedAnswers, Inbound, JobStatus, answer_check_in,
    connected_answers, move_to,
};
use askhava_ops::api::Actor;
use askhava_picks::api::{ContactChoice, record_contact_choice};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

fn t(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

/// October 5 and 9, 2026: a connection and its answer.
const OCT5: &str = "2026-10-05T12:00:00Z";
const OCT9: &str = "2026-10-09T12:00:00Z";

/// Calendar month `m` of 2026 in Arizona time.
fn month(m: u8) -> (Timestamp, Timestamp) {
    let start = |m: u8| t(&format!("2026-{m:02}-01T07:00:00Z"));
    (start(m), start(m + 1))
}

/// A day either side of `at`.
fn around(at: Timestamp) -> (Timestamp, Timestamp) {
    let day = SignedDuration::from_hours(24);
    (at - day, at + day)
}

/// A fake job; with `history`, its planted moves `(to_status, at)`, versions 1, 2, …
async fn job(db: &mut PgConnection, history: &[(&str, &str)]) -> Uuid {
    let at = t("2026-01-01T00:00:00Z").to_sqlx();
    let id: Uuid = sqlx::query_scalar("INSERT INTO jobs.job (status, created_at, status_at, timer_from) VALUES ('intake', $1, $1, $1) RETURNING id")
        .bind(at).fetch_one(&mut *db).await.expect("job");
    for (version, (to, at)) in (1..).zip(history) {
        sqlx::query("INSERT INTO jobs.status_history (job_id, version, from_status, to_status, at) VALUES ($1, $2, 'payment_pending', $3, $4)")
            .bind(id).bind(version).bind(*to).bind(t(at).to_sqlx()).execute(&mut *db).await.expect("move");
    }
    id
}

/// A planted `jobs.check_in_answer` row for `job` with field code `field`, written at `at`.
async fn answered(db: &mut PgConnection, job: Uuid, field: &str, at: &str) {
    sqlx::query("INSERT INTO ops.audit_log (at, actor_kind, action, subject_kind, subject_id, field) VALUES ($1, 'system', $2, 'jobs.job', $3, $4)")
        .bind(t(at).to_sqlx()).bind(CHECK_IN_ANSWER).bind(job).bind(field).execute(&mut *db).await.expect("row");
}

async fn rows(db: &mut PgConnection, job: Uuid) -> Vec<Option<String>> {
    sqlx::query_scalar("SELECT field FROM ops.audit_log WHERE action = $1 AND subject_id = $2")
        .bind(CHECK_IN_ANSWER)
        .bind(job)
        .fetch_all(&mut *db)
        .await
        .expect("rows")
}

fn ids(got: ConnectedAnswers) -> (Vec<Uuid>, Vec<Uuid>) {
    (got.connected, got.no_or_other)
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort_unstable();
    ids
}

async fn read(
    db: &mut PgConnection,
    jobs: &[Uuid],
    window: (Timestamp, Timestamp),
) -> ConnectedAnswers {
    connected_answers(db, jobs, window).await.expect("read")
}

/// TD-OPS-08 (OPS-031, D-4037): of one row of each day-3 and day-14 answer, only `day3_no` and
/// `day3_other` count; a job connected in another month, or not passed, does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_only_day3_no_and_other_count_in_the_month(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let mut jobs = Vec::new();
    for day in ["day3", "day14"] {
        for answer in ["done", "no", "problem", "later", "ongoing", "other"] {
            let id = job(&mut db, &[("connected", OCT5)]).await;
            answered(&mut db, id, &format!("{day}_{answer}"), OCT9).await;
            jobs.push(id);
        }
    }
    let september = job(&mut db, &[("connected", "2026-09-29T12:00:00Z")]).await;
    answered(&mut db, september, "day3_no", "2026-10-02T12:00:00Z").await;
    jobs.push(september);
    let not_passed = job(&mut db, &[("connected", OCT5)]).await;
    answered(&mut db, not_passed, "day3_no", OCT9).await;
    let got = read(&mut db, &jobs, month(10)).await;
    assert_eq!(got.connected, sorted(jobs[..12].to_vec()));
    assert_eq!(got.no_or_other, sorted(vec![jobs[1], jobs[5]]));
}

/// TD-OPS-08 (OPS-031, D-251): 31 NO-or-OTHER of 100 connected jobs is over 30%; 30 of 100 is not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_thirty_one_percent_is_over_thirty_and_thirty_is_not(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    for (answers, over) in [(31, true), (30, false)] {
        let mut jobs = Vec::new();
        for n in 0..100 {
            let id = job(&mut db, &[("connected", OCT5)]).await;
            if n < answers {
                answered(&mut db, id, ["day3_no", "day3_other"][n % 2], OCT9).await;
            }
            jobs.push(id);
        }
        let got = read(&mut db, &jobs, month(10)).await;
        assert_eq!((got.connected.len(), got.no_or_other.len()), (100, answers));
        assert_eq!(got.no_or_other.len() * 100 > 30 * got.connected.len(), over);
    }
}

/// TD-OPS-08 (OPS-031, D-4053): a row counts only for its own connection. Job `a` connects March
/// 30, is reopened April 2 with no row, connects April 4 and answers OTHER April 8; job `b`
/// answers NO April 1 to its March 30 connection and reconnects April 4 with no answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_an_answer_counts_for_its_own_connection_only(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let moves = [
        ("connected", "2026-03-30T17:00:00Z"),
        ("quotes_ready", "2026-04-02T17:00:00Z"),
        ("connected", "2026-04-04T17:00:00Z"),
    ];
    let a = job(&mut db, &moves).await;
    answered(&mut db, a, "day3_other", "2026-04-08T17:00:00Z").await;
    let b = job(&mut db, &moves).await;
    answered(&mut db, b, "day3_no", "2026-04-01T17:00:00Z").await;
    let both = [a, b];
    let all = sorted(both.to_vec());
    assert_eq!(
        ids(read(&mut db, &both, month(3)).await),
        (all.clone(), vec![b])
    );
    assert_eq!(ids(read(&mut db, &both, month(4)).await), (all, vec![a]));
}

/// TD-OPS-08 (OPS-031, D-3881): an applied check-in answer writes one `jobs.check_in_answer`
/// row in its transaction; a repeated answer, the job having moved on, writes none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_one_answer_writes_one_row_and_a_repeat_none(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let connected = Timestamp::now() - SignedDuration::from_hours(4 * 24);
    let id = job(&mut db, &[]).await;
    walk_to_connected(&mut db, id, connected).await;
    for applied in [true, false] {
        let inbound = Inbound {
            inbox: Uuid::new_v4(),
            safety: false,
        };
        let how = (Answer::Other, CheckIn::Day3, Contact::Call);
        let at = (
            Timestamp::now() - SignedDuration::from_mins(1),
            Actor::System,
        );
        let got = answer_check_in(&mut db, id, how, inbound, at)
            .await
            .expect("answer");
        assert_eq!(matches!(got, Guarded::Applied(_)), applied);
        assert_eq!(rows(&mut db, id).await, [Some("day3_other".to_owned())]);
    }
    let got = read(&mut db, &[id], around(connected)).await;
    assert_eq!(ids(got), (vec![id], vec![id]));
}

async fn walk_to_connected(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    for pair in path.windows(2) {
        let moved = move_to(db, job, (pair[0], pair[1]), at, Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{pair:?}");
    }
}

fn vault() -> Vault {
    let key = |byte: u8| format!("1:{}", STANDARD.encode([byte; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x5a).into(), &key(0x6a).into(), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn text_in(contact: Uuid, words: &str, at: Timestamp) -> TextIn<'_> {
    let inbound = Words {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: at,
    }
}

/// Her fake contact texts in a job, walked to Connected at `connected`, with a paid pick of a
/// fake business and an email contact choice (as T277's `connected_job` and `picked`): (her, job).
async fn email_contact_job(db: &mut PgConnection, connected: Timestamp) -> (Uuid, Uuid) {
    let new = NewContact {
        name: Some(SecretString::from("Rosa Fakename")),
        phone: Some(SecretString::from("+19285550171".to_owned())),
        email: Some(SecretString::from("rosa@example.test")),
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let at = connected - SignedDuration::from_hours(2);
    let first = text_in(her, words, at);
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at,
    };
    let Ok(Opened::Created(id)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    walk_to_connected(db, id, connected).await;
    let business = askhava_businesses::api::create(db, false, at)
        .await
        .expect("business");
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(id).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    record_contact_choice(db, pick, ContactChoice::Email, at)
        .await
        .expect("choice");
    (her, id)
}

/// TD-OPS-08 (OPS-031, FEE-035, D-4053): her texted day-3 NO under an email contact choice writes
/// one `day3_no` row and queues one `day3_no` step; a second NO adds neither; the job stays
/// Connected and counts as NO-or-OTHER for its connection; no contact ID is in the result.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_a_texted_email_contact_day3_no_is_recorded_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let connected = Timestamp::now() - SignedDuration::from_hours(4 * 24);
    let (her, id) = email_contact_job(&mut db, connected).await;
    for _ in 0..2 {
        let no = text_in(her, "NO", Timestamp::now() - SignedDuration::from_mins(1));
        let got = receive_reply(&mut db, &secret(), &no, (Settings::default(), &[])).await;
        assert!(matches!(got, Ok(Reply::CheckedIn(j)) if j == id));
        assert_eq!(rows(&mut db, id).await, [Some("day3_no".to_owned())]);
        let steps: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = 'picks.unreachable_step' AND payload->>'step' = 'day3_no'")
            .fetch_one(&mut *db).await.expect("steps");
        assert_eq!(steps, 1);
    }
    let status = askhava_jobs::api::current(&mut db, id)
        .await
        .expect("job")
        .status;
    assert!(status.takes_check_ins(), "still Connected");
    let got = read(&mut db, &[id], around(connected)).await;
    assert!(
        !format!("{got:?}").contains(&her.to_string()),
        "no contact ID"
    );
    assert_eq!(ids(got), (vec![id], vec![id]));
}
