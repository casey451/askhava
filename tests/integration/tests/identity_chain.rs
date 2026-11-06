//! Business sign-up, the identity chain and activation across `businesses`, `payments` and the
//! fake Stripe and Verify adapters (TD-BIZ-01, TD-BIZ-05, TD-BIZ-12, TD-TERMS-08, TD-LIC-17,
//! TD-LIC-21: BIZ-001 to BIZ-003, BIZ-016, BIZ-019 to BIZ-022, BIZ-053, FEE-011, TERMS-016,
//! TERMS-033). All contacts, codes and IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, CodeChannel, IdempotencyKey, PaymentIntent, PaymentsAdapter,
    VerifyAdapter,
};
use askhava_businesses::api::{
    AboutUsRefusal, BusinessesError, EntityValues, Flag, HeardFrom, RecordContacts, SignUpForm,
    SignupError, Step, TermsShown, WAITLIST_LINE, accept_terms, approve, business, category_list,
    check_code, hand_entity_keys, letter_code, next_step, record_match, review_due, review_told,
    screen_about_us, screen_public_name, send_code, set_about_us, sign_up,
};
use askhava_licensing::api::ClassMap;
use askhava_payments::api::{BusinessId, customer_of};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const PHONE: &str = "+15555550142";
const EMAIL: &str = "owner@example.test";
const CODE: &str = "123456";
const CATS: &str = "categories:
  - {id: plumbing_repair, enabled: true, launch_open: true, job_types: [leak repair]}
  - {id: pool_cleaning, enabled: true, launch_open: false, job_types: [weekly pool service]}
  - {id: junk_removal, enabled: false, launch_open: false, job_types: [junk haul-away]}
";

/// A fake Stripe: counts customers and refuses any charge (BIZ-001: no card, no charge).
#[derive(Default)]
struct FakeStripe(Mutex<u32>);

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        let mut n = self.0.lock().expect("lock");
        *n += 1;
        Ok(format!("cus_Fake{n}"))
    }
    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("sign-up never charges (BIZ-001)")
    }
    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        panic!("sign-up never reads a charge")
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("sign-up never refunds")
    }
}

/// A fake Verify recording every send; the right code is [`CODE`] at any contact.
#[derive(Default)]
struct FakeVerify(Mutex<Vec<(String, CodeChannel)>>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.lock().expect("lock").push((to.to_owned(), channel));
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned())
}

fn at(day: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_hours(day * 24)
}

fn map() -> ClassMap {
    ClassMap::load("commercial_only: []\nclasses: []\n", CATS).expect("test catalog")
}

fn form(categories: &[&str]) -> SignUpForm {
    SignUpForm {
        categories: categories.iter().map(|c| (*c).to_owned()).collect(),
        mailbox_id: Some(Uuid::from_u128(0xe1)),
        heard_from: Some(HeardFrom::FacebookGroup),
        g_code: Some("fb-havasu".to_owned()),
    }
}

fn full_record() -> RecordContacts {
    RecordContacts {
        texting_phone: Some(SecretString::from(PHONE.to_owned())),
        email: Some(SecretString::from(EMAIL.to_owned())),
        has_address: true,
    }
}

async fn status(db: &mut PgConnection, id: Uuid) -> &'static str {
    business(db, id).await.expect("business").status.code()
}

async fn new_business(pool: &PgPool) -> Uuid {
    let stripe = FakeStripe::default();
    let s = secret();
    let (m, f) = (map(), form(&["plumbing_repair"]));
    let done = sign_up(pool, (&stripe, &s), &m, &f, at(0)).await;
    done.expect("signed up").business
}

