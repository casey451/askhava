//! TD-OPS-08, TD-OPS-09, TD-OPS-11 and TD-OPS-07's crowding flag (T524; OPS-027, OPS-030,
//! OPS-031, OPS-033, OPS-034, OPS-041, BIZ-054, CON-016, D-3880, D-5318, D-5657): the weekly
//! digest email and the owner digest view on PostgreSQL with fake data only.
//!
//! The email goes through `server::digest_email::run_digest` and the owner session proof comes
//! from the composition root's `session_minter`, so `app/server/src/lib.rs` is compiled in place
//! as `problem_complaint.rs` does (D-2853's exception, logged for this file in D-5705). No test
//! moves the test-control clock; every row is planted in the past (D-4053). Every word is
//! fictional.

use askhava_businesses::api::{AskedBy, ReasonAnswer, record_reason};
use askhava_core::api::OwnerFlagLimits;
use askhava_jobs::api::rules::CheckIn;
use askhava_jobs::api::{Answer, PROBLEM_ALERT, create, job_number, record_check_in_answer};
use askhava_messaging::api::{MailboxKind, WROTE_AGAIN_ACTION};
use askhava_ops::api::{
    Actor, Asker, AuditEntry, Change, HelpLimits, NewAlert, Severity, VOLUME_LOW_ALERT, audit,
    open_help_request, raise_alert,
};
use askhava_portals::api::{
    DigestInputs, OWNER_DIGEST_ROUTE, OwnerDigestView, is_crowded, is_no_or_other,
    owner_digest_view, owner_flags,
};
use askhava_privacy::api::{CommonWords, Known, NameMemory, keep_problem_words, scrub};
use askhava_recovery::api::{RESTORE_FAILED_ACTION, RESTORE_PASSED_ACTION};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate
/// for its `digest_email` and `session_minter` only.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the digest task and the session minter are called here"
)]
mod server;
use server::digest_email::{DIGEST_TEMPLATE, DigestMailer, DigestRun, REMINDER_TEMPLATE};
use server::*;

/// Casey's fake owner record ID.
const OWNER: Uuid = Uuid::from_u128(0x524);
/// A fictional PROBLEM reply.
const WORDS: &str = "the gate latch they fixed fell off again on the second day";

fn days(d: i64) -> SignedDuration {
    SignedDuration::from_hours(d * 24)
}

fn inputs() -> DigestInputs<'static> {
    DigestInputs {
        limits: OwnerFlagLimits::default(),
        restore_actions: [RESTORE_PASSED_ACTION, RESTORE_FAILED_ACTION],
    }
}

fn mailer() -> DigestMailer {
    DigestMailer {
        secret: SecretString::from("digest-test-key-not-a-secret".to_owned()),
        postal: (
            "PO Box 0000, Lake Havasu City, AZ 86403".to_owned(),
            MailboxKind::PoBox,
        ),
    }
}

/// What [`plant`] made: the PROBLEM job's number and the kept words.
struct Planted {
    number: i64,
    words: String,
}

/// A request closed as not covered for `trade` at `at`.
async fn not_covered(db: &mut PgConnection, trade: &str, at: Timestamp) {
    let home: Uuid = sqlx::query_scalar(
        "INSERT INTO intake.homeowner (contact_id, created_at) \
         VALUES (gen_random_uuid(), $1) RETURNING id",
    )
    .bind(at.to_sqlx())
    .fetch_one(&mut *db)
    .await
    .expect("homeowner");
    sqlx::query(
        "WITH c AS (INSERT INTO intake.conversation (homeowner_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at, closed_at) \
         VALUES ($1, 'en', gen_random_uuid(), 'consent_first', $2, $2, $2) RETURNING id) \
         INSERT INTO intake.recruiting_demand (conversation_id, town, trade, created_at) \
         SELECT id, 'lake_havasu_city', $3, $2 FROM c",
    )
    .bind(home)
    .bind(at.to_sqlx())
    .bind(trade)
    .execute(db)
    .await
    .expect("demand");
}

/// A day-3 PROBLEM on a new job at `at`: its kept words, its answer row and its alert.
async fn problem(db: &mut PgConnection, at: Timestamp) -> Planted {
    let job = create(db, at, false, Actor::System).await.expect("job");
    let common = CommonWords::default();
    let known = Known {
        common: &common,
        vault_name: None,
    };
    let scrubbed = scrub(WORDS, &known, &mut NameMemory::default());
    let inbox = Uuid::new_v4();
    let kept = keep_problem_words(db, inbox, job, &scrubbed, false).await;
    assert!(kept.expect("kept"));
    let how = (CheckIn::Day3, Answer::Problem);
    record_check_in_answer(db, job, how, Actor::System)
        .await
        .expect("answer");
    let alert = NewAlert {
        kind: PROBLEM_ALERT,
        severity: Severity::Normal,
        subject: Some(("messaging.inbox", inbox)),
    };
    raise_alert(db, &alert).await.expect("alert");
    let number = job_number(db, job).await.expect("number");
    let words = scrubbed.as_str().to_owned();
    Planted { number, words }
}

