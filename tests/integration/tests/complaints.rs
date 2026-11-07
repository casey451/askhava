//! Complaints about a business and the owner's phone-link email across `businesses`, `ops`' alerts,
//! `core`'s task queue and `messaging`'s templates (TD-BIZ-03, the complaint and phone-link cases
//! of TD-BIZ-11, BIZ-033's link case of TD-BIZ-12; BIZ-011, BIZ-012, BIZ-029, BIZ-033; D-016,
//! D-200, D-222, D-258, D-280, D-491, D-493).
//!
//! Proves on a fresh database that the homeowner's reply holds the apology, the independence line,
//! the business's phone, the job and the ROC link; that 3 complaints within 90 days pause and a
//! third on day 91 does not; that a safety word raises the alert row at once and every complaint
//! is listed for the digest only once its 5-day reply box closes; that the business sees kind and
//! dates only and may reply once before the box closes; and that a member's phone link queues one
//! owner email while the owner's own link queues none. All IDs, names and numbers are fake.

use askhava_businesses::api::{
    Acting, BOX_CLOSED_KIND, Checks, ComplaintError, DIGEST_ALERT, Event, NewComplaint,
    PHONE_LINKED_KIND, REPLY_BOX, REPLY_TEMPLATE, ReplyFacts, Roles, SAFETY_ALERT, SenderLink,
    TeamError, add_member, add_owner, apply, business, complaints, create, for_casey,
    has_safety_word, link_phone, list_closed, phone_linked_line, record_complaint, reply,
    run_box_closed,
};
use askhava_core::api::Claimed;
use askhava_messaging::api::{Language, count_segments, render};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DAY: i64 = 24 * 60;

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(db)
        .await
        .expect("count")
}

const ALERTS: &str = "SELECT count(*) FROM ops.alert WHERE subject_id = $1 AND kind = $2";

async fn alerts(db: &mut PgConnection, complaint: Uuid, kind: &str) -> i64 {
    sqlx::query_scalar(ALERTS)
        .bind(complaint)
        .bind(kind)
        .fetch_one(db)
        .await
        .expect("alerts")
}

/// An active business with its owner.
async fn active_business(db: &mut PgConnection) -> Acting {
    let b = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, b, Uuid::from_u128(0xe0), at(0)).await;
    let done = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let to = apply(db, b, Event::Checked(done), Actor::System, at(0)).await;
    assert_eq!(to.expect("activate").code(), "active");
    Acting {
        business: b,
        member: owner.expect("owner"),
    }
}

async fn complain(db: &mut PgConnection, b: Uuid, words: &str, minute: i64) -> (Uuid, bool) {
    let c = NewComplaint {
        business: b,
        job: Uuid::from_u128(0x10b),
        kind: "no_show",
        words,
    };
    let done = record_complaint(db, &c, at(minute))
        .await
        .expect("complaint");
    (done.id, done.paused)
}

async fn status(db: &mut PgConnection, b: Uuid) -> &'static str {
    business(db, b).await.expect("read").status.code()
}

/// TD-BIZ-03 (BIZ-011): the reply holds the apology, the independence line, the business's own
/// phone, the job and the ROC link through askhava.com, in English and Spanish.
#[test]
fn td_biz_03_complaint_reply_content() {
    let facts = ReplyFacts {
        business: "Desert Pipe Co",
        number: "(928) 555-0142",
        job: "1042",
        trade: "plumbing",
    };
    for lang in [Language::En, Language::Es] {
        let text = render(REPLY_TEMPLATE, lang, true, &facts.values()).expect("render");
        for part in ["Desert Pipe Co", "(928) 555-0142", "1042", "plumbing"] {
            assert!(text.body.contains(part), "{part}");
        }
        assert!(text.body.contains("askhava.com/help/roc-complaint"));
        assert!(count_segments(&text.body).segments <= 3);
    }
    let en = render(REPLY_TEMPLATE, Language::En, true, &facts.values()).expect("en");
    assert!(en.body.contains("Sorry"));
    assert!(en.body.contains("is not a contractor"));
    assert!(en.body.contains("is an independent business"));
}

/// TD-BIZ-03, TD-BIZ-11 (BIZ-012, D-016): the third complaint within 90 days pauses the business;
/// with the first 91 days before the third it does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_03_three_in_90_days_pause_and_day_91_does_not(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let a = active_business(&mut db).await.business;
    assert!(!complain(&mut db, a, "late", 0).await.1);
    assert!(!complain(&mut db, a, "late", 10 * DAY).await.1);
    assert_eq!(status(&mut db, a).await, "active");
    assert!(complain(&mut db, a, "late", 90 * DAY).await.1);
    assert_eq!(status(&mut db, a).await, "paused");
    let reason = "SELECT pause_reason FROM businesses.business WHERE id = $1";
    let code: Option<String> = sqlx::query_scalar(reason)
        .bind(a)
        .fetch_one(&mut *db)
        .await
        .expect("reason");
    assert_eq!(code.as_deref(), Some("complaints"));
    assert!(
        !complain(&mut db, a, "late", 91 * DAY).await.1,
        "already paused"
    );

    let b = active_business(&mut db).await.business;
    complain(&mut db, b, "late", 0).await;
    complain(&mut db, b, "late", DAY).await;
    assert!(!complain(&mut db, b, "late", 91 * DAY).await.1);
    assert_eq!(status(&mut db, b).await, "active");
}