/// TD-BIZ-01 (BIZ-001): a Stripe customer at sign-up, no card, no charge.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_01_signup_makes_a_customer_without_a_card(pool: PgPool) {
    let stripe = FakeStripe::default();
    let s = secret();
    let (m, f) = (map(), form(&["plumbing_repair"]));
    let up = sign_up(&pool, (&stripe, &s), &m, &f, at(0)).await;
    let up = up.expect("signed up");
    assert!(up.customer_ready && !up.waitlist);
    let mut db = pool.acquire().await.expect("conn");
    let customer = customer_of(&mut db, BusinessId(up.business))
        .await
        .expect("read");
    assert!(customer.is_some());
    assert_eq!(*stripe.0.lock().expect("lock"), 1);
    assert_eq!(status(&mut db, up.business).await, "pending");
}

/// TD-BIZ-01 (BIZ-002, BIZ-046, D-059): text to the record phone, then the record email, then the
/// letter, never voice; nothing is left without a record contact.
#[test]
fn td_biz_01_chain_order_from_the_record() {
    let full = full_record();
    assert_eq!(next_step(&full, None), Some(Step::Text));
    assert_eq!(next_step(&full, Some(Step::Text)), Some(Step::Email));
    assert_eq!(next_step(&full, Some(Step::Email)), Some(Step::Letter));
    assert_eq!(next_step(&full, Some(Step::Letter)), None);
    let landline = RecordContacts {
        texting_phone: None,
        ..full_record()
    };
    assert_eq!(next_step(&landline, None), Some(Step::Email));
    assert_eq!(next_step(&RecordContacts::default(), None), None);
}

/// TD-BIZ-01, TD-BIZ-05 (BIZ-002, BIZ-003, BIZ-016): codes go only to the record contacts by text
/// then email; code alone or license alone never activates; code plus match with no flag does.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_01_code_and_match_activate_without_approval(pool: PgPool) {
    let id = new_business(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    let (v, s, rec) = (FakeVerify::default(), secret(), full_record());
    let first = send_code(&mut db, (&v, &s), id, &rec, at(0))
        .await
        .expect("send");
    let second = send_code(&mut db, (&v, &s), id, &rec, at(0))
        .await
        .expect("send");
    assert_eq!((first, second), (Some(Step::Text), Some(Step::Email)));
    let sent = v.0.lock().expect("lock").clone();
    let want = vec![
        (PHONE.to_owned(), CodeChannel::Sms),
        (EMAIL.to_owned(), CodeChannel::Email),
    ];
    assert_eq!(sent, want);
    let wrong = check_code(&mut db, (&v, &s), id, &rec, ("000000", at(0))).await;
    assert!(!wrong.expect("checked"));
    assert!(
        check_code(&mut db, (&v, &s), id, &rec, (CODE, at(0)))
            .await
            .expect("checked")
    );
    assert_eq!(status(&mut db, id).await, "pending", "code alone");
    let done = record_match(&mut db, id, Some(true), None, at(1))
        .await
        .expect("match");
    assert_eq!(done.code(), "active");
    let refused = Some(SignupError::Businesses(BusinessesError::Refused));
    let again = send_code(&mut db, (&v, &s), id, &rec, at(2)).await;
    assert_eq!(again.err(), refused, "no code once active (D-1703)");
    let again = check_code(&mut db, (&v, &s), id, &rec, (CODE, at(2))).await;
    assert_eq!(again.err(), refused, "no check once active (D-1703)");
}

/// TD-BIZ-01 (LIC-011): a no-license business waiting for its city-list match activates with its
/// code when a later daily list shows its name; license alone never activates.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_01_later_city_list_match_activates(pool: PgPool) {
    let id = new_business(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    let s = record_match(&mut db, id, Some(true), None, at(0))
        .await
        .expect("match");
    assert_eq!(s.code(), "pending", "match alone");
    let id2 = new_business(&pool).await;
    let (v, sec, rec) = (FakeVerify::default(), secret(), full_record());
    send_code(&mut db, (&v, &sec), id2, &rec, at(0))
        .await
        .expect("send");
    check_code(&mut db, (&v, &sec), id2, &rec, (CODE, at(0)))
        .await
        .expect("check");
    record_match(&mut db, id2, Some(false), None, at(0))
        .await
        .expect("no match yet");
    assert_eq!(status(&mut db, id2).await, "pending");
    let s = record_match(&mut db, id2, Some(true), None, at(3))
        .await
        .expect("match");
    assert_eq!(s.code(), "active");
}

