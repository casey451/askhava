//! D-4052's business account email fixes (T584): a sign-up whose activation fails after a right
//! code keeps the code open, so a retry activates (BIZ-002, BIZ-040, B9); the four account email
//! kinds that went to a main address unchecked skip one that may not get notices and reach the
//! owner's other verified address where the kind allows (BIZ-040, BIZ-055, B30); and the
//! deliverability writes change no row for another business's member (B31). Every address, code,
//! number and ID is fake.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{
    AdapterError, CodeChannel, Email, EmailAdapter, IdempotencyKey, VerifyAdapter,
};
use askhava_businesses::api::{
    AFTER_HOURS_OFF_KIND, AccountMailer, Acting, EMERGENCY_TEST_KIND, InboxAnswer,
    LICENSE_CHANGE_KIND, NOTICE_KINDS, NoticeParts, OFF_REMOVED, RecordContacts, Returned, Roles,
    SenderLink, TeamError, add_member, add_owner, business, check_code, confirm_mailbox, create,
    email_check, emergency_test_words, link_phone, mark_returned, may_get_notices,
    record_inbox_answer, record_match, record_ok_reply, record_pick_alert, returned_words,
    run_notice_email, send_code, start_signup_check,
};
use askhava_core::api::{
    HandlerError, Lease, NewTask, Outcome, Registry, SystemClock, WorkerId, claim, enqueue,
};
use askhava_messaging::api::{DailyLimit, MailboxKind};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const OWNER_MAILBOX: Uuid = Uuid::from_u128(0xb1);
const MEMBER_MAILBOX: Uuid = Uuid::from_u128(0xb2);
const BACKUP_MAILBOX: Uuid = Uuid::from_u128(0xb4);
const BACKUP_ADDRESS: &str = "backup@example.test";
const CODE: &str = "123456";

