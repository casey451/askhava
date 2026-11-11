//! A quote's notes and the edit or withdrawal before a pick (T164; TD-PICK-35's notes cases:
//! PICK-052; TD-PICK-34's edit re-check, stored withdrawal and no change after a pick: PICK-051;
//! D-442, D-446, D-2912). Fake IDs and made-up notes only.
//!
//! A note holding a contact or an off-platform phrase goes back with the reason; with the
//! stand-in AI check on, a note criticizing another business is flagged; a plain note is stored
//! as written. An edit is re-checked and keeps the first arrival time; a withdrawal sets
//! `withdrawn_at` and keeps the row; neither goes through once a pick has moved the job, even
//! when the move commits while the change waits on the job's row lock.

use std::time::Duration;

use askhava_core::api::{Guarded, brand};
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    Format, NOTE_LIMIT, NoteFlag, NoteProblem, NoteScreener, Price, Quote, QuoteForm, QuoteRules,
    Saved, Submitted, Withdrawal, check_note, mark_unavailable, parse_quote, save_quote,
    screen_note, shown_quotes, withdraw_quote,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

/// A firm quote with `notes`, parsed as the form would send it.
fn quote(cents: u64, notes: &str) -> Quote {
    let form = QuoteForm {
        price: Price::Firm(cents),
        notes: notes.to_owned(),
        original: String::new(),
        ai_read: false,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    parse_quote(form, &rules).expect("quote")
}

/// A job moved along `path` from Intake with business 1's card sent at 08:00 on the 3rd.
async fn job(db: &mut Db, path: &[JobStatus]) -> (Uuid, Uuid) {
    let id = create(db, at("02T21:00:00"), false, Actor::System)
        .await
        .expect("job");
    let mut from = JobStatus::Intake;
    for to in path {
        let moved = move_to(db, id, (from, *to), at("02T21:00:00"), Actor::System).await;
        assert_eq!(moved, Ok(Guarded::Applied(())));
        from = *to;
    }
    let business = Uuid::from_u128(1);
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(id)
        .bind(business)
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    (id, business)
}

/// The stored row: amount, notes, received, edited and withdrawn times.
type Row = (
    Option<i64>,
    String,
    jiff_sqlx::Timestamp,
    Option<jiff_sqlx::Timestamp>,
    Option<jiff_sqlx::Timestamp>,
);

async fn row(db: &mut Db, job: Uuid) -> Row {
    sqlx::query_as(
        "SELECT amount_cents, notes, received_at, edited_at, withdrawn_at FROM quotes.quote \
         WHERE job_id = $1",
    )
    .bind(job)
    .fetch_one(&mut **db)
    .await
    .expect("row")
}

/// The build's stand-in for the AI note check: flags a note that runs down another business.
struct StandInAi;

impl NoteScreener for StandInAi {
    async fn screen(&self, note: &str) -> Option<NoteFlag> {
        let lower = note.to_lowercase();
        (lower.contains("other company") || lower.contains("the other guys"))
            .then_some(NoteFlag::CriticizesBusiness)
    }
}

/// TD-PICK-35 (PICK-052, D-442): contact and off-platform notes come back with the reason; a
/// note over 300 characters too; a plain note passes; the stand-in AI flags criticism.
#[tokio::test]
async fn td_pick_35_notes_come_back_with_the_reason() {
    let outside = format!(
        "Happy to do it outside {} for less.",
        brand().display_name()
    );
    let off = [
        "Text me at five five five, one two one two.",
        "See my work at fake-example.com",
        outside.as_str(),
        "Cash discount if you book this week.",
        "Venmo works for the deposit.",
    ];
    for note in off {
        let got = check_note(note);
        assert!(
            matches!(got, Err(NoteProblem::Contact | NoteProblem::OffPlatform)),
            "{note}"
        );
        let reason = got.expect_err("bounced").message();
        assert!(
            reason.contains("outside") && !reason.contains(note),
            "{note}"
        );
    }
    assert_eq!(check_note(&"a".repeat(NOTE_LIMIT)), Ok(()));
    let long = check_note(&"a".repeat(NOTE_LIMIT + 1));
    assert_eq!(long, Err(NoteProblem::TooLong));
    let plain = "Includes haul-away of the old unit. Two of us on site.";
    assert_eq!(screen_note(plain, Some(&StandInAi)).await, Ok(()));
    let rude = "The other company in town does sloppy work. We don't.";
    assert_eq!(screen_note(rude, None::<&StandInAi>).await, Ok(()));
    let flagged = screen_note(rude, Some(&StandInAi)).await;
    assert_eq!(
        flagged,
        Err(NoteProblem::Flagged(NoteFlag::CriticizesBusiness))
    );
    assert!(
        flagged
            .expect_err("flag")
            .message()
            .contains("other businesses")
    );
}

/// TD-PICK-35 (PICK-052): a plain note is stored exactly as written; an off-platform one stores
/// nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_35_a_plain_note_is_stored_as_written(mut db: Db) {
    let (id, business) = job(&mut db, &[JobStatus::Open]).await;
    let bad = quote(30_000, "Pay cash and save 10%.");
    let got = save_quote(&mut db, (id, business), &bad, None, at("03T09:00:00")).await;
    assert_eq!(got, Ok(Saved::NoteBack(NoteProblem::OffPlatform)));
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![]));
    let note = "Includes haul-away of the old unit.  Two of us on site.";
    let good = quote(30_000, note);
    let got = save_quote(&mut db, (id, business), &good, None, at("03T09:05:00")).await;
    assert_eq!(got, Ok(Saved::Stored(Submitted::Saved)));
    assert_eq!(row(&mut db, id).await.1, note);
}

