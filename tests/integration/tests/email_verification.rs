//! The sign-up email check before activation, bounces, and the identity episode across
//! `businesses` and a fake Verify that records each request's idempotency key (TD-BIZ-40,
//! TD-BIZ-13: BIZ-002, BIZ-016, BIZ-026, BIZ-036, BIZ-040, BIZ-041, BIZ-055, CON-020; D-1712,
//! D-1756). All contacts, codes and IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{
    IDENTITY_CODE_TRIES, RecordContacts, RecordSeen, Returned, Step, business, check_code,
    confirm_mailbox, create, episode_letter_code, letter_code, mark_returned, may_get_cards,
    may_get_notices, number_is_old, record_match, request_welcome, resume_on_new_record, send_code,
    send_code_seen, start_signup_check, welcome_sent,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PHONE: &str = "+15555550142";
const NEW_PHONE: &str = "+15555550199";
const EMAIL: &str = "owner@example.test";
const NEW_EMAIL: &str = "office@example.test";
const ADDRESS: &str = "100 Fake St, Lake Havasu City, AZ 86403";
const NEW_ADDRESS: &str = "200 Test Ave, Lake Havasu City, AZ 86404";
const CODE: &str = "123456";
const MAILBOX: Uuid = Uuid::from_u128(0xe1);

/// A fake Verify recording each send's key and destination; it refuses while `fail` is set.
#[derive(Default)]
struct FakeVerify {
    sent: Mutex<Vec<(String, String)>>,
    fail: Mutex<bool>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        key: &IdempotencyKey,
        to: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        if std::mem::take(&mut *self.fail.lock().expect("lock")) {
            return Err(AdapterError::NotSent);
        }
        let row = (key.as_str().to_owned(), to.to_owned());
        self.sent.lock().expect("lock").push(row);
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

impl FakeVerify {
    fn sent(&self) -> Vec<(String, String)> {
        self.sent.lock().expect("lock").clone()
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned())
}

fn fake(v: &str) -> SecretString {
    SecretString::from(v.to_owned())
}

fn at(day: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_hours(day * 24)
}

fn record(phone: Option<&str>, email: &str) -> RecordContacts {
    RecordContacts {
        texting_phone: phone.map(fake),
        email: Some(fake(email)),
        has_address: true,
    }
}

async fn signed_up(db: &mut PgConnection) -> Uuid {
    let id = create(db, false, at(0)).await.expect("business");
    sqlx::query("INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at) VALUES ($1, $2, now())")
        .bind(id)
        .bind(MAILBOX)
        .execute(db)
        .await
        .expect("sign-up row");
    id
}

async fn status(db: &mut PgConnection, id: Uuid) -> &'static str {
    business(db, id).await.expect("business").status.code()
}

/// TD-BIZ-40 (BIZ-026, D-1756): the record phone fails, the list shows a new phone, the code goes
/// to the new phone under a new key; an identical retry reuses its key.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_40_a_new_record_phone_gets_a_new_key(pool: PgPool) {
    let (v, s, mut db) = (
        FakeVerify::default(),
        secret(),
        pool.acquire().await.expect("db"),
    );
    let id = signed_up(&mut db).await;
    let (old, addr) = (record(Some(PHONE), EMAIL), fake(ADDRESS));
    *v.fail.lock().expect("lock") = true;
    assert!(send_code(&mut db, (&v, &s), id, &old, at(0)).await.is_err());
    let first = send_code(&mut db, (&v, &s), id, &old, at(0))
        .await
        .expect("retry");
    assert_eq!(first, Some(Step::Text));
    let seen = |c| RecordSeen {
        contacts: c,
        address: Some(&addr),
    };
    number_is_old(&mut db, &s, (id, seen(&old)), at(0))
        .await
        .expect("old");
    let new = record(Some(NEW_PHONE), EMAIL);
    assert!(
        resume_on_new_record(&mut db, &s, (id, seen(&new)))
            .await
            .expect("resume")
    );
    send_code(&mut db, (&v, &s), id, &new, at(1))
        .await
        .expect("sent");
    let sent = v.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!((sent[0].1.as_str(), sent[1].1.as_str()), (PHONE, NEW_PHONE));
    assert_ne!(sent[0].0, sent[1].0, "a new phone gets a new key");
    sqlx::query("UPDATE businesses.signup SET identity_step = NULL WHERE business_id = $1")
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("replay");
    send_code(&mut db, (&v, &s), id, &new, at(1))
        .await
        .expect("same send");
    assert_eq!(v.sent()[2], sent[1], "an identical send keeps its key");
}

