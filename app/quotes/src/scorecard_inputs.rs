//! The pilot scorecard's quote inputs and PICK-052's returned-note record (OPS-039, OPS-047,
//! PICK-040, PICK-052; D-458, D-468, D-3753, D-3844, D-3877, D-3882, D-3883, D-3920, D-3921),
//! read-only apart from [`record_note_returned`].
//!
//! The trade of a job is the `category` of its first sent card (`quotes.shown_card`), and
//! [`jobs_by_trade`] lists every job carded before `to` per trade, so the `payments` and `picks`
//! readers, windowed on their own event times, see a pick or charge in the window on a job carded
//! earlier. A request is one carded job; request-level inputs ([`request_quotes`],
//! [`requests_with_quotes`]) are windowed on the job's first card `sent_at` (at or after `from`,
//! before `to`) and count only quotes received before `to`. Business-level inputs are windowed on
//! the quote's `received_at`. Counts and durations only; an empty window gives `None`.

use askhava_ops::api::{Actor, AuditEntry, Change, audit, audit_subjects};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::QuotesError;
use crate::notes::NoteProblem;

/// The audit action of a note sent back to its business (PICK-052, D-3844).
pub const NOTE_RETURNED_ACTION: &str = "quotes.note_returned";

const DB: QuotesError = QuotesError::Database;

/// PICK-052, PRIV-028: the code name of a returned note's reason, the audit row's `field`.
fn problem_code(problem: NoteProblem) -> &'static str {
    match problem {
        NoteProblem::TooLong => "too_long",
        NoteProblem::Contact => "contact",
        NoteProblem::Restricted => "restricted",
        NoteProblem::OffPlatform => "off_platform",
        NoteProblem::Flagged(_) => "flagged",
    }
}

/// PICK-052, OPS-039, PRIV-028, D-3844: records that `business`'s note on `job` went back for
/// `problem`: one [`NOTE_RETURNED_ACTION`] audit row in its own transaction, the business the
/// actor, the job (`jobs.job`) the subject and the reason's code name the field; never the note.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn record_note_returned(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    problem: NoteProblem,
) -> Result<(), QuotesError> {
    let mut tx = db.begin().await.map_err(|_| DB)?;
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action: NOTE_RETURNED_ACTION,
        subject_kind: "jobs.job",
        subject_id: job,
        change: Change::Field(problem_code(problem)),
    };
    audit(&mut tx, &entry).await.map_err(|_| DB)?;
    tx.commit().await.map_err(|_| DB)
}

/// OPS-039, PICK-052, D-3844: notes returned to businesses on `jobs` in the window, from the
/// audit log through `ops::api::audit_subjects`; `None` with none.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn notes_returned(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<i64>, QuotesError> {
    let rows = audit_subjects(db, NOTE_RETURNED_ACTION, from, to)
        .await
        .map_err(|_| DB)?;
    let count = rows.iter().filter(|r| jobs.contains(&r.subject_id)).count();
    let count = i64::try_from(count).map_err(|_| DB)?;
    Ok((count > 0).then_some(count))
}

/// One trade's jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeJobs {
    /// The trade's catalog code name (`quotes.shown_card.category`).
    pub trade: String,
    /// Its jobs, by ID.
    pub jobs: Vec<Uuid>,
}

/// OPS-039, D-3877, D-3920: every job whose first card was sent before `to`, per trade (the
/// first sent card's category), by trade name; the total is their union.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn jobs_by_trade(
    db: &mut PgConnection,
    to: Timestamp,
) -> Result<Vec<TradeJobs>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT category, array_agg(job_id ORDER BY job_id) AS \"jobs!\" FROM \
           (SELECT DISTINCT ON (job_id) job_id, category FROM quotes.shown_card \
            WHERE sent_at < $1 ORDER BY job_id, sent_at) first_card \
         GROUP BY category ORDER BY category",
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| DB)?;
    Ok(rows
        .into_iter()
        .map(|r| TradeJobs {
            trade: r.category,
            jobs: r.jobs,
        })
        .collect())
}

/// The window's requests and their quotes (OPS-039).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestQuotes {
    /// Requests whose first card was sent in the window.
    pub requests: i64,
    /// Of those, the ones with 1 or more, 2 or more and 3 or more quotes within 24 hours of the
    /// first card.
    pub within_24_hours: [i64; 3],
    /// The median time from the first card to the first quote; `None` with no quote.
    pub median_first_quote: Option<SignedDuration>,
    /// The 75th-percentile time to the first quote.
    pub p75_first_quote: Option<SignedDuration>,
    /// Requests with a photo on a sent card (D-3883).
    pub with_photo: i64,
}