/// TD-PICK-34 (PICK-051): an edit before any pick is re-checked and stored as an edit, keeping
/// its first arrival time; a withdrawal sets `withdrawn_at`, keeps the row and leaves the page.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_edit_is_rechecked_and_withdrawal_is_stored(mut db: Db) {
    let (id, business) = job(&mut db, &[JobStatus::Open]).await;
    let ids = (id, business);
    let first = save_quote(&mut db, ids, &quote(30_000, ""), None, at("03T09:00:00")).await;
    assert_eq!(first, Ok(Saved::Stored(Submitted::Saved)));
    let contact = quote(28_000, "Zelle works for the deposit.");
    let refused = save_quote(&mut db, ids, &contact, None, at("03T10:00:00")).await;
    assert!(matches!(refused, Ok(Saved::NoteBack(_))));
    assert_eq!(row(&mut db, id).await.0, Some(30_000));
    let edit = save_quote(&mut db, ids, &quote(28_000, ""), None, at("03T11:00:00")).await;
    assert_eq!(edit, Ok(Saved::Stored(Submitted::Saved)));
    let (amount, _, received, edited, withdrawn) = row(&mut db, id).await;
    assert_eq!(amount, Some(28_000));
    assert_eq!(received.to_jiff(), at("03T09:00:00"));
    assert_eq!(edited.map(|t| t.to_jiff()), Some(at("03T11:00:00")));
    assert!(withdrawn.is_none());
    let gone = withdraw_quote(&mut db, ids, at("03T12:00:00")).await;
    assert_eq!(gone, Ok(Withdrawal::Withdrawn));
    assert_eq!(
        row(&mut db, id).await.4.map(|t| t.to_jiff()),
        Some(at("03T12:00:00"))
    );
    assert_eq!(shown_quotes(&mut db, id).await, Ok(vec![]));
    let again = withdraw_quote(&mut db, ids, at("03T12:30:00")).await;
    assert_eq!(again, Ok(Withdrawal::AlreadyWithdrawn));
    let stranger = (id, Uuid::from_u128(9));
    let none = withdraw_quote(&mut db, stranger, at("03T12:30:00")).await;
    assert_eq!(none, Ok(Withdrawal::NoQuote));
}

/// TD-PICK-34 (PICK-051): after a pick (Awaiting confirm, or a quote marked unavailable) neither
/// an edit nor a withdrawal goes through, and the quote keeps its first time and amount.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_no_edit_or_withdrawal_after_a_pick(mut db: Db) {
    let (id, business) = job(&mut db, &[JobStatus::Open]).await;
    let ids = (id, business);
    let first = save_quote(&mut db, ids, &quote(30_000, ""), None, at("03T09:00:00")).await;
    assert_eq!(first, Ok(Saved::Stored(Submitted::Saved)));
    let step = (JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
    let moved = move_to(&mut db, id, step, at("03T10:00:00"), Actor::System).await;
    assert_eq!(moved, Ok(Guarded::Applied(())));
    let edit = save_quote(&mut db, ids, &quote(25_000, ""), None, at("03T11:00:00")).await;
    assert_eq!(edit, Ok(Saved::Stored(Submitted::EditClosed)));
    let gone = withdraw_quote(&mut db, ids, at("03T11:00:00")).await;
    assert_eq!(gone, Ok(Withdrawal::EditClosed));
    let (amount, _, received, edited, withdrawn) = row(&mut db, id).await;
    assert_eq!((amount, edited, withdrawn), (Some(30_000), None, None));
    assert_eq!(received.to_jiff(), at("03T09:00:00"));
    let (other, b2) = job(&mut db, &[JobStatus::Open]).await;
    let q = quote(30_000, "");
    let saved = save_quote(&mut db, (other, b2), &q, None, at("03T09:00:00"));
    assert_eq!(saved.await, Ok(Saved::Stored(Submitted::Saved)));
    let marked = mark_unavailable(&mut db, (other, b2), at("03T10:00:00")).await;
    assert_eq!(marked, Ok(Guarded::Applied(())));
    let gone = withdraw_quote(&mut db, (other, b2), at("03T11:00:00")).await;
    assert_eq!(gone, Ok(Withdrawal::EditClosed));
}

/// TD-PICK-34 (PICK-051, D-2912): a withdrawal waiting on the job's row lock while a pick's
/// move holds it reads the moved status after the move commits and is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_34_a_pick_move_holding_the_lock_wins(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (id, business) = job(&mut db, &[JobStatus::Open]).await;
    let q = quote(30_000, "");
    let first = save_quote(&mut db, (id, business), &q, None, at("03T09:00:00"));
    assert_eq!(first.await, Ok(Saved::Stored(Submitted::Saved)));
    let mut pick = pool.begin().await.expect("tx");
    sqlx::query(
        "UPDATE jobs.job SET status = 'awaiting_confirm', version = version + 1 WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *pick)
    .await
    .expect("move");
    let waiting =
        tokio::spawn(
            async move { withdraw_quote(&mut db, (id, business), at("03T11:00:00")).await },
        );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!waiting.is_finished(), "the withdrawal waits on the lock");
    pick.commit().await.expect("commit");
    let got = waiting.await.expect("join");
    assert_eq!(got, Ok(Withdrawal::EditClosed));
}
