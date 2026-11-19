//! The queued-email drain, `messaging.email_due`, on a fresh database with a recording email
//! adapter (T470; SEC-025, OPS-038, SEC-018, PRIV-016, BIZ-019, BIZ-043, BIZ-051, SEC-024; D-3750,
//! D-3889, D-4196, D-4327, D-4670 to D-4673). Every address, ID and body is fake.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::{
    Acting, PHONE_STOPPED_EMAIL, Roles, WELCOME_EMAIL, add_member, add_owner, create, leave,
    remove_member,
};
use askhava_core::api::{Claimed, Clock};
use askhava_messaging::api::{
    ALERT_EMAIL_TEMPLATE, CODE_MARK, COVERAGE_EMAIL_TEMPLATE, DailyLimit, EMAIL_DUE_KIND,
    EXPIRED_ALERT, EmailDrain, EmailSent, EmailTo, GIVEN_UP_ALERT, JOB_CARD_TEMPLATE, LINK_MARK,
    MARKED_ALERT, OWN_RETRY, OWNER_INBOX, REFUSED_ALERT, SELF_SENDING, SETUP_ALERT,
    SIGNIN_LINK_TEMPLATE, run_email_due, send_email,
};
use askhava_privacy::api::{NewContact, Vault, mailbox_for, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tokio::sync::Notify;

/// The composition's resolver, compiled in place (the server crate is not a dependency).
#[path = r"../../../app/server/src/email_due.rs"]
mod server_email_due;
use server_email_due::NoticeAddresses;

const OWNER: &str = "pat.owner@example.test";
const MEMBER: &str = "lee.member@example.test";
const BACKUP: &str = "pat.backup@example.test";
const HOMEOWNER: &str = "ada.example@example.test";
const INBOX: &str = "owner.inbox@example.test";

type Drain = EmailDrain<Rec, NoticeAddresses>;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + secs).expect("time")
}

fn mins(n: i64) -> i64 {
    n * 60
}

/// One recorded provider call: key, address, subject.
type Call = (String, String, String);

/// A recording adapter answering as planted (`Ok` when nothing is planted); with a gate, the
/// first call says so and waits to be let go.
#[derive(Clone, Default)]
struct Rec {
    calls: Arc<Mutex<Vec<Call>>>,
    answers: Arc<Mutex<VecDeque<AdapterError>>>,
    gate: Option<Arc<(Notify, Notify)>>,
}

impl Rec {
    fn plant(&self, answers: impl IntoIterator<Item = AdapterError>) {
        self.answers.lock().expect("lock").extend(answers);
    }
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("lock").clone()
    }
}

impl EmailAdapter for Rec {
    async fn send_email(
        &self,
        key: &IdempotencyKey,
        email: &Email<'_>,
    ) -> Result<String, AdapterError> {
        let n = {
            let mut calls = self.calls.lock().expect("lock");
            let call = (
                key.as_str().to_owned(),
                email.to.to_owned(),
                email.subject.to_owned(),
            );
            calls.push(call);
            calls.len()
        };
        if let Some(gate) = self.gate.as_ref().filter(|_| n == 1) {
            gate.0.notify_one();
            gate.1.notified().await;
        }
        match self.answers.lock().expect("lock").pop_front() {
            Some(e) => Err(e),
            None => Ok(format!("re_fake_{n}")),
        }
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn vendor(status: u16) -> AdapterError {
    AdapterError::Vendor { status, code: None }
}

/// A settable clock that moves `tick` after each read.
struct TestClock {
    now: Mutex<Timestamp>,
    tick: SignedDuration,
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        let mut now = self.now.lock().expect("lock");
        let read = *now;
        *now = read + self.tick;
        read
    }
}

struct Fix {
    pool: PgPool,
    vault: Arc<Vault>,
    mail: Rec,
    clock: Arc<TestClock>,
    drain: Arc<Drain>,
}

impl Fix {
    fn new(pool: &PgPool, mail: Rec, tick: SignedDuration) -> Self {
        let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
        let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
        let vault = Arc::new(Vault::new(&key(0x51), &key(0x61), &url).expect("vault"));
        let clock = Arc::new(TestClock {
            now: Mutex::new(at(0)),
            tick,
        });
        let resolver = NoticeAddresses::new(Arc::clone(&vault), Some(secret(INBOX)));
        let keys = (secret("fake-id-secret-for-tests"), mail.clone(), limit());
        let as_dyn: Arc<dyn Clock> = clock.clone();
        let drain = EmailDrain::new(keys, (Arc::clone(&vault), resolver), as_dyn);
        Self {
            pool: pool.clone(),
            vault,
            mail,
            clock,
            drain: Arc::new(drain),
        }
    }
    fn plain(pool: &PgPool) -> Self {
        Self::new(pool, Rec::default(), SignedDuration::ZERO)
    }
    fn set(&self, secs: i64) {
        *self.clock.now.lock().expect("lock") = at(secs);
    }
    async fn run_at(&self, secs: i64) {
        self.set(secs);
        run(&self.pool, Some(Arc::clone(&self.drain))).await;
    }
    async fn db(&self) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
        self.pool.acquire().await.expect("db")
    }
}

