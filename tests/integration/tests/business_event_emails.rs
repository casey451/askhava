//! The business account event emails through the worker (TD-BIZ-45: SEC-024, BIZ-033, BIZ-040,
//! BIZ-042, BIZ-007, BIZ-050; T329, D-2287, D-2706, D-3112, D-3687): each queued kind reaches a
//! fake email provider once, with its words, only to the people its spec line names, in English
//! and in Spanish, and a repeat run sends nothing. Every address, name, number and ID is fake.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, Email, EmailAdapter, IdempotencyKey};
use askhava_businesses::api::JobTypeChoice;
use askhava_businesses::api::{
    AFTER_HOURS_OFF_KIND, AccountMailer, Acting, BACKUP_CHANGED_KIND, CHANGED_KIND,
    EMERGENCY_TEST_KIND, LICENSE_CHANGE_KIND, NOTICE_KINDS, NoticeParts, OFF_REMOVED,
    OFF_SUSPENDED, Reapplied, Returned, Roles, SAMPLE_PICK_KIND, SenderLink, TeamError, add_member,
    add_owner, address_changed_words, after_hours_off_words, approve_phone, backup_changed_words,
    create, emergency_test_words, license_change_words, link_phone, mark_returned,
    phone_linked_words, reject_phone, remove_member, returned_words, run_notice_email,
    sample_card_words, sample_pick_words, send_sample_card, waiting_phones,
};
use askhava_core::api::{
    Claimed, HandlerError, Lease, NewTask, Outcome, Registry, SystemClock, WorkerId, arizona_time,
    brand, claim, enqueue,
};
use askhava_messaging::api::{DailyLimit, MailboxKind};
use askhava_portals::api::{ActionKind, ActionTarget, EMERGENCY_SWITCH_ROUTE, mint_action_link};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_MAILBOX: Uuid = Uuid::from_u128(0xa1);
const MEMBER_MAILBOX: Uuid = Uuid::from_u128(0xa2);
const OLD_MAILBOX: Uuid = Uuid::from_u128(0xa3);
const BACKUP_MAILBOX: Uuid = Uuid::from_u128(0xa4);
const OWNER_ADDRESS: &str = "owner@example.test";
const MEMBER_ADDRESS: &str = "member@example.test";
const OLD_ADDRESS: &str = "old@example.test";
const BACKUP_ADDRESS: &str = "backup@example.test";
const MEMBER_NAME: &str = "Pat Example";
const LAST4: &str = "0142";

/// One email the fake provider took.
#[derive(Clone, Debug)]
struct Took {
    to: String,
    subject: String,
    text: String,
}

/// The fake provider: records each email it took.
#[derive(Default)]
struct FakeMail {
    sent: Mutex<Vec<Took>>,
}

impl EmailAdapter for FakeMail {
    async fn send_email(&self, _: &IdempotencyKey, e: &Email<'_>) -> Result<String, AdapterError> {
        let mut sent = self.sent.lock().expect("lock");
        sent.push(Took {
            to: e.to.to_owned(),
            subject: e.subject.to_owned(),
            text: e.text.to_owned(),
        });
        Ok(format!("re_{:08}", sent.len()))
    }
    async fn remove_suppression(&self, _: &IdempotencyKey, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }
}

/// The composition's parts: fake addresses for four mailboxes, Spanish for the members listed in
/// `spanish`, the member's fake name and digits, the real Approve link and the re-test page.
struct FakeParts {
    spanish: Vec<Uuid>,
}

