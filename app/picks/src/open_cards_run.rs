//! The hourly open-cards email run, `picks.open_cards_email` (CARD-022, BIZ-037, BIZ-038,
//! OPS-038; T684, D-1993, D-4361, D-5017).
//!
//! The worker seeds one run at start ([`schedule_open_cards`], one per clock hour by dedupe key)
//! and each run queues the next hour's first. A run in the hour of 8 a.m., noon or 5 p.m. Arizona
//! time takes every business holding a card whose window is open (`quotes::api::card_businesses`)
//! or a pick waiting for Confirm, and for each active team member who gets job cards (the owner
//! and "Cards and quotes" members, BIZ-037) asks `messaging::api::digest_goes` with that person's
//! chosen times, the pause and busy state, the unquoted cards with [`MIN_LEFT`] left
//! (`quotes::api::open_cards`) and, only for a person who gets picks, the waiting picks. One
//! email per person and slot is then composed by `open_cards_email` (pick first, pick-waiting
//! subject) and stored with `queue_email` as `EmailClass::Bulk`, so above the daily limit's share
//! it waits for the reset (OPS-038); the queued-email drain sends it to the member's mailbox. The
//! key is the template, the mailbox, the business and an ID made from the clock hour, so a second
//! run in the hour stores nothing, and its links are rolled back with it. Each card line holds
//! only trade, town, end time and that person's signed Quote link; links come from the
//! composition's [`OpenCardsLinks`]. No member language is stored yet, so the email is English.
//! Without the footer postal line (dev and CI) a run only queues the next one.

use std::future::Future;
use std::sync::Arc;

use askhava_businesses::api::{
    OpenCardsTime, TeamMember, away_until, business, busy_until, members,
};
use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use askhava_jobs::api::job_number;
use askhava_messaging::api::{
    Audience, DigestFacts, EmailClass, Language, MIN_LEFT, NewEmail, Notice, OnDomain,
    PostalAddress, Recipient, digest_goes, listable, queue_email, slot_due,
};
use askhava_quotes::api::{OpenCardFacts, card_businesses, open_cards};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::open_cards_email::{
    ComposedEmail, OpenCard, WaitingPick, chosen_minutes, open_cards_email,
};
use crate::status::PickStatus;

/// The task kind of the hourly open-cards email run (CARD-022).
pub const OPEN_CARDS_EMAIL_KIND: &str = "picks.open_cards_email";
/// The template ID of the open-cards email's outbox rows (CARD-022).
pub const OPEN_CARDS_TEMPLATE: &str = "picks.open_cards_email";
/// The first half of the slot ID in an email's key ("opencard"); the second is the clock hour.
const SLOT_MARK: u64 = 0x6f70_656e_6361_7264;
const HOUR: i64 = 3600;
const PORTAL: OnDomain = OnDomain("/b");
const FAILED: HandlerError = HandlerError("the open-cards email run failed");

/// What a link in the open-cards email opens (BIZ-038).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestLink {
    /// The quote form of this job, until the card's quote window ends.
    Quote(Uuid),
    /// The Confirm review step of this pick, until its Confirm window ends.
    Confirm(Uuid),
}

/// The signed single-purpose links of the open-cards email (BIZ-038, SEC-025): the composition
/// implements it with `portals::api::mint_action_link`, which `picks` cannot call.
pub trait OpenCardsLinks: Send + Sync + 'static {
    /// The link to `to` for team member `member` of `business`, working from `now` until `ends`.
    fn link(
        &self,
        db: &mut PgConnection,
        business_member: (Uuid, Uuid),
        to: DigestLink,
        now_ends: (Timestamp, Timestamp),
    ) -> impl Future<Output = Result<SecretString, HandlerError>> + Send;
}

