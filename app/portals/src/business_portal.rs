//! The business portal home and its step-up gate (T085; BIZ-025, CARD-022, SEC-038; D-480,
//! D-491, D-492).
//!
//! [`business_home_page`] shows the "Your numbers" panel from the [`YourNumbers`] the composition
//! reads (BIZ-025, D-193, D-250, D-284, D-331) and the signed-in person's own open-cards email
//! times with their form (CARD-022); [`save_open_cards_times`] stores that person's choice through
//! `businesses::api`, for themselves only. [`require_step_up`] is the gate every SEC-038 change
//! calls first: a live 30-day session is not enough, the person must have passed a Verify code in
//! the last 5 minutes (`businesses::api::has_fresh_step_up`); a refusal is one `authz_denied`
//! security event (SEC-048). The routes ([`BUSINESS_HOME_ROUTE`], [`OPEN_CARDS_ROUTE`]) are served
//! with the business portal (D-1728).
//!
//! T195 (BIZ-015, BIZ-029, SEC-038; D-491, D-492, D-1912 to D-1914): [`billing_page`] and
//! [`statement_csv`] show a `payments::api::Statement` (the ledger only; CSV, no PDF);
//! [`change_fee_limit`] and [`change_saved_card`] pass the role check and [`require_step_up`]
//! first; [`complaints_page`] and [`reply_to_complaint`] read and write through
//! `businesses::api` (kind and date, no homeowner detail, the reply box open for 5 days).

use askama::Template;
use askhava_businesses::api::{
    Acting, Complaint, ComplaintError, OpenCardsTime, OpenCardsTimes, has_fresh_step_up, reply,
    set_open_cards_times,
};
use askhava_payments::api::{
    BusinessId, DISPUTE_FEE_CENTS, EntryKind, SavedCard, Statement, save_card, set_fee_limit,
};
use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::business_auth::{BusinessPrincipal, Need};
use crate::confirm_pay::money;
use crate::security::{SecurityEvent, SecurityEventKind, record_security_event};

/// The portal home's path.
pub const BUSINESS_HOME_ROUTE: &str = "/b";
/// Where the open-cards times form posts.
pub const OPEN_CARDS_ROUTE: &str = "/b/open-cards";

/// A change SEC-038 guards with a fresh Verify code (D-480).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveChange {
    /// A notice address.
    NoticeAddress,
    /// The backup address.
    BackupAddress,
    /// Linking a phone.
    LinkPhone,
    /// Approving a phone a member linked.
    ApprovePhone,
    /// A team member's role.
    TeamRole,
    /// The saved card.
    SavedCard,
    /// The monthly limit.
    MonthlyLimit,
    /// Opting in to auto-confirm.
    AutoConfirm,
}

impl SensitiveChange {
    /// Every guarded change.
    pub const ALL: [Self; 8] = [
        Self::NoticeAddress,
        Self::BackupAddress,
        Self::LinkPhone,
        Self::ApprovePhone,
        Self::TeamRole,
        Self::SavedCard,
        Self::MonthlyLimit,
        Self::AutoConfirm,
    ];
}

/// Why a guarded change did not go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StepUpError {
    /// No Verify code in the last 5 minutes; the person is asked for a new one (SEC-038).
    #[error("this change needs a fresh code")]
    NeedsCode,
    /// A database call failed.
    #[error("step-up database call failed")]
    Database,
}

/// SEC-038, SEC-048, D-480: `Ok` only when `principal` passed a Verify code no more than 5
/// minutes before `now`; else one `authz_denied` event for `route` with the member as actor, and
/// one log line naming `change` (no value).
///
/// The refusal's event is written on `db`, so call this on a connection whose work is kept, never
/// inside a transaction the caller rolls back when this refuses: the rollback would drop the
/// `authz_denied` event SEC-048 requires. Check first, then open the change's transaction.
///
/// # Errors
/// [`StepUpError`].
pub async fn require_step_up(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (change, route): (SensitiveChange, &str),
    now: Timestamp,
) -> Result<(), StepUpError> {
    let by = Acting {
        business: principal.business,
        member: principal.member,
    };
    let fresh = has_fresh_step_up(db, by, now).await;
    if fresh.map_err(|_| StepUpError::Database)? {
        return Ok(());
    }
    tracing::warn!(change = ?change, "sensitive change needs a fresh code");
    let event = SecurityEvent {
        kind: SecurityEventKind::AuthzDenied,
        route,
        request_id: None,
        actor_ref: Some(principal.member),
    };
    record_security_event(db, &event, now)
        .await
        .map_err(|_| StepUpError::Database)?;
    Err(StepUpError::NeedsCode)
}

