//! The job's stored words and cards: the scrubbed description and the card shown, approved and
//! sent, kept per job for 3 years after close (T411; PRIV-008, PRIV-011, PRIV-006, CARD-007,
//! CARD-023, INT-041; D-3189).
//!
//! [`keep_description`] stores one inbound text's scrubbed words for a job in
//! `quotes.job_description`; it takes only `privacy`'s [`ScrubbedText`], so raw words cannot be
//! stored (D-3118, D-801); [`description`] reads them back in time order. [`keep_card`] stores a
//! card in `quotes.shown_card` only under its own digest ([`card_digest`], the value
//! `intake::api::card_digest` gives and approval stores); [`stored_card`], [`unsent_cards`] and
//! [`sent_card`] give a card only while its stored text still has that digest, so the card sent is
//! the card approved (CARD-007); [`mark_card_sent`] stamps the one card sent per job. The PRIV-008
//! text stage ([`run_text_purge`]) and "Delete my data" ([`forget_requests`]) delete the rows.
//!
//! Only the card sender (T251), the intake order that builds and shows the card (T162) and the
//! quote form with the job page's data (T301) read the words and card text. Every returned type's
//! `Debug` hides them; nothing here logs them or sends them to an AI call.
//!
//! `keep_description` refuses plain text at compile time (TD-PRIV-27):
//!
//! ```compile_fail
//! # async fn f(db: &mut sqlx::PgConnection) {
//! let id = sqlx::types::Uuid::nil();
//! let at = jiff::Timestamp::UNIX_EPOCH;
//! let _ = askhava_quotes::api::keep_description(db, (id, id), id, "raw words", at).await;
//! # }
//! ```

use askhava_core::api::{Claimed, HandlerError};
use askhava_privacy::api::{PurgeStage, ScrubbedText, purge_target};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::cards::{JobCard, Place};
use crate::models::QuotesError;

const DB_FAILED: HandlerError = HandlerError("quotes text purge database call failed");

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, QuotesError> {
    result.map_err(|_| QuotesError::Database)
}

/// CARD-007, INT-041: a card text's digest, the first 32 hex digits of the SHA256 of the text;
/// the same value `intake::api::card_digest` gives and the approval stores.
#[must_use]
pub fn card_digest(text: &str) -> String {
    let hex: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    hex[..32].to_owned()
}

/// A job's stored scrubbed words, one entry per inbound text in time order (PRIV-008). `Debug`
/// prints a count only (PRIV-006).
#[derive(Clone, PartialEq, Eq)]
pub struct JobDescription(Vec<String>);

impl JobDescription {
    /// Each text's scrubbed words, oldest first.
    #[must_use]
    pub fn parts(&self) -> &[String] {
        &self.0
    }

    /// The words of every text, oldest first, one line each.
    #[must_use]
    pub fn text(&self) -> String {
        self.0.join("\n")
    }
}

impl std::fmt::Debug for JobDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JobDescription({} texts)", self.0.len())
    }
}

/// PRIV-008, D-3118: stores `words` of inbound `message` for `job` of `request`; `true` when
/// stored, `false` for a repeat of the same message and job. Takes only a [`ScrubbedText`].
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn keep_description(
    db_conn: &mut PgConnection,
    (request, job): (Uuid, Uuid),
    message: Uuid,
    words: &ScrubbedText,
    at: Timestamp,
) -> Result<bool, QuotesError> {
    let done = db(sqlx::query!(
        "INSERT INTO quotes.job_description (request_id, job_id, message_id, words, recorded_at)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (message_id, job_id) DO NOTHING",
        request,
        job,
        message,
        words.as_str(),
        at.to_sqlx(),
    )
    .execute(db_conn)
    .await)?;
    Ok(done.rows_affected() == 1)
}

/// PRIV-008, CARD-007: `job`'s stored words in time order, or `None` when none are stored.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn description(
    db_conn: &mut PgConnection,
    job: Uuid,
) -> Result<Option<JobDescription>, QuotesError> {
    let words = db(sqlx::query_scalar!(
        "SELECT words FROM quotes.job_description WHERE job_id = $1 ORDER BY recorded_at, id",
        job,
    )
    .fetch_all(db_conn)
    .await)?;
    Ok((!words.is_empty()).then_some(JobDescription(words)))
}

