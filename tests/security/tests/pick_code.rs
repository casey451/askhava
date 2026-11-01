//! The page pick's Verify code in the database (TD-PICK-06; PICK-013, PICK-014, PICK-055, D-015,
//! D-101, D-366, D-480). Fake numbers, IDs and codes only; Verify is a fake adapter.
//!
//! The code is refused at exactly 10:00; five wrong tries start a 15-minute cooldown in which no
//! try reaches Verify and no new code goes; a pasted code reads; a voice code goes only on the tap
//! and never at night from another session; the per-number, per-call, per-job and global caps
//! hold, the global one raising one High alert a day; no number is stored.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_picks::api::rules::{CodeVia, SendAsk, SendRefused};
use askhava_picks::api::{CodeCheck, CodeSent, CodeTo, VERIFY_CAP_ALERT, check_code, send_code};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

const CODE: &str = "481516";
const PHONE: &str = "+19285550123";
const ROOMY: i64 = 1_000;

/// A Verify stand-in: records each send's channel and each check, and passes only [`CODE`].
#[derive(Default)]
struct FakeVerify {
    sends: Mutex<Vec<CodeChannel>>,
    checks: Mutex<u32>,
}

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _key: &IdempotencyKey,
        _to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.sends.lock().expect("lock").push(channel);
        Ok("VEfake0000".to_owned())
    }

    async fn check_code(&self, _to: &str, code: &str) -> Result<bool, AdapterError> {
        *self.checks.lock().expect("lock") += 1;
        Ok(code == CODE)
    }
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

/// Noon Arizona time on a fixed day, plus `seconds`.
fn noon(seconds: i64) -> Timestamp {
    let t: Timestamp = "2026-11-03T12:00:00-07:00".parse().expect("time");
    t + SignedDuration::from_secs(seconds)
}

fn text() -> SendAsk {
    SendAsk {
        via: CodeVia::Text,
        own_session: false,
        local_hour: 12,
    }
}

fn call(own_session: bool) -> SendAsk {
    SendAsk {
        via: CodeVia::CallTapped,
        own_session,
        local_hour: 12,
    }
}

async fn send(
    db: &mut Db,
    v: &FakeVerify,
    to: &CodeTo<'_>,
    ask: SendAsk,
    at: Timestamp,
) -> CodeSent {
    send_capped(db, v, to, ask, (at, ROOMY)).await
}

async fn send_capped(
    db: &mut Db,
    v: &FakeVerify,
    to: &CodeTo<'_>,
    ask: SendAsk,
    when: (Timestamp, i64),
) -> CodeSent {
    send_code(db, (v, &secret()), to, ask, when)
        .await
        .expect("send")
}

async fn check(
    db: &mut Db,
    v: &FakeVerify,
    to: &CodeTo<'_>,
    code: &str,
    at: Timestamp,
) -> CodeCheck {
    check_code(db, v, to, code, at).await.expect("check")
}

fn to(job: u128, contact: u128, phone: &SecretString) -> CodeTo<'_> {
    CodeTo {
        job: Uuid::from_u128(job),
        contact: Uuid::from_u128(contact),
        to: phone,
    }
}

/// TD-PICK-06 (PICK-013, D-101): the right code passes at 9:59 and is refused at exactly 10:00
/// and at 10:01, before Verify is asked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_06_code_valid_at_959_refused_at_1000_and_1001(mut db: Db) {
    let (v, phone) = (FakeVerify::default(), SecretString::from(PHONE.to_owned()));
    let first = to(1, 11, &phone);
    assert_eq!(
        send(&mut db, &v, &first, text(), noon(0)).await,
        CodeSent::Sent
    );
    assert_eq!(
        check(&mut db, &v, &first, CODE, noon(599)).await,
        CodeCheck::Accepted
    );
    assert_eq!(
        check(&mut db, &v, &first, CODE, noon(600)).await,
        CodeCheck::NoCode
    );
    let second = to(2, 11, &phone);
    assert_eq!(
        send(&mut db, &v, &second, text(), noon(0)).await,
        CodeSent::Sent
    );
    assert_eq!(
        check(&mut db, &v, &second, CODE, noon(600)).await,
        CodeCheck::Expired
    );
    assert_eq!(
        check(&mut db, &v, &second, CODE, noon(601)).await,
        CodeCheck::Expired
    );
    assert_eq!(*v.checks.lock().expect("lock"), 1);
}

/// TD-PICK-06 (PICK-013, D-015): five wrong tries, then a 15-minute cooldown in which the right
/// code is refused without reaching Verify and no new code goes; after it a new code works.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_06_five_wrong_tries_then_a_15_minute_cooldown(mut db: Db) {
    let (v, phone) = (FakeVerify::default(), SecretString::from(PHONE.to_owned()));
    let her = to(3, 12, &phone);
    assert_eq!(
        send(&mut db, &v, &her, text(), noon(0)).await,
        CodeSent::Sent
    );
    for (n, left) in (1..=4).zip([4, 3, 2, 1]) {
        let got = check(&mut db, &v, &her, "000000", noon(n * 10)).await;
        assert_eq!(got, CodeCheck::Wrong { left });
    }
    let until = noon(50) + SignedDuration::from_mins(15);
    assert_eq!(
        check(&mut db, &v, &her, "000000", noon(50)).await,
        CodeCheck::CoolingDown(until)
    );
    assert_eq!(
        check(&mut db, &v, &her, CODE, noon(60)).await,
        CodeCheck::CoolingDown(until)
    );
    let refused = CodeSent::Refused(SendRefused::CoolingDown(until));
    assert_eq!(send(&mut db, &v, &her, text(), noon(60)).await, refused);
    assert_eq!(*v.checks.lock().expect("lock"), 5);
    assert_eq!(send(&mut db, &v, &her, text(), until).await, CodeSent::Sent);
    assert_eq!(
        check(
            &mut db,
            &v,
            &her,
            CODE,
            until + SignedDuration::from_secs(5)
        )
        .await,
        CodeCheck::Accepted
    );
}

