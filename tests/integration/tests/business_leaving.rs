//! Leaving Ask Hava and the optional reason on a fresh database (TD-BIZ-32's leaving and reason
//! cases; BIZ-051, BIZ-054; D-446, D-463, D-491). Fake IDs, card IDs and wording only.
//!
//! A signed-in owner's business has a waiting pick on job A (her backup is quote 3), a waiting
//! pick on job B (no backup), two saved cards, a ledger row and an unused sign-in link. Leaving
//! without a fresh code changes nothing; with one it withdraws both quotes, moves pick A to the
//! backup, returns job B to Quotes ready, removes and detaches both cards, uses up the link, ends
//! the session and keeps the ledger row. The reason is asked once with "Busy for a while" first,
//! on leaving or after a pause past 14 days, and the answer changes nothing else.

use std::path::Path;
use std::sync::Mutex;

use askhava_adapters::api::AdapterError;
use askhava_businesses::api::{
    Acting, LeaveReason, ReasonAnswer, add_owner, create, pause_self, reason_asked, record_step_up,
};
use askhava_core::api::Guarded;
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_payments::api::{BusinessId, SavedCard, save_card, saved_card_of};
use askhava_picks::api::{Backup, PagePick, bind_backup, choose_backup, page_pick};
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, BusinessPrincipal, CardDetach, CookieJar, LeaveError, LeaveScreen,
    Realm, business_principal, leave, leave_page, pause_reason, start_session,
};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DAY: &str = "2026-11-04T17:00:00Z";
const CARDS: [&str; 2] = ["pm_FakeLeave1", "pm_FakeLeave2"];

fn at(minutes: i64) -> Timestamp {
    DAY.parse::<Timestamp>().expect("instant") + SignedDuration::from_mins(minutes)
}

/// Records each card it is asked to detach, as stripe-mock's detach would answer.
#[derive(Default)]
struct FakeDetach(Mutex<Vec<String>>);