/// TD-BIZ-05, TD-BIZ-01 (BIZ-003, BIZ-016, BIZ-020): a name mismatch waits in needs review,
/// approval without the code never activates, approval with both does; the wait is told on day 0
/// and day 7, never in between.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_05_flag_waits_for_approval_and_is_told(pool: PgPool) {
    let id = new_business(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    let s = record_match(&mut db, id, Some(true), Some(Flag::NameMismatch), at(0)).await;
    assert_eq!(s.expect("flag").code(), "needs_review");
    assert_eq!(review_due(&mut db, at(0)).await.expect("due"), vec![id]);
    review_told(&mut db, id, at(0)).await.expect("told");
    assert!(review_due(&mut db, at(6)).await.expect("due").is_empty());
    assert_eq!(review_due(&mut db, at(7)).await.expect("due"), vec![id]);
    let owner = Uuid::from_u128(0xca5e);
    assert!(
        approve(&mut db, id, (owner, at(7))).await.is_err(),
        "no code"
    );
    let (v, sec) = (FakeVerify::default(), secret());
    send_code(&mut db, (&v, &sec), id, &full_record(), at(7))
        .await
        .expect("send");
    check_code(&mut db, (&v, &sec), id, &full_record(), (CODE, at(7)))
        .await
        .expect("check");
    assert_eq!(
        status(&mut db, id).await,
        "needs_review",
        "code does not skip review"
    );
    let s = approve(&mut db, id, (owner, at(8)))
        .await
        .expect("approved");
    assert_eq!(s.code(), "active");
}

/// TD-BIZ-12, TD-BIZ-01 (BIZ-002, BIZ-016, D-281): no record contact flags; a letter code entered
/// with no mismatch activates with no flag.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_12_letter_code_activates_with_no_flag(pool: PgPool) {
    let (v, sec) = (FakeVerify::default(), secret());
    let mut db = pool.acquire().await.expect("conn");
    let none = new_business(&pool).await;
    let step = send_code(&mut db, (&v, &sec), none, &RecordContacts::default(), at(0)).await;
    assert_eq!(step.expect("no step"), None);
    assert_eq!(status(&mut db, none).await, "needs_review");
    let id = new_business(&pool).await;
    let rec = RecordContacts {
        has_address: true,
        ..RecordContacts::default()
    };
    let step = send_code(&mut db, (&v, &sec), id, &rec, at(0))
        .await
        .expect("letter");
    assert_eq!(step, Some(Step::Letter));
    assert!(
        v.0.lock().expect("lock").is_empty(),
        "no Verify send for a letter"
    );
    let after = send_code(&mut db, (&v, &sec), id, &rec, at(0)).await;
    assert_eq!(after.expect("nothing left"), None);
    assert_eq!(
        status(&mut db, id).await,
        "pending",
        "the letter path is no flag"
    );
    record_match(&mut db, id, Some(true), None, at(0))
        .await
        .expect("match");
    let code = letter_code(&sec, id);
    assert_eq!(code.len(), 6);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for bad in [wrong, "", &code[..5]] {
        let r = check_code(&mut db, (&v, &sec), id, &rec, (bad, at(5))).await;
        assert!(!r.expect("checked"), "wrong letter code {bad:?}");
    }
    assert!(
        check_code(&mut db, (&v, &sec), id, &rec, (&code, at(5)))
            .await
            .expect("ok")
    );
    assert_eq!(status(&mut db, id).await, "active");
}