/// TD-PICK-06 (PICK-014): a pasted code with a space or dash passes; a short one is malformed and
/// uses no try.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_06_paste_reads_and_a_malformed_code_uses_no_try(mut db: Db) {
    let (v, phone) = (FakeVerify::default(), SecretString::from(PHONE.to_owned()));
    let her = to(4, 13, &phone);
    assert_eq!(
        send(&mut db, &v, &her, text(), noon(0)).await,
        CodeSent::Sent
    );
    for _ in 0..6 {
        assert_eq!(
            check(&mut db, &v, &her, "48151", noon(5)).await,
            CodeCheck::Malformed
        );
    }
    assert_eq!(
        check(&mut db, &v, &her, " 481-516 ", noon(9)).await,
        CodeCheck::Accepted
    );
}

/// PICK-013, PICK-055, D-366: a voice code only on the tap, never between 9 p.m. and 8 a.m. from
/// another session; at most 3 calls and 5 codes per number a day.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_055_voice_only_on_the_tap_and_per_number_caps(mut db: Db) {
    let (v, phone) = (FakeVerify::default(), SecretString::from(PHONE.to_owned()));
    let her = to(5, 14, &phone);
    let night = noon(10 * 3600);
    let quiet = CodeSent::Refused(SendRefused::CallQuietHours);
    assert_eq!(send(&mut db, &v, &her, call(false), night).await, quiet);
    assert_eq!(
        send(&mut db, &v, &her, call(true), night).await,
        CodeSent::Sent
    );
    assert_eq!(
        send(&mut db, &v, &her, text(), noon(0)).await,
        CodeSent::Sent
    );
    assert_eq!(
        send(&mut db, &v, &her, call(false), noon(1)).await,
        CodeSent::Sent
    );
    assert_eq!(
        send(&mut db, &v, &her, call(false), noon(2)).await,
        CodeSent::Sent
    );
    let calls = CodeSent::Refused(SendRefused::CallCap);
    assert_eq!(send(&mut db, &v, &her, call(false), noon(3)).await, calls);
    assert_eq!(
        send(&mut db, &v, &her, text(), noon(4)).await,
        CodeSent::Sent
    );
    let number = CodeSent::Refused(SendRefused::NumberCap);
    assert_eq!(send(&mut db, &v, &her, text(), noon(5)).await, number);
    let sends = v.sends.lock().expect("lock").clone();
    use CodeChannel::{Call, Sms};
    assert_eq!(sends, vec![Call, Sms, Call, Call, Sms]);
}

/// PICK-055: 10 codes per job a day across numbers; the next Arizona day starts again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_055_ten_codes_per_job_a_day(mut db: Db) {
    let (v, phone) = (FakeVerify::default(), SecretString::from(PHONE.to_owned()));
    for n in 0_u8..10 {
        let someone = to(6, 20 + u128::from(n / 5), &phone);
        assert_eq!(
            send(&mut db, &v, &someone, text(), noon(n.into())).await,
            CodeSent::Sent
        );
    }
    let third = to(6, 30, &phone);
    let job = CodeSent::Refused(SendRefused::JobCap);
    assert_eq!(send(&mut db, &v, &third, text(), noon(60)).await, job);
    let tomorrow = noon(24 * 3600);
    assert_eq!(
        send(&mut db, &v, &third, text(), tomorrow).await,
        CodeSent::Sent
    );
}

/// PICK-055: at the global threshold sends stop and one High alert is raised that day; no phone
/// number is stored anywhere in `picks`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_055_global_cap_stops_sends_with_one_high_alert(mut db: Db) {
    let (v, phone) = (FakeVerify::default(), SecretString::from(PHONE.to_owned()));
    for n in 0..3 {
        let her = to(7 + n, 40 + n, &phone);
        let at = noon(i64::try_from(n).expect("small"));
        assert_eq!(
            send_capped(&mut db, &v, &her, text(), (at, 3)).await,
            CodeSent::Sent
        );
    }
    let global = CodeSent::Refused(SendRefused::GlobalCap);
    for n in 0..2 {
        let her = to(20 + n, 60 + n, &phone);
        assert_eq!(
            send_capped(&mut db, &v, &her, text(), (noon(10), 3)).await,
            global
        );
    }
    let alerts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high'")
            .bind(VERIFY_CAP_ALERT)
            .fetch_one(&mut *db)
            .await
            .expect("alerts");
    assert_eq!(alerts, 1);
    let stored: String = sqlx::query_scalar(
        "SELECT coalesce(string_agg(row_to_json(c)::text, ''), '') FROM picks.code_attempt c",
    )
    .fetch_one(&mut *db)
    .await
    .expect("rows");
    assert!(!stored.contains("5550123"));
}