/// TD-BIZ-40 (BIZ-026, D-1756): a changed record email gets a new key; never a typed contact.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_40_a_new_record_email_gets_a_new_key(pool: PgPool) {
    let (v, s, mut db) = (
        FakeVerify::default(),
        secret(),
        pool.acquire().await.expect("db"),
    );
    let id = signed_up(&mut db).await;
    let (old, addr) = (record(None, EMAIL), fake(ADDRESS));
    let seen = |c| RecordSeen {
        contacts: c,
        address: Some(&addr),
    };
    send_code_seen(&mut db, (&v, &s), id, seen(&old), at(0))
        .await
        .expect("sent");
    number_is_old(&mut db, &s, (id, seen(&old)), at(0))
        .await
        .expect("old");
    let new = record(None, NEW_EMAIL);
    assert!(
        resume_on_new_record(&mut db, &s, (id, seen(&new)))
            .await
            .expect("resume")
    );
    let step = send_code_seen(&mut db, (&v, &s), id, seen(&new), at(1)).await;
    assert_eq!(step.expect("sent"), Some(Step::Email));
    let sent = v.sent();
    let to: Vec<&str> = sent.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        to,
        [EMAIL, NEW_EMAIL],
        "only record contacts, never a typed one"
    );
    assert_ne!(sent[0].0, sent[1].0);
}

/// TD-BIZ-40 (BIZ-002, D-1756): a changed record address prints a new letter code; the old one
/// stops working, and the wrong-code limit counts per episode.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_40_a_new_letter_address_gets_a_new_code(pool: PgPool) {
    let (v, s, mut db) = (
        FakeVerify::default(),
        secret(),
        pool.acquire().await.expect("db"),
    );
    let id = signed_up(&mut db).await;
    let rec = RecordContacts {
        has_address: true,
        ..RecordContacts::default()
    };
    let (addr, new_addr) = (fake(ADDRESS), fake(NEW_ADDRESS));
    let seen = |a| RecordSeen {
        contacts: &rec,
        address: Some(a),
    };
    send_code_seen(&mut db, (&v, &s), id, seen(&addr), at(0))
        .await
        .expect("letter");
    number_is_old(&mut db, &s, (id, seen(&addr)), at(0))
        .await
        .expect("old");
    assert!(
        resume_on_new_record(&mut db, &s, (id, seen(&new_addr)))
            .await
            .expect("resume")
    );
    let step = send_code_seen(&mut db, (&v, &s), id, seen(&new_addr), at(1)).await;
    assert_eq!(step.expect("letter"), Some(Step::Letter));
    let (old_code, new_code) = (letter_code(&s, id), episode_letter_code(&s, id, 1));
    assert_ne!(old_code, new_code);
    let wrong = if new_code == "000000" {
        "000001"
    } else {
        "000000"
    };
    for bad in [old_code.as_str(); 1].into_iter().chain([wrong; 4]) {
        let r = check_code(&mut db, (&v, &s), id, &rec, (bad, at(1))).await;
        assert!(!r.expect("checked"));
    }
    let r = check_code(&mut db, (&v, &s), id, &rec, (&new_code, at(1))).await;
    assert!(
        !r.expect("checked"),
        "after {IDENTITY_CODE_TRIES} wrong codes nothing passes"
    );
    assert!(v.sent().is_empty(), "a letter is never a Verify send");
}

/// TD-BIZ-40 (D-1712, D-1756): two concurrent resumes send once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_40_two_concurrent_resumes_send_once(pool: PgPool) {
    let (v, s) = (FakeVerify::default(), secret());
    let mut db = pool.acquire().await.expect("db");
    let id = signed_up(&mut db).await;
    let (old, new, addr) = (
        record(Some(PHONE), EMAIL),
        record(Some(NEW_PHONE), EMAIL),
        fake(ADDRESS),
    );
    send_code(&mut db, (&v, &s), id, &old, at(0))
        .await
        .expect("sent");
    let seen = |c| RecordSeen {
        contacts: c,
        address: Some(&addr),
    };
    number_is_old(&mut db, &s, (id, seen(&old)), at(0))
        .await
        .expect("old");
    let resume = || async {
        let mut c = pool.acquire().await.expect("db");
        if resume_on_new_record(&mut c, &s, (id, seen(&new)))
            .await
            .expect("resume")
        {
            send_code(&mut c, (&v, &s), id, &new, at(1))
                .await
                .expect("sent");
        }
    };
    tokio::join!(resume(), resume());
    assert_eq!(v.sent().len(), 2, "the first code and one resumed code");
}