/// What the run needs from the composition: the key secret, the footer postal line (unset in dev
/// and CI: nothing is queued) and the link maker.
pub struct OpenCardsParts<L> {
    /// `ID_SECRET`, for the email's key (SEC-018).
    pub secret: SecretString,
    /// The SEC-028 footer postal line.
    pub postal: Option<PostalAddress>,
    /// The Quote and Confirm link maker (BIZ-038).
    pub links: L,
}

/// A pick waiting for Confirm: its ID, job number and window end.
struct Waiting(Uuid, u32, Timestamp);

/// Queues the run due at `run_at`, one per clock hour (CARD-022, JOB-006).
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_open_cards(
    db: &mut PgConnection,
    run_at: Timestamp,
) -> Result<(), TaskError> {
    let mut task = NewTask::new(OPEN_CARDS_EMAIL_KIND, json!({}), run_at);
    let hour = run_at.as_second().div_euclid(HOUR);
    task.dedupe_key = Some(format!("{OPEN_CARDS_EMAIL_KIND}:{hour}"));
    enqueue(db, &task).await.map(drop)
}

/// CARD-022, BIZ-037, OPS-038: the [`OPEN_CARDS_EMAIL_KIND`] handler at `now` (the worker's
/// clock): queues the next hour's run first, then, in a slot's hour, each due person's email.
/// Safe to run twice: an email already stored for the person and slot is not stored again.
///
/// # Errors
///
/// A fixed [`HandlerError`] when a read, a link or the store fails.
pub async fn run_open_cards_email<L: OpenCardsLinks>(
    pool: PgPool,
    parts: Arc<OpenCardsParts<L>>,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let next = (now.as_second().div_euclid(HOUR) + 1) * HOUR;
    let next = Timestamp::from_second(next).map_err(|_| FAILED)?;
    schedule_open_cards(&mut db, next)
        .await
        .map_err(|_| FAILED)?;
    let slots = OpenCardsTime::ALL.map(OpenCardsTime::minute);
    if parts.postal.is_none() || slot_due(&slots, now).is_none() {
        return Ok(());
    }
    let mut ids = card_businesses(&mut db, now).await.map_err(|_| FAILED)?;
    let waiting = sqlx::query_scalar!(
        "SELECT DISTINCT business_id FROM picks.pick WHERE status = $1 AND confirm_due > $2",
        PickStatus::AwaitingConfirm.code(),
        now.to_sqlx() as _
    );
    ids.extend(waiting.fetch_all(&mut *db).await.map_err(|_| FAILED)?);
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        email_business(&mut db, &parts, id, now).await?;
    }
    Ok(())
}