/// One row of every kind the week's digest reads that a test can plant, a day before `now`.
async fn plant(db: &mut PgConnection, now: Timestamp) -> Planted {
    let at = now - days(1);
    let business = askhava_businesses::api::create(db, false, now - days(60)).await;
    let business = business.expect("business");
    let complaint = NewAlert {
        kind: askhava_businesses::api::DIGEST_ALERT,
        severity: Severity::Normal,
        subject: Some(("businesses.business", business)),
    };
    raise_alert(db, &complaint).await.expect("complaint");
    let volume = NewAlert {
        kind: VOLUME_LOW_ALERT,
        severity: Severity::Normal,
        subject: None,
    };
    raise_alert(db, &volume).await.expect("volume");
    let again = AuditEntry {
        actor: Actor::System,
        action: WROTE_AGAIN_ACTION,
        subject_kind: "messaging.optout_event",
        subject_id: Uuid::new_v4(),
        change: Change::Action,
    };
    audit(db, &again).await.expect("wrote again");
    let fee = ReasonAnswer::parse("fee", "").expect("answer");
    let left = record_reason(db, business, (AskedBy::Left, Some(&fee)), at).await;
    assert!(left.expect("reason"));
    not_covered(db, "pool_service", at).await;
    let help = (Uuid::new_v4(), at, now + days(1));
    open_help_request(db, Asker::Email, help, HelpLimits::default())
        .await
        .expect("help");
    problem(db, at).await
}

/// One digest run at `at`.
async fn run(db: &mut PgConnection, at: Timestamp) -> DigestRun {
    let done = server::digest_email::run_digest(db, &mailer(), at).await;
    done.expect("run")
}

/// The plain text of each queued owner digest email with `template`.
async fn emails(db: &mut PgConnection, template: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT plain FROM messaging.email_outbox WHERE template_id = $1")
        .bind(template)
        .fetch_all(db)
        .await
        .expect("emails")
}

/// The item's line text in `view`, as `<count> [refs]`.
fn item(view: &OwnerDigestView, label: &str) -> String {
    let line = view.digest.lines.iter().find(|l| l.item == label);
    let line = line.expect(label);
    format!("{:?} {}", line.count, line.refs.join(","))
}

/// TD-OPS-08, TD-OPS-07 (OPS-027, D-464): 5 active businesses and a median of 4 quotes is
/// crowded, 4 businesses or a median of 3 is not; OPS-031: 31% NO or OTHER in two consecutive
/// months is flagged, 30% in either is not, and one month alone never is.
#[test]
fn td_ops_08_crowded_and_no_or_other_follow_the_settings() {
    let limits = OwnerFlagLimits::default();
    assert!(is_crowded(5, 40, &limits));
    assert!(!is_crowded(4, 40, &limits));
    assert!(!is_crowded(5, 39, &limits));
    assert!(!is_crowded(5, 30, &limits));
    assert!(is_no_or_other(&[(100, 31), (100, 31)], &limits));
    assert!(!is_no_or_other(&[(100, 30), (100, 31)], &limits));
    assert!(!is_no_or_other(&[(100, 31), (0, 0)], &limits));
    assert!(!is_no_or_other(&[(100, 31)], &limits));
}

/// TD-OPS-08 (OPS-034, D-284): 3 lapsed picks in 30 days flag the business for Casey, 2 do not,
/// and the flag changes nothing (the business stays as it was).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_08_three_lapsed_picks_flag_two_do_not(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let mut ids = Vec::new();
    for lapsed in [3, 2] {
        let b = askhava_businesses::api::create(&mut db, false, now - days(60)).await;
        let b = b.expect("business");
        for n in 0..lapsed {
            let job = create(&mut db, now - days(20), false, Actor::System).await;
            sqlx::query(
                "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, \
                 picked_at) VALUES ($1, $2, 1, 'page', 'expired', $3)",
            )
            .bind(job.expect("job"))
            .bind(b)
            .bind((now - days(10 + n)).to_sqlx())
            .execute(&mut *db)
            .await
            .expect("pick");
        }
        ids.push(b);
    }
    let status = "SELECT status FROM businesses.business WHERE id = $1";
    let before: String = sqlx::query_scalar(status)
        .bind(ids[0])
        .fetch_one(&mut *db)
        .await
        .expect("s");
    let flags = owner_flags(&mut db, now, &OwnerFlagLimits::default()).await;
    let flags = flags.expect("flags");
    assert_eq!(flags.lapsed, vec![(ids[0], 3)]);
    let after: String = sqlx::query_scalar(status)
        .bind(ids[0])
        .fetch_one(&mut *db)
        .await
        .expect("s");
    assert_eq!(before, after, "a flag changes nothing");
}

