//! TD-CON-35 (CON-019, BIZ-036; D-191, D-2707, D-2711, T373): a business phone's texting consent
//! is per number, so changing the number ends it until the box is ticked for the new number.
//!
//! Real PostgreSQL through `businesses::api`, `quotes`' card notice step and `messaging`'s
//! business-notice sender: a consented line may be texted; after `change_phone_at` it may not;
//! a box ticked for the new number counts again; a box shown for the old number and posted after
//! the change is refused; a change and a tick racing on one line never let the old number's box
//! count for the new number; a refused change leaves consent as it was; the email consent is
//! untouched; and a text-only business's job card after the change goes by email. Fake numbers
//! only (555-01xx) and fake IDs.

use std::time::Duration;

use askhava_businesses::api::{
    Acting, Checks, ConsentChannel, ConsentError, Event, NoticeChannel, Roles, SenderLink,
    TeamError, add_member, add_owner, apply, change_phone_at, create, link_phone, may_text,
    phone_version, record_consent, record_consent_for, request_welcome, set_notice_channel,
    unlink_phone, welcome_sent,
};
use askhava_core::api::IdSecret;
use askhava_messaging::api::{
    Line, Lines, MailboxKind, NoticeOutcome, NoticeSender, NumberState, OutboxError, SendChecks,
    SendingNumber, business_notice, sender_add,
};
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{CardNotice, queue_card_notice};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use ConsentChannel::{Email, Text};

struct Directory(Vault);

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        sender_add(db, &self.0, business_line, number.expose_secret())
            .await
            .map_err(|e| match e {
                OutboxError::BadPhone => TeamError::BadPhone,
                OutboxError::SenderTaken => TeamError::PhoneTaken,
                _ => TeamError::Database,
            })
    }
}

fn directory() -> Directory {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Directory(Vault::new(&key(0x37), &key(0x73), &url).expect("vault"))
}

/// The composition's send-time check: `businesses::api::may_text`, read when the notice is sent.
struct Gate;

impl SendChecks for Gate {
    async fn may_text(&self, conn: &mut PgConnection, business: Uuid, phone: Uuid) -> bool {
        may_text(conn, business, phone).await.unwrap_or(false)
    }

    async fn accept_by(&self, _: &mut PgConnection, _: Uuid, _: Timestamp) -> Option<Timestamp> {
        None
    }
}

/// Noon in Arizona (19:00 UTC) on a fixed day, plus `minute` minutes.
fn at(minute: i64) -> Timestamp {
    let noon: Timestamp = "2026-11-10T19:00:00Z".parse().expect("time");
    noon + jiff::SignedDuration::from_mins(minute)
}

fn n(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

/// An active business with the welcome sent and text-only notices: the owner (mailbox `seed`)
/// and a "Cards and quotes" member (`seed + 1`), who may not change a phone (CALL-012).
async fn team(db: &mut PgConnection, seed: u128) -> (Acting, Acting) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(seed), at(0)).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let cards = Roles::new(true, false).expect("roles");
    let member = add_member(db, owner, Uuid::from_u128(seed + 1), cards, at(0)).await;
    let cards = Acting {
        business,
        member: member.expect("member"),
    };
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let active = apply(db, business, Event::Checked(ok), Actor::System, at(0)).await;
    active.expect("active");
    request_welcome(db, business, at(0)).await.expect("welcome");
    welcome_sent(db, business, at(0)).await.expect("sent");
    set_notice_channel(db, business, (NoticeChannel::Text, true), at(0))
        .await
        .expect("text");
    (owner, cards)
}

/// The owner's main phone with the owner's email and text boxes ticked for its first number.
async fn consented_line(db: &mut PgConnection, dir: &Directory, owner: Acting) -> Uuid {
    let line = link_phone(db, dir, (owner, true), &n("928-555-0151"), at(1)).await;
    let line = line.expect("link");
    let ids = (owner.business, owner.member, Some(line));
    record_consent(db, ids, &[Email, Text], true, at(1))
        .await
        .expect("consent");
    line
}

async fn change(db: &mut PgConnection, dir: &Directory, by: Acting, line: Uuid) -> TeamResult {
    change_phone_at(db, dir, (by, line), &n("928-555-0152"), at(2)).await
}

type TeamResult = Result<(), TeamError>;

async fn may(db: &mut PgConnection, by: Acting, line: Uuid) -> bool {
    may_text(db, by.business, line).await.expect("may_text")
}