/// CARD-022, D-345: `business`'s picks waiting for Confirm at `now`, earliest end first.
async fn waiting_picks(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<Vec<Waiting>, HandlerError> {
    let rows = sqlx::query!(
        "SELECT id, job_id, confirm_due AS \"due!\" FROM picks.pick \
         WHERE business_id = $1 AND status = $2 AND confirm_due > $3 ORDER BY confirm_due, id",
        business,
        PickStatus::AwaitingConfirm.code(),
        now.to_sqlx() as _
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| FAILED)?;
    let mut picks = Vec::new();
    for r in rows {
        let number = job_number(db, r.job_id).await.map_err(|_| FAILED)?;
        let number = u32::try_from(number).map_err(|_| FAILED)?;
        picks.push(Waiting(r.id, number, r.due.to_jiff()));
    }
    Ok(picks)
}

/// CARD-022, BIZ-037: queues the email of each person of business `id` for whom `digest_goes`
/// says so: only people who get job cards, the picks only for people who get picks, never while
/// the business is paused (a status that receives no cards, or away) or busy.
async fn email_business<L: OpenCardsLinks>(
    db: &mut PgConnection,
    parts: &OpenCardsParts<L>,
    id: Uuid,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let account = business(db, id).await.map_err(|_| FAILED)?;
    let away = away_until(db, id, now).await.map_err(|_| FAILED)?;
    let paused = !account.status.receives_cards() || away.is_some();
    let busy = busy_until(db, id, now).await.map_err(|_| FAILED)?.is_some();
    let mut cards = open_cards(db, id, now).await.map_err(|_| FAILED)?;
    cards.retain(|c| listable(c.ends, now, MIN_LEFT));
    let picks = waiting_picks(db, id, now).await?;
    for person in members(db, id).await.map_err(|_| FAILED)? {
        if !person.roles.cards_and_quotes() {
            continue;
        }
        let picks = match person.roles.confirm_and_billing() {
            true => picks.as_slice(),
            false => &[],
        };
        let facts = DigestFacts {
            chosen: chosen_minutes(db, person.id).await.map_err(|_| FAILED)?,
            paused,
            busy,
            card_ends: cards.iter().map(|c| c.ends).collect(),
            picks_waiting: picks.len(),
        };
        if digest_goes(&facts, now, MIN_LEFT) {
            queue_one(db, parts, (&person, &cards, picks), now).await?;
        }
    }
    Ok(())
}

/// CARD-022, BIZ-038, OPS-038, SEC-018: in one transaction, makes `person`'s Quote and Confirm
/// links and stores their email once for this clock hour as bulk; a second call in the hour
/// rolls its links back and stores nothing.
async fn queue_one<L: OpenCardsLinks>(
    db: &mut PgConnection,
    parts: &OpenCardsParts<L>,
    (person, cards, picks): (&TeamMember, &[OpenCardFacts], &[Waiting]),
    now: Timestamp,
) -> Result<(), HandlerError> {
    let Some(postal) = &parts.postal else {
        return Ok(());
    };
    let (who, mut links) = ((person.business, person.id), Vec::new());
    let mut tx = db.begin().await.map_err(|_| FAILED)?;
    let targets = (picks.iter().map(|p| (DigestLink::Confirm(p.0), p.2)))
        .chain(cards.iter().map(|c| (DigestLink::Quote(c.job), c.ends)));
    for (to, ends) in targets {
        links.push(parts.links.link(&mut tx, who, to, (now, ends)).await?);
    }
    let (confirm, quote) = links.split_at(picks.len());
    let picks: Vec<_> = (picks.iter().zip(confirm))
        .map(|(p, link)| WaitingPick {
            job: p.1,
            ends: p.2,
            confirm_link: link.expose_secret(),
        })
        .collect();
    let cards: Vec<_> = (cards.iter().zip(quote))
        .map(|(c, link)| OpenCard {
            trade: &c.trade,
            town: &c.town,
            ends: c.ends,
            quote_link: link.expose_secret(),
        })
        .collect();
    let Some(email) = open_cards_email((&cards, &picks), now, MIN_LEFT, Language::En) else {
        return Ok(());
    };
    match store(&mut tx, (&parts.secret, postal), person, &email, now).await? {
        true => tx.commit().await.map_err(|_| FAILED),
        false => Ok(()),
    }
}

/// OPS-038, SEC-018: stores `email` to `person`'s mailbox as bulk under the key of this clock
/// hour's slot; whether this call stored it.
async fn store(
    db: &mut PgConnection,
    keys: (&SecretString, &PostalAddress),
    person: &TeamMember,
    email: &ComposedEmail,
    now: Timestamp,
) -> Result<bool, HandlerError> {
    let slot = Uuid::from_u64_pair(SLOT_MARK, now.as_second().div_euclid(HOUR).unsigned_abs());
    let to = Recipient {
        id: person.mailbox,
        audience: Audience::Business,
        language: Language::En,
        manage_url: &PORTAL.to_string(),
    };
    let new = NewEmail {
        template_id: OPEN_CARDS_TEMPLATE,
        subject: &[person.business, slot],
        class: EmailClass::Bulk,
        notice: Notice::OpenCards {
            subject: &email.subject,
            lines: &email.lines,
        },
    };
    let stored = queue_email(db, keys, &to, &new, now).await;
    stored.map(|(_, new)| new).map_err(|_| FAILED)
}
