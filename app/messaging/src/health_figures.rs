//! The messaging figures of the owner health page (OPS-008, OPS-004, OPS-037; T475, D-3753).
//!
//! Each reader takes a window `[from, to)` and runs one query; shares are per thousand, rounded
//! down, and medians are whole seconds, all computed in SQL; an empty window gives `None`, never a
//! division by zero. Counts and durations only, never a phone, an address, a name or a body
//! (PRIV-005).
//!
//! - [`text_figures`]: per audience, texts sent in the window (`messaging.outbox` `sent`, by
//!   `sent_at`), those Twilio reported `delivered` (`messaging.delivery_event`) and its rate, and
//!   failed texts: a sent text reported `undelivered` or `failed`, or a row whose retries ran out
//!   (`failed`, by `created_at`).
//! - [`email_figures`]: per audience, emails sent in the window (`messaging.email_outbox` `sent`)
//!   and how many drew an `email.delivered`, `email.bounced` or `email.complained` event
//!   (`messaging.email_event`), each with its rate.
//! - [`emails_sent`]: emails sent in the window, for the OPS-004 sending-limit use; the page calls
//!   it with today's and this month's windows and takes the limits from settings. Only `sent`
//!   counts; any other status, a later one too, counts as not sent.
//! - [`notice_timing`]: OPS-037's medians from `messaging.notice_log` (`email_timing`, T198): from
//!   send to first opening (card and pick notices), to quote (cards) and to Confirm (picks), and
//!   the share of cards quoted within a limit (a setting, 6 hours). One business's notices about
//!   one job of one kind are one card or pick: its first send, first opening and first act.

use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;

use crate::models::EmailError;

/// The two audiences a figure is split by (OPS-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerAudience<T> {
    /// Homeowners.
    pub homeowner: T,
    /// Businesses.
    pub business: T,
}

impl<T> PerAudience<T> {
    /// The slot of `kind` (`homeowner` or `business`, the tables' `recipient_kind` codes).
    fn slot(&mut self, kind: &str) -> Option<&mut T> {
        match kind {
            "homeowner" => Some(&mut self.homeowner),
            "business" => Some(&mut self.business),
            _ => None,
        }
    }
}

/// One audience's texts in a window (OPS-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextFigures {
    /// Texts sent.
    pub sent: i64,
    /// Of those, the texts reported delivered.
    pub delivered: i64,
    /// `delivered` per thousand `sent`; `None` with none sent.
    pub delivered_per_mille: Option<i64>,
    /// Sent texts reported undelivered or failed, and rows whose retries ran out.
    pub failed: i64,
}

/// One audience's emails in a window (OPS-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmailFigures {
    /// Emails sent.
    pub sent: i64,
    /// Of those, the emails with a delivered, a bounced and a complained event.
    pub delivered: i64,
    /// See `delivered`.
    pub bounced: i64,
    /// See `delivered`.
    pub complained: i64,
    /// `delivered`, `bounced` and `complained` per thousand `sent`; `None` with none sent.
    pub per_mille: Option<(i64, i64, i64)>,
}

/// OPS-037's weekly timing figures (OPS-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoticeTiming {
    /// Median from send to the first opening of a card's Quote page or a pick's Confirm page.
    pub to_first_open: Option<SignedDuration>,
    /// Median from a card's send to its quote.
    pub to_quote: Option<SignedDuration>,
    /// Median from a pick's send to its Confirm.
    pub to_confirm: Option<SignedDuration>,
    /// Cards quoted within the limit, per thousand cards sent; `None` with no card.
    pub quoted_within_per_mille: Option<i64>,
}

/// OPS-008: [`TextFigures`] per audience for texts of `[from, to)`.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn text_figures(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<PerAudience<TextFigures>, EmailError> {
    let rows = sqlx::query!(
        r#"WITH t AS (
             SELECT o.recipient_kind AS kind, o.status = 'sent' AS sent,
                    EXISTS (SELECT 1 FROM messaging.delivery_event e
                             WHERE e.provider_sid = o.provider_sid AND e.status = 'delivered') AS ok,
                    o.status = 'failed' OR EXISTS (SELECT 1 FROM messaging.delivery_event e
                             WHERE e.provider_sid = o.provider_sid
                               AND e.status IN ('undelivered', 'failed')) AS bad
               FROM messaging.outbox o
              WHERE (o.status = 'sent' AND o.sent_at >= $1 AND o.sent_at < $2)
                 OR (o.status = 'failed' AND o.created_at >= $1 AND o.created_at < $2))
           SELECT kind AS "kind!", count(*) FILTER (WHERE sent) AS "sent!",
                  count(*) FILTER (WHERE sent AND ok) AS "delivered!",
                  1000 * count(*) FILTER (WHERE sent AND ok)
                    / NULLIF(count(*) FILTER (WHERE sent), 0) AS "rate?",
                  count(*) FILTER (WHERE bad) AS "failed!"
             FROM t GROUP BY kind"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| EmailError::Database)?;
    let mut out = PerAudience::<TextFigures>::default();
    for r in rows {
        if let Some(slot) = out.slot(&r.kind) {
            *slot = TextFigures {
                sent: r.sent,
                delivered: r.delivered,
                delivered_per_mille: r.rate,
                failed: r.failed,
            };
        }
    }
    Ok(out)
}

