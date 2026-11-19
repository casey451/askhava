//! The owner health page and its monthly spot-check (OPS-008, OPS-009; T476, D-3753, D-3844,
//! D-3882, D-4280).
//!
//! [`owner_health_page`] renders `templates/health.html` from T475's readers only
//! (`messaging::api`, `quotes::api`, `payments::api`, `ai::api`) and computes no figure itself; it
//! only words them. Windows, all ending at `now`: the last 7 days for text and email rates, the
//! OPS-037 medians and 6-hour share, payment errors and AI errors; the Arizona day and month for
//! sending-limit use; the Arizona month for the share of cards that ended without a quote; the last
//! `spot_check_every_days` days (`ASKHAVA_SPOT_CHECK_EVERY_DAYS`, default 30) for the OPS-009
//! spot-check of [`SPOT_CHECK_CARDS`] random cards, each shown with only the owner card view's
//! fields (card, job, trade, send time). No business-call rate (D-331, D-344).
//!
//! `portals` cannot read `Settings` or the process, so the serving route (T304, D-2128) passes in
//! [`HealthInputs`]: the email limits, the quote limit, the spot-check cadence, the uptime (web
//! time since start and the worker state `/healthz` reports), one [`SecretLine`] per required
//! secret (name and three flags, converted from T304's `SecretReport`; never a value, length or
//! hash, D-492) and the four SEC-036 [`AccountIds`] (D-3882). No uptime history is stored, so the
//! page links UptimeRobot's own 30-day figure. The last backup reads "not yet recorded" until T109
//! replaces it with `ops::api::latest_audit` (D-493). A GET: reads only, writes nothing.

use askama::Template;
use askhava_ai::api::ai_figures;
use askhava_core::api::arizona_time;
use askhava_messaging::api::{
    EmailFigures, PerAudience, TextFigures, email_figures, emails_sent, notice_timing, text_figures,
};
use askhava_payments::api::payment_errors;
use askhava_quotes::api::{SpotCard, card_figures, spot_check_cards};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;

use crate::owner_portal::OwnerToolError;

/// The owner health page's path (OPS-008); `server` serves it (T304).
pub const OWNER_HEALTH_ROUTE: &str = "/owner/health";
/// OPS-009: how many random job cards the spot-check shows.
pub const SPOT_CHECK_CARDS: u32 = 10;
/// OPS-009, D-4280: `ASKHAVA_SPOT_CHECK_EVERY_DAYS`'s default.
pub const DEFAULT_SPOT_CHECK_EVERY_DAYS: u32 = 30;
/// OPS-037: the quote limit of the "quoted within" share when no setting overrides it.
pub const DEFAULT_QUOTED_WITHIN: SignedDuration = SignedDuration::from_hours(6);
/// OPS-008: UptimeRobot's own 30-day uptime figure, linked, never copied (no history is stored).
pub const UPTIME_HISTORY_URL: &str = "https://dashboard.uptimerobot.com/monitors";

/// One required secret as the page shows it: its name and three flags, never a value, a length
/// or a hash (SEC-002, SEC-036, OPS-008, D-492, D-3844).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretLine {
    /// The setting's name, such as `ASKHAVA_STRIPE_SECRET_KEY`.
    pub name: String,
    /// The variable is set.
    pub present: bool,
    /// Its value parsed.
    pub parsed: bool,
    /// It belongs to this environment (a test key in `test`, a live key in `live`).
    pub env_matched: bool,
}

/// The four SEC-036 account identifiers (not secrets, D-3882); `None` when unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountIds {
    /// The Twilio Account SID.
    pub twilio_account_sid: Option<String>,
    /// The Stripe account ID.
    pub stripe_account_id: Option<String>,
    /// The Resend sending domain.
    pub resend_domain: Option<String>,
    /// The storage bucket name.
    pub storage_bucket: Option<String>,
}

/// What the serving route passes in, since `portals` reads neither `Settings` nor the process
/// (OPS-008, OPS-009, D-3753, D-4280).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthInputs {
    /// `ASKHAVA_EMAIL_DAILY_LIMIT` (OPS-004).
    pub email_daily_limit: u64,
    /// `ASKHAVA_EMAIL_MONTHLY_LIMIT` (OPS-004).
    pub email_monthly_limit: u64,
    /// OPS-037's quote limit; [`DEFAULT_QUOTED_WITHIN`] by default.
    pub quoted_within: SignedDuration,
    /// `ASKHAVA_SPOT_CHECK_EVERY_DAYS` (OPS-009); [`DEFAULT_SPOT_CHECK_EVERY_DAYS`] by default.
    pub spot_check_every_days: u32,
    /// How long the web process has run.
    pub web_up_for: SignedDuration,
    /// The worker state `/healthz` reports: `true` when up (T106).
    pub worker_up: bool,
    /// One line per required secret.
    pub secrets: Vec<SecretLine>,
    /// The SEC-036 account identifiers.
    pub accounts: AccountIds,
}