impl NoticeParts for FakeParts {
    async fn address(
        &self,
        _: &mut PgConnection,
        mailbox: Uuid,
    ) -> Result<Option<SecretString>, HandlerError> {
        let found = match mailbox {
            OWNER_MAILBOX => Some(OWNER_ADDRESS),
            MEMBER_MAILBOX => Some(MEMBER_ADDRESS),
            OLD_MAILBOX => Some(OLD_ADDRESS),
            BACKUP_MAILBOX => Some(BACKUP_ADDRESS),
            _ => None,
        };
        Ok(found.map(|a| SecretString::from(a.to_owned())))
    }
    async fn spanish(&self, _: &mut PgConnection, member: Uuid) -> Result<bool, HandlerError> {
        Ok(self.spanish.contains(&member))
    }
    async fn member_name(
        &self,
        _: &mut PgConnection,
        _: Uuid,
    ) -> Result<Option<String>, HandlerError> {
        Ok(Some(MEMBER_NAME.to_owned()))
    }
    async fn line_last4(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
    ) -> Result<Option<String>, HandlerError> {
        Ok(Some(LAST4.to_owned()))
    }
    async fn approve_link(
        &self,
        db: &mut PgConnection,
        (business, member, line): (Uuid, Uuid, Uuid),
        now: Timestamp,
    ) -> Result<SecretString, HandlerError> {
        let target = ActionTarget {
            kind: ActionKind::Approve,
            object: line,
            member,
            business,
        };
        let ends = now + SignedDuration::from_hours(24);
        mint_action_link(db, target, None, (now, ends))
            .await
            .map_err(|_| HandlerError("no link"))
    }
    fn retest_link(&self) -> String {
        retest()
    }
}

fn retest() -> String {
    format!("https://{}{EMERGENCY_SWITCH_ROUTE}", brand().domain())
}

/// A sender directory that accepts every fake number.
struct AnyPhone;

impl SenderLink for AnyPhone {
    async fn link(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
        _: &SecretString,
    ) -> Result<(), TeamError> {
        Ok(())
    }
}

type Mailer = Arc<AccountMailer<FakeMail, FakeParts>>;

fn mailer(spanish: Vec<Uuid>) -> Mailer {
    Arc::new(AccountMailer {
        secret: SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned()),
        postal: (
            "PO Box 100, Faketown, AZ 00000".to_owned(),
            MailboxKind::PoBox,
        ),
        provider: FakeMail::default(),
        limit: DailyLimit::new(1000, 80).expect("limit"),
        directory: FakeParts { spanish },
        clock: Arc::new(SystemClock),
    })
}

fn registry(m: &Mailer) -> Registry {
    let mut registry = Registry::new();
    for kind in NOTICE_KINDS {
        let m = Arc::clone(m);
        registry.register(kind, move |pool, task| {
            run_notice_email(pool, Some(Arc::clone(&m)), task)
        });
    }
    registry
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

/// Runs every due task through `registry` as the worker would; each event email must finish
/// (other modules' tasks, such as the audit stream's, have no handler here and wait).
async fn run_due(pool: &PgPool, registry: &Registry) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let clock = SystemClock;
    loop {
        let now = Timestamp::now();
        let tasks = claim(pool, owner, now, SignedDuration::from_secs(60), 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            let kind = task.kind.clone();
            let outcome = registry.run(pool, &clock, owner, task, Lease::WORKER).await;
            if NOTICE_KINDS.contains(&kind.as_str()) {
                assert!(matches!(outcome, Ok(Outcome::Done)), "{kind}: {outcome:?}");
            }
        }
    }
}

struct Team {
    business: Uuid,
    owner: Acting,
    member: Acting,
}

async fn team(db: &mut PgConnection) -> Team {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, OWNER_MAILBOX, at(1))
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let member = add_member(db, owner, MEMBER_MAILBOX, Roles::BOTH, at(1))
        .await
        .expect("member");
    for (mailbox, who) in [(OWNER_MAILBOX, owner.member), (MEMBER_MAILBOX, member)] {
        sqlx::query(
            "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, \
             sent_at, window_at, verified_at) VALUES ($1, $2, $3, false, to_timestamp($4), \
             to_timestamp($4), to_timestamp($4))",
        )
        .bind(business)
        .bind(who)
        .bind(mailbox)
        .bind(at(2).as_second())
        .execute(&mut *db)
        .await
        .expect("verified");
    }
    Team {
        business,
        owner,
        member: Acting { business, member },
    }
}