fn secret(v: &str) -> SecretString {
    SecretString::from(v.to_owned())
}

fn limit() -> DailyLimit {
    DailyLimit::new(100, 90).expect("limit")
}

async fn run(pool: &PgPool, drain: Option<Arc<Drain>>) {
    let task = Claimed {
        id: 1,
        kind: EMAIL_DUE_KIND.to_owned(),
        payload: serde_json::json!({}),
        attempts: 1,
    };
    run_email_due(pool.clone(), drain, task).await.expect("run");
}

/// A queued row: template, recipient kind and ID, subjects, class and body, queued at `queued`.
async fn row(
    db: &mut PgConnection,
    (template, kind, to, subjects): (&str, &str, Uuid, &[Uuid]),
    (class, body): (&str, &str),
    queued: i64,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, subject, html, plain, not_before, queued_at) VALUES \
         (encode(sha256(gen_random_uuid()::text::bytea), 'hex'), $1, $2, $3, $4, $5, \
         'Fake subject', $6, $6, to_timestamp($7), to_timestamp($7)) RETURNING id",
    )
    .bind(template)
    .bind(to)
    .bind(kind)
    .bind(subjects)
    .bind(class)
    .bind(body)
    .bind(at(queued).as_second() as f64)
    .fetch_one(db)
    .await
    .expect("row")
}

async fn card(db: &mut PgConnection, to: Uuid, business: Uuid, queued: i64) -> Uuid {
    let ids = (
        JOB_CARD_TEMPLATE,
        "business",
        to,
        &[business, Uuid::new_v4()][..],
    );
    row(db, ids, ("notice", "A fake job card"), queued).await
}

async fn exec(db: &mut PgConnection, sql: &'static str, id: Uuid, secs: i64) {
    sqlx::query(sql)
        .bind(id)
        .bind(at(secs).as_second() as f64)
        .execute(db)
        .await
        .expect("update");
}