/// TD-BIZ-03, TD-BIZ-11 (OPS-005, D-222, D-280): a safety word writes the alert row at once; every
/// complaint writes the digest row only once its reply box closes, and only once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_11_safety_alert_at_once_and_digest_after_the_box_closes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let b = active_business(&mut db).await.business;
    let (unsafe_one, _) = complain(&mut db, b, "He made a THREAT at me.", 0).await;
    let (plain, _) = complain(&mut db, b, "He was late and rude.", 0).await;
    assert_eq!(alerts(&mut db, unsafe_one, SAFETY_ALERT).await, 1);
    assert_eq!(alerts(&mut db, plain, SAFETY_ALERT).await, 0);
    assert_eq!(alerts(&mut db, plain, DIGEST_ALERT).await, 0);
    let tasks =
        "SELECT count(*) FROM core.task WHERE kind = $2 AND payload->>'complaint' = $1::text";
    let queued: i64 = sqlx::query_scalar(tasks)
        .bind(plain)
        .bind(BOX_CLOSED_KIND)
        .fetch_one(&mut *db)
        .await
        .expect("task");
    assert_eq!(queued, 1);
    let early = list_closed(&mut db, plain, at(5 * DAY - 1)).await;
    assert_eq!(early, Err(ComplaintError::BoxOpen));
    assert_eq!(list_closed(&mut db, plain, at(5 * DAY)).await, Ok(true));
    assert_eq!(list_closed(&mut db, plain, at(6 * DAY)).await, Ok(false));
    assert_eq!(alerts(&mut db, plain, DIGEST_ALERT).await, 1);
    let unknown = list_closed(&mut db, Uuid::from_u128(1), at(6 * DAY)).await;
    assert_eq!(unknown, Err(ComplaintError::Unknown));
    let view = for_casey(&mut db, unsafe_one).await.expect("view");
    assert!(view.safety && view.listed_at.is_none());
}

fn claimed(payload: serde_json::Value) -> Claimed {
    Claimed {
        id: 1,
        kind: BOX_CLOSED_KIND.to_owned(),
        payload,
        attempts: 1,
    }
}

/// TD-BIZ-11 (OPS-005, JOB-006): the box-closed handler lists a closed complaint, is safe to run
/// twice, refuses a bad payload and drops an unknown complaint.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_11_box_closed_handler_lists_once(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let b = active_business(&mut db).await.business;
    let (id, _) = complain(&mut db, b, "never came", 0).await;
    for _ in 0..2 {
        let run = run_box_closed(pool.clone(), claimed(json!({"complaint": id}))).await;
        assert!(run.is_ok());
    }
    assert_eq!(alerts(&mut db, id, DIGEST_ALERT).await, 1);
    let other = json!({"complaint": Uuid::from_u128(2)});
    assert!(run_box_closed(pool.clone(), claimed(other)).await.is_ok());
    assert!(
        run_box_closed(pool.clone(), claimed(json!({})))
            .await
            .is_err()
    );
    let fresh = NewComplaint {
        business: b,
        job: Uuid::from_u128(0x10c),
        kind: "work",
        words: "",
    };
    let now = record_complaint(&mut db, &fresh, Timestamp::now()).await;
    let open = claimed(json!({"complaint": now.expect("complaint").id}));
    assert!(run_box_closed(pool, open).await.is_err(), "box still open");
}

/// TD-BIZ-11 (BIZ-029, D-200): the business sees kind and dates only, replies once before the box
/// closes (4 days 23:59 accepted, exactly 5 days refused), and Casey sees the reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn biz_029_reply_box_open_five_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let b = active_business(&mut db).await.business;
    let other = active_business(&mut db).await.business;
    let (first, _) = complain(&mut db, b, "Rude visit, call Jane", 0).await;
    let (second, _) = complain(&mut db, b, "late", 60).await;
    let shown = complaints(&mut db, b).await.expect("list");
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[1].id, first);
    assert_eq!(shown[1].kind, "no_show");
    assert_eq!(shown[1].reply_until, at(0) + REPLY_BOX);
    assert!(!format!("{shown:?}").contains("Jane"));
    let last = at(5 * DAY - 1);
    assert_eq!(
        reply(&mut db, (other, first), "Not ours", last).await,
        Err(ComplaintError::Closed)
    );
    assert_eq!(
        reply(&mut db, (b, first), "  ", last).await,
        Err(ComplaintError::BadReply)
    );
    let long = "x".repeat(1001);
    assert_eq!(
        reply(&mut db, (b, first), &long, last).await,
        Err(ComplaintError::BadReply)
    );
    reply(&mut db, (b, first), "We came at 9 as agreed.", last)
        .await
        .expect("reply in time");
    let again = reply(&mut db, (b, first), "Again", last).await;
    assert_eq!(again, Err(ComplaintError::Closed));
    let late = reply(&mut db, (b, second), "Late", at(5 * DAY + 60)).await;
    assert_eq!(late, Err(ComplaintError::Closed));
    let view = for_casey(&mut db, first).await.expect("view");
    assert_eq!(view.reply.as_deref(), Some("We came at 9 as agreed."));
    assert_eq!((view.business, view.kind.as_str()), (b, "no_show"));
    let unknown = for_casey(&mut db, Uuid::from_u128(3)).await;
    assert_eq!(unknown, Err(ComplaintError::Unknown));
}