/// TD-BIZ-05, TD-LIC-21, TD-LIC-17 (BIZ-019, BIZ-021, LIC-046): no email refused; the category
/// list marks "Open now" and "Coming later" and leaves out switched-off ones; pest is a plain item;
/// only coming-later choices waitlist with the no-priority line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_05_category_list_waitlist_and_email(pool: PgPool) {
    let m = map();
    let listed = category_list(&m, &["plumbing_repair", "pool_cleaning", "junk_removal"]);
    let want = [
        ("plumbing_repair", "Open now"),
        ("pool_cleaning", "Coming later"),
    ];
    let want: Vec<_> = want.iter().map(|(c, l)| ((*c).to_owned(), *l)).collect();
    assert_eq!(listed, want);
    let twice = category_list(&m, &["plumbing_repair", "plumbing_repair"]);
    assert_eq!(twice.len(), 1, "a repeated category is listed once");
    let r = sign_up(
        &pool,
        (&FakeStripe::default(), &secret()),
        &m,
        &form(&["plumbing_repair", "plumbing_repair"]),
        at(0),
    )
    .await;
    assert!(!r.expect("repeat accepted").waitlist);
    let shipped = ClassMap::shipped().expect("shipped");
    let pest = category_list(shipped, &["pest_control"]);
    assert_eq!(pest, vec![("pest_control".to_owned(), "Open now")]);
    let (stripe, s) = (FakeStripe::default(), secret());
    let mut no_email = form(&["plumbing_repair"]);
    no_email.mailbox_id = None;
    let r = sign_up(&pool, (&stripe, &s), &m, &no_email, at(0)).await;
    assert_eq!(r.err(), Some(SignupError::NoEmail));
    let r = sign_up(&pool, (&stripe, &s), &m, &form(&["junk_removal"]), at(0)).await;
    assert_eq!(r.err(), Some(SignupError::BadCategories));
    let mut bad_g = form(&["plumbing_repair"]);
    bad_g.g_code = Some("Has Spaces".to_owned());
    let r = sign_up(&pool, (&stripe, &s), &m, &bad_g, at(0)).await;
    assert_eq!(r.err(), Some(SignupError::BadSourceCode));
    let up = sign_up(&pool, (&stripe, &s), &m, &form(&["pool_cleaning"]), at(0)).await;
    let up = up.expect("waitlisted");
    assert!(up.waitlist);
    let mut db = pool.acquire().await.expect("conn");
    assert_eq!(status(&mut db, up.business).await, "waitlist");
    assert!(WAITLIST_LINE.contains("Your join date is kept."));
    assert!(WAITLIST_LINE.contains("No priority; everyone waiting is told at once."));
}

