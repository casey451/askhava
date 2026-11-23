//! T745 (CARD-023, FEE-013, FEE-036, PRIV-011, PRIV-001, PICK-022; D-4748 (5a), D-5317, D-5553):
//! `intake::api::job_answers` reads each job's own trade-question answers, open or closed, never
//! a contact answer; `intake.request_job` keeps every job a request opened after its close, so
//! `jobs_of_contact` and the Confirm page's same-customer check see an INT-032 ALSO-reply job; a
//! later job's purge deletes only its own `bank.<job hex>.` rows, and the contact purge sees a
//! hold on, and finds her as owner of, an ALSO-reply job after the close. Fake data only: every ID,
//! contact and amount is made up, and no words are stored.

use askhava_core::api::Claimed;
use askhava_intake::api::{
    JobAnswer, JobStart, Opened, begin_job, close_from_page, job_answers, job_homeowner,
    jobs_of_contact, purge_answers, run_purge_at,
};
use askhava_payments::api::{
    BusinessId, EntryKind, FeeJob, FeeMode, FeeQuote, JoinDates, LedgerEntry, NewEntry, PeriodJob,
    PickedFee, RecurringCap, Service, VisitTier, Waiver, WinCheck, fee_at_pick, ledger_append,
    waive_at_confirm,
};
use askhava_privacy::api::{
    HoldSource, NewContact, PURGE_KIND, Vault, place_hold, release_hold, store_contact,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb745_0001));
const HER: Uuid = Uuid::from_u128(0x0745_c001);
const WORDS: &str = "the kitchen sink is leaking under the cabinet and the air is not cooling";

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