async fn queue(db: &mut PgConnection, kind: &'static str, payload: Value, key: &str) {
    let mut task = NewTask::new(kind, payload, at(3));
    task.dedupe_key = Some(format!("{kind}.{key}"));
    enqueue(db, &task).await.expect("enqueue");
}

const LICENSE: (&str, &str, &str) = ("plumbing_repair", "leak repair", "drain cleaning");

/// Queues all nine kinds: the sample card, the phone link and the returned address through the
/// code that queues them in the app, the rest with the payloads that code writes.
async fn queue_all(db: &mut PgConnection, t: &Team) {
    let (b, member, owner) = (t.business, t.member.member, t.owner.member);
    send_sample_card(db, t.member, at(3)).await.expect("card");
    link_phone(
        db,
        &AnyPhone,
        (t.member, false),
        &SecretString::from("+15555550142".to_owned()),
        at(3),
    )
    .await
    .expect("phone");
    mark_returned(db, (b, MEMBER_MAILBOX), Returned::Hard, at(3))
        .await
        .expect("returned");
    let to = json!([MEMBER_MAILBOX]);
    queue(
        db,
        BACKUP_CHANGED_KIND,
        json!({"business": b, "member": member, "to": to}),
        "1",
    )
    .await;
    let pick =
        json!({"business": b, "member": member, "to": to, "confirm_by": at(300).as_second()});
    queue(db, SAMPLE_PICK_KIND, pick, "1").await;
    queue(
        db,
        CHANGED_KIND,
        json!({"business": b, "member": member, "old": OLD_MAILBOX}),
        "1",
    )
    .await;
    queue(
        db,
        EMERGENCY_TEST_KIND,
        json!({"business": b, "member": member}),
        "1",
    )
    .await;
    for reason in [OFF_REMOVED, OFF_SUSPENDED] {
        let p = json!({"business": b, "member": owner, "reason": reason});
        queue(db, AFTER_HOURS_OFF_KIND, p, reason).await;
    }
    let (c, removed, offered) = LICENSE;
    let p = json!({"business": b, "removed": [[c, removed]], "offered": [[c, offered]],
                   "dropped_classes": ["CR-37"], "paused": false});
    queue(db, LICENSE_CHANGE_KIND, p, "1").await;
}

/// The subject and body each kind must carry and who gets it (`link` stands for the Approve link,
/// checked by its prefix), in the owner's and the member's language.
fn expected((owner_es, member_es): (bool, bool)) -> Vec<([String; 2], Vec<&'static str>)> {
    let (c, removed, offered) = LICENSE;
    let change = Reapplied {
        removed: vec![JobTypeChoice::new(c, removed)],
        offered: vec![JobTypeChoice::new(c, offered)],
        paused: false,
    };
    let time = arizona_time(at(300)).strftime("%-I:%M %p").to_string();
    vec![
        (sample_card_words(member_es), vec![MEMBER_ADDRESS]),
        (sample_pick_words(&time, member_es), vec![MEMBER_ADDRESS]),
        (backup_changed_words(member_es), vec![MEMBER_ADDRESS]),
        (address_changed_words(member_es), vec![OLD_ADDRESS]),
        (emergency_test_words(member_es), vec![MEMBER_ADDRESS]),
        (
            returned_words(owner_es, MEMBER_ADDRESS),
            vec![OWNER_ADDRESS],
        ),
        (
            after_hours_off_words(owner_es, false, &retest()),
            vec![OWNER_ADDRESS],
        ),
        (
            after_hours_off_words(owner_es, true, &retest()),
            vec![OWNER_ADDRESS],
        ),
        (
            license_change_words(owner_es, &change, &["CR-37"]),
            vec![OWNER_ADDRESS],
        ),
        (
            phone_linked_words(owner_es, (MEMBER_NAME, LAST4), "link"),
            vec![OWNER_ADDRESS],
        ),
    ]
}

fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Checks that every expected email reached exactly its people once, with its words.
fn check(sent: &[Took], langs: (bool, bool), line: Uuid) {
    let approve = format!("https://{}/b/a/approve/{line}/", brand().domain());
    let mut all = 0;
    for ([subject, body], people) in expected(langs) {
        let got: Vec<&Took> = sent.iter().filter(|t| t.subject == subject).collect();
        let to: BTreeSet<&str> = got.iter().map(|t| t.to.as_str()).collect();
        assert_eq!(got.len(), people.len(), "{subject}: {got:?}");
        assert_eq!(to, people.into_iter().collect(), "{subject}");
        for took in &got {
            let text = flat(&took.text);
            let body = flat(&body);
            match body.strip_suffix(" link") {
                Some(before) => {
                    assert!(text.contains(before), "{subject}: {text}");
                    assert!(text.contains(&approve), "{subject}: the Approve link");
                }
                _ => assert!(text.contains(&body), "{subject}: {text}"),
            }
        }
        all += got.len();
    }
    assert_eq!(sent.len(), all, "no other email went out");
}

async fn links(db: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM portals.login_link")
        .fetch_one(db)
        .await
        .expect("links")
}

/// Runs every kind once, then again, with the owner and the member in their own `langs`.
async fn every_kind_once(pool: PgPool, (owner_es, member_es): (bool, bool)) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    queue_all(&mut db, &t).await;
    let who = [(owner_es, t.owner.member), (member_es, t.member.member)];
    let m = mailer(who.iter().filter(|w| w.0).map(|w| w.1).collect());
    let registry = registry(&m);
    run_due(&pool, &registry).await;
    let line: Uuid =
        sqlx::query_scalar("SELECT id FROM businesses.staff_phone WHERE business_id = $1")
            .bind(t.business)
            .fetch_one(&mut *db)
            .await
            .expect("line");
    let first = m.provider.sent.lock().expect("lock").clone();
    check(&first, (owner_es, member_es), line);
    let minted = links(&mut db).await;
    let kinds: BTreeSet<&str> = NOTICE_KINDS.into_iter().collect();
    let rows: Vec<(i64, String, Value)> =
        sqlx::query_as("SELECT id, kind, payload FROM core.task ORDER BY id")
            .fetch_all(&mut *db)
            .await
            .expect("tasks");
    run_due(&pool, &registry).await;
    for (id, kind, payload) in rows.into_iter().filter(|r| kinds.contains(r.1.as_str())) {
        let task = Claimed {
            id,
            kind,
            payload,
            attempts: 2,
        };
        run_notice_email(pool.clone(), Some(Arc::clone(&m)), task)
            .await
            .expect("repeat");
    }
    assert_eq!(
        m.provider.sent.lock().expect("lock").len(),
        first.len(),
        "a repeat sends nothing"
    );
    assert_eq!(
        links(&mut db).await,
        minted,
        "a repeat mints no second link"
    );
}

/// TD-BIZ-45: each kind, in English, once to its people only; a repeat run sends nothing; the
/// suspended after-hours email carries the re-test link.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_each_kind_is_emailed_once_in_english(pool: PgPool) {
    every_kind_once(pool, (false, false)).await;
}

/// TD-BIZ-45, BIZ-018: the same in Spanish, for people who picked it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_each_kind_is_emailed_once_in_spanish(pool: PgPool) {
    every_kind_once(pool, (true, true)).await;
}

/// TD-BIZ-45, BIZ-018: each person gets their own language: the member's emails in Spanish, the
/// owner's in English.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_each_person_gets_their_own_language(pool: PgPool) {
    every_kind_once(pool, (false, true)).await;
}

/// TD-BIZ-45, BIZ-040: when the owner's own main address is returned and the owner has no other
/// address, nobody else on the team is told and the task still finishes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_a_returned_owner_address_tells_no_member(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    mark_returned(
        &mut db,
        (t.business, OWNER_MAILBOX),
        Returned::Complaint,
        at(3),
    )
    .await
    .expect("returned");
    let m = mailer(Vec::new());
    run_due(&pool, &registry(&m)).await;
    assert!(m.provider.sent.lock().expect("lock").is_empty());
}