/// Status, lease count, `first_unsure_at`, `not_before` and `claimed_at` (seconds after `at(0)`).
async fn state(db: &mut PgConnection, id: Uuid) -> (String, i16, Option<i64>, i64, Option<i64>) {
    let base = at(0).as_second();
    let r: (String, i16, Option<i64>, i64, Option<i64>) = sqlx::query_as(
        "SELECT status, lease_count, extract(epoch FROM first_unsure_at)::int8, \
         extract(epoch FROM not_before)::int8, extract(epoch FROM claimed_at)::int8 \
         FROM messaging.email_outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db)
    .await
    .expect("state");
    (
        r.0,
        r.1,
        r.2.map(|t| t - base),
        r.3 - base,
        r.4.map(|t| t - base),
    )
}

async fn status(db: &mut PgConnection, id: Uuid) -> String {
    state(db, id).await.0
}

async fn alerts(db: &mut PgConnection, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(kind)
        .fetch_one(db)
        .await
        .expect("alerts")
}

/// A business with its owner at `OWNER` and a member at `MEMBER`: (business, owner, member,
/// owner mailbox, member mailbox).
async fn team(db: &mut PgConnection, vault: &Vault) -> (Uuid, Acting, Uuid, Uuid, Uuid) {
    let b = create(db, false, at(0)).await.expect("business");
    let (om, mm) = (
        mailbox(db, vault, OWNER).await,
        mailbox(db, vault, MEMBER).await,
    );
    let owner = Acting {
        business: b,
        member: add_owner(db, b, om, at(0)).await.expect("owner"),
    };
    let member = add_member(db, owner, mm, Roles::BOTH, at(0))
        .await
        .expect("member");
    (b, owner, member, om, mm)
}

async fn mailbox(db: &mut PgConnection, vault: &Vault, address: &str) -> Uuid {
    mailbox_for(db, vault, address).await.expect("mailbox")
}

async fn homeowner(db: &mut PgConnection, vault: &Vault, email: Option<&str>) -> Uuid {
    let new = NewContact {
        name: Some(secret("Ada Example")),
        phone: Some(secret("+19285550142")),
        email: email.map(secret),
        address: None,
        onsite_contact: None,
    };
    store_contact(db, vault, &new).await.expect("contact")
}

/// A subscriber keeping every event's and span's fields as text, to search for addresses.
struct Capture(Arc<Mutex<Vec<String>>>);
struct Line(String);

impl tracing::field::Visit for Line {
    fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
        let _ = write!(self.0, "{}={v:?} ", f.name());
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, s: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut line = Line(String::new());
        s.record(&mut line);
        self.0.lock().expect("lock").push(line.0);
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, r: &tracing::span::Record<'_>) {
        let mut line = Line(String::new());
        r.record(&mut line);
        self.0.lock().expect("lock").push(line.0);
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut line = Line(String::new());
        e.record(&mut line);
        self.0.lock().expect("lock").push(line.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A queued homeowner email and a queued business notice each go once to the right address, the
/// next run is queued, a second run sends nothing, and no address reaches any log line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_queued_email_goes_once_to_its_address_and_no_address_is_logged(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let her = homeowner(&mut db, &f.vault, Some(HOMEOWNER)).await;
    let mine = row(
        &mut db,
        ("pick_lapsed", "homeowner", her, &[b]),
        ("notice", "Fake"),
        0,
    )
    .await;
    let theirs = card(&mut db, om, b, 0).await;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let guard = tracing::subscriber::set_default(Capture(Arc::clone(&lines)));
    f.run_at(1).await;
    f.run_at(mins(3)).await;
    drop(guard);
    let to: Vec<String> = f.mail.calls().into_iter().map(|c| c.1).collect();
    assert_eq!(to.len(), 2, "{to:?}");
    assert!(to.contains(&HOMEOWNER.to_owned()) && to.contains(&OWNER.to_owned()));
    for id in [mine, theirs] {
        assert_eq!(status(&mut db, id).await, "sent");
    }
    let next: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(EMAIL_DUE_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(next, 2, "each run queues its next");
    let logged = lines.lock().expect("lock").join("\n");
    for address in [HOMEOWNER, OWNER] {
        assert!(!logged.contains(address), "an address was logged");
    }
}

/// BIZ-043: a member's verified backup gets the notice queued to it; a removed backup does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_verified_backup_gets_its_notice_and_a_removed_one_does_not(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, owner, member, _, _) = team(&mut db, &f.vault).await;
    let backup = mailbox(&mut db, &f.vault, BACKUP).await;
    let gone = mailbox(&mut db, &f.vault, "old.backup@example.test").await;
    for (who, mb, removed) in [(owner.member, backup, false), (member, gone, true)] {
        sqlx::query(
            "INSERT INTO businesses.backup_address (business_id, member_id, mailbox_id, \
             requested_by, sent_at, verified_at, removed_at) VALUES ($1, $2, $3, $2, now(), now(), \
             CASE WHEN $4 THEN now() END)",
        )
        .bind(b)
        .bind(who)
        .bind(mb)
        .bind(removed)
        .execute(&mut *db)
        .await
        .expect("backup");
    }
    let (to_backup, to_gone) = (
        card(&mut db, backup, b, 0).await,
        card(&mut db, gone, b, 0).await,
    );
    f.run_at(1).await;
    let to: Vec<String> = f.mail.calls().into_iter().map(|c| c.1).collect();
    assert_eq!(to, [BACKUP.to_owned()]);
    assert_eq!(status(&mut db, to_backup).await, "sent");
    assert_eq!(status(&mut db, to_gone).await, "dropped");
}

/// BIZ-051, SEC-024: a notice to a removed member or a business that left is dropped unsent; a
/// mailbox shared by two businesses gets only its current one's notices; a row naming no
/// business is dropped.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn notices_to_a_removed_member_or_a_business_that_left_are_dropped(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, owner, member, _, mm) = team(&mut db, &f.vault).await;
    let other = create(&mut db, false, at(0)).await.expect("business");
    add_owner(&mut db, other, mm, at(0)).await.expect("owner");
    let (stays, left_one) = (
        card(&mut db, mm, other, 0).await,
        card(&mut db, mm, b, 0).await,
    );
    let nobody = card(&mut db, mm, Uuid::new_v4(), 0).await;
    remove_member(&mut db, owner, member, at(1))
        .await
        .expect("removed");
    let third = create(&mut db, false, at(0)).await.expect("business");
    let pat = mailbox(&mut db, &f.vault, "sam.left@example.test").await;
    add_owner(&mut db, third, pat, at(0)).await.expect("owner");
    let left = card(&mut db, pat, third, 0).await;
    leave(&mut db, third, at(1)).await.expect("left");
    f.run_at(2).await;
    let to: Vec<String> = f.mail.calls().into_iter().map(|c| c.1).collect();
    assert_eq!(
        to,
        [MEMBER.to_owned()],
        "only the current business's notice"
    );
    assert_eq!(status(&mut db, stays).await, "sent");
    for id in [left_one, nobody, left] {
        assert_eq!(status(&mut db, id).await, "dropped");
    }
}

/// Two workers' leases over the same rows, run at once, make one provider call per row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn two_workers_at_once_make_one_call_per_row(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    for _ in 0..5 {
        card(&mut db, om, b, 0).await;
    }
    f.set(1);
    let (one, two) = (Some(Arc::clone(&f.drain)), Some(Arc::clone(&f.drain)));
    tokio::join!(run(&pool, one), run(&pool, two));
    let keys: std::collections::BTreeSet<String> =
        f.mail.calls().into_iter().map(|c| c.0).collect();
    assert_eq!((f.mail.calls().len(), keys.len()), (5, 5));
}

