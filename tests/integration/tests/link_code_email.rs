//! TD-BIZ-25 Confirm-link email case (SEC-004, SEC-027, SEC-014, SEC-020, TXT-038, BIZ-046;
//! T871, D-5315, D-5971): `portals::api::send_link_code` and `check_link_code` against
//! PostgreSQL over a fake Verify that records each (to, channel) sent and each address checked.
//! An email-only person gets one `CodeChannel::Email` send and never a text or call; the right
//! code checks and a wrong one does not; a shared inbox or an empty address gets `NoPhone` and
//! nothing sent; a phone gets the text as the press path does; the send and try limits refuse
//! as for a text. The press path (`send_confirm_code`) still answers `NoPhone` for an email-only
//! person. Every ID, number, address and code is fake.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_portals::api::{
    CodeAddress, CodeCheck, CodeFuture, CodeSent, CodeTo, ConfirmCodes, PortalsError,
    check_link_code, send_confirm_code, send_link_code,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PHONE: &str = "+15005550171";
const EMAIL: &str = "crew.member@example.com";
const CODE: &str = "246810";
const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 71));
const THRESHOLD: i32 = 1000;

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minutes * 60).expect("time")
}

/// What the fake Verify was asked: each send's (to, channel) and each check's address.
#[derive(Default)]
struct Asked {
    sent: Vec<(String, CodeChannel)>,
    checked: Vec<String>,
}

type Calls = Arc<Mutex<Asked>>;

/// A fake Verify: records every send and check; [`CODE`] is right.
struct FakeVerify(Calls);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0
            .lock()
            .expect("lock")
            .sent
            .push((to.to_owned(), channel));
        Ok("VEfake871".to_owned())
    }

    async fn check_code(&self, to: &str, code: &str) -> Result<bool, AdapterError> {
        self.0.lock().expect("lock").checked.push(to.to_owned());
        Ok(code == CODE)
    }
}

/// Fake people: `phone` has a texting phone, `email` only a sign-in address, anyone else an
/// empty address.
struct Book {
    phone: Uuid,
    email: Uuid,
}

impl CodeAddress for Book {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let to = if member == self.phone {
            CodeTo::Phone(SecretString::from(PHONE.to_owned()))
        } else if member == self.email {
            CodeTo::Email(SecretString::from(EMAIL.to_owned()))
        } else {
            CodeTo::Email(SecretString::default())
        };
        Box::pin(async move { Ok(to) })
    }
}

struct World {
    calls: Calls,
    codes: ConfirmCodes<FakeVerify>,
    phone: Uuid,
    email: Uuid,
    business: Uuid,
}

fn world() -> World {
    let calls = Calls::default();
    let (phone, email) = (Uuid::new_v4(), Uuid::new_v4());
    let key = SecretString::from("fake-key-871".to_owned());
    let verify = FakeVerify(Arc::clone(&calls));
    let codes = ConfirmCodes::new(verify, Book { phone, email }, key);
    let business = Uuid::new_v4();
    World {
        calls,
        codes,
        phone,
        email,
        business,
    }
}

impl World {
    fn sent(&self) -> Vec<(String, CodeChannel)> {
        self.calls.lock().expect("lock").sent.clone()
    }

    fn checked(&self) -> Vec<String> {
        self.calls.lock().expect("lock").checked.clone()
    }
}

/// Marks `member`'s inbox shared or not, as `confirm_terms.rs` does (no FK on the table).
async fn mark_shared(pool: &PgPool, (member, business): (Uuid, Uuid), shared: bool) {
    sqlx::query(
        "INSERT INTO portals.shared_inbox (member_id, business_id, shared, changed_at) \
         VALUES ($1, $2, $3, now())",
    )
    .bind(member)
    .bind(business)
    .bind(shared)
    .execute(pool)
    .await
    .expect("shared row");
}

/// TD-BIZ-25 (SEC-004, TXT-038, BIZ-046): a person with no phone that gets texts, whose inbox
/// is not shared (a `shared = false` row, the nearest wrong input), gets exactly one emailed
/// code, no text and no call; the line says email and never shows the address; the press path
/// still answers `NoPhone` for the same person and sends nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_25_an_email_only_member_gets_one_email_code_never_a_text_or_call(pool: PgPool) {
    let w = world();
    mark_shared(&pool, (w.email, w.business), false).await;
    let pick = Uuid::new_v4();
    let got = send_link_code(&w.codes, &pool, (IP, w.email, pick), (at(1), THRESHOLD)).await;
    assert_eq!(got.expect("send"), CodeSent::Emailed);
    assert_eq!(w.sent(), [(EMAIL.to_owned(), CodeChannel::Email)]);
    let line = CodeSent::Emailed.line();
    assert!(line.contains("email"), "{line}");
    assert!(
        !line.contains('@') && !line.contains("example"),
        "no address shown"
    );
    assert_ne!(line, CodeSent::Sent.line());
    let press = send_confirm_code(&w.codes, &pool, (IP, w.email, pick), (at(2), THRESHOLD)).await;
    assert_eq!(press.expect("press"), CodeSent::NoPhone);
    assert_eq!(w.sent().len(), 1, "the press path never emails");
}