/// What a stored card is for: its category code, the remaining candidate job type codes and the
/// INT-029 emergency mark (CARD-001, INT-029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFacts {
    /// The category code (`plumbing_repair`).
    pub category: String,
    /// The remaining candidate job type codes.
    pub candidates: Vec<String>,
    /// True only for a card stored on the INT-029 emergency path (no approval step).
    pub emergency: bool,
}

/// What [`keep_card`] did (CARD-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptCard {
    /// The card was stored.
    Stored,
    /// The same request, job and digest were stored before; nothing was written.
    Repeat,
    /// `digest` is not the card text's digest; nothing was written.
    WrongDigest,
}

/// CARD-007, INT-041, INT-029: stores `card` for `job` of `request` under `digest` with its facts,
/// only when `digest` is [`card_digest`] of the card text; a repeat stores nothing.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn keep_card(
    db_conn: &mut PgConnection,
    (request, job): (Uuid, Uuid),
    digest: &str,
    card: &JobCard,
    facts: &CardFacts,
    at: Timestamp,
) -> Result<KeptCard, QuotesError> {
    if card_digest(card.text()) != digest {
        return Ok(KeptCard::WrongDigest);
    }
    let done = db(sqlx::query!(
        "INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place,
           photo_ids, category, candidates, emergency, recorded_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (request_id, job_id, digest) DO NOTHING",
        request,
        job,
        digest,
        card.text(),
        card.town(),
        card.place().code(),
        card.photos(),
        facts.category,
        &facts.candidates,
        facts.emergency,
        at.to_sqlx(),
    )
    .execute(db_conn)
    .await)?;
    Ok(match done.rows_affected() {
        1 => KeptCard::Stored,
        _ => KeptCard::Repeat,
    })
}

/// A stored card with its job, digest and facts (CARD-007, CARD-023). Its `Debug` hides the card
/// text through [`JobCard`]'s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCard {
    /// The request the card was shown in.
    pub request: Uuid,
    /// The job.
    pub job: Uuid,
    /// Its 32-hex card digest.
    pub digest: String,
    /// The card exactly as `build_card` made it.
    pub card: JobCard,
    /// Category, candidates and the emergency mark.
    pub facts: CardFacts,
}

struct CardRow {
    request_id: Uuid,
    job_id: Uuid,
    digest: String,
    card_text: String,
    town: String,
    place: String,
    photo_ids: Vec<Uuid>,
    category: String,
    candidates: Vec<String>,
    emergency: bool,
}

/// CARD-007: the row as a card, only while its text still has its stored digest.
fn checked(row: CardRow) -> Option<StoredCard> {
    let place = Place::from_code(&row.place)?;
    (card_digest(&row.card_text) == row.digest).then(|| StoredCard {
        request: row.request_id,
        job: row.job_id,
        digest: row.digest,
        card: JobCard::from_stored(row.town, place, row.photo_ids, row.card_text),
        facts: CardFacts {
            category: row.category,
            candidates: row.candidates,
            emergency: row.emergency,
        },
    })
}

/// CARD-007: `job`'s card stored under `digest`, only while its text's digest still equals it.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn stored_card(
    db_conn: &mut PgConnection,
    job: Uuid,
    digest: &str,
) -> Result<Option<StoredCard>, QuotesError> {
    let row = db(sqlx::query_as!(
        CardRow,
        "SELECT request_id, job_id, digest, card_text, town, place, photo_ids, category,
           candidates, emergency FROM quotes.shown_card WHERE job_id = $1 AND digest = $2
         ORDER BY recorded_at DESC, id LIMIT 1",
        job,
        digest,
    )
    .fetch_optional(db_conn)
    .await)?;
    Ok(row.and_then(checked))
}

