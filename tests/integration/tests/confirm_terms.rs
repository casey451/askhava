//! The Confirm step's "Can't take it" link, first opening, business-terms record and shared-inbox
//! code (T232; TD-PICK-43 Confirm half, TD-TERMS-11, TD-SEC-32; PICK-050, OPS-037, TERMS-033,
//! SEC-027, SEC-014; D-493, D-3055). No route serves the Confirm page yet (T261), so each case
//! calls the functions against PostgreSQL over a fake Verify. Every ID, number and code is fake.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{
    ConfirmTerms, add_owner, confirm_assent_line, create, record_confirm_terms,
};
use askhava_messaging::api::{NewNoticeLog, NoticeKind, Sending, notice_log, record_notice};
use askhava_payments::api::{Fee, Tier};
use askhava_portals::api::{
    CodeAddress, CodeCheck, CodeFuture, CodeSent, CodeStep, CodeTo, ConfirmCodes, ConfirmFacts,
    PortalsError, check_confirm_code, confirm_code_needed, confirm_page_with_code,
    open_confirm_page, send_confirm_code,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PHONE: &str = "+15005550188";
const CODE: &str = "135790";
const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 44));

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minutes * 60).expect("time")
}

type Calls = Arc<Mutex<(Vec<(String, CodeChannel)>, u32)>>;

/// A fake Verify like Twilio's: records each send and counts checks; [`CODE`] is right, and a
/// check after a code's fifth is refused (its own cap of 5 checks, error 60202).
struct FakeVerify(Calls);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let mut calls = self.0.lock().expect("lock");
        calls.0.push((to.to_owned(), channel));
        calls.1 = 0;
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        let mut calls = self.0.lock().expect("lock");
        calls.1 += 1;
        if calls.1 > 5 {
            let code = Some(60202);
            return Err(AdapterError::Vendor { status: 429, code });
        }
        Ok(code == CODE)
    }
}

/// `phone_person` has a phone that gets texts; anyone else gets codes by email only.
struct FakeCodeTo(Uuid);

impl CodeAddress for FakeCodeTo {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let to = if member == self.0 {
            CodeTo::Phone(SecretString::from(PHONE.to_owned()))
        } else {
            CodeTo::Email(SecretString::from("crew@x.example".to_owned()))
        };
        Box::pin(async move { Ok(to) })
    }
}

/// A business with its owner: (business, owner member, owner's mailbox).
async fn business(db: &mut PgConnection) -> (Uuid, Uuid, Uuid) {
    let id = create(db, false, at(0)).await.expect("business");
    let mailbox = Uuid::new_v4();
    let owner = add_owner(db, id, mailbox, at(0)).await.expect("owner");
    (id, owner, mailbox)
}

fn facts() -> ConfirmFacts {
    ConfirmFacts {
        pick: Uuid::from_u128(0x232),
        job_number: 1042,
        fee: Fee::Charged {
            basis: Some(100_000),
            tier: Tier::Medium,
        },
        waived: None,
        terms_version: 4,
        paused: false,
        press: Uuid::from_u128(0x233),
        csrf: "fake-csrf".to_owned(),
        stripe_key: "pk_test_fake".to_owned(),
    }
}

/// TD-PICK-43 (Confirm half, PICK-050): every Confirm page with a form links to the one "Can't
/// take it" page of its pick; the paused refusal has no form and no link.
#[test]
fn td_pick_43_the_confirm_page_carries_cant_take_it() {
    let link =
        "<a href=\"/b/picks/00000000-0000-0000-0000-000000000232/cant-take\">Can't take it</a>";
    let step = CodeStep {
        line: Some(CodeSent::Sent.line()),
        error: CodeCheck::Wrong.error(),
    };
    for code in [None, Some(CodeStep::default()), Some(step)] {
        let page = confirm_page_with_code(&facts(), code).expect("page");
        assert_eq!(page.matches(link).count(), 1, "{page}");
        assert_eq!(page.matches("/cant-take").count(), 1);
    }
    let paused = ConfirmFacts {
        paused: true,
        ..facts()
    };
    let page = confirm_page_with_code(&paused, None).expect("page");
    assert!(!page.contains("cant-take") && !page.contains("<form"));
}

