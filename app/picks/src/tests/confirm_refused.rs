//! `confirm_refused` on PostgreSQL (PICK-020, PICK-046, PICK-047; D-056, D-4349, D-5017; T772):
//! an expired, an account and a license refusal each end the pick, move it to her backup or
//! return the job to Quotes ready and queue her one text event; a paused job gets none; a pick no
//! longer waiting changes nothing; `confirm_deadline` reads the stored end. Fake IDs only; the
//! quotes are seeded through `quotes`' own API (TD-SEC-10: no SQL on another module's schema).

use std::collections::BTreeSet;

use askhava_businesses::api::AccountStatus;
use askhava_core::api::{Guarded, due_or_running};
use askhava_jobs::api::{JobStatus, TEXT_KIND, current, move_to};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::api::{
    Backup, PagePick, Refusal, bind_backup, choose_backup, confirm_deadline, page_pick,
    refuse_confirm,
};
use crate::models::PicksError;
use crate::pick_ended::{PICK_LAPSED_TEMPLATE, PICK_MOVED_TEMPLATE};

const TOWN: &str = "lake_havasu_city";
const TRADE: (&str, &str) = ("hvac_repair", "AC not cooling");

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_638_800 + minute * 60).expect("time")
}

/// Sends the job's card to fake businesses 1 and 2 and stores a firm $190 quote from each.
async fn quoted(db: &mut PgConnection, job: Uuid) {
    let candidate = Job {
        category: TRADE.0.to_owned(),
        job_type: TRADE.1.to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let card = CardJob {
        towns: vec![OfferTown {
            code: TOWN.to_owned(),
            state: "AZ".to_owned(),
        }],
        candidates: vec![candidate],
        text: String::new(),
    };
    let businesses = [1_u128, 2].map(|n| CardBusiness {
        id: Uuid::from_u128(0xb772_0100 + n),
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from([TOWN.to_owned()]),
        ticked: BTreeSet::from([(TRADE.0.to_owned(), TRADE.1.to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    });
    let map = ClassMap::shipped().expect("map");
    let sent = send_cards(db, map, job, &card, &businesses, at(1)).await;
    assert_eq!(sent.expect("cards").len(), 2, "both get the card");
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: "Parts and labor included.".to_owned(),
        original: "Swap the capacitor and check the coil, parts and labor included".to_owned(),
        ai_read: false,
        earliest_visit: None,
    };
    let quote = parse_quote(form, &rules).expect("quote");
    for (business, minute) in businesses.iter().zip(2..) {
        let saved = submit_quote(db, (job, business.id), &quote, at(minute)).await;
        saved.expect("quote saved");
    }
}

/// An emergency job (its fee needs no catalog facts, FEE-045) at Quotes ready whose quote 1 is
/// picked on the page, quote 2 named as her backup with `backup`: `(job, pick)`.
async fn picked(db: &mut PgConnection, backup: bool) -> (Uuid, Uuid) {
    let job = askhava_jobs::api::create(db, at(0), true, Actor::System).await;
    let job = job.expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, at(0), Actor::System).await;
    assert!(matches!(moved, Ok(Guarded::Applied(()))), "open");
    quoted(db, job).await;
    let got = page_pick(db, (job, false), 1, Actor::System, at(7)).await;
    let Ok(PagePick::Picked(pick)) = got else {
        panic!("a page pick: {got:?}");
    };
    if backup {
        let browser = [7; 32];
        let asked = choose_backup(db, (job, 1), (Some(2), &browser), at(7)).await;
        asked.expect("asked");
        bind_backup(db, pick, &browser, at(7)).await.expect("bound");
    }
    (job, pick)
}

/// What a refusal may change: the pick's status and end reason, the job's status, and whether her
/// PICK-046 and PICK-047 text events for the pick are queued (one each at most: the pick and the
/// template are the event's dedupe key).
type State = (String, JobStatus, [bool; 2]);

async fn state(db: &mut PgConnection, (job, pick): (Uuid, Uuid)) -> State {
    let row = sqlx::query!(
        "SELECT status, end_reason FROM picks.pick WHERE id = $1",
        pick
    )
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    let ended = [Some(row.status), row.end_reason].map(Option::unwrap_or_default);
    let mut texts = [false; 2];
    for (queued, template) in texts
        .iter_mut()
        .zip([PICK_LAPSED_TEMPLATE, PICK_MOVED_TEMPLATE])
    {
        let key = format!("{TEXT_KIND}.{pick}.{template}");
        *queued = due_or_running(db, TEXT_KIND, &key).await.expect("task");
    }
    let status = current(db, job).await.expect("job").status;
    (ended.join("|"), status, texts)
}

/// Refuses a fresh pick's Confirm for `refusal` twice and returns the first answer and the
/// state, which the second call (the pick no longer waits) must answer `Done` and leave as is.
async fn refused(
    pool: &PgPool,
    refusal: Refusal,
    (backup, paused): (bool, bool),
) -> (Backup, State) {
    let mut tx = pool.begin().await.expect("tx");
    let (job, pick) = picked(&mut tx, backup).await;
    let by = (Actor::System, at(10));
    let first = refuse_confirm(&mut tx, (pick, job), refusal, paused, by).await;
    let after = state(&mut tx, (job, pick)).await;
    let again = refuse_confirm(&mut tx, (pick, job), refusal, paused, by).await;
    assert_eq!(again, Ok(Backup::Done), "a pick no longer waiting");
    assert_eq!(state(&mut tx, (job, pick)).await, after, "changes nothing");
    assert_eq!(confirm_deadline(&mut tx, pick).await, Ok(None));
    (first.expect("refused"), after)
}

/// PICK-020, PICK-046: with no backup an expired, an account and a license refusal each end the
/// pick, return the job to Quotes ready and queue her PICK-046 text event once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refusal_with_no_backup_returns_the_job_and_queues_pick_046_once(pool: PgPool) {
    for (refusal, ended) in [
        (Refusal::Expired, "expired|"),
        (Refusal::Account, "cancelled|ineligible"),
        (Refusal::License, "cancelled|ineligible"),
    ] {
        let (backup, state) = refused(&pool, refusal, (false, false)).await;
        assert_eq!(backup, Backup::NoBackup, "{refusal:?}");
        let want = (ended.to_owned(), JobStatus::QuotesReady, [true, false]);
        assert_eq!(state, want, "{refusal:?}");
    }
}

/// PICK-020, PICK-047: with a backup named each refusal moves the pick to it, the job stays in
/// Awaiting confirm for the backup's own window and her PICK-047 text event is queued once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refusal_moves_to_her_backup_and_queues_pick_047_once(pool: PgPool) {
    for (refusal, ended) in [
        (Refusal::Expired, "expired|"),
        (Refusal::Account, "cancelled|ineligible"),
        (Refusal::License, "cancelled|ineligible"),
    ] {
        let (backup, state) = refused(&pool, refusal, (true, false)).await;
        assert!(matches!(backup, Backup::Moved(_)), "{refusal:?}");
        let want = (ended.to_owned(), JobStatus::AwaitingConfirm, [false, true]);
        assert_eq!(state, want, "{refusal:?}");
    }
}