fn fake(v: &str) -> SecretString {
    SecretString::from(v.to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

/// A fake Verify that takes [`CODE`].
struct FakeVerify;

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// B9 (BIZ-002, BIZ-040): the first settle after a right email code fails (a test trigger that
/// raises once on the business row); the code stays open and the retry activates.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failed_settle_leaves_the_code_for_a_retry(pool: PgPool) {
    let (s, mut db) = (
        fake("fake-id-secret-for-tests-only-0123456789"),
        pool.acquire().await.expect("db"),
    );
    let id = create(&mut db, false, at(0)).await.expect("business");
    sqlx::query("INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at) VALUES ($1, $2, now())")
        .bind(id)
        .bind(OWNER_MAILBOX)
        .execute(&mut *db)
        .await
        .expect("sign-up row");
    let code = start_signup_check(&mut db, &s, id, at(0))
        .await
        .expect("check")
        .expect("code");
    let rec = RecordContacts {
        texting_phone: Some(fake("+15555550142")),
        email: None,
        has_address: true,
    };
    send_code(&mut db, (&FakeVerify, &s), id, &rec, at(0))
        .await
        .expect("sent");
    assert!(
        check_code(&mut db, (&FakeVerify, &s), id, &rec, (CODE, at(0)))
            .await
            .expect("code")
    );
    record_match(&mut db, id, Some(true), None, at(0))
        .await
        .expect("match");
    for sql in [
        "CREATE SEQUENCE t584_fail",
        "CREATE FUNCTION t584_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         IF nextval('t584_fail') = 1 THEN RAISE EXCEPTION 'fake settle failure'; END IF; \
         RETURN NEW; END $$",
        "CREATE TRIGGER t584_fail BEFORE UPDATE ON businesses.business \
         FOR EACH ROW EXECUTE FUNCTION t584_fail()",
    ] {
        sqlx::query(sql).execute(&mut *db).await.expect("trigger");
    }
    let first = confirm_mailbox(&mut db, &s, (id, OWNER_MAILBOX), &code, at(1)).await;
    assert!(first.is_err(), "the settle failure is reported");
    assert!(
        !may_get_notices(&mut db, id, OWNER_MAILBOX)
            .await
            .expect("read"),
        "code not taken"
    );
    assert_eq!(
        business(&mut db, id).await.expect("read").status.code(),
        "pending"
    );
    assert!(
        confirm_mailbox(&mut db, &s, (id, OWNER_MAILBOX), &code, at(2))
            .await
            .expect("retry")
    );
    sqlx::query("DROP TRIGGER t584_fail ON businesses.business")
        .execute(&mut *db)
        .await
        .expect("drop");
    assert_eq!(
        business(&mut db, id).await.expect("read").status.code(),
        "active"
    );
}

/// One email the fake provider took: (to, subject).
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

/// Fake addresses, English, a fake name, digits and links.
struct FakeParts;

impl NoticeParts for FakeParts {
    async fn address(
        &self,
        _: &mut PgConnection,
        mailbox: Uuid,
    ) -> Result<Option<SecretString>, HandlerError> {
        Ok(Some(fake(&match mailbox {
            BACKUP_MAILBOX => BACKUP_ADDRESS.to_owned(),
            other => format!("{}@example.test", other.as_u128()),
        })))
    }
    async fn spanish(&self, _: &mut PgConnection, _: Uuid) -> Result<bool, HandlerError> {
        Ok(false)
    }
    async fn member_name(
        &self,
        _: &mut PgConnection,
        _: Uuid,
    ) -> Result<Option<String>, HandlerError> {
        Ok(Some("Pat Example".to_owned()))
    }
    async fn line_last4(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
    ) -> Result<Option<String>, HandlerError> {
        Ok(Some("0142".to_owned()))
    }
    async fn approve_link(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid, Uuid),
        _: Timestamp,
    ) -> Result<SecretString, HandlerError> {
        Ok(fake("https://example.test/approve"))
    }
    fn retest_link(&self) -> String {
        "https://example.test/retest".to_owned()
    }
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

/// Runs every due task through the notice-email handlers; each must finish.
async fn run_due(pool: &PgPool) -> Vec<(String, String)> {
    let m: Mailer = Arc::new(AccountMailer {
        secret: fake("fake-id-secret-for-tests-only-0123456789"),
        postal: (
            "PO Box 100, Faketown, AZ 00000".to_owned(),
            MailboxKind::PoBox,
        ),
        provider: FakeMail::default(),
        limit: DailyLimit::new(1000, 80).expect("limit"),
        directory: FakeParts,
        clock: Arc::new(SystemClock),
    });
    let mut registry = Registry::new();
    for kind in NOTICE_KINDS {
        let m = Arc::clone(&m);
        registry.register(kind, move |pool, task| {
            run_notice_email(pool, Some(Arc::clone(&m)), task)
        });
    }
    let worker = WorkerId::new(pool).await.expect("worker");
    loop {
        let tasks = claim(
            pool,
            worker,
            Timestamp::now(),
            SignedDuration::from_secs(60),
            50,
        )
        .await
        .expect("claim");
        if tasks.is_empty() {
            return m.provider.0.lock().expect("lock").clone();
        }
        for task in tasks {
            let kind = task.kind.clone();
            let outcome = registry
                .run(pool, &SystemClock, worker, task, Lease::WORKER)
                .await;
            if NOTICE_KINDS.contains(&kind.as_str()) {
                assert!(matches!(outcome, Ok(Outcome::Done)), "{kind}: {outcome:?}");
            }
        }
    }
}

async fn insert(
    db: &mut PgConnection,
    sql: &'static str,
    (business, member, mailbox): (Uuid, Uuid, Uuid),
) {
    sqlx::query(sql)
        .bind(business)
        .bind(member)
        .bind(mailbox)
        .bind(at(2).as_second())
        .execute(db)
        .await
        .expect("insert");
}

/// A business whose owner's and member's verified main addresses are both returned, the owner
/// with a verified backup when `backup`, and the four kinds queued.
async fn returned_team(db: &mut PgConnection, backup: bool) {
    let b = create(db, false, at(0)).await.expect("business");
    let owner = Acting {
        business: b,
        member: add_owner(db, b, OWNER_MAILBOX, at(1)).await.expect("owner"),
    };
    let member = add_member(db, owner, MEMBER_MAILBOX, Roles::BOTH, at(1))
        .await
        .expect("member");
    for (who, mailbox) in [(owner.member, OWNER_MAILBOX), (member, MEMBER_MAILBOX)] {
        insert(
            db,
            "INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, \
             sent_at, window_at, verified_at) VALUES ($1, $2, $3, false, to_timestamp($4), \
             to_timestamp($4), to_timestamp($4))",
            (b, who, mailbox),
        )
        .await;
    }
    if backup {
        insert(
            db,
            "INSERT INTO businesses.backup_address (business_id, member_id, mailbox_id, \
             requested_by, sent_at, verified_at) VALUES ($1, $2, $3, $2, to_timestamp($4), \
             to_timestamp($4))",
            (b, owner.member, BACKUP_MAILBOX),
        )
        .await;
    }
    for mailbox in [OWNER_MAILBOX, MEMBER_MAILBOX] {
        mark_returned(db, (b, mailbox), Returned::Hard, at(3))
            .await
            .expect("returned");
    }
    queue_kinds(db, (owner, member)).await;
}

/// Links the member's phone (the phone-linked email) and queues the other three kinds.
async fn queue_kinds(db: &mut PgConnection, (owner, member): (Acting, Uuid)) {
    let b = owner.business;
    let by = Acting {
        business: b,
        member,
    };
    link_phone(db, &AnyPhone, (by, false), &fake("+15555550143"), at(3))
        .await
        .expect("phone");
    let license = json!({"business": b, "removed": [["plumbing_repair", "leak repair"]],
        "offered": [], "dropped_classes": [], "paused": false});
    for (kind, payload) in [
        (
            EMERGENCY_TEST_KIND,
            json!({"business": b, "member": member}),
        ),
        (
            AFTER_HOURS_OFF_KIND,
            json!({"business": b, "member": owner.member, "reason": OFF_REMOVED}),
        ),
        (LICENSE_CHANGE_KIND, license),
    ] {
        let mut task = NewTask::new(kind, payload, at(3));
        task.dedupe_key = Some(format!("{kind}.t584"));
        enqueue(db, &task).await.expect("enqueue");
    }
}

/// B30 (BIZ-040, BIZ-055): with both main addresses returned, nothing goes to either; the owner's
/// three kinds go to the owner's verified backup, and the member's emergency test goes nowhere.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_returned_main_address_gets_no_account_email(pool: PgPool) {
    returned_team(&mut pool.acquire().await.expect("db"), true).await;
    let sent = run_due(&pool).await;
    let to: BTreeSet<&str> = sent.iter().map(|(to, _)| to.as_str()).collect();
    assert_eq!(to, BTreeSet::from([BACKUP_ADDRESS]), "{sent:?}");
    let [notice, _] = returned_words(false, BACKUP_ADDRESS);
    let subjects: BTreeSet<&str> = sent.iter().map(|(_, s)| s.as_str()).collect();
    let notices = sent.iter().filter(|(_, s)| *s == notice).count();
    assert_eq!(
        (notices, subjects.len()),
        (2, 4),
        "each kind once: {sent:?}"
    );
    let [test, _] = emergency_test_words(false);
    assert!(
        !subjects.contains(test.as_str()),
        "no emergency test to a backup"
    );
    assert_eq!(
        sent.len(),
        5,
        "two returned notices and the three owner kinds: {sent:?}"
    );
}