/// TD-BIZ-45, BIZ-040, D-3682: a returned member address is told to the owner's main address and
/// the owner's verified backup.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_a_returned_address_reaches_the_owners_backup(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    sqlx::query(
        "INSERT INTO businesses.backup_address (business_id, member_id, mailbox_id, \
         requested_by, sent_at, verified_at) VALUES ($1, $2, $3, $2, to_timestamp($4), \
         to_timestamp($4))",
    )
    .bind(t.business)
    .bind(t.owner.member)
    .bind(BACKUP_MAILBOX)
    .bind(at(2).as_second())
    .execute(&mut *db)
    .await
    .expect("backup");
    mark_returned(&mut db, (t.business, MEMBER_MAILBOX), Returned::Hard, at(3))
        .await
        .expect("returned");
    let m = mailer(Vec::new());
    run_due(&pool, &registry(&m)).await;
    let [subject, _] = returned_words(false, MEMBER_ADDRESS);
    let sent = m.provider.sent.lock().expect("lock").clone();
    let to: BTreeSet<&str> = sent
        .iter()
        .filter(|s| s.subject == subject)
        .map(|s| s.to.as_str())
        .collect();
    assert_eq!(to, BTreeSet::from([OWNER_ADDRESS, BACKUP_ADDRESS]));
    assert_eq!(sent.len(), 2, "{sent:?}");
}

/// What happens to the member's waiting phone before the phone-linked email task runs.
#[derive(Clone, Copy)]
enum Before {
    MemberRemoved,
    Approved,
    Declined,
}

/// BIZ-033, JOB-006: the member links a phone, then `before` happens; the task finishes, sends
/// nothing and mints no Approve link.
async fn no_email_once_decided(pool: PgPool, before: Before) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let number = SecretString::from("+15555550142".to_owned());
    link_phone(&mut db, &AnyPhone, (t.member, false), &number, at(3))
        .await
        .expect("phone");
    let waiting = waiting_phones(&mut db, t.business).await.expect("waiting");
    let phone = (waiting[0].line, waiting[0].number_set_at);
    match before {
        Before::MemberRemoved => remove_member(&mut db, t.owner, t.member.member, at(4)).await,
        Before::Approved => approve_phone(&mut db, t.owner, phone, at(4)).await,
        Before::Declined => reject_phone(&mut db, t.owner, phone, at(4)).await,
    }
    .expect("decided");
    let m = mailer(Vec::new());
    run_due(&pool, &registry(&m)).await;
    assert!(m.provider.sent.lock().expect("lock").is_empty());
    assert_eq!(links(&mut db).await, 0, "no Approve link");
}

/// TD-BIZ-45, BIZ-033, D-3640: a member removed before the task runs: no email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_no_phone_email_after_the_member_left(pool: PgPool) {
    no_email_once_decided(pool, Before::MemberRemoved).await;
}

/// TD-BIZ-45, BIZ-033: a phone the owner already approved: no email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_no_phone_email_once_approved(pool: PgPool) {
    no_email_once_decided(pool, Before::Approved).await;
}

/// TD-BIZ-45, BIZ-033: a phone the owner already declined: no email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_no_phone_email_once_declined(pool: PgPool) {
    no_email_once_decided(pool, Before::Declined).await;
}

/// TD-BIZ-45, D-3186: with no sender the task fails and is tried again; nothing is sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_45_no_sender_retries(pool: PgPool) {
    let task = Claimed {
        id: 1,
        kind: EMERGENCY_TEST_KIND.to_owned(),
        payload: json!({"business": Uuid::from_u128(1), "member": Uuid::from_u128(2)}),
        attempts: 1,
    };
    let none = run_notice_email::<FakeMail, FakeParts>(pool, None, task).await;
    assert!(none.is_err());
}