/// INT-029, T251: for each job of `request` with no card sent, its newest stored card (with its
/// emergency mark), oldest job first; a card whose text no longer matches its digest is left out.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn unsent_cards(
    db_conn: &mut PgConnection,
    request: Uuid,
) -> Result<Vec<StoredCard>, QuotesError> {
    let rows = db(sqlx::query_as!(
        CardRow,
        r#"SELECT request_id AS "request_id!", job_id AS "job_id!", digest AS "digest!",
             card_text AS "card_text!", town AS "town!", place AS "place!",
             photo_ids AS "photo_ids!", category AS "category!", candidates AS "candidates!",
             emergency AS "emergency!"
           FROM (SELECT DISTINCT ON (s.job_id) s.*,
                   min(s.recorded_at) OVER (PARTITION BY s.job_id) AS first_at
                 FROM quotes.shown_card s WHERE s.request_id = $1
                   AND NOT EXISTS (SELECT 1 FROM quotes.shown_card t
                                   WHERE t.job_id = s.job_id AND t.sent_at IS NOT NULL)
                 ORDER BY s.job_id, s.recorded_at DESC, s.id) newest
           ORDER BY first_at, job_id"#,
        request,
    )
    .fetch_all(db_conn)
    .await)?;
    Ok(rows.into_iter().filter_map(checked).collect())
}

/// CARD-007, CARD-023: marks `job`'s card under `digest` sent at `at`, once per job; `true` when
/// this call marked it.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn mark_card_sent(
    db_conn: &mut PgConnection,
    job: Uuid,
    digest: &str,
    at: Timestamp,
) -> Result<bool, QuotesError> {
    let done = db(sqlx::query!(
        "UPDATE quotes.shown_card SET sent_at = $3 WHERE id = (SELECT id FROM quotes.shown_card
           WHERE job_id = $1 AND digest = $2 AND recorded_at <= $3 ORDER BY recorded_at DESC, id
           LIMIT 1)
           AND NOT EXISTS (SELECT 1 FROM quotes.shown_card WHERE job_id = $1 AND sent_at IS NOT NULL)",
        job,
        digest,
        at.to_sqlx(),
    )
    .execute(db_conn)
    .await)?;
    Ok(done.rows_affected() == 1)
}

/// CARD-023: the card that went out for `job`, for the business job page and the quote form;
/// `None` before one was sent or when its stored text no longer matches its digest.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn sent_card(
    db_conn: &mut PgConnection,
    job: Uuid,
) -> Result<Option<StoredCard>, QuotesError> {
    let row = db(sqlx::query_as!(
        CardRow,
        "SELECT request_id, job_id, digest, card_text, town, place, photo_ids, category,
           candidates, emergency FROM quotes.shown_card WHERE job_id = $1 AND sent_at IS NOT NULL",
        job,
    )
    .fetch_optional(db_conn)
    .await)?;
    Ok(row.and_then(checked))
}

/// PRIV-008: deletes `job`'s stored words and cards; returns how many rows went.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn purge_job_text(db_conn: &mut PgConnection, job: Uuid) -> Result<u64, QuotesError> {
    let words = db(
        sqlx::query!("DELETE FROM quotes.job_description WHERE job_id = $1", job)
            .execute(&mut *db_conn)
            .await,
    )?;
    let cards = db(
        sqlx::query!("DELETE FROM quotes.shown_card WHERE job_id = $1", job)
            .execute(db_conn)
            .await,
    )?;
    Ok(words.rows_affected() + cards.rows_affected())
}

/// PRIV-008: the `privacy::api::PURGE_KIND` step: on the text stage (3 years after close, holds
/// already applied by `privacy`) deletes the job's rows; the photo and contact stages do nothing.
/// Safe to run twice.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database failure.
pub async fn run_text_purge(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    if stage != PurgeStage::Text {
        return Ok(());
    }
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    purge_job_text(&mut conn, job)
        .await
        .map(|_| ())
        .map_err(|_| DB_FAILED)
}

/// PRIV-011: "Delete my data": deletes the stored words and cards of `requests` (every request of
/// hers, open or closed) on the caller's transaction and returns the jobs those rows named, so
/// the caller passes them to `privacy::api::delete_my_data` and their photos go too.
///
/// # Errors
/// [`QuotesError::Database`].
pub async fn forget_requests(
    db_conn: &mut PgConnection,
    requests: &[Uuid],
) -> Result<Vec<Uuid>, QuotesError> {
    let jobs = db(sqlx::query_scalar!(
        r#"SELECT job_id AS "job!" FROM quotes.job_description WHERE request_id = ANY($1)
           UNION SELECT job_id FROM quotes.shown_card WHERE request_id = ANY($1)
           ORDER BY 1"#,
        requests,
    )
    .fetch_all(&mut *db_conn)
    .await)?;
    for job in &jobs {
        purge_job_text(db_conn, *job).await?;
    }
    Ok(jobs)
}
