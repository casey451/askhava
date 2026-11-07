//! The business sign-up screens across `businesses`, `quotes`, `jobs` and `portals` (TD-BIZ-07,
//! TD-BIZ-10, TD-BIZ-11, TD-BIZ-12, TD-BIZ-27, TD-PICK-33: BIZ-024, BIZ-026, BIZ-027, BIZ-030,
//! BIZ-031, LIC-047, WEB-024). All contacts, codes and IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, CodeChannel, IdempotencyKey, PaymentIntent, PaymentsAdapter,
    VerifyAdapter,
};
use askhava_businesses::api::{
    ContactKind, HeardFrom, MaskedContact, RECORD_UPDATED_TEMPLATE, RecordContacts, RecordSeen,
    SOMEONE_ELSE_LINE, SignUpForm, SignupSettings, Step, claim_launch_notice, fee_summary,
    number_is_old, resume_on_new_record, review_target, review_target_line, send_code, sign_up,
    waiting_on_record,
};
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::ClassMap;
use askhava_messaging::api::{Language, Registry};
use askhava_ops::api::Actor;
use askhava_payments::api::Tier;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    DoneFacts, PageLanguage, Parts, Screen, done_facts, done_lines, fee_numbers, signup_page,
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
const CATS: &str = "categories:
  - {id: plumbing_repair, enabled: true, launch_open: true, job_types: [leak repair]}
  - {id: pool_cleaning, enabled: true, launch_open: false, job_types: [weekly pool service]}
";

/// A fake Stripe that makes customers and never charges.
#[derive(Default)]
struct FakeStripe;

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_Fake1".to_owned())
    }
    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("sign-up never charges")
    }
    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        panic!("sign-up never reads a charge")
    }
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("sign-up never refunds")
    }
}

/// A fake Verify recording where each code went.
#[derive(Default)]
struct FakeVerify(Mutex<Vec<String>>);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.lock().expect("lock").push(to.to_owned());
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, _: &str) -> Result<bool, AdapterError> {
        Ok(false)
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-for-tests-only-0123456789".to_owned())
}

fn at(day: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000).expect("time") + SignedDuration::from_hours(day * 24)
}

fn record(phone: &str) -> RecordContacts {
    RecordContacts {
        texting_phone: Some(SecretString::from(phone.to_owned())),
        email: Some(SecretString::from(EMAIL.to_owned())),
        has_address: true,
    }
}

fn seen<'a>(contacts: &'a RecordContacts, address: &'a SecretString) -> RecordSeen<'a> {
    RecordSeen {
        contacts,
        address: Some(address),
    }
}

fn fake(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

fn parts(texting_on: bool, language: PageLanguage) -> Parts<'static> {
    let fees = fee_numbers(ConfirmSettings::default(), WindowRules::default());
    Parts {
        texting_on,
        language,
        csrf: "fake-csrf",
        fees,
    }
}

async fn signed_up(pool: &PgPool, category: &str) -> Uuid {
    let map = ClassMap::load("commercial_only: []\nclasses: []\n", CATS).expect("catalog");
    let form = SignUpForm {
        categories: vec![category.to_owned()],
        mailbox_id: Some(Uuid::from_u128(0xe1)),
        heard_from: Some(HeardFrom::Search),
        g_code: None,
    };
    let s = secret();
    let up = sign_up(pool, (&FakeStripe, &s), &map, &form, at(0)).await;
    up.expect("signed up").business
}

async fn make_active(db: &mut PgConnection, business: Uuid, category: &str) {
    sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
        .bind(business)
        .execute(&mut *db)
        .await
        .expect("active");
    sqlx::query("INSERT INTO businesses.job_type_choice (business_id, category, job_type) VALUES ($1, $2, 'leak repair')")
        .bind(business)
        .bind(category)
        .execute(&mut *db)
        .await
        .expect("job type");
}

async fn open_job_with_card(db: &mut PgConnection, business: Uuid) -> Uuid {
    let job = create(db, at(1), false, Actor::System).await.expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(db, job, open, at(1), Actor::System)
        .await
        .expect("open");
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, now())")
        .bind(job)
        .bind(business)
        .execute(&mut *db)
        .await
        .expect("card");
    job
}