/// OPS-037: the first opening stamps that person's pick notice once; a reload, another
/// person's notice and another kind stay as they were.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ops_037_the_first_opening_is_stamped_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (biz, owner, mailbox) = business(&mut db).await;
    let job = Uuid::new_v4();
    let log = |kind, mailbox| NewNoticeLog {
        business: biz,
        job,
        kind,
        mailbox,
        email: None,
    };
    for (n, kind, to) in [
        (1, NoticeKind::Card, mailbox),
        (2, NoticeKind::Pick, mailbox),
        (3, NoticeKind::Pick, Uuid::new_v4()),
    ] {
        record_notice(&mut db, &log(kind, to), Sending::Sent(at(n)))
            .await
            .expect("logged");
    }
    let who = (biz, job, owner);
    assert!(
        open_confirm_page(&mut db, who, at(5))
            .await
            .expect("opened")
    );
    assert!(
        !open_confirm_page(&mut db, who, at(9))
            .await
            .expect("reloaded")
    );
    let entries = notice_log(&mut db, biz, job).await.expect("log");
    let opened: Vec<_> = entries.iter().map(|e| e.first_opened).collect();
    assert_eq!(opened, [None, Some(at(5)), None]);
    let stranger = open_confirm_page(&mut db, (biz, job, Uuid::new_v4()), at(10)).await;
    assert!(!stranger.expect("no member"));
}

/// TD-TERMS-11 (TERMS-033): a press stores one `confirm_page` row with the pick, person, version,
/// the text's SHA256, language and the assent line shown; a second press for the pick stores
/// none; the table stays append-only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_terms_11_a_press_stores_one_confirm_page_acceptance(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (biz, owner, _) = business(&mut db).await;
    let pick = Uuid::new_v4();
    let terms = ConfirmTerms {
        version: 4,
        published_text: "abc",
        language: "en",
    };
    let page = confirm_page_with_code(&facts(), None).expect("page");
    assert!(
        page.contains(&confirm_assent_line(4)),
        "the line shown is the line stored"
    );
    let mut tx = sqlx::Connection::begin(&mut *db).await.expect("tx");
    assert!(
        record_confirm_terms(&mut tx, (biz, owner), pick, terms, at(2))
            .await
            .expect("stored")
    );
    assert!(
        !record_confirm_terms(&mut tx, (biz, owner), pick, terms, at(3))
            .await
            .expect("again")
    );
    tx.commit().await.expect("commit");
    let rows: Vec<(Option<Uuid>, Uuid, String)> = sqlx::query_as(
        "SELECT pick_id, person_id, concat_ws('|', channel, version, text_sha256, language, \
         assent_line) FROM businesses.terms_acceptance WHERE business_id = $1",
    )
    .bind(biz)
    .fetch_all(&mut *db)
    .await
    .expect("read");
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let fields = format!("confirm_page|4|{sha}|en|You're agreeing to business terms version 4.");
    assert_eq!(rows, [(Some(pick), owner, fields)]);
    let no_pick = "INSERT INTO businesses.terms_acceptance (business_id, person_id, document, version, \
         text_sha256, language, channel, assent_line, accepted_at) \
         VALUES ($1, $1, 'business_terms', '4', $2, 'en', 'confirm_page', 'x', now())";
    let refused = sqlx::query(no_pick)
        .bind(biz)
        .bind(sha)
        .execute(&mut *db)
        .await;
    assert!(refused.is_err(), "a confirm_page row needs its pick");
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let upd = sqlx::query("UPDATE businesses.terms_acceptance SET pick_id = NULL");
    assert!(upd.execute(&mut *db).await.is_err(), "append-only");
}

