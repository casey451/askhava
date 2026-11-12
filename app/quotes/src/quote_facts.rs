//! A quote's PICK-040 quote facts, stored with the quote and read back for its card and for the
//! quote form's "Same as last time" pre-fill (PICK-040, PICK-051; D-233, D-283, D-3134, D-3190).
//!
//! [`submit_quote_with_facts`] runs `intake::submit_quote` inside its own transaction (whose
//! `begin` becomes a savepoint) and, only when the quote is `Saved`, writes the five answers to
//! `quotes.quote_facts` (migration 0168) with the job's type code read through `jobs::api`, so a
//! quote and its facts commit together and a refused quote stores neither. An edit replaces the
//! answers (PICK-051). The earliest visit stays on `quotes.quote`: [`stored_facts`] and
//! [`last_facts`] read it from there, and the visit inside the `facts` argument is never stored.
//! The answers are the business's own statements; nothing here sets or checks them (PICK-040).

use askhava_jobs::api::job_facts;
use jiff::Timestamp;
use jiff::civil::Date;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::intake::{DayPart, Quote, Submitted, submit_quote};
use crate::models::QuotesError;
use crate::quote_card::{Parts, Permit, QuoteFacts, Spanish, TripFee};

/// PICK-040, PICK-051, D-3134: stores `quote` through `submit_quote` and, only when it answers
/// [`Submitted::Saved`], `facts`' five answers in the same transaction, replacing any earlier ones
/// (an edit). The job's type code is the first candidate of its latest `jobs::api::job_facts` row,
/// none while the job has no row (D-3190). `facts.earliest_visit` is not stored: the quote's own
/// earliest visit is the one read back. Any other answer stores nothing and is returned as it is.
///
/// # Errors
///
/// [`QuotesError::Job`] when the job's facts could not be read, else `submit_quote`'s errors or
/// [`QuotesError::Database`]; nothing is stored then.
pub async fn submit_quote_with_facts(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    quote: &Quote,
    facts: &QuoteFacts,
    now: Timestamp,
) -> Result<Submitted, QuotesError> {
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    let submitted = submit_quote(&mut tx, (job, business), quote, now).await?;
    if submitted != Submitted::Saved {
        return Ok(submitted);
    }
    let job_type = job_facts(&mut tx, job)
        .await
        .map_err(|_| QuotesError::Job)?
        .and_then(|f| f.job_types.into_iter().next());
    sqlx::query!(
        "INSERT INTO quotes.quote_facts (job_id, business_id, trip_fee, parts, permit, \
         may_change, spanish, job_type, recorded_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (job_id, business_id) DO UPDATE SET trip_fee = EXCLUDED.trip_fee, \
         parts = EXCLUDED.parts, permit = EXCLUDED.permit, may_change = EXCLUDED.may_change, \
         spanish = EXCLUDED.spanish, job_type = EXCLUDED.job_type, \
         recorded_at = EXCLUDED.recorded_at",
        job,
        business,
        trip_code(facts.trip_fee),
        parts_code(facts.parts),
        permit_code(facts.permit),
        facts.may_change,
        spanish_code(facts.spanish),
        job_type,
        now.to_sqlx()
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| QuotesError::Database)?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(Submitted::Saved)
}

/// PICK-040: the quote facts of `business`'s quote on `job`: its stored answers with the quote's
/// earliest visit, or none when the quote has no facts row or no earliest visit (a card is then
/// not built, D-3134). Withdrawn or unconfirmed quotes are read too; the card read decides what
/// is shown.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn stored_facts(
    conn: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<Option<QuoteFacts>, QuotesError> {
    let row = sqlx::query!(
        "SELECT f.trip_fee, f.parts, f.permit, f.may_change, f.spanish, \
         q.earliest_visit::text AS visit, q.visit_part \
         FROM quotes.quote_facts f JOIN quotes.quote q USING (job_id, business_id) \
         WHERE f.job_id = $1 AND f.business_id = $2",
        job,
        business
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| QuotesError::Database)?;
    let Some(r) = row else { return Ok(None) };
    let answers = (r.trip_fee, r.parts, r.permit, r.may_change, r.spanish);
    facts_of(&answers, r.visit.as_deref(), r.visit_part.as_deref())
}