/// TD-BIZ-07 (BIZ-024, BIZ-026, BIZ-046, D-366): the masked record contact and its kind show
/// before any code; a landline is never called; no full contact reaches the page.
#[test]
fn td_biz_07_masked_contact_and_kind_before_any_code() {
    let rec = record(PHONE);
    let text =
        MaskedContact::for_step(&rec, Step::Text, ("Lake Havasu City", "86403")).expect("phone");
    assert_eq!(
        (text.kind(), text.shown()),
        (ContactKind::Mobile, "(•••) •••-0142")
    );
    let mail = MaskedContact::for_step(&rec, Step::Email, ("", "")).expect("email");
    assert_eq!(
        (mail.kind(), mail.shown()),
        (ContactKind::Email, "o•••@example.test")
    );
    let land = MaskedContact::phone(&SecretString::from(PHONE.to_owned()), true);
    assert_eq!(land.kind(), ContactKind::Landline);
    assert!(
        land.line()
            .contains("so we can't text it, and we never call")
    );
    let page = signup_page(
        Screen::Contact(&mail, Some(&land)),
        &parts(false, PageLanguage::En),
    );
    let page = page.expect("page");
    assert!(page.contains("is a landline") && page.contains("o•••@example.test"));
    assert!(page.contains("That number is old") && page.contains(SOMEONE_ELSE_LINE));
    assert!(!page.contains(PHONE) && !page.contains(EMAIL) && !page.contains("5550142"));
    let none = RecordContacts::default();
    assert!(MaskedContact::for_step(&none, Step::Letter, ("x", "y")).is_none());
}

/// TD-BIZ-07, TD-BIZ-10 (BIZ-026, D-178, D-254): "That number is old" saves the sign-up; the
/// same record keeps it waiting; a new record contact resumes it with the code to that contact.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_07_number_old_waits_and_a_new_record_resumes(pool: PgPool) {
    let id = signed_up(&pool, "plumbing_repair").await;
    let mut db = pool.acquire().await.expect("conn");
    let (v, s) = (FakeVerify::default(), secret());
    let (old, addr) = (record(PHONE), fake(ADDRESS));
    let sent = send_code(&mut db, (&v, &s), id, &old, at(0)).await;
    assert_eq!(sent.expect("sent"), Some(Step::Text));
    number_is_old(&mut db, &s, (id, seen(&old, &addr)), at(0))
        .await
        .expect("saved");
    assert!(waiting_on_record(&mut db, id).await.expect("read"));
    let same = resume_on_new_record(&mut db, &s, (id, seen(&old, &addr))).await;
    assert!(
        !same.expect("same list"),
        "an unchanged record keeps waiting"
    );
    let new = record(NEW_PHONE);
    assert!(
        resume_on_new_record(&mut db, &s, (id, seen(&new, &addr)))
            .await
            .expect("resumed")
    );
    assert!(!waiting_on_record(&mut db, id).await.expect("read"));
    let again = send_code(&mut db, (&v, &s), id, &new, at(1)).await;
    assert_eq!(again.expect("sent"), Some(Step::Text));
    let to = v.0.lock().expect("lock").clone();
    assert_eq!(to, vec![PHONE.to_owned(), NEW_PHONE.to_owned()]);
    let registry = Registry::embedded().expect("templates");
    let (en, _) = registry
        .template(RECORD_UPDATED_TEMPLATE, Language::En)
        .expect("en");
    assert_eq!(
        en,
        "Ask Hava: We see your updated record. Tap to finish sign-up: {link}."
    );
    assert!(
        registry
            .template(RECORD_UPDATED_TEMPLATE, Language::Es)
            .is_some()
    );
}

/// TD-BIZ-07, TD-BIZ-10 (BIZ-026, D-1712): a record whose only change is its email resumes at the
/// email step, so the code goes to the new email and never again to the old phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_10_an_email_only_change_resumes_at_the_email(pool: PgPool) {
    let id = signed_up(&pool, "plumbing_repair").await;
    let mut db = pool.acquire().await.expect("conn");
    let (v, s, addr) = (FakeVerify::default(), secret(), fake(ADDRESS));
    let old = record(PHONE);
    let sent = send_code(&mut db, (&v, &s), id, &old, at(0)).await;
    assert_eq!(sent.expect("sent"), Some(Step::Text));
    let saved = number_is_old(&mut db, &s, (id, seen(&old, &addr)), at(0)).await;
    saved.expect("saved");
    let new = RecordContacts {
        email: Some(fake(NEW_EMAIL)),
        ..record(PHONE)
    };
    let resumed = resume_on_new_record(&mut db, &s, (id, seen(&new, &addr))).await;
    assert!(resumed.expect("resumed"));
    let again = send_code(&mut db, (&v, &s), id, &new, at(1)).await;
    assert_eq!(again.expect("sent"), Some(Step::Email));
    let to = v.0.lock().expect("lock").clone();
    assert_eq!(to, vec![PHONE.to_owned(), NEW_EMAIL.to_owned()]);
}