/// A new request of the homeowner with vault contact `contact`, opened at `at(minute)`.
async fn request(db: &mut PgConnection, contact: Uuid, minute: i64) -> Uuid {
    let known: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM intake.homeowner WHERE contact_id = $1 LIMIT 1")
            .bind(contact)
            .fetch_optional(&mut *db)
            .await
            .expect("her homeowner row");
    let homeowner: Uuid = match known {
        Some(id) => id,
        None => sqlx::query_scalar(
            "INSERT INTO intake.homeowner (contact_id, created_at) VALUES ($1, now()) RETURNING id",
        )
        .bind(contact)
        .fetch_one(&mut *db)
        .await
        .expect("homeowner"),
    };
    sqlx::query_scalar(
        "INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at) VALUES ($1, 'en', gen_random_uuid(), \
         'consent_first', $2, $2) RETURNING id",
    )
    .bind(homeowner)
    .bind(at(minute).to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("conversation")
}

/// Opens a job of `conversation` for `trade` through `begin_job` (the first job, or the second
/// trade of an INT-032 ALSO reply, which comes through the same writer).
async fn job(db: &mut PgConnection, conversation: Uuid, trade: &str, minute: i64) -> Uuid {
    let start = JobStart {
        conversation,
        words: WORDS,
        emergency: false,
        trade,
        at: at(minute),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    job
}

/// Plants one stored answer as the intake writers store it (a code, never her words).
async fn answer(
    db: &mut PgConnection,
    conversation: Uuid,
    (question, choice): (&str, &str),
    minute: i64,
) {
    sqlx::query(
        "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(conversation)
    .bind(question)
    .bind(choice)
    .bind(at(minute).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("answer");
}

fn later(job: Uuid, question: &str) -> String {
    format!("bank.{}.{question}", job.simple())
}

fn pairs(answers: &[JobAnswer]) -> Vec<(&str, &str)> {
    answers
        .iter()
        .map(|a| (a.question.as_str(), a.value.as_str()))
        .collect()
}

async fn answer_rows(db: &mut PgConnection, conversation: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT question_id FROM intake.intake_answer WHERE conversation_id = $1 ORDER BY question_id",
    )
    .bind(conversation)
    .fetch_all(&mut *db)
    .await
    .expect("rows")
}

/// A request with a plumbing first job and an HVAC ALSO-reply job, each with its own answers
/// (both answered `details`), and the step-11 contact rows.
async fn two_job_request(db: &mut PgConnection, contact: Uuid) -> (Uuid, Uuid, Uuid) {
    let request = request(db, contact, 0).await;
    let first = job(db, request, "plumbing_repair", 1).await;
    let second = job(db, request, "hvac_repair", 2).await;
    answer(db, request, ("bank.details", "text"), 3).await;
    answer(db, request, ("bank.fixture", "1"), 4).await;
    answer(db, request, ("bank.fixture", "2"), 5).await;
    answer(db, request, (&later(second, "symptom"), "1"), 6).await;
    answer(db, request, (&later(second, "details"), "not_sure"), 7).await;
    answer(db, request, ("address_contact", "text"), 8).await;
    answer(db, request, ("name_address_asked", "yes"), 9).await;
    (request, first, second)
}

/// CARD-023, PRIV-001, D-5553: the first job and the ALSO-reply job each read their own latest
/// answers in question-bank order before and after the request closes; no contact answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn each_job_reads_its_own_answers_before_and_after_the_close(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, first, second) = two_job_request(&mut db, HER).await;
    let first_reads = [("fixture", "2"), ("details", "text")];
    let second_reads = [("symptom", "1"), ("details", "not_sure")];
    for closed in [false, true] {
        let mine = job_answers(&mut db, first).await.expect("first");
        assert_eq!(pairs(&mine), first_reads, "closed {closed}");
        let theirs = job_answers(&mut db, second).await.expect("second");
        assert_eq!(pairs(&theirs), second_reads, "closed {closed}");
        for read in [&mine, &theirs] {
            assert!(read.iter().all(|a| !a.question.contains("address")));
            assert!(read.iter().all(|a| !a.question.contains("name")));
        }
        assert!(
            close_from_page(&mut db, first, at(20))
                .await
                .expect("close first")
                != closed
        );
        assert!(
            close_from_page(&mut db, second, at(21))
                .await
                .expect("close second")
                != closed
        );
    }
    let none = job_answers(&mut db, Uuid::from_u128(0x0745_dead)).await;
    assert_eq!(none.expect("no request"), []);
}

/// PRIV-011, FEE-013 (D-3719's limit closed): after the close drops the job links,
/// `jobs_of_contact` still names both jobs, and only hers.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn jobs_of_contact_keeps_the_also_reply_job_after_the_close(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, first, second) = two_job_request(&mut db, HER).await;
    let other = Uuid::from_u128(0x0745_c002);
    let (_, theirs, _) = two_job_request(&mut db, other).await;
    close_from_page(&mut db, first, at(20))
        .await
        .expect("close");
    close_from_page(&mut db, second, at(21))
        .await
        .expect("close");
    let mut jobs = jobs_of_contact(&mut db, HER).await.expect("jobs");
    jobs.sort();
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(jobs, expected);
    assert!(!jobs.contains(&theirs));
}

fn picked(cents: u64) -> PickedFee {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    fee_at_pick(FeeQuote::Fixed(cents), None, job, FeeMode::Tiers, None).expect("fee")
}

/// The business's earlier paid introduction on `on`, as Confirm-and-pay records it.
async fn charged(db: &mut PgConnection, on: Uuid) {
    let key = format!("fake-charge-{on}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::Charge,
        business: Some(BUSINESS.0),
        job: Some(on),
        amount_cents: 7_500,
        stripe_id: Some("ch_FakeT745"),
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(db, &entry).await.expect("append");
}

/// FEE-013, FEE-036, D-5317 (the risky order): the earlier paid pick is on an ALSO-reply job of a
/// request that is now closed; a later pick of the same business on her new request is found by
/// the Confirm route's same-customer check (`job_homeowner`, then `jobs_of_contact`, as
/// `portals`' `her_jobs` feeds `same_customer_jobs`), waived as Repeat, and uses no free first win.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_repeat_on_an_also_reply_job_of_a_closed_request_is_waived(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, first, also) = two_job_request(&mut db, HER).await;
    charged(&mut db, also).await;
    close_from_page(&mut db, first, at(20))
        .await
        .expect("close");
    close_from_page(&mut db, also, at(21)).await.expect("close");
    let again = request(&mut db, HER, 30).await;
    let new_job = job(&mut db, again, "plumbing_repair", 31).await;
    let her = job_homeowner(&mut db, new_job)
        .await
        .expect("read")
        .expect("her");
    let jobs = jobs_of_contact(&mut db, her.contact).await.expect("jobs");
    assert!(jobs.contains(&also));
    let fee = picked(90_000);
    let launch = at(-60 * 24);
    let dates = JoinDates {
        joined: launch,
        launch,
        trades_opened: Vec::new(),
    };
    let check = WinCheck {
        business: BUSINESS,
        job: new_job,
        picked: &fee,
        dates: &dates,
        same_customer_jobs: &jobs,
    };
    let secret = SecretString::from("fake-id-secret-t745".to_owned());
    let waived = waive_at_confirm(&mut db, &secret, check)
        .await
        .expect("waive");
    assert!(matches!(waived, Waiver::Repeat(_)));
    let wins: i64 = sqlx::query_scalar("SELECT count(*) FROM payments.free_win")
        .fetch_one(&mut *db)
        .await
        .expect("wins");
    assert_eq!(wins, 0);
}

/// PRIV-008, D-5553: two jobs of one request that both answered `details` each read their own;
/// purging the second job leaves the first job's rows; after both are purged, in either order,
/// the request has no answer row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_later_jobs_purge_keeps_the_first_jobs_answers(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (request, first, second) = two_job_request(&mut db, HER).await;
    let before = answer_rows(&mut db, request).await;
    purge_answers(&mut db, second).await.expect("purge second");
    let kept = answer_rows(&mut db, request).await;
    let theirs = [later(second, "details"), later(second, "symptom")];
    let expected: Vec<String> = before.into_iter().filter(|q| !theirs.contains(q)).collect();
    assert_eq!(kept, expected);
    assert_eq!(
        pairs(&job_answers(&mut db, first).await.expect("first")),
        [("fixture", "2"), ("details", "text")]
    );
    assert_eq!(job_answers(&mut db, second).await.expect("second"), []);
    purge_answers(&mut db, first).await.expect("purge first");
    assert!(answer_rows(&mut db, request).await.is_empty());
    let other = Uuid::from_u128(0x0745_c003);
    let (request, first, second) = two_job_request(&mut db, other).await;
    purge_answers(&mut db, first).await.expect("purge first");
    assert!(answer_rows(&mut db, request).await.is_empty());
    assert_eq!(
        purge_answers(&mut db, second).await.expect("purge second"),
        0
    );
}