/// TD-OPS-08, TD-OPS-09, TD-OPS-11 (OPS-030, OPS-033, OPS-041, BIZ-054, CON-016, D-5318): with
/// no proof the owner view lists counts and job numbers, the open HELP request due in the next 4
/// days, the not-covered requests by trade and the leaving answer, and no PROBLEM words; with a
/// genuine owner proof it shows the fake PROBLEM words (D-5657).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_owner_view_shows_words_only_with_the_proof(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let planted = plant(&mut db, now).await;
    let at = now + SignedDuration::from_mins(1);
    let view = owner_digest_view(&mut db, None, at, &inputs())
        .await
        .expect("view");
    assert!(view.problem_words.is_empty(), "no words without the proof");
    assert_eq!(view.due_soon.len(), 1, "the HELP request due tomorrow");
    let job = format!("#{}", planted.number);
    assert_eq!(item(&view, "PROBLEM replies"), format!("Some(1) {job}"));
    assert_eq!(item(&view, "Complaints"), "Some(1) ");
    assert_eq!(item(&view, "Database volume alerts"), "Some(1) ");
    assert_eq!(
        item(&view, "Opted-out numbers that wrote again"),
        "Some(1) "
    );
    assert_eq!(
        item(&view, "Not covered, by trade"),
        "Some(1) pool_service: 1"
    );
    assert_eq!(item(&view, "Leaving answers"), "Some(1) left: the fee");
    assert_eq!(
        item(&view, "Unreadable business texts"),
        "None no reader yet"
    );
    assert!(item(&view, "Help requests open (reply by)").starts_with("Some(1) "));
    let proof = session_minter().owner(OWNER);
    let view = owner_digest_view(&mut db, Some(&proof), at, &inputs()).await;
    let view = view.expect("view");
    assert_eq!(view.problem_words.len(), 1);
    assert_eq!(view.problem_words[0].number, planted.number);
    assert_eq!(view.problem_words[0].words, planted.words);
}

/// TD-OPS-08, TD-OPS-09 (OPS-030, OPS-033, D-219): the weekly email holds every item as counts
/// and job numbers with the one owner-portal link and no PROBLEM words or contact detail; a repeat
/// run queues nothing; unopened 3 days, no reminder; 4 days, exactly one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_ops_09_weekly_email_and_one_reminder_after_4_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let now = Timestamp::now();
    let planted = plant(&mut db, now).await;
    let at = now + SignedDuration::from_mins(1);
    assert_eq!(run(&mut db, at).await, DigestRun::Sent);
    let sent = emails(&mut db, DIGEST_TEMPLATE).await;
    assert_eq!(sent.len(), 1);
    let body = &sent[0];
    for item in [
        "Automatic activations: 0",
        "Self-acknowledged alerts: 0",
        "No-shows: 0",
        "Automatic pauses (no review needed): 0",
        "License unknown: 0",
        "Opt-outs (of those texted): 2 (homeowners 0 of 0, businesses 0 of 0)",
        "Complaints: 1",
        "Dispute filings: 0",
        "Unknown charges refunded: 0",
        "Unreadable business texts: not counted (no reader yet)",
        "Call-backs due: 0",
        "Help requests open (reply by): 1",
        "HELP messages answered automatically: 0",
        "Test restore: 0",
        "Database volume alerts: 1",
        "Opted-out numbers that wrote again: 1",
        "Not covered, by trade: 1 (pool_service: 1)",
        "Leaving answers: 1 (left: the fee)",
        OWNER_DIGEST_ROUTE,
    ] {
        assert!(body.contains(item), "{item}");
    }
    assert!(body.contains(&format!("PROBLEM replies: 1 (#{})", planted.number)));
    assert!(
        !body.contains(&planted.words) && !body.contains("latch"),
        "no words"
    );
    let digits = body
        .split(|c: char| !c.is_ascii_digit())
        .map(str::len)
        .max();
    assert!(digits.unwrap_or(0) < 7, "no phone-shaped number");
    assert_eq!(run(&mut db, at).await, DigestRun::Nothing);
    assert_eq!(run(&mut db, at + days(3)).await, DigestRun::Nothing);
    assert_eq!(run(&mut db, at + days(4)).await, DigestRun::Reminded);
    assert_eq!(run(&mut db, at + days(5)).await, DigestRun::Nothing);
    assert_eq!(
        emails(&mut db, REMINDER_TEMPLATE).await.len(),
        1,
        "one reminder"
    );
}