/// A second worker that runs after the first committed its lease but before its send finished
/// takes none of the first's rows.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_second_worker_mid_send_takes_none_of_the_leased_rows(pool: PgPool) {
    let gate = Arc::new((Notify::new(), Notify::new()));
    let mail = Rec {
        gate: Some(Arc::clone(&gate)),
        ..Rec::default()
    };
    let f = Fix::new(&pool, mail, SignedDuration::ZERO);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    for _ in 0..3 {
        card(&mut db, om, b, 0).await;
    }
    f.set(1);
    let second = async {
        gate.0.notified().await;
        run(&pool, Some(Arc::clone(&f.drain))).await;
        let during = f.mail.calls().len();
        gate.1.notify_one();
        during
    };
    let (_, during) = tokio::join!(run(&pool, Some(Arc::clone(&f.drain))), second);
    assert_eq!(during, 1, "the second worker sent nothing");
    assert_eq!(f.mail.calls().len(), 3);
}

/// A `sending` row under a fresh lease is not taken; under a stale one it is, with the same key.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_fresh_lease_is_kept_and_a_stale_one_is_retaken_under_the_same_key(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let id = card(&mut db, om, b, 0).await;
    f.mail.plant([vendor(500)]);
    f.run_at(0).await;
    let lease = "UPDATE messaging.email_outbox SET status = 'sending', claimed_at = to_timestamp($2) WHERE id = $1";
    exec(&mut db, lease, id, mins(9)).await;
    f.run_at(mins(10)).await;
    assert_eq!(f.mail.calls().len(), 1, "a fresh lease is not taken");
    f.run_at(mins(14) + 1).await;
    let calls = f.mail.calls();
    assert_eq!(calls.len(), 2, "a stale lease is taken");
    assert_eq!(calls[0].0, calls[1].0, "the same key");
    assert_eq!(status(&mut db, id).await, "sent");
}