/// TD-BIZ-12 (BIZ-022, PICK-007, PICK-041, WORD-003): 140 characters pass and 141 fail; channel
/// directions, phone numbers, links, claims and "licensed" on a no-license card go back with the
/// reason; a public name with a phone, link or "find us on" goes back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_12_about_us_line_is_screened(pool: PgPool) {
    let ok = "a".repeat(140);
    assert_eq!(screen_about_us(&ok, false), Ok(()));
    assert_eq!(
        screen_about_us(&"a".repeat(141), false),
        Err(AboutUsRefusal::Length)
    );
    let refused = [
        ("Find us on Google", AboutUsRefusal::Channel),
        ("Call 928 555 0142 any time", AboutUsRefusal::Contact),
        (
            "Ring nine two eight five five five",
            AboutUsRefusal::Contact,
        ),
        ("Ring 928 five five five 0142", AboutUsRefusal::Contact),
        ("See example.us today", AboutUsRefusal::Contact),
        ("See www.example.test", AboutUsRefusal::Contact),
        ("Best plumber in town", AboutUsRefusal::Claim),
        ("Known as the best in town", AboutUsRefusal::Claim),
        ("Top plumber here", AboutUsRefusal::Claim),
        ("El mejor servicio", AboutUsRefusal::Claim),
        ("Equipo verificado", AboutUsRefusal::Claim),
        ("We are #1 here", AboutUsRefusal::Claim),
        ("Vetted crew", AboutUsRefusal::Claim),
        ("Licensed and careful", AboutUsRefusal::Licensed),
    ];
    for (line, why) in refused {
        assert_eq!(screen_about_us(line, true), Err(why), "{line}");
        assert!(!why.reason().is_empty());
    }
    assert_eq!(screen_about_us("Licensed and careful", false), Ok(()));
    let whole_words = [
        "Asbestos-safe demolition",
        "We research every leak",
        "Open since 1998.Usually same week",
        "One or two crews, seven days",
    ];
    for line in whole_words {
        assert_eq!(screen_about_us(line, true), Ok(()), "{line}");
    }
    assert_eq!(
        screen_public_name("Desert Pipes 9285550142"),
        Err(AboutUsRefusal::Contact)
    );
    assert_eq!(
        screen_public_name("Find us on Yelp Pipes"),
        Err(AboutUsRefusal::Channel)
    );
    assert_eq!(screen_public_name("Desert Pipes LLC"), Ok(()));
    let id = new_business(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    let line = "Family shop since 1998, we fix leaks fast.";
    set_about_us(&mut db, id, Some(line), false)
        .await
        .expect("stored");
    let bad = set_about_us(&mut db, id, Some("Find us on Facebook"), false).await;
    assert_eq!(
        bad.err(),
        Some(SignupError::AboutUs(AboutUsRefusal::Channel))
    );
}

/// TD-TERMS-08 (TERMS-016, TERMS-033): the business terms acceptance keeps channel "business
/// sign-up", the person, version, the text's SHA-256, language and assent line, append-only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_terms_08_business_terms_acceptance_is_stored(pool: PgPool) {
    let id = new_business(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    let person = Uuid::from_u128(0x9e);
    let shown = TermsShown {
        document: "business_terms",
        version: "2026-11-01",
        published_text: "abc",
        language: "en",
        assent_line: "I agree to the Ask Hava business terms.",
    };
    let row = accept_terms(&mut db, (id, person), shown, at(0))
        .await
        .expect("stored");
    let got: (String, Uuid, String, String, String) = sqlx::query_as(
        "SELECT channel, person_id, version, text_sha256, assent_line \
         FROM businesses.terms_acceptance WHERE id = $1",
    )
    .bind(row)
    .fetch_one(&mut *db)
    .await
    .expect("read");
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(got.0, "business_signup");
    assert_eq!(
        (got.1, got.2.as_str(), got.3.as_str()),
        (person, "2026-11-01", sha)
    );
    assert_eq!(got.4, shown.assent_line);
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let upd = sqlx::query("UPDATE businesses.terms_acceptance SET version = 'x'");
    assert!(upd.execute(&mut *db).await.is_err(), "append-only");
}

/// FEE-011, D-151: sign-up hands the ROC number, city entity, identity contact and owner name to
/// `payments` as digests only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn fee_011_signup_hands_entity_keys(pool: PgPool) {
    let id = new_business(&pool).await;
    let mut db = pool.acquire().await.expect("conn");
    let (contact, owner) = (
        SecretString::from(PHONE.to_owned()),
        SecretString::from("Pat Example".to_owned()),
    );
    let values = EntityValues {
        roc: Some("ROC 999001"),
        city_entity: Some("Example Pipes LLC"),
        identity_contact: Some(&contact),
        owner_name: Some(&owner),
    };
    hand_entity_keys(&mut db, &secret(), id, &values)
        .await
        .expect("stored");
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payments.entity_key WHERE business_id = $1")
            .bind(id)
            .fetch_one(&mut *db)
            .await
            .expect("count");
    assert_eq!(n, 4);
    let leaked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payments.entity_key WHERE digest LIKE '%555%'")
            .fetch_one(&mut *db)
            .await
            .expect("count");
    assert_eq!(leaked, 0);
}