/// TD-BIZ-13 (BIZ-040, CON-020): the sign-up email must be verified before activation; the
/// welcome goes before the first card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_13_the_signup_email_is_verified_before_activation(pool: PgPool) {
    let (v, s, mut db) = (
        FakeVerify::default(),
        secret(),
        pool.acquire().await.expect("db"),
    );
    let id = signed_up(&mut db).await;
    let code = start_signup_check(&mut db, &s, id, at(0))
        .await
        .expect("check")
        .expect("code");
    let rec = record(Some(PHONE), EMAIL);
    send_code(&mut db, (&v, &s), id, &rec, at(0))
        .await
        .expect("sent");
    assert!(
        check_code(&mut db, (&v, &s), id, &rec, (CODE, at(0)))
            .await
            .expect("code")
    );
    record_match(&mut db, id, Some(true), None, at(0))
        .await
        .expect("match");
    assert_eq!(
        status(&mut db, id).await,
        "pending",
        "no activation before the email check"
    );
    let wrong = if code == "000000" { "000001" } else { "000000" };
    assert!(
        !confirm_mailbox(&mut db, &s, (id, MAILBOX), wrong, at(0))
            .await
            .expect("ok")
    );
    assert!(!may_get_notices(&mut db, id, MAILBOX).await.expect("read"));
    assert!(
        confirm_mailbox(&mut db, &s, (id, MAILBOX), &code, at(0))
            .await
            .expect("ok")
    );
    assert_eq!(status(&mut db, id).await, "active");
    assert!(
        !confirm_mailbox(&mut db, &s, (id, MAILBOX), &code, at(0))
            .await
            .expect("ok"),
        "single use"
    );
    assert!(may_get_notices(&mut db, id, MAILBOX).await.expect("read"));
    request_welcome(&mut db, id, at(0)).await.expect("welcome");
    assert!(
        !may_get_cards(&mut db, id).await.expect("read"),
        "no card before the welcome"
    );
    welcome_sent(&mut db, id, at(0)).await.expect("sent");
    assert!(may_get_cards(&mut db, id).await.expect("read"));
}

/// TD-BIZ-13 (BIZ-040, BIZ-055): a hard bounce, or two soft bounces 14 days apart, marks the
/// address returned and queues one notice to the owner's other addresses and the digest; one soft
/// bounce, or two 13 days apart, does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_13_bounces_mark_the_address_returned(pool: PgPool) {
    let (s, mut db) = (secret(), pool.acquire().await.expect("db"));
    let (hard, soft) = (signed_up(&mut db).await, signed_up(&mut db).await);
    for id in [hard, soft] {
        let code = start_signup_check(&mut db, &s, id, at(0))
            .await
            .expect("c")
            .expect("code");
        assert!(
            confirm_mailbox(&mut db, &s, (id, MAILBOX), &code, at(0))
                .await
                .expect("ok")
        );
    }
    assert!(
        mark_returned(&mut db, (hard, MAILBOX), Returned::Hard, at(1))
            .await
            .expect("hard")
    );
    assert!(!may_get_notices(&mut db, hard, MAILBOX).await.expect("read"));
    assert!(
        !mark_returned(&mut db, (soft, MAILBOX), Returned::Soft, at(1))
            .await
            .expect("soft")
    );
    assert!(
        !mark_returned(&mut db, (soft, MAILBOX), Returned::Soft, at(14))
            .await
            .expect("13 days")
    );
    assert!(may_get_notices(&mut db, soft, MAILBOX).await.expect("read"));
    assert!(
        mark_returned(&mut db, (soft, MAILBOX), Returned::Soft, at(15))
            .await
            .expect("14 days")
    );
    let tasks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM core.task WHERE kind = 'businesses.notice_address_returned'",
    )
    .fetch_one(&mut *db)
    .await
    .expect("tasks");
    assert_eq!(tasks, 2);
}