/// A 429 holds the row with no lease and no unsure mark; it waits for its `not_before` and goes;
/// a `dropped` row is never held or sent; a later `not_before` waits.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_held_row_waits_for_its_time_and_a_dropped_row_never_goes(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let held = card(&mut db, om, b, 0).await;
    let later = card(&mut db, om, b, 0).await;
    exec(
        &mut db,
        "UPDATE messaging.email_outbox SET not_before = to_timestamp($2) WHERE id = $1",
        later,
        mins(30),
    )
    .await;
    let dropped = row(
        &mut db,
        ("open_cards", "business", om, &[b]),
        ("bulk", "Fake"),
        0,
    )
    .await;
    let drop = "UPDATE messaging.email_outbox SET status = 'dropped', subject = NULL, html = NULL, \
                plain = NULL WHERE id = $1 AND $2::float8 IS NOT NULL";
    exec(&mut db, drop, dropped, 0).await;
    f.mail.plant([vendor(429)]);
    f.run_at(0).await;
    let s = state(&mut db, held).await;
    assert_eq!((s.0.as_str(), s.2, s.3, s.4), ("queued", None, 60, None));
    f.run_at(30).await;
    assert_eq!(f.mail.calls().len(), 1, "it waits for its time");
    f.run_at(61).await;
    assert_eq!(status(&mut db, held).await, "sent");
    assert_eq!(
        status(&mut db, later).await,
        "queued",
        "a later not_before waits"
    );
    let full = DailyLimit::new(1, 1).expect("limit");
    let keys = (&secret("fake-id-secret-for-tests"), &f.mail, full);
    let again = send_email(&mut db, keys, dropped, EmailTo::Business(OWNER), at(62)).await;
    assert!(!matches!(again, Ok(EmailSent::Sent(_))), "{again:?}");
    let s = state(&mut db, dropped).await;
    assert_eq!((s.0.as_str(), s.3), ("dropped", 0), "never held");
    f.run_at(mins(31)).await;
    assert_eq!(f.mail.calls().len(), 3, "the dropped row never goes");
}

/// OPS-038: above the hold share a bulk row waits for the reset while a job card still goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_bulk_row_waits_above_the_hold_share_while_a_job_card_goes(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    sqlx::query(
        "INSERT INTO messaging.email_outbox (idempotency_key, template_id, recipient_id, \
         recipient_kind, subject_ids, class, status, provider_id, not_before, queued_at, sent_at) \
         SELECT encode(sha256(n::text::bytea), 'hex'), 'job_card_notice', gen_random_uuid(), \
         'business', '{}', 'notice', 'sent', 're_old', to_timestamp($1), to_timestamp($1), \
         to_timestamp($1) FROM generate_series(1, 91) AS n",
    )
    .bind(at(0).as_second() as f64)
    .execute(&mut *db)
    .await
    .expect("sent today");
    let bulk = row(
        &mut db,
        ("open_cards", "business", om, &[b]),
        ("bulk", "Fake"),
        0,
    )
    .await;
    let job = card(&mut db, om, b, 0).await;
    f.run_at(1).await;
    assert_eq!(f.mail.calls().len(), 1);
    assert_eq!(status(&mut db, job).await, "sent");
    let s = state(&mut db, bulk).await;
    assert_eq!(s.0, "queued");
    assert!(s.3 > mins(1), "held until the reset");
}

/// D-4196: a self-sending row is never taken unheld; a held welcome goes once after its time; a
/// held coverage notice goes to the owner inbox; a held sign-in link is dropped with no alert;
/// an owner alert row is never taken, held or not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn self_sending_rows_go_only_when_held_and_owner_alerts_never(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let unheld = row(
        &mut db,
        (PHONE_STOPPED_EMAIL, "business", om, &[b]),
        ("notice", "Fake"),
        0,
    )
    .await;
    let welcome = row(
        &mut db,
        (WELCOME_EMAIL, "business", om, &[b]),
        ("notice", "Fake"),
        0,
    )
    .await;
    let cover = row(
        &mut db,
        (COVERAGE_EMAIL_TEMPLATE, "business", OWNER_INBOX, &[b]),
        ("notice", "Fake"),
        0,
    )
    .await;
    let link = format!("Your link: {LINK_MARK}");
    let signin = row(
        &mut db,
        (SIGNIN_LINK_TEMPLATE, "business", om, &[b]),
        ("notice", &link),
        0,
    )
    .await;
    let alert = row(
        &mut db,
        (ALERT_EMAIL_TEMPLATE, "business", om, &[b]),
        ("notice", "Fake"),
        0,
    )
    .await;
    let hold = "UPDATE messaging.email_outbox SET waiting_for = 'daily_limit', \
                not_before = to_timestamp($2) WHERE id = $1";
    for id in [welcome, cover, signin, alert] {
        exec(&mut db, hold, id, mins(10)).await;
    }
    f.run_at(mins(5)).await;
    assert!(f.mail.calls().is_empty(), "nothing before its time");
    f.run_at(mins(11)).await;
    f.run_at(mins(20)).await;
    let to: Vec<String> = f.mail.calls().into_iter().map(|c| c.1).collect();
    assert_eq!(to.len(), 2, "{to:?}");
    assert!(to.contains(&OWNER.to_owned()) && to.contains(&INBOX.to_owned()));
    for (id, want) in [
        (welcome, "sent"),
        (cover, "sent"),
        (signin, "dropped"),
        (unheld, "queued"),
        (alert, "queued"),
    ] {
        assert_eq!(status(&mut db, id).await, want);
    }
    assert_eq!(
        alerts(&mut db, MARKED_ALERT).await,
        0,
        "an expired link raises nothing"
    );
}