/// TD-BIZ-25 (SEC-004, SEC-020): the emailed code checks right against the same address; a
/// wrong code and a short one do not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_25_the_emailed_code_checks_and_a_wrong_one_does_not(pool: PgPool) {
    let w = world();
    let to = (IP, w.email, Uuid::new_v4());
    let sent = send_link_code(&w.codes, &pool, to, (at(1), THRESHOLD)).await;
    assert_eq!(sent.expect("send"), CodeSent::Emailed);
    let check = |code| check_link_code(&w.codes, &pool, (IP, w.email, w.business), code, at(2));
    assert_eq!(check("12").await.expect("form"), CodeCheck::Missing);
    assert_eq!(check("000000").await.expect("wrong"), CodeCheck::Wrong);
    assert_eq!(check(CODE).await.expect("right"), CodeCheck::Right);
    assert_eq!(w.checked(), [EMAIL.to_owned(), EMAIL.to_owned()]);
}

/// SEC-027, TD-BIZ-25: while the email-only person's inbox is marked shared, no code is
/// emailed (`NoPhone`, nothing sent, no send spent) and a check asks nothing of Verify; a
/// person with a phone and a shared inbox still gets the text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_027_a_shared_inbox_gets_no_email_code(pool: PgPool) {
    let w = world();
    mark_shared(&pool, (w.email, w.business), true).await;
    mark_shared(&pool, (w.phone, w.business), true).await;
    let pick = Uuid::new_v4();
    let got = send_link_code(&w.codes, &pool, (IP, w.email, pick), (at(1), THRESHOLD)).await;
    assert_eq!(got.expect("send"), CodeSent::NoPhone);
    assert!(w.sent().is_empty(), "nothing sent to a shared inbox");
    let who = (IP, w.email, w.business);
    let check = check_link_code(&w.codes, &pool, who, CODE, at(2)).await;
    assert_eq!(check.expect("check"), CodeCheck::Ended);
    assert!(w.checked().is_empty(), "Verify not asked");
    let text = send_link_code(&w.codes, &pool, (IP, w.phone, pick), (at(3), THRESHOLD)).await;
    assert_eq!(text.expect("text"), CodeSent::Sent);
    assert_eq!(w.sent(), [(PHONE.to_owned(), CodeChannel::Sms)]);
}

/// SEC-004, SEC-014: an empty address gets `NoPhone`, sends nothing and spends no send, so after
/// 11 such answers from one IP (its limit is 10 an hour) a phone behind it still gets its text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_004_an_empty_address_sends_nothing_and_spends_no_send(pool: PgPool) {
    let w = world();
    let (nobody, pick) = (Uuid::new_v4(), Uuid::new_v4());
    for _ in 0..11 {
        let got = send_link_code(&w.codes, &pool, (IP, nobody, pick), (at(0), THRESHOLD)).await;
        assert_eq!(got.expect("send"), CodeSent::NoPhone);
    }
    assert!(w.sent().is_empty());
    let text = send_link_code(&w.codes, &pool, (IP, w.phone, pick), (at(1), THRESHOLD)).await;
    assert_eq!(text.expect("text"), CodeSent::Sent);
}

/// TD-BIZ-25 (SEC-004, TXT-038): a person with a phone gets the link's code by text exactly as
/// the press path sends it, and it checks against that phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_25_a_phone_gets_the_text_as_the_press_path_does(pool: PgPool) {
    let w = world();
    let pick = Uuid::new_v4();
    let link = send_link_code(&w.codes, &pool, (IP, w.phone, pick), (at(1), THRESHOLD)).await;
    let press = send_confirm_code(&w.codes, &pool, (IP, w.phone, pick), (at(2), THRESHOLD)).await;
    assert_eq!(link.expect("link"), CodeSent::Sent);
    assert_eq!(press.expect("press"), CodeSent::Sent);
    let text = (PHONE.to_owned(), CodeChannel::Sms);
    assert_eq!(w.sent(), [text.clone(), text]);
    let who = (IP, w.phone, w.business);
    let check = check_link_code(&w.codes, &pool, who, CODE, at(3)).await;
    assert_eq!(check.expect("check"), CodeCheck::Right);
    assert_eq!(w.checked(), [PHONE.to_owned()]);
}

/// SEC-014: the sixth emailed code to one person in a day is refused as a sixth text would be,
/// and reaches no Verify.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_014_the_send_limit_refuses_an_email_as_a_text(pool: PgPool) {
    let w = world();
    let pick = Uuid::new_v4();
    for n in 0..5 {
        let got = send_link_code(&w.codes, &pool, (IP, w.email, pick), (at(n), THRESHOLD)).await;
        assert_eq!(got.expect("send"), CodeSent::Emailed);
    }
    let sixth = send_link_code(&w.codes, &pool, (IP, w.email, pick), (at(6), THRESHOLD)).await;
    assert_eq!(sixth.expect("send"), CodeSent::Limited);
    let sent = w.sent();
    assert_eq!(sent.len(), 5, "a refused send reaches no Verify");
    assert!(sent.iter().all(|(_, c)| *c == CodeChannel::Email));
}

/// SEC-020: the 21st emailed-code try in an hour for one business is refused before Verify.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn sec_020_the_try_limit_refuses_an_email_code_as_a_text(pool: PgPool) {
    let w = world();
    let who = (IP, w.email, w.business);
    let mut answers = Vec::new();
    for _ in 0..21 {
        let tried = check_link_code(&w.codes, &pool, who, "000000", at(7)).await;
        answers.push(tried.expect("try"));
    }
    assert!(
        answers[..20].iter().all(|a| *a == CodeCheck::Wrong),
        "{answers:?}"
    );
    assert_eq!(answers[20], CodeCheck::Limited);
    assert_eq!(w.checked().len(), 20, "the refused try reaches no Verify");
}