/// This month's "Your numbers", counted by the composition from the ledger and the jobs (BIZ-025).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YourNumbers {
    /// Job cards sent.
    pub cards: u32,
    /// Cards that ended without a quote (D-331).
    pub cards_without_quote: u32,
    /// Quotes sent.
    pub quotes: u32,
    /// Picks.
    pub picks: u32,
    /// Picks confirmed.
    pub confirms: u32,
    /// Introduction fees charged, in cents.
    pub fees_cents: u64,
    /// Picks that ran out of time (D-284).
    pub lapsed_picks: u32,
    /// Picked jobs by the homeowner's DONE, NO and OTHER replies (D-250).
    pub finished: (u32, u32, u32),
    /// The typical (median) quotes per job in each trade, in tenths: (trade, tenths).
    pub typical_quotes: Vec<(String, u32)>,
}

/// One labelled number of the panel.
struct Line {
    label: &'static str,
    value: String,
}

/// BIZ-025: the panel's lines in order; fee per win is fees over confirms, none before one.
fn number_lines(n: &YourNumbers) -> Vec<Line> {
    let per_win = (n.confirms > 0).then(|| money(n.fees_cents / u64::from(n.confirms)));
    let (done, no, other) = n.finished;
    let rows: [(&'static str, String); 11] = [
        ("Job requests sent to you", n.cards.to_string()),
        (
            "Requests that closed before you quoted",
            n.cards_without_quote.to_string(),
        ),
        ("Quotes you sent", n.quotes.to_string()),
        ("Times you were picked", n.picks.to_string()),
        ("Picks you confirmed", n.confirms.to_string()),
        ("Introduction fees charged", money(n.fees_cents)),
        (
            "Average introduction fee per confirmed pick",
            per_win.unwrap_or_else(|| "None yet".to_owned()),
        ),
        ("Picks not confirmed in time", n.lapsed_picks.to_string()),
        ("Homeowner said it's done", done.to_string()),
        ("Homeowner said it isn't done", no.to_string()),
        ("Homeowner gave another answer", other.to_string()),
    ];
    rows.into_iter()
        .map(|(label, value)| Line { label, value })
        .collect()
}

/// One open-cards time box.
struct TimeBox {
    code: &'static str,
    label: &'static str,
    checked: bool,
}

#[derive(Template)]
#[template(path = "business_home.html")]
struct HomeView<'a> {
    lines: Vec<Line>,
    typical: Vec<(&'a str, String)>,
    boxes: Vec<TimeBox>,
    too_many: bool,
    csrf: &'a str,
}

/// BIZ-025, CARD-022: the portal home with `numbers` and the person's `times`; `too_many` shows
/// the form's error after more than two times were sent (A11Y-030).
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn business_home_page(
    numbers: &YourNumbers,
    (times, too_many): (OpenCardsTimes, bool),
    csrf: &str,
) -> Result<String, askama::Error> {
    let chosen = times.times();
    let label = |t: OpenCardsTime| match t {
        OpenCardsTime::EightAm => "8 a.m.",
        OpenCardsTime::Noon => "Noon",
        OpenCardsTime::FivePm => "5 p.m.",
    };
    let view = HomeView {
        lines: number_lines(numbers),
        typical: (numbers.typical_quotes.iter())
            .map(|(trade, t)| (trade.as_str(), format!("{}.{}", t / 10, t % 10)))
            .collect(),
        boxes: (OpenCardsTime::ALL.into_iter())
            .map(|t| TimeBox {
                code: t.code(),
                label: label(t),
                checked: chosen.contains(&t),
            })
            .collect(),
        too_many,
        csrf,
    };
    view.render()
}

/// CARD-022, D-492: stores the form's `codes` as `principal`'s own open-cards times. `Ok(None)`
/// for more than two times or an unknown value (nothing stored); the page shows the error.
///
/// # Errors
/// `askhava_businesses::api::BusinessesError` when the person is no longer on the team or the
/// database fails.
pub async fn save_open_cards_times(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    codes: &[String],
    now: Timestamp,
) -> Result<Option<OpenCardsTimes>, askhava_businesses::api::BusinessesError> {
    let parsed: Option<Vec<_>> = codes.iter().map(|c| OpenCardsTime::parse(c)).collect();
    let Some(times) = parsed.and_then(|p| OpenCardsTimes::new(&p)) else {
        return Ok(None);
    };
    let by = Acting {
        business: principal.business,
        member: principal.member,
    };
    set_open_cards_times(db, by, times, now).await?;
    Ok(Some(times))
}

/// The billing page's path (BIZ-015; not served yet, D-1728).
pub const BILLING_ROUTE: &str = "/b/billing";
/// The CSV statement's path; V1 offers no PDF statement (BIZ-015, D-491).
pub const STATEMENT_ROUTE: &str = "/b/billing/statement.csv";
/// The complaints page's path (BIZ-029; not served yet, D-1728).
pub const COMPLAINTS_ROUTE: &str = "/b/complaints";

/// A saved payment method as the billing page shows it: brand and last four only (FEE-014).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentMethod {
    /// The card brand, such as `visa`.
    pub brand: String,
    /// The card's last four digits.
    pub last4: String,
}