/// PICK-046, CON-029: on a paused job the refused pick ends and the job returns to Quotes ready,
/// but nothing moves to the backup and no text is queued.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_paused_job_gets_no_text(pool: PgPool) {
    let (backup, state) = refused(&pool, Refusal::License, (true, true)).await;
    assert_eq!(backup, Backup::Done);
    let ended = "cancelled|ineligible".to_owned();
    let want = (ended, JobStatus::QuotesReady, [false, false]);
    assert_eq!(state, want);
}

/// PICK-020, D-056: `confirm_deadline` is `None` before the window starts and for an unknown
/// pick, and the stored `confirm_due` once the pick's notice set it; a refusal on a job that
/// cannot be read is `PicksError::Job`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn confirm_deadline_returns_the_stored_end_and_none(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (_, pick) = picked(&mut db, false).await;
    assert_eq!(confirm_deadline(&mut db, pick).await, Ok(None));
    assert_eq!(
        confirm_deadline(&mut db, Uuid::from_u128(9)).await,
        Ok(None)
    );
    sqlx::query!(
        "UPDATE picks.pick SET confirm_due = $2 WHERE id = $1",
        pick,
        at(247).to_sqlx() as _,
    )
    .execute(&mut *db)
    .await
    .expect("window");
    assert_eq!(confirm_deadline(&mut db, pick).await, Ok(Some(at(247))));
    let unknown = (Uuid::from_u128(9), Uuid::from_u128(8));
    let by = (Actor::System, at(10));
    let refused = refuse_confirm(&mut db, unknown, Refusal::Account, false, by).await;
    assert_eq!(refused, Err(PicksError::Job), "an unknown job");
}
