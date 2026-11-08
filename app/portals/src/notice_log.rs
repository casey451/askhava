//! The notice log on each portal job page (BIZ-045, OPS-037; T198, D-332, D-344, D-347, D-1995).
//!
//! [`notice_log_page`] shows the signed-in business, for one job it holds a card for, every card,
//! open-cards line, pick, reminder, card-fix, release and account notice sent to it, oldest
//! first: the send time or "held by quiet hours until <time>", the address and its latest provider
//! status, the "may still be in Spam" line, "Link first opened <time> (may be a mail scanner)"
//! and the quote or Confirm time. The rows come from `messaging::api::notice_log`, filtered by
//! the business, so another business's job is refused (`NotFound`). No call outcome is shown (no
//! call is placed, D-344); no homeowner detail is read or shown; no tracking pixel or redirect.
//! The addresses are the business's own, read from the vault by the composition and handed in.

use askama::Template;
use askhava_core::api::arizona_time;
use askhava_messaging::api::{NoticeEntry, NoticeKind, ProviderStatus, Sending, notice_log};
use askhava_quotes::api::card_holders;
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::session::PortalsError;

/// The page's answer (BIZ-045).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeLogPage {
    /// The rendered page.
    Shown(String),
    /// The business holds no card for the job: nothing is read or shown.
    NotFound,
}

struct Row {
    what: &'static str,
    when: String,
    to: String,
    status: &'static str,
    opened: Option<String>,
    acted: Option<String>,
}

#[derive(Template)]
#[template(path = "notice_log.html")]
struct NoticeLogView {
    job: u32,
    rows: Vec<Row>,
}

/// "Nov 9, 5:00 p.m.", Arizona time.
fn when(at: Timestamp) -> String {
    let t = arizona_time(at);
    let half = if t.hour() < 12 { "a.m." } else { "p.m." };
    format!("{} {half}", t.strftime("%b %-d, %-I:%M"))
}

fn what(kind: NoticeKind) -> &'static str {
    match kind {
        NoticeKind::Card => "Job card",
        NoticeKind::OpenCards => "Open job requests email",
        NoticeKind::Pick => "Pick notice",
        NoticeKind::Reminder => "Confirm reminder",
        NoticeKind::CardFix => "Payment card update",
        NoticeKind::Release => "Homeowner details ready",
        NoticeKind::Account => "Account notice",
    }
}

fn acted(e: &NoticeEntry) -> Option<String> {
    let label = match e.kind {
        NoticeKind::Card | NoticeKind::OpenCards => "You quoted",
        NoticeKind::Pick | NoticeKind::Reminder => "You confirmed",
        _ => return None,
    };
    e.acted.map(|at| format!("{label} {}", when(at)))
}

fn row(e: &NoticeEntry, addresses: &[(Uuid, String)]) -> Row {
    let to = addresses.iter().find(|(id, _)| *id == e.mailbox);
    Row {
        what: what(e.kind),
        when: match e.sending {
            Sending::Sent(at) => format!("Sent {}", when(at)),
            Sending::Held(until) => format!("Held by quiet hours until {}", when(until)),
        },
        to: to.map_or_else(|| "an address you removed".to_owned(), |(_, a)| a.clone()),
        status: e
            .status
            .map_or("No word from your mail server yet", ProviderStatus::line),
        opened: e
            .first_opened
            .map(|at| format!("Link first opened {} (may be a mail scanner)", when(at))),
        acted: acted(e),
    }
}

/// BIZ-045, OPS-037, SEC-024: `business`'s notice log for `job` (number `number`), with
/// `addresses` its own notice addresses by mailbox ID; `NotFound` unless it holds a card for it.
///
/// # Errors
/// [`PortalsError::Database`], also when the page does not render.
pub async fn notice_log_page(
    db: &mut PgConnection,
    business: Uuid,
    (job, number): (Uuid, u32),
    addresses: &[(Uuid, String)],
) -> Result<NoticeLogPage, PortalsError> {
    let holders = card_holders(db, job)
        .await
        .map_err(|_| PortalsError::Database)?;
    if !holders.contains(&business) {
        return Ok(NoticeLogPage::NotFound);
    }
    let entries = notice_log(db, business, job)
        .await
        .map_err(|_| PortalsError::Database)?;
    let rows = entries.iter().map(|e| row(e, addresses)).collect();
    let page = NoticeLogView { job: number, rows }.render();
    page.map(NoticeLogPage::Shown)
        .map_err(|_| PortalsError::Database)
}