/// B30 (BIZ-040): with no address left that may get notices, none of the kinds sends anything.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn with_no_address_left_nothing_is_sent(pool: PgPool) {
    returned_team(&mut pool.acquire().await.expect("db"), false).await;
    let sent = run_due(&pool).await;
    assert!(sent.is_empty(), "{sent:?}");
}

/// B31: each deliverability write with another business's member ID changes no row, while the
/// member's own business still writes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_deliverability_write_stays_in_its_business(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (a, b) = (
        create(&mut db, false, at(0)).await.expect("a"),
        create(&mut db, false, at(0)).await.expect("b"),
    );
    add_owner(&mut db, a, OWNER_MAILBOX, at(1))
        .await
        .expect("owner a");
    let theirs = Acting {
        business: b,
        member: add_owner(&mut db, b, MEMBER_MAILBOX, at(1))
            .await
            .expect("owner b"),
    };
    let wrong = Acting {
        business: a,
        member: theirs.member,
    };
    let rows = "SELECT count(*) FROM businesses.email_check";
    record_inbox_answer(&mut db, wrong, InboxAnswer::SpamOrJunk, at(2))
        .await
        .expect("write");
    record_ok_reply(&mut db, wrong, at(2)).await.expect("write");
    record_pick_alert(&mut db, wrong, true, at(2))
        .await
        .expect("write");
    let none: i64 = sqlx::query_scalar(rows)
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(none, 0, "no row for another business's member");
    record_inbox_answer(&mut db, theirs, InboxAnswer::Inbox, at(3))
        .await
        .expect("write");
    record_inbox_answer(&mut db, wrong, InboxAnswer::SpamOrJunk, at(4))
        .await
        .expect("write");
    record_ok_reply(&mut db, wrong, at(4)).await.expect("write");
    record_pick_alert(&mut db, wrong, true, at(4))
        .await
        .expect("write");
    let (owner, untouched): (Uuid, bool) = sqlx::query_as(
        "SELECT business_id, pick_alert_yes_at IS NULL AND ok_reply_at IS NULL \
         FROM businesses.email_check",
    )
    .fetch_one(&mut *db)
    .await
    .expect("row");
    assert_eq!(owner, b);
    assert!(
        untouched,
        "no reply or alert stored through another business"
    );
    let check = email_check(&mut db, theirs).await.expect("read");
    assert_eq!(check.answer, Some(InboxAnswer::Inbox));
    assert!(!check.reply_received);
}