/// BIZ-015, D-1912: a ledger entry in the business's words; `None` for Ask Hava's own Stripe
/// costs (processing fees, countering fees, payouts), which are not the business's money.
fn entry_label(kind: EntryKind) -> Option<&'static str> {
    match kind {
        EntryKind::Charge => Some("Introduction fee by job size"),
        EntryKind::Refund => Some("Refund"),
        EntryKind::Credit => Some("Credit"),
        EntryKind::FreeFirstWin => Some("Free first win"),
        EntryKind::RepeatCustomer => Some("Repeat customer: no fee"),
        EntryKind::FeeFreeJobType => Some("No fee for this job type"),
        EntryKind::Dispute => Some("You disputed this charge with your bank"),
        EntryKind::CounteringFee
        | EntryKind::CounteringFeeReturn
        | EntryKind::ProcessingFee
        | EntryKind::Payout => None,
    }
}

/// One statement row the business sees: Arizona date, words, job and cents as the ledger holds
/// them (a dispute without Stripe's dispute fee, as `MoneyView` splits it).
struct Row {
    date: String,
    what: &'static str,
    job: String,
    cents: i64,
}

/// BIZ-015: the statement's business rows, in ledger order.
fn statement_rows(statement: &Statement) -> Vec<Row> {
    let date = |t: Timestamp| {
        t.in_tz("America/Phoenix")
            .map_or_else(|_| String::new(), |z| z.strftime("%Y-%m-%d").to_string())
    };
    (statement.lines.iter())
        .filter_map(|l| {
            let fee = if l.kind == EntryKind::Dispute {
                l.amount_cents.min(DISPUTE_FEE_CENTS)
            } else {
                0
            };
            Some(Row {
                date: date(l.recorded_at),
                what: entry_label(l.kind)?,
                job: l.job.map(|j| j.to_string()).unwrap_or_default(),
                cents: l.amount_cents - fee,
            })
        })
        .collect()
}

/// Signed cents as dollars for the page, "$" and thousands groups.
fn signed_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{}", money(cents.unsigned_abs()))
}

/// BIZ-015, D-491: the statement as CSV (date, entry, job, amount in dollars), one line per
/// business row of the ledger; the only statement format in V1.
#[must_use]
pub fn statement_csv(statement: &Statement) -> String {
    let mut csv = String::from("date,entry,job,amount\n");
    for r in statement_rows(statement) {
        let abs = r.cents.unsigned_abs();
        let sign = if r.cents < 0 { "-" } else { "" };
        let amount = format!("{sign}{}.{:02}", abs / 100, abs % 100);
        csv.push_str(&format!("{},{},{},{amount}\n", r.date, r.what, r.job));
    }
    csv
}