impl CardDetach for FakeDetach {
    fn detach(
        &self,
        payment_method: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send {
        self.0.lock().expect("lock").push(payment_method.to_owned());
        std::future::ready(Ok(()))
    }
}

async fn count(db: &mut PgConnection, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(db)
        .await
        .expect("count")
}

/// A job in Quotes ready with confirmed quotes 1 (`first`'s), 2 and 3.
async fn quoted(db: &mut PgConnection, first: Uuid) -> Uuid {
    let job = askhava_jobs::api::create(db, at(0), false, Actor::System)
        .await
        .expect("job");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(0), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    for (n, business) in [
        (1, first),
        (2, Uuid::from_u128(0x2282)),
        (3, Uuid::from_u128(0x2283)),
    ] {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(business)
        .bind(at(n).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
    job
}

/// A business, its owner's member ID, the owner's session cookie and principal.
async fn signed_in(db: &mut PgConnection) -> (Acting, CookieJar, BusinessPrincipal) {
    let business = create(db, false, at(0)).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(0x228d1), at(0))
        .await
        .expect("owner");
    let jar = start_session(
        db,
        CookieJar::new(),
        (Realm::Business, member),
        BUSINESS_SESSION_LIFE,
        at(0),
    )
    .await
    .expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(member)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let principal = business_principal(db, &jar, at(1)).await.expect("read");
    (
        Acting { business, member },
        jar,
        principal.expect("signed in"),
    )
}

/// Two saved cards, one ledger row and one unused sign-in link for `who`.
async fn money_and_links(db: &mut PgConnection, who: Acting, job: Uuid) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(who.business)
        .bind("cus_FakeT228")
        .execute(&mut *db)
        .await
        .expect("customer");
    for (n, pm) in CARDS.into_iter().enumerate() {
        let card = SavedCard::new(pm, "card", "visa", "4242").expect("card");
        let when = at(2 + i64::try_from(n).expect("small"));
        save_card(db, BusinessId(who.business), &card, when)
            .await
            .expect("saved");
    }
    sqlx::query(
        "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents, stripe_id, \
         idempotency_key) VALUES ('charge', $1, $2, 2500, 'ch_FakeT228', 'fake.t228.charge')",
    )
    .bind(who.business)
    .bind(job)
    .execute(&mut *db)
    .await
    .expect("ledger");
    sqlx::query(
        "INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, \
         expires_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(vec![7_u8; 32])
    .bind(who.member)
    .bind(who.business)
    .bind(at(8).to_sqlx())
    .bind(at(30).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("link");
}

/// Job A with a waiting pick of `who` and her backup on quote 3; job B with a waiting pick and no
/// backup.
async fn picked(db: &mut PgConnection, who: Acting) -> (Uuid, Uuid, Uuid) {
    let (a, b) = (
        quoted(db, who.business).await,
        quoted(db, who.business).await,
    );
    let Ok(PagePick::Picked(pick)) = page_pick(db, (a, false), 1, Actor::System, at(7)).await
    else {
        panic!("pick A");
    };
    choose_backup(db, (a, 1), (Some(3), &[5; 32]), at(7))
        .await
        .expect("asked");
    bind_backup(db, pick, &[5; 32], at(7)).await.expect("bound");
    let Ok(PagePick::Picked(_)) = page_pick(db, (b, false), 1, Actor::System, at(7)).await else {
        panic!("pick B");
    };
    (a, b, pick)
}

/// TD-BIZ-32 (BIZ-051, PICK-047, PICK-051, SEC-038, D-446): leaving needs a fresh code, then
/// withdraws the open quotes, moves the waiting picks, removes the cards, ends sessions and links,
/// and keeps the ledger; a second leave changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_32_leaving_withdraws_moves_removes_and_ends(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (who, jar, principal) = signed_in(&mut db).await;
    let (a, b, pick) = picked(&mut db, who).await;
    money_and_links(&mut db, who, a).await;
    let stripe = FakeDetach::default();
    let early = leave(&mut db, &principal, (None, &stripe), at(10)).await;
    assert_eq!(early, Err(LeaveError::NeedsCode));
    assert!(
        !reason_asked(&mut db, who.business).await.expect("read"),
        "nothing changed"
    );
    record_step_up(&mut db, who, at(10)).await.expect("code");
    let answer = ReasonAnswer::parse("few_jobs", "ignored beside this choice");
    let left = leave(&mut db, &principal, (answer.as_ref(), &stripe), at(11))
        .await
        .expect("left");
    assert_eq!(left.picks.len(), 2, "{left:?}");
    assert!(
        left.picks
            .iter()
            .any(|p| matches!(p, (j, Backup::Moved(_)) if *j == a))
    );
    assert!(left.picks.contains(&(b, Backup::NoBackup)), "{left:?}");
    assert_eq!((left.quotes, left.cards, left.detach_failed), (2, 2, 0));
    assert_eq!(left.sessions, 1);
    assert_eq!(
        *stripe.0.lock().expect("lock"),
        CARDS.map(str::to_owned).to_vec()
    );
    let ended: Option<String> =
        sqlx::query_scalar("SELECT end_reason FROM picks.pick WHERE id = $1")
            .bind(pick)
            .fetch_one(&mut *db)
            .await
            .expect("pick");
    assert_eq!(ended.as_deref(), Some("ineligible"));
    let backup = "SELECT count(*) FROM picks.pick WHERE job_id = $1 AND quote_number = 3 \
                  AND status = 'awaiting_confirm'";
    assert_eq!(
        count(&mut db, backup, a).await,
        1,
        "the backup has the pick"
    );
    assert_eq!(
        current(&mut db, b).await.expect("job").status,
        JobStatus::QuotesReady
    );
    let open = "SELECT count(*) FROM quotes.quote WHERE business_id = $1 AND withdrawn_at IS NULL";
    assert_eq!(
        count(&mut db, open, who.business).await,
        0,
        "quotes withdrawn"
    );
    let saved = saved_card_of(&mut db, BusinessId(who.business))
        .await
        .expect("read");
    assert_eq!(saved, None, "no saved card left");
    let rows = "SELECT count(*) FROM payments.payment_method WHERE business_id = $1";
    assert_eq!(
        count(&mut db, rows, who.business).await,
        2,
        "card rows stay, marked removed"
    );
    let ledger = "SELECT count(*) FROM payments.ledger_entry WHERE business_id = $1";
    assert_eq!(
        count(&mut db, ledger, who.business).await,
        1,
        "ledger rows stay"
    );
    let links =
        "SELECT count(*) FROM portals.login_link WHERE business_id = $1 AND used_at IS NULL";
    assert_eq!(
        count(&mut db, links, who.business).await,
        0,
        "links used up"
    );
    let gone = business_principal(&mut db, &jar, at(12))
        .await
        .expect("read");
    assert!(gone.is_none(), "signed out");
    let status = "SELECT count(*) FROM businesses.business WHERE id = $1 AND status = 'removed'";
    assert_eq!(count(&mut db, status, who.business).await, 1);
    let choice: Option<String> =
        sqlx::query_scalar("SELECT choice FROM businesses.leave_reason WHERE business_id = $1")
            .bind(who.business)
            .fetch_one(&mut *db)
            .await
            .expect("reason");
    assert_eq!(choice.as_deref(), Some("few_jobs"));
    let again = leave(&mut db, &principal, (None, &stripe), at(13)).await;
    assert_eq!(again, Err(LeaveError::AlreadyLeft));
}

/// TD-BIZ-32 (BIZ-054, D-463): the question lists the six choices, "Busy for a while" first with
/// the BUSY line, nothing preselected, one button; it is left off once asked; the box counts only
/// beside "Something else"; no choice is no answer.
#[test]
fn td_biz_32_the_reason_is_optional_with_busy_first() {
    let page = leave_page(LeaveScreen::Leave { ask: true }, "csrf-sample").expect("page");
    let mut last = 0;
    for reason in LeaveReason::ALL {
        let place = page
            .find(&format!(
                ">{}</label>",
                reason.label().replace('\'', "&#39;")
            ))
            .expect("listed");
        assert!(place > last, "{} in order", reason.label());
        last = place;
    }
    assert_eq!(LeaveReason::ALL[0].label(), "Busy for a while");
    assert!(
        page.contains("\"Busy for a number of days\""),
        "Busy points to BUSY"
    );
    assert!(!page.contains("checked"), "nothing preselected");
    assert_eq!(page.matches("<button").count(), 1);
    let asked = leave_page(LeaveScreen::Leave { ask: false }, "csrf-sample").expect("page");
    assert!(!asked.contains("name=\"reason\""), "asked once");
    assert_eq!(ReasonAnswer::parse("", "words"), None);
    assert_eq!(ReasonAnswer::parse("unknown", ""), None);
    let other = ReasonAnswer::parse("other", "  Moving away  ").expect("other");
    assert_eq!(other.choice(), LeaveReason::Other);
    assert_ne!(
        Some(other),
        ReasonAnswer::parse("other", ""),
        "the box is kept"
    );
    let fee = ReasonAnswer::parse("fee", "words");
    assert_eq!(
        fee,
        ReasonAnswer::parse("fee", ""),
        "the box only beside Something else"
    );
}

/// TD-BIZ-32 (BIZ-054): a pause asks only after more than 14 days, once; the answer changes no
/// status or pause, a later leave keeps it, and no code but this task's reads it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_32_a_long_pause_asks_once_and_changes_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (who, _, principal) = signed_in(&mut db).await;
    sqlx::query("UPDATE businesses.business SET status = 'active' WHERE id = $1")
        .bind(who.business)
        .execute(&mut *db)
        .await
        .expect("active");
    pause_self(&mut db, who.business, at(0))
        .await
        .expect("paused");
    let busy = ReasonAnswer::parse("busy", "");
    let day = 24 * 60;
    let early = pause_reason(&mut db, &principal, busy.as_ref(), at(14 * day)).await;
    assert_eq!(early, Ok(false), "14 days exactly is not more than 14");
    let first = pause_reason(&mut db, &principal, busy.as_ref(), at(14 * day + 1)).await;
    assert_eq!(first, Ok(true));
    let second = pause_reason(&mut db, &principal, None, at(15 * day)).await;
    assert_eq!(second, Ok(false), "asked once");
    let state = "SELECT count(*) FROM businesses.business WHERE id = $1 \
                 AND status = 'paused' AND pause_reason = 'self'";
    assert_eq!(
        count(&mut db, state, who.business).await,
        1,
        "nothing else changed"
    );
    record_step_up(&mut db, who, at(15 * day))
        .await
        .expect("code");
    let fee = ReasonAnswer::parse("fee", "");
    leave(
        &mut db,
        &principal,
        (fee.as_ref(), &FakeDetach::default()),
        at(15 * day),
    )
    .await
    .expect("left");
    let kept = "SELECT count(*) FROM businesses.leave_reason WHERE business_id = $1 \
                AND choice = 'busy' AND asked_by = 'paused'";
    assert_eq!(
        count(&mut db, kept, who.business).await,
        1,
        "the first answer stays"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
    let readers = files_naming(&root, "leave_reason");
    assert_eq!(
        readers,
        ["businesses/src/leaving.rs"],
        "no routing or fee code reads it"
    );
}

/// The `.rs` files under `dir` whose text names `word`, relative to `dir`.
fn files_naming(dir: &Path, word: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).expect("dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && std::fs::read_to_string(&path).is_ok_and(|t| t.contains(word))
            {
                let rel = path.strip_prefix(dir).expect("under dir");
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}