/// One worded figure.
struct Figure {
    label: String,
    value: String,
}

/// One headed group of figures.
struct Section {
    title: &'static str,
    figures: Vec<Figure>,
}

/// One spot-check row, as the owner's card view shows it.
struct SpotRow {
    card: String,
    job: String,
    trade: String,
    sent: String,
}

#[derive(Template)]
#[template(path = "health.html")]
struct HealthView<'a> {
    sections: Vec<Section>,
    uptime_url: &'a str,
    spot_days: u32,
    spot: Vec<SpotRow>,
    secrets: &'a [SecretLine],
    accounts: Vec<Figure>,
}

/// The windows the page reads, each ending at `now`.
struct Windows {
    week: Timestamp,
    today: Timestamp,
    month: Timestamp,
    spot: Timestamp,
}

fn windows(now: Timestamp, spot_days: u32) -> Result<Windows, jiff::Error> {
    let az = arizona_time(now);
    let days = |d: i64| now.checked_sub(SignedDuration::from_hours(24 * d));
    Ok(Windows {
        week: days(7)?,
        today: az.start_of_day()?.timestamp(),
        month: az.first_of_month()?.start_of_day()?.timestamp(),
        spot: days(i64::from(spot_days))?,
    })
}

fn db<E>(_: E) -> OwnerToolError {
    OwnerToolError::Database
}

fn figure(label: impl Into<String>, value: impl Into<String>) -> Figure {
    Figure {
        label: label.into(),
        value: value.into(),
    }
}

/// A per-mille share as a percent with one decimal, or `none` with nothing to share.
fn percent(per_mille: Option<i64>, none: &str) -> String {
    per_mille.map_or_else(
        || none.to_owned(),
        |p| format!("{}.{}%", p / 10, (p % 10).abs()),
    )
}

/// A median as whole minutes, or a "none" line.
fn minutes(median: Option<SignedDuration>) -> String {
    median.map_or_else(
        || "No data yet".to_owned(),
        |d| format!("{} minutes", (d.as_secs() + 30) / 60),
    )
}

/// The web process's time since start as days, hours and minutes.
fn running_for(up: SignedDuration) -> String {
    let mins = up.as_secs() / 60;
    format!(
        "{} days, {} hours, {} minutes",
        mins / 1440,
        mins / 60 % 24,
        mins % 60
    )
}

fn text_lines(texts: &PerAudience<TextFigures>) -> Vec<Figure> {
    let mut out = Vec::new();
    for (who, t) in [
        ("homeowners", texts.homeowner),
        ("businesses", texts.business),
    ] {
        let rate = percent(t.delivered_per_mille, "none sent");
        out.push(figure(
            format!("Texts to {who} delivered"),
            format!("{} of {} ({rate})", t.delivered, t.sent),
        ));
        out.push(figure(
            format!("Failed texts to {who}"),
            t.failed.to_string(),
        ));
    }
    out
}

fn email_lines(emails: &PerAudience<EmailFigures>) -> Vec<Figure> {
    let mut out = Vec::new();
    for (who, e) in [
        ("homeowners", emails.homeowner),
        ("businesses", emails.business),
    ] {
        let share = |pick: fn((i64, i64, i64)) -> i64| percent(e.per_mille.map(pick), "none sent");
        out.push(figure(
            format!("Emails to {who}"),
            format!(
                "{} sent; {} delivered ({}), {} bounced ({}), {} complaints ({})",
                e.sent,
                e.delivered,
                share(|p| p.0),
                e.bounced,
                share(|p| p.1),
                e.complained,
                share(|p| p.2),
            ),
        ));
    }
    out
}

/// OPS-008, OPS-004: text and email rates per audience and sending-limit use.
async fn messages(
    conn: &mut PgConnection,
    inputs: &HealthInputs,
    w: &Windows,
    now: Timestamp,
) -> Result<Section, OwnerToolError> {
    let texts = text_figures(conn, w.week, now).await.map_err(db)?;
    let emails = email_figures(conn, w.week, now).await.map_err(db)?;
    let today = emails_sent(conn, w.today, now).await.map_err(db)?;
    let month = emails_sent(conn, w.month, now).await.map_err(db)?;
    let mut figures = text_lines(&texts);
    figures.extend(email_lines(&emails));
    figures.push(figure(
        "Emails sent today",
        format!("{today} of the daily limit of {}", inputs.email_daily_limit),
    ));
    figures.push(figure(
        "Emails sent this month",
        format!(
            "{month} of the monthly limit of {}",
            inputs.email_monthly_limit
        ),
    ));
    Ok(Section {
        title: "Texts and emails, last 7 days",
        figures,
    })
}