async fn rows(db: &mut PgConnection, business: Uuid, channel: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM businesses.esign_consent WHERE business_id = $1 AND channel = $2",
    )
    .bind(business)
    .bind(channel)
    .fetch_one(db)
    .await
    .expect("count")
}

/// TD-CON-35 (CON-019, BIZ-036): a consented line may be texted; after its number changes it may
/// not; a box ticked for the new number counts again; the email consent stays as it was.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_35_a_number_change_ends_that_numbers_consent(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    let (owner, _) = team(&mut db, 0x350).await;
    let line = consented_line(&mut db, &dir, owner).await;
    assert!(may(&mut db, owner, line).await, "a consented number");
    assert_eq!(phone_version(&mut db, owner.business, line).await, Ok(1));
    change(&mut db, &dir, owner, line).await.expect("change");
    assert!(!may(&mut db, owner, line).await, "no box for the new one");
    let shown = phone_version(&mut db, owner.business, line).await;
    assert_eq!(shown, Ok(2));
    let ids = (owner.business, owner.member);
    let again = record_consent_for(&mut db, ids, (line, 2), &[Text], true, at(3)).await;
    assert_eq!(again, Ok(()));
    assert!(may(&mut db, owner, line).await, "the new number consented");
    assert_eq!(
        rows(&mut db, owner.business, "text").await,
        2,
        "history kept"
    );
    assert_eq!(rows(&mut db, owner.business, "email").await, 1);
    let email = record_consent(&mut db, (ids.0, ids.1, None), &[Email], true, at(3)).await;
    assert_eq!(email, Ok(()));
    assert_eq!(
        rows(&mut db, owner.business, "email").await,
        1,
        "email untouched"
    );
}

/// TD-CON-35 (CON-019): a text box shown for the old number and posted after the change stores
/// nothing; the current-version form still records for the new number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_35_a_box_shown_for_the_old_number_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    let (owner, _) = team(&mut db, 0x360).await;
    let line = consented_line(&mut db, &dir, owner).await;
    let shown = phone_version(&mut db, owner.business, line).await;
    let shown = shown.expect("version");
    change(&mut db, &dir, owner, line).await.expect("change");
    let ids = (owner.business, owner.member);
    let late = record_consent_for(&mut db, ids, (line, shown), &[Text], true, at(3)).await;
    assert_eq!(late, Err(ConsentError::NumberChanged));
    assert!(!may(&mut db, owner, line).await);
    assert_eq!(rows(&mut db, owner.business, "text").await, 1);
    let current = record_consent(&mut db, (ids.0, ids.1, Some(line)), &[Text], true, at(3)).await;
    assert_eq!(current, Ok(()));
    assert!(
        may(&mut db, owner, line).await,
        "recorded for the current number"
    );
}

/// TD-CON-35 (CON-019, CALL-012): a change by a member without the right is refused and leaves
/// the number version and the consent as they were.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_35_a_refused_change_leaves_consent(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    let (owner, cards) = team(&mut db, 0x370).await;
    let line = consented_line(&mut db, &dir, owner).await;
    let refused = change(&mut db, &dir, cards, line).await;
    assert_eq!(refused, Err(TeamError::NotAllowed));
    assert_eq!(phone_version(&mut db, owner.business, line).await, Ok(1));
    assert!(may(&mut db, owner, line).await, "consent as it was");
}

/// Waits until `waiting` sessions of this database wait on a row lock.
async fn lock_waits(db: &mut PgConnection, waiting: i64) {
    for _ in 0..200 {
        let now: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&mut *db)
        .await
        .expect("waits");
        if now >= waiting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the racing calls never queued on the lock");
}

/// Runs a change of `line` and a tick shown for its first number, both by the owner, queued on a
/// held lock of the phone row (the tick first when `tick_first`), then releases them.
async fn race(pool: &PgPool, (owner, line): (Acting, Uuid), tick_first: bool) {
    let mut blocker = pool.acquire().await.expect("blocker");
    let mut tx = blocker.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM businesses.staff_phone WHERE id = $1 FOR UPDATE")
        .bind(line)
        .execute(&mut *tx)
        .await
        .expect("hold");
    let p = pool.clone();
    let tick = async move {
        let mut db = p.acquire().await.expect("tick");
        let ids = (owner.business, owner.member);
        record_consent_for(&mut db, ids, (line, 1), &[Text], true, at(2)).await
    };
    let p = pool.clone();
    let moved = async move {
        let mut db = p.acquire().await.expect("change");
        change(&mut db, &directory(), owner, line).await
    };
    let mut watch = pool.acquire().await.expect("watch");
    let (tick, moved) = if tick_first {
        let tick = tokio::spawn(tick);
        lock_waits(&mut watch, 1).await;
        (tick, tokio::spawn(moved))
    } else {
        let moved = tokio::spawn(moved);
        lock_waits(&mut watch, 1).await;
        (tokio::spawn(tick), moved)
    };
    lock_waits(&mut watch, 2).await;
    tx.rollback().await.expect("release");
    let tick = tick.await.expect("tick task");
    assert!(matches!(tick, Ok(()) | Err(ConsentError::NumberChanged)));
    moved.await.expect("change task").expect("change");
}