/// SEC-024, D-4671: a held "notice address changed" email to the member's old address, which no
/// member has any more, goes once after its time; one for a business that left is dropped.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_held_address_changed_email_goes_to_the_old_address(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, owner, _, om, _) = team(&mut db, &f.vault).await;
    let moved_to = mailbox(&mut db, &f.vault, "pat.new@example.test").await;
    sqlx::query("UPDATE businesses.team_member SET mailbox_id = $2 WHERE id = $1")
        .bind(owner.member)
        .bind(moved_to)
        .execute(&mut *db)
        .await
        .expect("moved");
    let gone = create(&mut db, false, at(0)).await.expect("business");
    let sam = mailbox(&mut db, &f.vault, "sam.left@example.test").await;
    let changed = "business_address_changed";
    let (told, left) = (
        row(
            &mut db,
            (changed, "business", om, &[b, Uuid::new_v4()]),
            ("notice", "Fake"),
            0,
        )
        .await,
        row(
            &mut db,
            (changed, "business", sam, &[gone, Uuid::new_v4()]),
            ("notice", "Fake"),
            0,
        )
        .await,
    );
    leave(&mut db, gone, at(1)).await.expect("left");
    let hold = "UPDATE messaging.email_outbox SET waiting_for = 'daily_limit', \
                not_before = to_timestamp($2) WHERE id = $1";
    for id in [told, left] {
        exec(&mut db, hold, id, mins(10)).await;
    }
    f.run_at(mins(5)).await;
    assert!(f.mail.calls().is_empty(), "nothing before its time");
    f.run_at(mins(11)).await;
    f.run_at(mins(20)).await;
    let to: Vec<String> = f.mail.calls().into_iter().map(|c| c.1).collect();
    assert_eq!(to, [OWNER.to_owned()], "once, to the old address");
    assert_eq!(status(&mut db, told).await, "sent");
    assert_eq!(status(&mut db, left).await, "dropped");
}

/// The self-sending list names every handler's own template.
#[test]
fn the_self_sending_list_names_every_own_sender() {
    for t in [
        WELCOME_EMAIL,
        PHONE_STOPPED_EMAIL,
        SIGNIN_LINK_TEMPLATE,
        COVERAGE_EMAIL_TEMPLATE,
    ] {
        assert!(SELF_SENDING.contains(&t), "{t}");
    }
    assert!(OWN_RETRY.iter().all(|t| SELF_SENDING.contains(t)));
    assert!(!SELF_SENDING.contains(&JOB_CARD_TEMPLATE));
}

/// A row holding `LINK_MARK` or `CODE_MARK` is dropped with one alert and never sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_marked_row_is_dropped_with_one_alert_and_never_sent(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let mut ids = Vec::new();
    for mark in [LINK_MARK, CODE_MARK] {
        let body = format!("Fake card {mark}");
        ids.push(
            row(
                &mut db,
                (JOB_CARD_TEMPLATE, "business", om, &[b]),
                ("notice", &body),
                0,
            )
            .await,
        );
    }
    f.run_at(1).await;
    f.run_at(mins(5)).await;
    assert!(f.mail.calls().is_empty());
    for id in ids {
        assert_eq!(status(&mut db, id).await, "dropped");
    }
    assert_eq!(alerts(&mut db, MARKED_ALERT).await, 2);
}