/// TD-BIZ-10, TD-BIZ-12 (BIZ-026, D-254, D-1712): a business that fixes only its mailing address on
/// the letter screen resumes at the letter step; a list with no present changed contact (a phone
/// dropped from the record) keeps it waiting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_10_an_address_only_change_resumes_at_the_letter(pool: PgPool) {
    let id = signed_up(&pool, "plumbing_repair").await;
    let mut db = pool.acquire().await.expect("conn");
    let (v, s) = (FakeVerify::default(), secret());
    let old = RecordContacts {
        has_address: true,
        ..RecordContacts::default()
    };
    let (addr, new_addr) = (fake(ADDRESS), fake(NEW_ADDRESS));
    let sent = send_code(&mut db, (&v, &s), id, &old, at(0)).await;
    assert_eq!(sent.expect("letter"), Some(Step::Letter));
    let saved = number_is_old(&mut db, &s, (id, seen(&old, &addr)), at(0)).await;
    saved.expect("saved");
    let none = RecordSeen {
        contacts: &old,
        address: None,
    };
    let gone = resume_on_new_record(&mut db, &s, (id, none)).await;
    assert!(!gone.expect("read"), "a dropped address is not a new one");
    let same = resume_on_new_record(&mut db, &s, (id, seen(&old, &addr))).await;
    assert!(!same.expect("read"), "the same address keeps waiting");
    let moved = resume_on_new_record(&mut db, &s, (id, seen(&old, &new_addr))).await;
    assert!(moved.expect("resumed"));
    assert!(!waiting_on_record(&mut db, id).await.expect("read"));
    let again = send_code(&mut db, (&v, &s), id, &old, at(1)).await;
    assert_eq!(again.expect("letter"), Some(Step::Letter));
    assert!(
        v.0.lock().expect("lock").is_empty(),
        "no text or email went out"
    );
}

/// TD-BIZ-07, TD-BIZ-10, TD-BIZ-12 (BIZ-026, BIZ-027, D-223, D-254, D-288): the review target is
/// 4 calendar days and blocks nothing; the letter step shows town and ZIP and the stop button;
/// "That number is old" shows the typical delay.
#[test]
fn td_biz_10_review_target_letter_step_and_delay() {
    let settings = SignupSettings::from_pairs(Vec::new()).expect("defaults");
    assert_eq!(settings.review_target_days, 4);
    let since = at(0);
    let due = review_target(since, settings.review_target_days);
    assert_eq!(due, since + SignedDuration::from_hours(96));
    let line = review_target_line(since, 4);
    assert!(line.contains("4 calendar days") && line.contains("Nothing is held up"));
    let bad = vec![("ASKHAVA_BIZ_REVIEW_TARGET_DAYS".to_owned(), "0".to_owned())];
    assert!(SignupSettings::from_pairs(bad).is_err());
    let rec = record(PHONE);
    let letter = MaskedContact::for_step(&rec, Step::Letter, ("Lake Havasu City", "86403"))
        .expect("address");
    let page =
        signup_page(Screen::Letter(&letter, 3), &parts(false, PageLanguage::En)).expect("page");
    assert!(page.contains("address on your record in Lake Havasu City 86403"));
    assert!(
        page.contains("Stop, I&#39;ll fix my record first")
            || page.contains("Stop, I&#x27;ll fix my record first")
    );
    let old = signup_page(Screen::NumberOld(3), &parts(false, PageLanguage::En)).expect("page");
    assert!(old.contains("about 3 days after you make them"));
}