/// OPS-039, D-458, D-468, D-3883, D-3921: the [`RequestQuotes`] of `jobs` whose first card was sent
/// in the window, counting only quotes received before `to`; `None` with no request.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn request_quotes(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<RequestQuotes>, QuotesError> {
    let r = sqlx::query!(
        "WITH req AS (SELECT job_id, min(sent_at) AS carded, \
             bool_or(cardinality(photo_ids) > 0) AS photo FROM quotes.shown_card \
           WHERE job_id = ANY($1) AND sent_at < $3 GROUP BY job_id HAVING min(sent_at) >= $2), \
         got AS (SELECT r.photo, \
             count(q.id) FILTER (WHERE q.received_at < r.carded + interval '24 hours') AS day, \
             extract(epoch FROM min(q.received_at) - r.carded) AS first \
           FROM req r LEFT JOIN quotes.quote q ON q.job_id = r.job_id AND q.received_at < $3 \
           GROUP BY r.job_id, r.carded, r.photo) \
         SELECT count(*) AS \"requests!\", count(*) FILTER (WHERE day >= 1) AS \"one!\", \
           count(*) FILTER (WHERE day >= 2) AS \"two!\", \
           count(*) FILTER (WHERE day >= 3) AS \"three!\", \
           round(percentile_cont(0.5) WITHIN GROUP (ORDER BY first))::bigint AS median, \
           round(percentile_cont(0.75) WITHIN GROUP (ORDER BY first))::bigint AS p75, \
           count(*) FILTER (WHERE photo) AS \"photo!\" FROM got",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok((r.requests > 0).then_some(RequestQuotes {
        requests: r.requests,
        within_24_hours: [r.one, r.two, r.three],
        median_first_quote: r.median.map(SignedDuration::from_secs),
        p75_first_quote: r.p75.map(SignedDuration::from_secs),
        with_photo: r.photo,
    }))
}

/// OPS-039, D-468, D-3921: the IDs of `jobs` whose first card was sent in the window and that
/// have `min` or more quotes received before `to`, for `picks::api::picked_requests`.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn requests_with_quotes(
    db: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
    min: i64,
) -> Result<Vec<Uuid>, QuotesError> {
    sqlx::query_scalar!(
        "SELECT c.job_id FROM quotes.shown_card c \
         WHERE c.job_id = ANY($1) AND c.sent_at < $3 GROUP BY c.job_id \
         HAVING min(c.sent_at) >= $2 AND (SELECT count(*) FROM quotes.quote q \
           WHERE q.job_id = c.job_id AND q.received_at < $3) >= $4 ORDER BY c.job_id",
        jobs,
        from.to_sqlx(),
        to.to_sqlx(),
        min
    )
    .fetch_all(db)
    .await
    .map_err(|_| DB)
}

/// OPS-047, D-468: each business's quotes on `jobs` received in the window, by business ID, for
/// the caller to divide by its picks and wins; `None` with none.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn quotes_per_business(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<Vec<(Uuid, i64)>>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT business_id, count(*) AS \"n!\" FROM quotes.quote WHERE job_id = ANY($1) \
           AND received_at >= $2 AND received_at < $3 GROUP BY business_id ORDER BY business_id",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| DB)?;
    let counts: Vec<_> = rows.into_iter().map(|r| (r.business_id, r.n)).collect();
    Ok((!counts.is_empty()).then_some(counts))
}

/// OPS-039, D-468: active businesses: those that quoted on `jobs` in the 30 days before `to`;
/// `None` with none.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn active_businesses(
    db: &mut PgConnection,
    jobs: &[Uuid],
    to: Timestamp,
) -> Result<Option<i64>, QuotesError> {
    let n = sqlx::query_scalar!(
        "SELECT count(DISTINCT business_id) AS \"n!\" FROM quotes.quote WHERE job_id = ANY($1) \
           AND received_at >= $2::timestamptz - interval '30 days' AND received_at < $2",
        jobs,
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok((n > 0).then_some(n))
}

/// OPS-047, PICK-040: the median of the optional "minutes this quote took" on `jobs`' quotes
/// received in the window, to the second; `None` when no quote gave it.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn median_quote_minutes(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<SignedDuration>, QuotesError> {
    let secs = sqlx::query_scalar!(
        "SELECT round(percentile_cont(0.5) WITHIN GROUP (ORDER BY quote_minutes * 60))::bigint \
         FROM quotes.quote WHERE job_id = ANY($1) AND received_at >= $2 AND received_at < $3",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok(secs.map(SignedDuration::from_secs))
}

/// OPS-039: the (job, business) of every diagnostic-visit quote on `jobs`, for
/// `picks::api::diagnostic_share`.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn diagnostic_quotes(
    db: &mut PgConnection,
    jobs: &[Uuid],
) -> Result<Vec<(Uuid, Uuid)>, QuotesError> {
    let rows = sqlx::query!(
        "SELECT job_id, business_id FROM quotes.quote \
         WHERE job_id = ANY($1) AND format = 'diagnostic_visit' ORDER BY job_id, business_id",
        jobs
    )
    .fetch_all(db)
    .await
    .map_err(|_| DB)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.job_id, r.business_id))
        .collect())
}