#[derive(Template)]
#[template(path = "business_billing.html")]
struct BillingView<'a> {
    totals: [(&'static str, String); 4],
    rows: Vec<(String, &'static str, String)>,
    methods: Vec<(String, &'a str)>,
    limit: Option<String>,
}

/// BIZ-015, D-491: the billing page from `statement` (the ledger), the saved `methods` and the
/// monthly fee `limit` in cents; the statement link is CSV only.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn billing_page(
    statement: &Statement,
    methods: &[PaymentMethod],
    limit: Option<u32>,
) -> Result<String, askama::Error> {
    let free = statement_rows(statement)
        .iter()
        .filter(|r| r.what == "Free first win")
        .count();
    let t = &statement.totals;
    let view = BillingView {
        totals: [
            ("Introduction fees charged", signed_money(t.charges)),
            ("Free first win", free.to_string()),
            ("Refunds", signed_money(t.refunds)),
            ("Credits", signed_money(t.credits)),
        ],
        rows: (statement_rows(statement).into_iter())
            .map(|r| (r.date, r.what, signed_money(r.cents)))
            .collect(),
        methods: (methods.iter())
            .map(|m| (kind_words(&m.brand), m.last4.as_str()))
            .collect(),
        limit: limit.map(|c| money(u64::from(c))),
    };
    view.render()
}

/// Why a billing change did not go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BillingChangeError {
    /// The person is neither the owner nor a "Confirm and billing" member.
    #[error("this person may not change billing")]
    Role,
    /// The step-up gate refused or failed (SEC-038).
    #[error(transparent)]
    StepUp(#[from] StepUpError),
    /// `payments` refused the change or failed.
    #[error("the payments change failed")]
    Payments,
}

/// Checks the role, then the SEC-038 step-up for `change` on [`BILLING_ROUTE`].
async fn billing_gate(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    change: SensitiveChange,
    now: Timestamp,
) -> Result<(), BillingChangeError> {
    if !principal.may(principal.business, Need::ConfirmAndBilling) {
        return Err(BillingChangeError::Role);
    }
    require_step_up(db, principal, (change, BILLING_ROUTE), now).await?;
    Ok(())
}

/// SEC-038, FEE-041, D-480: sets (or with `None` removes) the monthly fee limit, only for the owner
/// or a "Confirm and billing" member with a Verify code from the last 5 minutes.
///
/// # Errors
/// [`BillingChangeError`].
pub async fn change_fee_limit(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    limit_cents: Option<u32>,
    now: Timestamp,
) -> Result<(), BillingChangeError> {
    billing_gate(db, principal, SensitiveChange::MonthlyLimit, now).await?;
    let business = BusinessId(principal.business);
    set_fee_limit(db, business, limit_cents)
        .await
        .map_err(|_| BillingChangeError::Payments)
}

/// SEC-038, FEE-014, FEE-015: saves a new card with the authorization accepted `now`, only for the
/// owner or a "Confirm and billing" member with a Verify code from the last 5 minutes.
///
/// # Errors
/// [`BillingChangeError`].
pub async fn change_saved_card(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    card: &SavedCard,
    now: Timestamp,
) -> Result<(), BillingChangeError> {
    billing_gate(db, principal, SensitiveChange::SavedCard, now).await?;
    save_card(db, BusinessId(principal.business), card, now)
        .await
        .map(|_| ())
        .map_err(|_| BillingChangeError::Payments)
}

/// One complaint as the page shows it: no homeowner detail and no job (BIZ-029).
struct ComplaintRow {
    id: String,
    kind: String,
    date: String,
    until: String,
    reply: Option<String>,
    open: bool,
}

#[derive(Template)]
#[template(path = "business_complaints.html")]
struct ComplaintsView<'a> {
    rows: Vec<ComplaintRow>,
    refused: Option<String>,
    csrf: &'a str,
}

/// BIZ-029, D-200: the complaints page: each complaint's kind and date, the reply if written, and
/// the reply box while it is open (before exactly 5 days); `refused` names a reply that was not
/// saved, shown as the A11Y-030 error summary.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn complaints_page(
    list: &[Complaint],
    (now, refused): (Timestamp, Option<Uuid>),
    csrf: &str,
) -> Result<String, askama::Error> {
    let at = |t: Timestamp| {
        t.in_tz("America/Phoenix").map_or_else(
            |_| String::new(),
            |z| z.strftime("%b %-d, %Y, %-I:%M %p").to_string(),
        )
    };
    let rows = (list.iter())
        .map(|c| ComplaintRow {
            id: c.id.to_string(),
            kind: kind_words(&c.kind),
            date: at(c.received_at),
            until: at(c.reply_until),
            reply: c.reply.clone(),
            open: c.reply.is_none() && c.received_at <= now && now < c.reply_until,
        })
        .collect();
    let refused = refused.map(|id| id.to_string());
    ComplaintsView {
        rows,
        refused,
        csrf,
    }
    .render()
}