/// BIZ-012 (PRIV-001): a bad kind is refused before anything is written, the homeowner's words are
/// never stored, and the app role cannot delete a complaint.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn complaints_keep_no_words_and_are_never_deleted(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let b = active_business(&mut db).await.business;
    let bad = NewComplaint {
        business: b,
        job: Uuid::from_u128(0x10d),
        kind: "No Show",
        words: "late",
    };
    let refused = record_complaint(&mut db, &bad, at(0)).await;
    assert_eq!(refused, Err(ComplaintError::BadKind));
    let (id, _) = complain(&mut db, b, "Call me at 928-555-0199", 0).await;
    let row = "SELECT count(*) FROM businesses.complaint c WHERE id = $1 \
               AND row_to_json(c)::text LIKE '%555%'";
    assert_eq!(count(&mut db, row, id).await, 0);
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *db)
        .await
        .expect("role");
    let gone = sqlx::query("DELETE FROM businesses.complaint WHERE id = $1")
        .bind(id)
        .execute(&mut *db)
        .await;
    assert!(gone.is_err());
}

struct AnyNumber;

impl SenderLink for AnyNumber {
    async fn link(
        &self,
        _db: &mut PgConnection,
        _business_line: (Uuid, Uuid),
        _number: &SecretString,
    ) -> Result<(), TeamError> {
        Ok(())
    }
}

const LINK_TASKS: &str = "SELECT count(*) FROM core.task WHERE kind = 'businesses.phone_linked' \
                          AND payload->>'business' = $1::text";

/// TD-BIZ-11, TD-BIZ-12 (BIZ-033, D-258, D-311): a member's phone link queues one owner email; the
/// owner's own link queues none (the text and the Approve gate are T148's).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_11_member_phone_link_emails_the_owner(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let owner = active_business(&mut db).await;
    let b = owner.business;
    let roles = Roles::new(false, true).expect("roles");
    let member = add_member(&mut db, owner, Uuid::from_u128(0xe1), roles, at(1)).await;
    let biller = Acting {
        business: b,
        member: member.expect("member"),
    };
    let n = SecretString::from("928-555-0170".to_owned());
    link_phone(&mut db, &AnyNumber, (owner, false), &n, at(2))
        .await
        .expect("owner link");
    assert_eq!(count(&mut db, LINK_TASKS, b).await, 0);
    link_phone(&mut db, &AnyNumber, (biller, false), &n, at(3))
        .await
        .expect("member link");
    assert_eq!(count(&mut db, LINK_TASKS, b).await, 1);
    assert_eq!(PHONE_LINKED_KIND, "businesses.phone_linked");
    let stranger = Acting {
        business: b,
        member: Uuid::from_u128(4),
    };
    let refused = link_phone(&mut db, &AnyNumber, (stranger, false), &n, at(4)).await;
    assert!(refused.is_err());
    for spanish in [false, true] {
        let line = phone_linked_line("0170", spanish);
        assert!(line.starts_with("Ask Hava: ") && line.contains("0170"));
        assert!(line.contains("askhava.com/b"));
    }
}

/// OPS-005, D-222: the safety words, English and Spanish, as whole words in any case.
#[test]
fn ops_005_safety_words() {
    for words in [
        "unsafe work",
        "He STOLE tools",
        "llamé a la policía",
        "fire!",
        "Hurt.",
    ] {
        assert!(has_safety_word(words), "{words}");
    }
    for words in ["fired up", "late again", "threaten", ""] {
        assert!(!has_safety_word(words), "{words}");
    }
}

proptest! {
    /// OPS-005: a safety word anywhere among plain words is always found.
    #[test]
    fn a_safety_word_is_always_found(
        before in "[a-z ]{0,20}",
        after in "[a-z ]{0,20}",
        word in prop::sample::select(vec!["threat", "injured", "police", "amenaza", "incendio"]),
    ) {
        let text = format!("{before} {word} {after}");
        prop_assert!(has_safety_word(&text));
    }
}

/// The reply box is exactly 5 days (BIZ-029).
#[test]
fn biz_029_box_is_five_days() {
    assert_eq!(REPLY_BOX, SignedDuration::from_hours(120));
}