/// TD-SEC-32 (SEC-027): with "This inbox is shared" on, a Confirm in a live session needs a
/// fresh code, sent by Verify only to that person's own phone; without it, none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_32_a_shared_inbox_needs_a_fresh_code_to_the_own_phone(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (biz, owner, _) = business(&mut db).await;
    assert!(
        !confirm_code_needed(&mut db, owner, true)
            .await
            .expect("read")
    );
    sqlx::query(
        "INSERT INTO portals.shared_inbox (member_id, business_id, shared, changed_at) \
         VALUES ($1, $2, true, now())",
    )
    .bind(owner)
    .bind(biz)
    .execute(&mut *db)
    .await
    .expect("shared");
    assert!(
        confirm_code_needed(&mut db, owner, true)
            .await
            .expect("read")
    );
    let calls = Calls::default();
    let codes = codes(&calls, owner);
    let pick = Uuid::new_v4();
    let sent = send_confirm_code(&codes, &pool, (IP, owner, pick), (at(1), 1000)).await;
    assert_eq!(sent.expect("sent"), CodeSent::Sent);
    let other = send_confirm_code(&codes, &pool, (IP, Uuid::new_v4(), pick), (at(1), 1000)).await;
    assert_eq!(other.expect("no phone"), CodeSent::NoPhone);
    let check = |code| check_confirm_code(&codes, &pool, (IP, owner, biz), code, at(2));
    assert_eq!(check("12").await.expect("form"), CodeCheck::Missing);
    assert_eq!(check("000000").await.expect("wrong"), CodeCheck::Wrong);
    assert_eq!(check(CODE).await.expect("right"), CodeCheck::Right);
    let tries: i64 = sqlx::query_scalar("SELECT count(*) FROM portals.code_try")
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(tries, 0, "no sign-in link, no code_try row");
    let sends = calls.lock().expect("lock").0.clone();
    assert_eq!(sends, [(PHONE.to_owned(), CodeChannel::Sms)]);
}

fn codes(calls: &Calls, phone_person: Uuid) -> ConfirmCodes<FakeVerify> {
    let key = SecretString::from("fake-key".to_owned());
    ConfirmCodes::new(FakeVerify(Arc::clone(calls)), FakeCodeTo(phone_person), key)
}

/// TD-SEC-32 (SEC-014, SEC-020): Verify ends a code after 5 checks; the 21st try in an hour is
/// refused before Verify; the 6th send in a day to one person is refused and sends nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_32_code_brute_force_hits_the_limits(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (biz, owner, _) = business(&mut db).await;
    let calls = Calls::default();
    let codes = codes(&calls, owner);
    let pick = Uuid::new_v4();
    for n in 0..5 {
        let sent = send_confirm_code(&codes, &pool, (IP, owner, pick), (at(n), 1000)).await;
        assert_eq!(sent.expect("send"), CodeSent::Sent);
    }
    let sixth = send_confirm_code(&codes, &pool, (IP, owner, pick), (at(6), 1000)).await;
    assert_eq!(sixth.expect("send"), CodeSent::Limited);
    assert_eq!(
        calls.lock().expect("lock").0.len(),
        5,
        "a refused send reaches no Verify"
    );
    let mut answers = Vec::new();
    for _ in 0..21 {
        let tried = check_confirm_code(&codes, &pool, (IP, owner, biz), "000000", at(7)).await;
        answers.push(tried.expect("try"));
    }
    assert_eq!(answers[..5], [CodeCheck::Wrong; 5]);
    assert_eq!(answers[5..20], [CodeCheck::Ended; 15]);
    assert_eq!(answers[20], CodeCheck::Limited);
    assert_eq!(
        calls.lock().expect("lock").1,
        20,
        "a refused try reaches no Verify"
    );
}