/// PICK-040, D-233, D-283: the facts of `business`'s most recently recorded quote on a job of
/// `job_type` that has an earliest visit, for the quote form's pre-fill (`portals::api::prefill`
/// drops the visit), or none for its first quote of that type. Another business's or another
/// type's answers are never read.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub async fn last_facts(
    conn: &mut PgConnection,
    business: Uuid,
    job_type: &str,
) -> Result<Option<QuoteFacts>, QuotesError> {
    let row = sqlx::query!(
        "SELECT f.trip_fee, f.parts, f.permit, f.may_change, f.spanish, \
         q.earliest_visit::text AS visit, q.visit_part \
         FROM quotes.quote_facts f JOIN quotes.quote q USING (job_id, business_id) \
         WHERE f.business_id = $1 AND f.job_type = $2 \
         AND q.earliest_visit IS NOT NULL AND q.visit_part IS NOT NULL \
         ORDER BY f.recorded_at DESC, f.job_id DESC LIMIT 1",
        business,
        job_type
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| QuotesError::Database)?;
    let Some(r) = row else { return Ok(None) };
    let answers = (r.trip_fee, r.parts, r.permit, r.may_change, r.spanish);
    facts_of(&answers, r.visit.as_deref(), r.visit_part.as_deref())
}

/// The stored codes, in column order: trip fee, parts, permit, may change, Spanish.
type Answers = (String, String, String, bool, String);

/// The facts from stored `answers` and the quote's visit; none without a visit. A code the
/// migration's checks would refuse is a [`QuotesError::Database`].
fn facts_of(
    answers: &Answers,
    visit: Option<&str>,
    part: Option<&str>,
) -> Result<Option<QuoteFacts>, QuotesError> {
    let date = visit.and_then(|d| d.parse::<Date>().ok());
    let Some(earliest_visit) = date.zip(part.and_then(DayPart::from_code)) else {
        return Ok(None);
    };
    let (trip, parts, permit, may_change, spanish) = answers;
    let facts = QuoteFacts {
        trip_fee: from_code(trip, TRIP, trip_code).ok_or(QuotesError::Database)?,
        parts: from_code(parts, PARTS, parts_code).ok_or(QuotesError::Database)?,
        permit: from_code(permit, PERMIT, permit_code).ok_or(QuotesError::Database)?,
        may_change: *may_change,
        earliest_visit,
        spanish: from_code(spanish, SPANISH, spanish_code).ok_or(QuotesError::Database)?,
    };
    Ok(Some(facts))
}

const TRIP: [TripFee; 3] = [TripFee::Counts, TripFee::DoesNotCount, TripFee::None];
const PARTS: [Parts; 3] = [Parts::Included, Parts::Extra, Parts::OnSite];
const PERMIT: [Permit; 3] = [Permit::Included, Permit::NotIncluded, Permit::NotNeeded];
const SPANISH: [Spanish; 4] = [Spanish::Phone, Spanish::OnSite, Spanish::Both, Spanish::No];

/// The answer in `all` whose `code` is `stored`.
fn from_code<T: Copy, const N: usize>(
    stored: &str,
    all: [T; N],
    code: fn(T) -> &'static str,
) -> Option<T> {
    all.into_iter().find(|a| code(*a) == stored)
}

/// The `quotes.quote_facts.trip_fee` code (the quote form's own codes, `portals::api::prefill`).
const fn trip_code(t: TripFee) -> &'static str {
    match t {
        TripFee::Counts => "counts",
        TripFee::DoesNotCount => "does_not_count",
        TripFee::None => "none",
    }
}

/// The `quotes.quote_facts.parts` code.
const fn parts_code(p: Parts) -> &'static str {
    match p {
        Parts::Included => "included",
        Parts::Extra => "extra",
        Parts::OnSite => "on_site",
    }
}

/// The `quotes.quote_facts.permit` code.
const fn permit_code(p: Permit) -> &'static str {
    match p {
        Permit::Included => "included",
        Permit::NotIncluded => "not_included",
        Permit::NotNeeded => "not_needed",
    }
}

/// The `quotes.quote_facts.spanish` code.
const fn spanish_code(s: Spanish) -> &'static str {
    match s {
        Spanish::Phone => "phone",
        Spanish::OnSite => "on_site",
        Spanish::Both => "both",
        Spanish::No => "no",
    }
}