/// OPS-008: [`EmailFigures`] per audience for emails sent in `[from, to)`.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn email_figures(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<PerAudience<EmailFigures>, EmailError> {
    let rows = sqlx::query!(
        r#"WITH m AS (
             SELECT o.recipient_kind AS kind,
                    count(*) FILTER (WHERE e.kind = 'email.delivered') > 0 AS ok,
                    count(*) FILTER (WHERE e.kind = 'email.bounced') > 0 AS bounced,
                    count(*) FILTER (WHERE e.kind = 'email.complained') > 0 AS complained
               FROM messaging.email_outbox o
               LEFT JOIN messaging.email_event e ON e.provider_id = o.provider_id
              WHERE o.status = 'sent' AND o.sent_at >= $1 AND o.sent_at < $2
              GROUP BY o.id, o.recipient_kind)
           SELECT kind AS "kind!", count(*) AS "sent!", count(*) FILTER (WHERE ok) AS "ok!",
                  count(*) FILTER (WHERE bounced) AS "bounced!",
                  count(*) FILTER (WHERE complained) AS "complained!",
                  1000 * count(*) FILTER (WHERE ok) / NULLIF(count(*), 0) AS "ok_rate?",
                  1000 * count(*) FILTER (WHERE bounced) / NULLIF(count(*), 0) AS "bounced_rate?",
                  1000 * count(*) FILTER (WHERE complained) / NULLIF(count(*), 0)
                    AS "complained_rate?"
             FROM m GROUP BY kind"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| EmailError::Database)?;
    let mut out = PerAudience::<EmailFigures>::default();
    for r in rows {
        if let Some(slot) = out.slot(&r.kind) {
            *slot = EmailFigures {
                sent: r.sent,
                delivered: r.ok,
                bounced: r.bounced,
                complained: r.complained,
                per_mille: r
                    .ok_rate
                    .zip(r.bounced_rate)
                    .zip(r.complained_rate)
                    .map(|((ok, bounced), complained)| (ok, bounced, complained)),
            };
        }
    }
    Ok(out)
}

/// OPS-004, OPS-008: emails sent in `[from, to)`; only a `sent` row counts.
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn emails_sent(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<i64, EmailError> {
    sqlx::query_scalar!(
        r#"SELECT count(*) AS "sent!" FROM messaging.email_outbox
            WHERE status = 'sent' AND sent_at >= $1 AND sent_at < $2"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// OPS-037, OPS-008: the [`NoticeTiming`] of cards and picks first sent in `[from, to)`, with
/// `quoted_within` as the quote limit (6 hours by default, a setting the caller passes).
///
/// # Errors
///
/// [`EmailError::Database`].
pub async fn notice_timing(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
    quoted_within: SignedDuration,
) -> Result<NoticeTiming, EmailError> {
    let r = sqlx::query!(
        r#"WITH n AS (
             SELECT kind, extract(epoch FROM min(first_opened_at) - min(sent_at))::float8 AS open,
                    extract(epoch FROM min(acted_at) - min(sent_at))::float8 AS acted
               FROM messaging.notice_log WHERE kind IN ('card', 'pick') AND sent_at IS NOT NULL
              GROUP BY business_id, job_id, kind
             HAVING min(sent_at) >= $1 AND min(sent_at) < $2)
           SELECT round(percentile_cont(0.5) WITHIN GROUP (ORDER BY open))::int8 AS "open?",
                  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY acted)
                    FILTER (WHERE kind = 'card'))::int8 AS "quote?",
                  round(percentile_cont(0.5) WITHIN GROUP (ORDER BY acted)
                    FILTER (WHERE kind = 'pick'))::int8 AS "confirm?",
                  1000 * count(*) FILTER (WHERE kind = 'card' AND acted <= $3)
                    / NULLIF(count(*) FILTER (WHERE kind = 'card'), 0) AS "within?"
             FROM n"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
        quoted_within.as_secs_f64(),
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)?;
    Ok(NoticeTiming {
        to_first_open: r.open.map(SignedDuration::from_secs),
        to_quote: r.quote.map(SignedDuration::from_secs),
        to_confirm: r.confirm.map(SignedDuration::from_secs),
        quoted_within_per_mille: r.within,
    })
}