/// TD-CON-35 (CON-019): a number change and a box shown for the old number racing on one line,
/// in either order, never leave the old number's box counted for the new number, and neither
/// deadlocks.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_35_a_race_never_counts_the_old_numbers_box(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    for (seed, tick_first) in [(0x380, true), (0x390, false)] {
        let (owner, _) = team(&mut db, seed).await;
        let line = link_phone(&mut db, &dir, (owner, true), &n("928-555-0153"), at(1)).await;
        let line = line.expect("link");
        race(&pool, (owner, line), tick_first).await;
        assert_eq!(phone_version(&mut db, owner.business, line).await, Ok(2));
        assert!(
            !may(&mut db, owner, line).await,
            "tick_first = {tick_first}"
        );
        let new: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM businesses.esign_consent WHERE line_id = $1 AND number_version = 2",
        )
        .bind(line)
        .fetch_one(&mut *db)
        .await
        .expect("count");
        assert_eq!(new, 0, "no box recorded for the new number");
        let unlinked = unlink_phone(&mut db, owner, line).await;
        unlinked.expect("unlink, freeing both numbers for the next round");
    }
}

fn sender() -> NoticeSender {
    let secret = SecretString::from("test-only-email-secret-not-real".to_owned());
    let links = IdSecret::new(
        1,
        SecretString::from("test-only-link-secret-not-a-real-key".to_owned()),
    );
    let postal = "PO Box 0000, Lake Havasu City, AZ 86403";
    NoticeSender::new((secret, links.expect("links")), postal, MailboxKind::PoBox).expect("sender")
}

fn lines() -> Lines {
    let line = || Line {
        texting_on: true,
        number: Some(SendingNumber::parse("+15555550100").expect("fake number")),
        state: NumberState::Working,
    };
    Lines {
        homeowner: line(),
        business: line(),
    }
}

/// Queues `business`'s card for job `job` and sends it at noon; returns what was sent.
async fn card(db: &mut PgConnection, business: Uuid, job: u128) -> NoticeOutcome {
    let notice = CardNotice {
        number: 1042,
        category: "plumbing",
        town: "Lake Havasu City",
        card: "Kitchen sink drips under the cabinet. How soon: this week.",
    };
    let job = Uuid::from_u128(job);
    let queued = queue_card_notice(db, (job, business), &notice, (true, at(5))).await;
    assert!(queued.expect("card"));
    let (id, payload): (i64, Value) = sqlx::query_as(
        "SELECT id, payload FROM core.task WHERE kind = 'messaging.business_notice' \
         AND payload->>'job' = $1::text",
    )
    .bind(job)
    .fetch_one(&mut *db)
    .await
    .expect("task");
    let sent = business_notice(db, (&sender(), &Gate, &lines()), (id, &payload), at(5)).await;
    sent.expect("sent")
}

/// TD-CON-35 (BIZ-036, CON-019, D-2711): a text-only business's job card goes by text to the
/// consented number, and by email once that number changed and nobody ticked the box for it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_35_a_text_only_card_goes_by_email_after_the_change(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let dir = directory();
    let (owner, _) = team(&mut db, 0x3a0).await;
    let line = consented_line(&mut db, &dir, owner).await;
    let before = card(&mut db, owner.business, 0x3a01).await;
    assert!(
        matches!(before, NoticeOutcome::Sent { texts: 1, .. }),
        "{before:?}"
    );
    change(&mut db, &dir, owner, line).await.expect("change");
    let after = card(&mut db, owner.business, 0x3a02).await;
    assert!(
        matches!(after, NoticeOutcome::Sent { texts: 0, emails } if emails >= 1),
        "{after:?}"
    );
}