fn vault() -> Vault {
    let key = |byte: u8| SecretString::from(format!("1:{}", STANDARD.encode([byte; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid:5432/askhava".to_owned());
    Vault::new(&key(0x31), &key(0x41), &url).expect("vault keys")
}

/// A fake vault contact with a name and an email (no phone, so no consent rule keeps a field).
async fn vault_contact(db: &mut PgConnection) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Zelda Canary".to_owned())),
        phone: None,
        email: Some(SecretString::from("zelda@example.com".to_owned())),
        address: None,
        onsite_contact: None,
    };
    store_contact(db, &vault(), &new).await.expect("contact")
}

/// Whether the contact's name and email are cleared.
async fn cleared(db: &mut PgConnection, contact: Uuid) -> (bool, bool) {
    sqlx::query_as("SELECT name IS NULL, email IS NULL FROM privacy.contact WHERE id = $1")
        .bind(contact)
        .fetch_one(&mut *db)
        .await
        .expect("contact row")
}

fn contact_stage(job: Uuid) -> Claimed {
    Claimed {
        id: 1,
        kind: PURGE_KIND.to_owned(),
        payload: json!({ "job": job, "stage": "contact" }),
        attempts: 1,
    }
}

/// PRIV-007, D-1839, D-5553: a dispute hold on the ALSO-reply job of her closed request keeps her
/// contact when either job's contact stage runs 181 days after the close (the hold check reads her
/// `intake.request_job` jobs); once the hold is released, the ALSO-reply job's own contact stage
/// finds her as its owner and clears the contact.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_hold_on_an_also_reply_job_keeps_her_contact_past_180_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let contact = vault_contact(&mut db).await;
    let (_, first, also) = two_job_request(&mut db, contact).await;
    close_from_page(&mut db, first, at(20))
        .await
        .expect("close");
    close_from_page(&mut db, also, at(21)).await.expect("close");
    let now = at(21) + SignedDuration::from_hours(24 * 181);
    let dispute = Uuid::from_u128(0x0745_0d15);
    place_hold(&mut db, HoldSource::Dispute, dispute, also, now)
        .await
        .expect("hold");
    for job in [also, first] {
        run_purge_at(pool.clone(), contact_stage(job), now)
            .await
            .expect("held purge");
        assert_eq!(cleared(&mut db, contact).await, (false, false), "{job}");
    }
    release_hold(&mut db, HoldSource::Dispute, dispute, also, now)
        .await
        .expect("release");
    run_purge_at(pool.clone(), contact_stage(also), now)
        .await
        .expect("purge");
    assert_eq!(cleared(&mut db, contact).await, (true, true));
}