/// A code (`no_show`, `visa`) in words, first letter capital ("No show", "Visa").
fn kind_words(code: &str) -> String {
    let words = code.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// BIZ-029, D-200: stores the signed-in person's business's one reply to `complaint` through
/// `businesses::api::reply` (refused at exactly 5 days, empty or over 1,000 characters).
///
/// # Errors
/// `askhava_businesses::api::ComplaintError`.
pub async fn reply_to_complaint(
    db: &mut PgConnection,
    principal: &BusinessPrincipal,
    (complaint, words): (Uuid, &str),
    now: Timestamp,
) -> Result<(), ComplaintError> {
    reply(db, (principal.business, complaint), words, now).await
}

#[cfg(test)]
mod tests {
    use askhava_businesses::api::{
        BusinessesError, NewComplaint, add_owner, complaints, create, open_cards_times,
        record_complaint, record_step_up,
    };
    use askhava_payments::api::MoneyView;

    use super::*;

    type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

    /// TD-PICK-17 and TD-BIZ-11 page cases (BIZ-025): every panel number shows as given, fee per
    /// win is fees over confirms, and the typical quotes per trade show.
    #[test]
    fn td_pick_17_your_numbers_panel_shows_each_number() {
        let n = YourNumbers {
            cards: 12,
            cards_without_quote: 7,
            quotes: 5,
            picks: 3,
            confirms: 2,
            fees_cents: 10_000,
            lapsed_picks: 1,
            finished: (4, 2, 9),
            typical_quotes: vec![("Plumbing".to_owned(), 25)],
        };
        let page = business_home_page(&n, (OpenCardsTimes::DEFAULT, false), "t").expect("page");
        for want in [
            "<dd>12</dd>",
            "<dd>7</dd>",
            "$100.00",
            "<dd>$50.00</dd>",
            "<dd>9</dd>",
        ] {
            assert!(page.contains(want), "{want}");
        }
        assert!(page.contains("Plumbing: 2.5"));
        assert!(page.contains(r#"value="5pm" checked"#), "5 p.m. by default");
        assert!(!page.contains("role=\"alert\""));
        let none = business_home_page(
            &YourNumbers::default(),
            (OpenCardsTimes::DEFAULT, true),
            "t",
        );
        let none = none.expect("page");
        assert!(none.contains("None yet") && none.contains("<title>Error: "));
        assert!(none.contains(r#"aria-describedby="times-hint times-error""#));
        assert!(!page.contains("times-error"));
    }

    /// CARD-022, D-492: the person's own times are stored; more than two times or an unknown
    /// value stores nothing; a person not on the business's team is refused.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn card_022_save_open_cards_times_for_the_person_only(mut db: Db) {
        let at = Timestamp::from_second(1_790_000_000).expect("time");
        let business = create(&mut db, false, at).await.expect("business");
        let member = add_owner(&mut db, business, Uuid::from_u128(0xd1), at).await;
        let me = BusinessPrincipal {
            member: member.expect("owner"),
            business,
            owner: true,
            roles: (true, true),
        };
        let codes = |c: &[&str]| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        for bad in [codes(&["8am", "noon", "5pm"]), codes(&["9am"])] {
            let none = save_open_cards_times(&mut db, &me, &bad, at).await;
            assert_eq!(none.expect("ok"), None, "{bad:?}");
        }
        let unchanged = open_cards_times(&mut db, me.member).await.expect("read");
        assert_eq!(unchanged, OpenCardsTimes::DEFAULT, "nothing stored");
        let saved = save_open_cards_times(&mut db, &me, &codes(&["noon"]), at).await;
        let noon = OpenCardsTimes::new(&[OpenCardsTime::Noon]).expect("one");
        assert_eq!(saved.expect("ok"), Some(noon));
        assert_eq!(open_cards_times(&mut db, me.member).await.expect("r"), noon);
        let stranger = BusinessPrincipal {
            member: Uuid::from_u128(0xd2),
            ..me
        };
        let refused = save_open_cards_times(&mut db, &stranger, &codes(&["8am"]), at).await;
        assert_eq!(refused, Err(BusinessesError::UnknownBusiness));
    }

    fn t(secs: i64) -> Timestamp {
        Timestamp::from_second(1_790_000_000 + secs).expect("time")
    }

    /// TD-BIZ-04 page case (BIZ-015, D-491, D-1912): the totals, rows and CSV equal the ledger's
    /// business lines (a dispute without the dispute fee), Ask Hava's own costs are left out, the
    /// cards show brand and last four, and no PDF statement is offered.
    #[test]
    fn td_biz_04_billing_and_csv_equal_the_ledger() {
        let job = Uuid::from_u128(0x10b);
        let entries = [
            (EntryKind::FreeFirstWin, 0),
            (EntryKind::Charge, 7_500),
            (EntryKind::ProcessingFee, 248),
            (EntryKind::Refund, 7_500),
            (EntryKind::Credit, 2_500),
            (EntryKind::Charge, 19_900),
            (EntryKind::Dispute, 19_900 + DISPUTE_FEE_CENTS),
        ];
        let lines: Vec<_> = (entries.iter())
            .map(
                |&(kind, amount_cents)| askhava_payments::api::StatementLine {
                    kind,
                    job: Some(job),
                    amount_cents,
                    recorded_at: t(0),
                },
            )
            .collect();
        let totals = MoneyView::from_entries(entries);
        let statement = Statement { lines, totals };
        let visa = PaymentMethod {
            brand: "visa".to_owned(),
            last4: "4242".to_owned(),
        };
        let page = billing_page(&statement, &[visa], Some(50_000)).expect("page");
        for want in [
            "<dd>$274.00</dd>",
            "<dd>1</dd>",
            "<dd>$75.00</dd>",
            "<dd>$25.00</dd>",
        ] {
            assert!(page.contains(want), "{want}");
        }
        assert!(page.contains("Visa card ending in 4242") && page.contains("$500.00"));
        assert!(page.contains(STATEMENT_ROUTE) && !page.to_lowercase().contains("pdf"));
        assert!(!page.contains("$2.48") && page.contains("<td>$199.00</td>"));
        let csv = statement_csv(&statement);
        let rows: Vec<&str> = csv.lines().collect();
        assert_eq!(rows.len(), 7, "header and six business lines");
        assert_eq!(rows[0], "date,entry,job,amount");
        assert_eq!(
            rows[2],
            format!("2026-09-21,Introduction fee by job size,{job},75.00")
        );
        assert!(rows[6].ends_with(",199.00") && rows.iter().all(|r| r.split(',').count() == 4));
    }

    /// TD-BIZ-04 empty case (BIZ-015): an empty ledger shows no card, no limit and a header-only
    /// CSV; a negative amount keeps its sign.
    #[test]
    fn td_biz_04_empty_statement() {
        let empty = Statement {
            lines: Vec::new(),
            totals: MoneyView::default(),
        };
        let none = billing_page(&empty, &[], None).expect("page");
        assert!(none.contains("No card saved.") && none.contains("haven't set a monthly limit"));
        assert_eq!(statement_csv(&empty), "date,entry,job,amount\n");
        assert_eq!(signed_money(-150), "-$1.50");
    }

    /// TD-BIZ-09 page case (BIZ-029, D-200): kind and date only, no job; the box is open at
    /// 4 days 23:59 and closed at exactly 5 days; a written reply shows; a refusal is summarized.
    #[test]
    fn td_biz_09_complaint_shows_kind_and_date_and_a_five_day_box() {
        let job = Uuid::from_u128(0x10c);
        let c = Complaint {
            id: Uuid::from_u128(0xc1),
            kind: "no_show".to_owned(),
            received_at: t(0),
            reply_until: t(0) + askhava_businesses::api::REPLY_BOX,
            reply: None,
        };
        let open_at = t(5 * 86_400 - 60);
        let open = complaints_page(std::slice::from_ref(&c), (open_at, None), "t").expect("page");
        assert!(open.contains("No show, Sep 21, 2026") && open.contains("<textarea"));
        assert!(!open.contains(&job.to_string()) && !open.contains("role=\"alert\""));
        let shut = complaints_page(std::slice::from_ref(&c), (t(5 * 86_400), Some(c.id)), "t");
        let shut = shut.expect("page");
        assert!(!shut.contains("<textarea") && shut.contains("reply box closed"));
        assert!(shut.contains("<title>Error: ") && shut.contains("href=\"#reply-"));
        let replied = Complaint {
            reply: Some("We came on Tuesday.".to_owned()),
            ..c
        };
        let page = complaints_page(&[replied], (open_at, None), "t").expect("page");
        assert!(page.contains("Your reply: We came on Tuesday.") && !page.contains("<textarea"));
        let none = complaints_page(&[], (open_at, None), "t").expect("page");
        assert!(none.contains("No complaints."));
        assert_eq!(kind_words(""), "");
    }

    async fn owner(db: &mut Db) -> (Acting, BusinessPrincipal) {
        let business = create(db, false, t(0)).await.expect("business");
        let member = add_owner(db, business, Uuid::from_u128(0xd1), t(0)).await;
        let member = member.expect("owner");
        let me = BusinessPrincipal {
            member,
            business,
            owner: true,
            roles: (true, true),
        };
        (Acting { business, member }, me)
    }

    async fn limit_of(db: &mut Db, business: Uuid) -> Option<i64> {
        let q = "SELECT limit_cents FROM payments.fee_limit WHERE business_id = $1";
        let q = sqlx::query_scalar::<_, i64>(q).bind(business);
        q.fetch_optional(&mut **db).await.expect("read")
    }

    /// TD-SEC-17 billing cases (SEC-038, FEE-041, D-1913): the fee limit and the card change need
    /// the role and a Verify code from the last 5 minutes: none is refused, 4 minutes passes, 5:01
    /// is refused, and a refusal stores nothing.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_sec_17_billing_changes_need_a_fresh_code(mut db: Db) {
        let (who, me) = owner(&mut db).await;
        let needs = Err(BillingChangeError::StepUp(StepUpError::NeedsCode));
        assert_eq!(
            change_fee_limit(&mut db, &me, Some(9_000), t(10)).await,
            needs
        );
        let card = SavedCard::new("pm_fake1", "card", "visa", "4242").expect("card");
        assert_eq!(change_saved_card(&mut db, &me, &card, t(10)).await, needs);
        assert_eq!(limit_of(&mut db, who.business).await, None);
        record_step_up(&mut db, who, t(100)).await.expect("code");
        let ok = change_fee_limit(&mut db, &me, Some(9_000), t(100 + 240)).await;
        assert_eq!(ok, Ok(()));
        assert_eq!(limit_of(&mut db, who.business).await, Some(9_000));
        let late = change_fee_limit(&mut db, &me, None, t(100 + 301)).await;
        assert_eq!(late, needs);
        assert_eq!(
            limit_of(&mut db, who.business).await,
            Some(9_000),
            "unchanged at 5:01"
        );
        let cards_only = BusinessPrincipal {
            owner: false,
            roles: (true, false),
            ..me
        };
        let role = change_fee_limit(&mut db, &cards_only, None, t(120)).await;
        assert_eq!(role, Err(BillingChangeError::Role));
        let fresh = change_saved_card(&mut db, &me, &card, t(120)).await;
        assert_eq!(
            fresh,
            Err(BillingChangeError::Payments),
            "no Stripe customer"
        );
    }

    /// TD-BIZ-09 In case (BIZ-029, D-200): through `businesses::api`, a reply at 4 days 23:59 is
    /// accepted and shown; at exactly 5 days it is refused.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_biz_09_reply_through_the_portal(mut db: Db) {
        let (who, me) = owner(&mut db).await;
        let mut ids = Vec::new();
        for job in [0xa1, 0xa2] {
            let new = NewComplaint {
                business: who.business,
                job: Uuid::from_u128(job),
                kind: "no_show",
                words: "Nobody came.",
            };
            ids.push(record_complaint(&mut db, &new, t(0)).await.expect("in").id);
        }
        let first = reply_to_complaint(&mut db, &me, (ids[0], "We came."), t(5 * 86_400 - 60));
        assert_eq!(first.await, Ok(()));
        let late = reply_to_complaint(&mut db, &me, (ids[1], "We came."), t(5 * 86_400));
        assert_eq!(late.await, Err(ComplaintError::Closed));
        let list = complaints(&mut db, who.business).await.expect("list");
        let page = complaints_page(&list, (t(5 * 86_400), None), "t").expect("page");
        assert!(page.contains("Your reply: We came.") && !page.contains("Nobody came."));
    }
}
