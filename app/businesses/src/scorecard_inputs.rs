//! The pilot scorecard's sign-up and complaint inputs from `businesses` (OPS-039, BIZ-053;
//! D-457, D-3753, D-3883; T478).
//!
//! Read-only, codes and counts only, each over a half-open window (`from` counted, `to` not).
//! Both sign-up readers take a `trade`: `None` counts every sign-up (the total), `Some(t)` only
//! those whose stored sign-up trades (`signup.trades`, D-3779) include `t`; a sign-up for two
//! trades counts once in each, so per-trade figures need not sum to the total.
//! [`signups_by_source`] counts the sign-ups submitted per BIZ-053 source: the `g` code of the link
//! they arrived on and the optional "Where did you hear about us?" answer, either may be absent.
//! [`signup_routes`] counts the same sign-ups, those sent to a mailed letter (`identity_step`
//! `letter`) or to a review (`review_since` set), and those active within 15 minutes. That last
//! one is not stored, so it is an approximation: a business whose status is now `active` and whose
//! `status_changed_at` is at most 15 minutes after the sign-up's `submitted_at`; a later status
//! change hides an early activation, so the figure is a floor. [`complaints_for_jobs`] counts the
//! homeowner complaints received in the window for the job IDs passed (one trade's or all, from
//! `jobs::api::jobs_by_trade`; a complaint stores no trade, D-3883); the scorecard divides per
//! 20 jobs. Leave and pause reasons (BIZ-054) come from `digest_counts::leaving_answers`, not a
//! second reader. No source code reaches a card or routing (D-457).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{BusinessesError, db};

/// The sign-ups from one BIZ-053 source in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupSource {
    /// The `g` code of the link, if any.
    pub g_code: Option<String>,
    /// The "Where did you hear about us?" answer code, if given.
    pub heard_from: Option<String>,
    /// The sign-ups.
    pub signups: i64,
}

/// Where the window's sign-ups went (OPS-039).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignupRoutes {
    /// The sign-ups submitted.
    pub signups: i64,
    /// Those sent to a mailed letter or to a review (one sign-up counted once).
    pub letter_or_review: i64,
    /// Approximation, not stored: those now active whose status changed within 15 minutes of
    /// the sign-up (module comment).
    pub active_within_15_minutes_approx: i64,
}

/// BIZ-053, OPS-039: the sign-ups submitted at or after `from` and before `to`, of trade `trade`
/// (or all with `None`), per `g` code and answer, in code order with absent ones last.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn signups_by_source(
    conn: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
    trade: Option<&str>,
) -> Result<Vec<SignupSource>, BusinessesError> {
    db(sqlx::query_as!(
        SignupSource,
        r#"SELECT g_code, heard_from, count(*) AS "signups!" FROM businesses.signup
           WHERE submitted_at >= $1 AND submitted_at < $2
             AND ($3::text IS NULL OR $3 = ANY(trades))
           GROUP BY g_code, heard_from ORDER BY g_code NULLS LAST, heard_from NULLS LAST"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        trade,
    )
    .fetch_all(conn)
    .await)
}

/// OPS-039: the sign-ups submitted at or after `from` and before `to`, of trade `trade` (or all
/// with `None`), those sent to a mailed
/// letter or a review, and the approximate active-within-15-minutes count (module comment).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn signup_routes(
    conn: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
    trade: Option<&str>,
) -> Result<SignupRoutes, BusinessesError> {
    db(sqlx::query_as!(
        SignupRoutes,
        r#"SELECT count(*) AS "signups!",
                  count(*) FILTER (WHERE s.identity_step = 'letter' OR s.review_since IS NOT NULL)
                    AS "letter_or_review!",
                  count(*) FILTER (WHERE b.status = 'active'
                    AND b.status_changed_at <= s.submitted_at + interval '15 minutes')
                    AS "active_within_15_minutes_approx!"
           FROM businesses.signup s JOIN businesses.business b ON b.id = s.business_id
           WHERE s.submitted_at >= $1 AND s.submitted_at < $2
             AND ($3::text IS NULL OR $3 = ANY(s.trades))"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        trade,
    )
    .fetch_one(conn)
    .await)
}

/// OPS-039: of jobs `jobs`, the homeowner complaints received at or after `from` and before `to`.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn complaints_for_jobs(
    conn: &mut PgConnection,
    jobs: &[Uuid],
    (from, to): (Timestamp, Timestamp),
) -> Result<i64, BusinessesError> {
    db(sqlx::query_scalar!(
        r#"SELECT count(*) AS "complaints!" FROM businesses.complaint
           WHERE job_id = ANY($1) AND received_at >= $2 AND received_at < $3"#,
        jobs,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_one(conn)
    .await)
}