/// With no provider a run leases, sends and marks nothing and queues no next run; a homeowner
/// with no address on file is dropped (D-3889).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn with_no_provider_nothing_changes_and_no_address_drops(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let her = homeowner(&mut db, &f.vault, None).await;
    let id = row(
        &mut db,
        ("pick_lapsed", "homeowner", her, &[her]),
        ("notice", "Fake"),
        0,
    )
    .await;
    run(&pool, None).await;
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(EMAIL_DUE_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("tasks");
    assert_eq!(
        (state(&mut db, id).await, tasks),
        (("queued".to_owned(), 0, None, 0, None), 0)
    );
    f.run_at(1).await;
    assert_eq!(status(&mut db, id).await, "dropped");
    assert!(f.mail.calls().is_empty());
}

/// D-4327: a 400 and a 422 each drop the row after one call with one alert; no later call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_400_or_422_drops_the_row_with_one_alert(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let ids = [card(&mut db, om, b, 0).await, card(&mut db, om, b, 1).await];
    f.mail.plant([vendor(400), vendor(422)]);
    f.run_at(2).await;
    f.run_at(mins(90)).await;
    assert_eq!(f.mail.calls().len(), 2);
    for id in ids {
        assert_eq!(status(&mut db, id).await, "dropped");
    }
    assert_eq!(alerts(&mut db, REFUSED_ALERT).await, 2);
}

/// D-4327: 500s over 22 h 59 min keep the row backing off under one key, its first unsure time
/// the first lease's start; at 23 hours it is dropped with one alert and no call. A 409 and a
/// timeout back off alike, and a success after back-offs resets the count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn unsure_answers_back_off_until_the_row_is_given_up(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let id = card(&mut db, om, b, 0).await;
    let mut runs: Vec<i64> = (0..22).map(|h| mins(60 * h)).collect();
    runs.push(mins(22 * 60 + 59));
    f.mail.plant(runs.iter().map(|_| vendor(500)));
    for (n, t) in runs.iter().enumerate() {
        f.run_at(*t).await;
        assert_eq!(f.mail.calls().len(), n + 1, "run {n}");
    }
    let s = state(&mut db, id).await;
    assert_eq!((s.0.as_str(), s.2), ("queued", Some(0)));
    let keys: std::collections::BTreeSet<String> =
        f.mail.calls().into_iter().map(|c| c.0).collect();
    assert_eq!(keys.len(), 1, "one key");
    f.run_at(mins(23 * 60 + 59)).await;
    assert_eq!(
        (f.mail.calls().len(), status(&mut db, id).await.as_str()),
        (23, "dropped")
    );
    assert_eq!(alerts(&mut db, GIVEN_UP_ALERT).await, 1);
    let other = card(&mut db, om, b, 0).await;
    f.mail.plant([vendor(409), AdapterError::OutcomeUnknown]);
    f.run_at(mins(24 * 60)).await;
    assert_eq!(state(&mut db, other).await.1, 1);
    f.run_at(mins(24 * 60 + 1)).await;
    let s = state(&mut db, other).await;
    assert_eq!((s.0.as_str(), s.1, s.2), ("queued", 2, Some(mins(24 * 60))));
    f.run_at(mins(24 * 60 + 3)).await;
    let s = state(&mut db, other).await;
    assert_eq!(
        (s.0.as_str(), s.1),
        ("sent", 0),
        "a success resets the count"
    );
}

/// D-4327: a 500 stores the lease's start, not the answer's time; a stale `sending` row whose
/// lease began 23 hours ago is dropped with one alert and no call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_unsure_time_is_the_lease_start_and_an_old_stale_lease_is_given_up(pool: PgPool) {
    let f = Fix::new(&pool, Rec::default(), SignedDuration::from_secs(7));
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let id = card(&mut db, om, b, 0).await;
    f.mail.plant([vendor(503)]);
    f.run_at(100).await;
    assert_eq!(state(&mut db, id).await.2, Some(100), "the lease start");
    f.run_at(mins(5)).await;
    assert_eq!(status(&mut db, id).await, "sent");
    let old = card(&mut db, om, b, 0).await;
    let lease = "UPDATE messaging.email_outbox SET status = 'sending', claimed_at = to_timestamp($2) WHERE id = $1";
    exec(&mut db, lease, old, mins(60)).await;
    f.run_at(mins(23 * 60 + 61)).await;
    assert_eq!(status(&mut db, old).await, "dropped");
    assert_eq!(
        state(&mut db, old).await.2,
        Some(mins(60)),
        "the old lease time"
    );
    assert_eq!(alerts(&mut db, GIVEN_UP_ALERT).await, 1);
    assert_eq!(f.mail.calls().len(), 2, "no call for the old lease");
}