/// TD-BIZ-07, TD-BIZ-11, TD-BIZ-12, TD-BIZ-27, TD-PICK-33 (BIZ-030, D-173, D-205, D-248): the
/// plain-words box holds every BIZ-030 line in order, its numbers equal the fee settings, a changed
/// setting changes the copy, and it sits above the Terms link.
#[test]
fn td_biz_27_fee_box_lines_and_numbers_follow_the_settings() {
    let fees = fee_numbers(ConfirmSettings::default(), WindowRules::default());
    let tiers = [Tier::Small, Tier::Medium, Tier::Large].map(|t| format!("${}", t.cents() / 100));
    let lines = fee_summary(&fees);
    let first = format!(
        "Free to join. Pay only when you confirm a pick: {} / {} / {} by job size. Refund if you can't reach them.",
        tiers[0], tiers[1], tiers[2]
    );
    assert_eq!(lines[0], first);
    assert_eq!(
        lines[0],
        "Free to join. Pay only when you confirm a pick: $25 / $75 / $199 by job size. Refund if you can't reach them."
    );
    assert_eq!(
        lines[1],
        "Picks must be confirmed within 4 business hours (8 a.m. to 8 p.m., every day), or within 60 minutes for an emergency job. If you check email only once or twice a day, add a backup address for someone who checks more often."
    );
    for want in [
        "It's an introduction fee for a homeowner who chose you.",
        "You pay one fixed fee when you confirm a pick: $25, $75 or $199 by job size. It's not a share of the job. If you call twice inside the homeowner's window and can't reach them, the refund rule gives the fee back without you asking.",
        "It's a fixed introduction fee, never a share of the job, and it doesn't depend on the work being done or paid for.",
        "Ask Hava is not a contractor. It passes along this request and the homeowner's choice. If the homeowner picks you, they hire you directly; Ask Hava doesn't direct, schedule, supervise or pay for the work.",
        "The homeowner sees every quote and picks one business: you. It isn't shared with anyone else.",
        "A new fee never applies until you accept it.",
        "Repeat picks from the same homeowner are free for 180 days after the job closes.",
        "Fees follow the published schedule, and a change needs your OK first.",
        "Statements download as CSV.",
        "If many picks in your trade end without a hire, the Ask Hava owner reviews it and may add a credit.",
    ] {
        assert!(lines.contains(&want.to_owned()), "missing: {want}");
    }
    let longer = ConfirmSettings {
        window_minutes: 300,
    };
    let moved = fee_summary(&fee_numbers(longer, WindowRules::default()));
    assert!(moved[1].starts_with("Picks must be confirmed within 5 business hours ("));
}

/// TD-BIZ-27, TD-PICK-33 (BIZ-030, LIC-047, D-1713): the fee box sits above the Terms link; the
/// pest fields show for pest control and carry no `required`; categories read in sentence case.
#[test]
fn td_biz_27_fee_box_above_terms_and_pest_fields() {
    let cats = vec![
        ("plumbing_repair".to_owned(), "Open now"),
        ("pest_control".to_owned(), "Open now"),
    ];
    let page = signup_page(Screen::Form(&cats), &parts(false, PageLanguage::En)).expect("page");
    let (fee_at, terms_at) = (
        page.find("id=\"fee-summary\""),
        page.find("id=\"terms-link\""),
    );
    assert!(fee_at.expect("fee box") < terms_at.expect("terms link"));
    assert!(page.contains(
        "I hold an active Arizona Department of Agriculture pest-management business license"
    ));
    assert!(page.contains("id=\"pest-number\"") && page.contains("weed control"));
    assert!(
        !page.contains("required"),
        "the server checks the pest fields (LIC-047)"
    );
    assert!(page.contains("Plumbing repair (Open now)"));
}

/// TD-BIZ-27 (BIZ-030, D-1713): a Confirm window that is not whole hours is never cut down: it
/// shows as hours and minutes, or minutes alone under an hour.
#[test]
fn td_biz_27_fee_box_keeps_leftover_minutes() {
    let odd = ConfirmSettings {
        window_minutes: 270,
    };
    let odd = fee_summary(&fee_numbers(odd, WindowRules::default()));
    assert!(odd[1].starts_with("Picks must be confirmed within 4 business hours and 30 minutes ("));
    let short = ConfirmSettings { window_minutes: 45 };
    let short = fee_summary(&fee_numbers(short, WindowRules::default()));
    assert!(short[1].starts_with("Picks must be confirmed within 45 business minutes ("));
}