/// OPS-037, BIZ-044: the weekly medians, the quoted-within share and this month's cards that
/// ended without a quote.
async fn timing(
    conn: &mut PgConnection,
    inputs: &HealthInputs,
    w: &Windows,
    now: Timestamp,
) -> Result<Section, OwnerToolError> {
    let t = notice_timing(conn, w.week, now, inputs.quoted_within)
        .await
        .map_err(db)?;
    let cards = card_figures(conn, w.month, now).await.map_err(db)?;
    let hours = inputs.quoted_within.as_secs() / 3600;
    let ended = percent(cards.without_quote_per_mille, "no cards yet");
    Ok(Section {
        title: "Quote timing, last 7 days",
        figures: vec![
            figure(
                "Median time from email sent to first opened by the business",
                minutes(t.to_first_open),
            ),
            figure("Median time from card to quote", minutes(t.to_quote)),
            figure(
                "Median time from pick to the business confirming",
                minutes(t.to_confirm),
            ),
            figure(
                format!("Job cards quoted within {hours} hours"),
                percent(t.quoted_within_per_mille, "no cards yet"),
            ),
            figure(
                "Job cards this month that ended without a quote",
                format!("{} of {} ({ended})", cards.ended_without_quote, cards.cards),
            ),
        ],
    })
}

/// OPS-008: payment errors, AI errors, uptime and the last backup.
async fn errors_and_uptime(
    conn: &mut PgConnection,
    inputs: &HealthInputs,
    w: &Windows,
    now: Timestamp,
) -> Result<Section, OwnerToolError> {
    let pay = payment_errors(conn, w.week, now).await.map_err(db)?;
    let ai = ai_figures(conn, w.week, now).await.map_err(db)?;
    let worker = if inputs.worker_up { "Up" } else { "Down" };
    Ok(Section {
        title: "Errors and uptime",
        figures: vec![
            figure(
                "Failed charges, last 7 days",
                pay.failed_charges.to_string(),
            ),
            figure("Charges still pending", pay.undecided_charges.to_string()),
            figure(
                "Failed refunds, last 7 days",
                pay.failed_refunds.to_string(),
            ),
            figure(
                "AI errors, last 7 days",
                format!("{} of {} calls", ai.errors, ai.calls),
            ),
            figure("Web app running for", running_for(inputs.web_up_for)),
            figure("Background worker", worker),
            figure("Last backup", "Not yet recorded"),
        ],
    })
}

fn spot_row(c: SpotCard) -> SpotRow {
    SpotRow {
        card: c.card.to_string(),
        job: c.job.to_string(),
        trade: c.trade.unwrap_or_else(|| "Not stored".to_owned()),
        sent: arizona_time(c.sent_at)
            .strftime("%b %-d, %Y %-I:%M %p")
            .to_string(),
    }
}

fn account_lines(a: &AccountIds) -> Vec<Figure> {
    let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "Not set".to_owned());
    vec![
        figure("Twilio Account SID", show(&a.twilio_account_sid)),
        figure("Stripe account ID", show(&a.stripe_account_id)),
        figure("Resend sending domain", show(&a.resend_domain)),
        figure("Storage bucket", show(&a.storage_bucket)),
    ]
}

/// OPS-008, OPS-009: the owner health page at `now` from `inputs` and T475's readers. Reads only.
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read, window or render.
pub async fn owner_health_page(
    conn: &mut PgConnection,
    inputs: &HealthInputs,
    now: Timestamp,
) -> Result<String, OwnerToolError> {
    let w = windows(now, inputs.spot_check_every_days).map_err(db)?;
    let sections = vec![
        messages(conn, inputs, &w, now).await?,
        timing(conn, inputs, &w, now).await?,
        errors_and_uptime(conn, inputs, &w, now).await?,
    ];
    let spot = spot_check_cards(conn, w.spot, now, SPOT_CHECK_CARDS)
        .await
        .map_err(db)?;
    HealthView {
        sections,
        uptime_url: UPTIME_HISTORY_URL,
        spot_days: inputs.spot_check_every_days,
        spot: spot.into_iter().map(spot_row).collect(),
        secrets: &inputs.secrets,
        accounts: account_lines(&inputs.accounts),
    }
    .render()
    .map_err(db)
}

#[cfg(test)]
mod tests {
    //! The page's wording helpers (OPS-008).

    use super::*;

    #[test]
    fn shares_minutes_and_uptime_are_worded() {
        assert_eq!(percent(Some(975), "x"), "97.5%");
        assert_eq!(percent(None, "none sent"), "none sent");
        assert_eq!(minutes(Some(SignedDuration::from_mins(20))), "20 minutes");
        assert_eq!(minutes(None), "No data yet");
        let up = SignedDuration::from_mins(2 * 1440 + 3 * 60 + 4);
        assert_eq!(running_for(up), "2 days, 3 hours, 4 minutes");
    }
}