/// D-4327: a 401 on three rows backs all three off with one setup alert, and they go once the
/// provider answers 200; a hold keeps an earlier unsure time and sets none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_setup_fault_backs_off_with_one_alert_and_holds_keep_the_unsure_time(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(card(&mut db, om, b, 0).await);
    }
    f.mail.plant([vendor(401), vendor(401), vendor(401)]);
    f.run_at(0).await;
    for &id in &ids {
        let s = state(&mut db, id).await;
        assert_eq!((s.0.as_str(), s.1, s.2), ("queued", 1, None));
    }
    assert_eq!(alerts(&mut db, SETUP_ALERT).await, 1);
    f.run_at(mins(2)).await;
    for &id in &ids {
        assert_eq!(status(&mut db, id).await, "sent");
    }
    let unsure = card(&mut db, om, b, 0).await;
    f.mail.plant([vendor(500), vendor(429)]);
    f.run_at(mins(3)).await;
    f.run_at(mins(5)).await;
    let s = state(&mut db, unsure).await;
    assert_eq!(
        (s.0.as_str(), s.1, s.2),
        ("queued", 0, Some(mins(3))),
        "kept, count reset"
    );
}

/// D-4327: a row answered only 401 is dropped with one expiry alert once queued 72 hours, and
/// not at 71 hours 59 minutes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_row_with_no_unsure_call_expires_at_72_hours(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let young = card(&mut db, om, b, mins(1)).await;
    let old = card(&mut db, om, b, 0).await;
    f.mail.plant([vendor(401), vendor(401)]);
    f.run_at(mins(72 * 60)).await;
    assert_eq!(status(&mut db, old).await, "dropped");
    assert_eq!(state(&mut db, young).await.0, "queued");
    assert_eq!(
        f.mail.calls().len(),
        1,
        "only the 71 h 59 min row was tried"
    );
    assert_eq!(alerts(&mut db, EXPIRED_ALERT).await, 1);
}

/// A run stops starting sends after its budget and gives back the leases it did not reach.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_run_past_its_budget_gives_back_the_rest(pool: PgPool) {
    let f = Fix::new(&pool, Rec::default(), SignedDuration::from_secs(20));
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let mut ids = Vec::new();
    for n in 0..6 {
        ids.push(card(&mut db, om, b, n).await);
    }
    f.run_at(10).await;
    let sent = f.mail.calls().len();
    assert!((1..6).contains(&sent), "{sent}");
    let s = state(&mut db, ids[5]).await;
    assert_eq!(
        (s.0.as_str(), s.4),
        ("queued", None),
        "given back with no lease"
    );
}

/// FEE-031: a receipt names only its pick; it goes to the pick's business's current member and is
/// dropped for a mailbox that is not one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_receipt_naming_only_its_pick_goes_to_that_business(pool: PgPool) {
    let f = Fix::plain(&pool);
    let mut db = f.db().await;
    let (b, _, _, om, _) = team(&mut db, &f.vault).await;
    let stranger = mailbox(&mut db, &f.vault, "not.a.member@example.test").await;
    let pick: Uuid = sqlx::query_scalar(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES (gen_random_uuid(), $1, 1, 'page', 'awaiting_confirm', now()) RETURNING id",
    )
    .bind(b)
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    let mut ids = Vec::new();
    for to in [om, stranger] {
        ids.push(
            row(
                &mut db,
                ("charge_receipt", "business", to, &[pick]),
                ("notice", "Fake"),
                0,
            )
            .await,
        );
    }
    f.run_at(1).await;
    let to: Vec<String> = f.mail.calls().into_iter().map(|c| c.1).collect();
    assert_eq!(to, [OWNER.to_owned()]);
    assert_eq!(status(&mut db, ids[0]).await, "sent");
    assert_eq!(status(&mut db, ids[1]).await, "dropped");
}