/// TD-BIZ-10 (WEB-024, D-307, D-493): the switch-off line by default and the switch-on line when
/// business texting is on, in English and Spanish.
#[test]
fn td_biz_10_notice_line_follows_the_texting_switch() {
    let cases = [
        (
            false,
            PageLanguage::En,
            "Job requests and picks come by email. We don&#39;t text or call businesses.",
        ),
        (
            true,
            PageLanguage::En,
            "Choose how you get job requests and picks: email, text or both.",
        ),
        (
            false,
            PageLanguage::Es,
            "No enviamos mensajes de texto ni llamamos a los negocios.",
        ),
        (
            true,
            PageLanguage::Es,
            "Ask Hava nunca le llama con una grabación.",
        ),
    ];
    for (on, language, want) in cases {
        let page = signup_page(Screen::NumberOld(3), &parts(on, language)).expect("page");
        let plain = page.replace("&#x27;", "&#39;");
        assert!(plain.contains(want), "{on} {want}");
        let es = plain.contains("<p id=\"notice-line\" lang=\"es\">");
        assert_eq!(
            es,
            language == PageLanguage::Es,
            "the Spanish line is marked lang=es"
        );
    }
}

/// TD-BIZ-07 (BIZ-031, CARD-018, D-187, D-462, D-492): the done screen counts the trade's
/// businesses; an active business with 2 open jobs sees them first with a link to each; the launch
/// notice is claimed once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_07_done_screen_counts_and_leads_with_open_jobs(pool: PgPool) {
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(signed_up(&pool, "plumbing_repair").await);
    }
    let mut db = pool.acquire().await.expect("conn");
    for id in &ids[..2] {
        make_active(&mut db, *id, "plumbing_repair").await;
    }
    let new = ids[2];
    let trade = ("plumbing_repair", "plumbing");
    let facts = done_facts(&mut db, new, trade, (false, false))
        .await
        .expect("facts");
    let (lines, links) = done_lines(&facts);
    assert_eq!(lines, vec!["You're one of 3 businesses in plumbing so far. We'll tell you once, by email, when requests in your trade start.".to_owned()]);
    assert!(links.is_empty());
    make_active(&mut db, new, "plumbing_repair").await;
    let jobs = [
        open_job_with_card(&mut db, new).await,
        open_job_with_card(&mut db, new).await,
    ];
    let facts = done_facts(&mut db, new, trade, (false, true))
        .await
        .expect("facts");
    let (lines, links) = done_lines(&facts);
    assert_eq!(lines[0], "2 open plumbing jobs you can quote now");
    let want: Vec<String> = jobs.iter().map(|j| format!("/b/jobs/{j}")).collect();
    assert_eq!(
        links.iter().map(|(h, _)| h.clone()).collect::<Vec<_>>(),
        want
    );
    assert!(
        claim_launch_notice(&mut db, new, at(2))
            .await
            .expect("first")
    );
    assert!(
        !claim_launch_notice(&mut db, new, at(3))
            .await
            .expect("second"),
        "one notice only"
    );
}

/// TD-BIZ-10 (BIZ-021, BIZ-031, D-261): the waitlist done screen counts the businesses waiting
/// for the trade; the first one reads "the first".
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_10_waitlist_done_screen(pool: PgPool) {
    let first = signed_up(&pool, "pool_cleaning").await;
    let mut db = pool.acquire().await.expect("conn");
    let trade = ("pool_cleaning", "pool cleaning");
    let one = done_facts(&mut db, first, trade, (true, false))
        .await
        .expect("facts");
    assert_eq!(
        done_lines(&one).0[0],
        "You're on the waitlist for pool cleaning, the first business waiting. We'll email you once when it opens; your join date is kept."
    );
    let second = signed_up(&pool, "pool_cleaning").await;
    let two = done_facts(&mut db, second, trade, (true, false))
        .await
        .expect("facts");
    assert_eq!(
        done_lines(&two).0[0],
        "You're on the waitlist for pool cleaning, one of 2 businesses waiting. We'll email you once when it opens; your join date is kept."
    );
    let alone = DoneFacts {
        trade: "plumbing".to_owned(),
        count: 1,
        waitlist: false,
        open_jobs: Vec::new(),
    };
    assert!(done_lines(&alone).0[0].starts_with("You're the first plumbing business so far."));
}
