//! The owner's pilot scorecard page and its weekly minutes form (OPS-039, CALL-024, OPS-046,
//! OPS-047; T158, D-396, D-458, D-468, D-3753, D-3877, D-3890, D-4202, D-4280).
//!
//! [`owner_scorecard_page`] computes no input itself: every count comes from another crate's
//! scorecard reader (`quotes`, `picks`, `payments`, `jobs`, `intake`, `calls`, `businesses`,
//! `ops`), and this module only combines them (shares, per-introduction ratios, per-week averages,
//! the ladder) and colors them with T621's judge (`scorecard_lines`). No `payments` or `picks`
//! table stores a trade, so each trade's job IDs come from `quotes::api::jobs_by_trade` and go to
//! every per-job reader; the total is all those jobs. Two windows: the Arizona week holding `now`
//! (from Monday 00:00) and the pilot to date (from [`ScorecardInputs::pilot_start`]); the ladder
//! counts the Arizona month. Requests and sign-ups by source, intake completion, minutes per paid
//! introduction and the BIZ-054 leaving and pause answers are total only (their readers take no
//! job IDs). T622's panels (`scorecard_panels`) add OPS-046's email panel and OPS-047's figures
//! for the pushed trades. A color is also written as a word, never shown by color alone. The page
//! decides nothing: a GET reads only, and no color raises an alert, a task or a change (D-396,
//! D-446). The one write is [`owner_record_minutes`], the weekly minutes form, through
//! `ops::api::record_minutes`. `portals` cannot read `Settings`, so the serving route (T305,
//! D-2128) passes [`ScorecardInputs`]. Counts, code names and record IDs only.

use askama::Template;
use askhava_businesses::api::{
    AskedBy, complaints_for_jobs, leaving_answers, signup_routes, signups_by_source,
};
use askhava_calls::api::calls_answered;
use askhava_core::api::arizona_time;
use askhava_intake::api::{cards_approved, intake_completion, requests_by_source};
use askhava_jobs::api::{jobs_by_trade as routable_jobs, requests_per_week};
use askhava_ops::api::{WeekMinutes, minutes_for_week, record_minutes};
use askhava_payments::api::{Share, free_share, ledger_totals, paying_again};
use askhava_picks::api::{confirmed_share, diagnostic_share, picked_requests};
use askhava_quotes::api::{
    active_businesses, diagnostic_quotes, jobs_by_trade, notes_returned, quotes_per_business,
    request_quotes, requests_with_quotes,
};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::owner_auth::OwnerPrincipal;
use crate::owner_portal::OwnerToolError;
use crate::scorecard_lines::{Color, LadderRung, Measure as M, ScorecardLines, color, ladder_rung};
use crate::scorecard_panels::{EmailPanel, TradeFigures, email_panel, trade_figures};

/// OPS-039, D-3890: the scorecard page's fixed path (served by T305).
pub const OWNER_SCORECARD_ROUTE: &str = "/owner/scorecard";
/// OPS-039: where the weekly minutes form posts.
pub const OWNER_SCORECARD_MINUTES_ROUTE: &str = "/owner/scorecard/minutes";
const WEEK: SignedDuration = SignedDuration::from_hours(7 * 24);
const LISTS: [&str; 4] = [
    "Requests by source",
    "Business sign-ups by source",
    "Step where unfinished intakes stopped",
    "Leaving and pause answers",
];

/// Awaits a reader and maps its error to [`OwnerToolError::Database`].
macro_rules! read {
    ($call:expr) => {
        $call.await.map_err(db)?
    };
}

/// What the serving route passes in from `Settings` (OPS-039, OPS-047, FEE-046).
#[derive(Debug, Clone)]
pub struct ScorecardInputs {
    /// The go and stop lines and the ladder (`Settings::scorecard_lines()`).
    pub lines: ScorecardLines,
    /// `Settings::sales_tax_set_aside_basis_points()`, for net per paid introduction.
    pub set_aside_basis_points: u16,
    /// The pilot's first instant; pilot week 1 starts here.
    pub pilot_start: Timestamp,
    /// The pushed trades' code names, for OPS-047's figures.
    pub pushed: Vec<String>,
}

/// The weekly minutes form's body, as typed (OPS-039).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MinutesForm {
    /// Minutes spent recruiting businesses this week.
    pub recruiting: String,
    /// Minutes spent on everything else this week.
    pub other: String,
}

/// What the page says about the last minutes form sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinutesNotice {
    /// The week's minutes were saved.
    Saved,
    /// The form was refused; it is shown again with the values as typed.
    Refused(MinutesForm),
}

/// OPS-039: saves `form` as the minutes of the Arizona week holding `now` through
/// `ops::api::record_minutes`, by `principal`; a later entry for the week replaces it when read.
///
/// # Errors
///
/// [`OwnerToolError::Refused`] when a part is not a whole number from 0 to a week's minutes;
/// [`OwnerToolError::Database`].
pub async fn owner_record_minutes(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    form: &MinutesForm,
    now: Timestamp,
) -> Result<(), OwnerToolError> {
    let part = |s: &str| s.trim().parse::<u32>().ok();
    let minutes = part(&form.recruiting)
        .zip(part(&form.other))
        .and_then(|(r, o)| WeekMinutes::new(r, o))
        .ok_or(OwnerToolError::Refused)?;
    read!(record_minutes(conn, principal.owner(), minutes, now));
    Ok(())
}

fn db<E>(_: E) -> OwnerToolError {
    OwnerToolError::Database
}

/// A window: at or after `.0`, before `.1`.
type Window = (Timestamp, Timestamp);

/// Whole weeks a window spans, a part week counted as one.
fn weeks((from, to): Window) -> i64 {
    let secs = to.duration_since(from).as_secs();
    ((secs + WEEK.as_secs() - 1) / WEEK.as_secs()).max(1)
}

/// One measure's figure in one window: its words and the value its color is judged on.
struct Fig {
    measure: M,
    text: String,
    value: Option<f64>,
}

/// One figure as shown, with its color word when it has one.
struct Cell {
    text: String,
    color: Option<&'static str>,
}

struct Row {
    label: &'static str,
    week: Cell,
    pilot: Cell,
}

struct Scope {
    title: String,
    rows: Vec<Row>,
}

type Counts = Vec<(String, i64)>;

struct PanelTrade {
    trade: String,
    minutes: String,
    per_pick: Vec<(Uuid, String)>,
    not_a_fit: Vec<(&'static str, i64)>,
    refunds: Vec<(String, i64)>,
}

#[derive(Template)]
#[template(path = "scorecard.html")]
struct ScorecardView<'a> {
    pilot_week: u32,
    scopes: Vec<Scope>,
    ladder: [u32; 3],
    paid_month: Cell,
    rung: &'static str,
    lists: Vec<(&'static str, Counts, Counts)>,
    email: Option<EmailPanel>,
    panels: Vec<(&'static str, Vec<PanelTrade>)>,
    csrf: &'a str,
    minutes_route: &'static str,
    saved: bool,
    refused: Option<&'a MinutesForm>,
}

/// OPS-039, CALL-024, D-458, D-468: each measure's row label.
fn label(m: M) -> &'static str {
    match m {
        M::QuotedWithin24h => "Requests with 1 or more quotes within 24 hours",
        M::TwoQuotesWithin24h => "Requests with 2 or more quotes within 24 hours",
        M::ThreeQuotesWithin24h => "Requests with 3 or more quotes within 24 hours",
        M::MedianFirstQuoteHours => "Median time to the first quote",
        M::P75FirstQuoteHours => "Time for 3 in 4 requests to get a first quote",
        M::RequestsWithPhoto => "Requests with a photo",
        M::PicksPerQuotedRequest => "Requests with quotes that got a pick",
        M::PicksAmongTwoQuoteRequests => "Picks among requests with 2 or more quotes",
        M::DiagnosticVisitPicks => "Picks that were diagnostic-visit quotes",
        M::PicksConfirmed => "Picks confirmed rather than lapsed",
        M::ActiveBusinesses => "Active businesses (quoted in the last 30 days)",
        M::QuotesPerWin => "Quotes per win",
        M::NotesReturned => "Quote notes returned to businesses",
        M::AverageFee => "Average introduction fee by job size",
        M::NetPerIntroduction => "Net per paid introduction",
        M::FreeShare => "Free introductions after a business's first win",
        M::Refunds => "Refunds per paid introduction",
        M::BankDisputes => "Bank disputes per paid introduction",
        M::PayingAgainIn60Days => "Businesses paying again within 60 days",
        M::MaskedCallsAnswered => "Masked calls a person answered",
        M::ComplaintsPer20Jobs => "Homeowner complaints per 20 jobs",
        M::CardsApprovedWithoutEdit => "Cards approved without an edit",
        M::RequestsPerWeek => "Requests started per week",
        M::SignupsActiveIn15Min => "Business sign-ups ready to quote within 15 minutes",
        M::SignupsToLetterOrReview => "Business sign-ups held for a mailed letter or a review",
        M::IntakeReachedCard => "Started intakes that reached the card step",
        M::MinutesPerIntroduction => "Your minutes per paid introduction",
        M::PaidIntroductionsThisMonth => "Paid introductions this month",
    }
}

fn fig(measure: M, value: Option<f64>, text: impl FnOnce(f64) -> String) -> Fig {
    let text = value.map_or_else(|| "No data yet".to_owned(), text);
    Fig {
        measure,
        text,
        value,
    }
}

fn part(part: i64, whole: i64) -> Option<Share> {
    (whole > 0).then_some(Share { part, whole })
}

#[expect(clippy::cast_precision_loss, reason = "counts stay far below 2^52")]
fn ratio(top: i64, bottom: i64) -> Option<f64> {
    (bottom > 0).then(|| top as f64 / bottom as f64)
}

/// A share as a percent with one decimal and its counts.
fn share(measure: M, s: Option<Share>) -> Fig {
    let s = s.and_then(|s| part(s.part, s.whole));
    let value = s.and_then(|s| ratio(s.part * 100, s.whole));
    let counts = s.map_or((0, 0), |s| (s.part, s.whole));
    fig(measure, value, |v| {
        format!("{v:.1}% ({} of {})", counts.0, counts.1)
    })
}

fn num(measure: M, value: Option<f64>) -> Fig {
    fig(measure, value, |v| format!("{v:.1}"))
}

fn whole(measure: M, n: i64) -> Fig {
    fig(measure, ratio(n, 1), |_| n.to_string())
}

fn hours(measure: M, d: Option<SignedDuration>) -> Fig {
    let value = d.map(|d| d.as_secs_f64() / 3600.0);
    fig(measure, value, |v| format!("{v:.1} hours"))
}

fn dollars(measure: M, cents: Option<i64>) -> Fig {
    let c = cents.unwrap_or(0);
    let sign = if c < 0 { "-" } else { "" };
    let text = format!("{sign}${}.{:02}", c.abs() / 100, c.abs() % 100);
    fig(measure, cents.and_then(|c| ratio(c, 100)), |_| text)
}

/// OPS-039, D-468: request, first-quote and pick figures of `jobs` in `w`.
async fn quote_figs(
    c: &mut PgConnection,
    jobs: &[Uuid],
    w: Window,
) -> Result<Vec<Fig>, OwnerToolError> {
    let rq = read!(request_quotes(c, jobs, w.0, w.1));
    let (n, within) = rq.map_or((0, [0; 3]), |r| (r.requests, r.within_24_hours));
    let one = read!(requests_with_quotes(c, jobs, w, 1));
    let two = read!(requests_with_quotes(c, jobs, w, 2));
    let pairs = read!(diagnostic_quotes(c, jobs));
    Ok(vec![
        share(M::QuotedWithin24h, part(within[0], n)),
        share(M::TwoQuotesWithin24h, part(within[1], n)),
        share(M::ThreeQuotesWithin24h, part(within[2], n)),
        hours(
            M::MedianFirstQuoteHours,
            rq.and_then(|r| r.median_first_quote),
        ),
        hours(M::P75FirstQuoteHours, rq.and_then(|r| r.p75_first_quote)),
        share(M::RequestsWithPhoto, rq.and_then(|r| part(r.with_photo, n))),
        share(
            M::PicksPerQuotedRequest,
            read!(picked_requests(c, &one, w.1)),
        ),
        share(
            M::PicksAmongTwoQuoteRequests,
            read!(picked_requests(c, &two, w.1)),
        ),
        share(
            M::DiagnosticVisitPicks,
            read!(diagnostic_share(c, jobs, w, &pairs)),
        ),
    ])
}

/// OPS-039, D-468: confirmations, active businesses, quotes per win (a win is a confirmed pick)
/// and returned notes (PICK-052) of `jobs` in `w`.
async fn pick_figs(
    c: &mut PgConnection,
    jobs: &[Uuid],
    w: Window,
) -> Result<Vec<Fig>, OwnerToolError> {
    let confirmed = read!(confirmed_share(c, jobs, w.0, w.1));
    let per_business = read!(quotes_per_business(c, jobs, w.0, w.1)).unwrap_or_default();
    let quotes: i64 = per_business.iter().map(|(_, n)| n).sum();
    let wins = confirmed.map_or(0, |s| s.part);
    Ok(vec![
        share(M::PicksConfirmed, confirmed),
        whole(
            M::ActiveBusinesses,
            read!(active_businesses(c, jobs, w.1)).unwrap_or(0),
        ),
        num(M::QuotesPerWin, ratio(quotes, wins)),
        whole(
            M::NotesReturned,
            read!(notes_returned(c, jobs, w.0, w.1)).unwrap_or(0),
        ),
    ])
}

/// OPS-039, FEE-046, D-458: the money figures of `jobs` in `w`, and the paid introductions.
async fn money_figs(
    c: &mut PgConnection,
    jobs: &[Uuid],
    w: Window,
    set_aside: u16,
) -> Result<(Vec<Fig>, i64), OwnerToolError> {
    let t = read!(ledger_totals(c, jobs, w.0, w.1));
    let paid = t.map_or(0, |t| t.paid_introductions);
    let figs = vec![
        dollars(M::AverageFee, t.and_then(|t| t.average_fee_cents())),
        dollars(
            M::NetPerIntroduction,
            t.and_then(|t| t.net_per_introduction_cents(set_aside)),
        ),
        share(M::FreeShare, read!(free_share(c, jobs, w.0, w.1))),
        share(M::Refunds, t.and_then(|t| part(t.refunds, paid))),
        share(M::BankDisputes, t.and_then(|t| part(t.disputes, paid))),
        share(
            M::PayingAgainIn60Days,
            read!(paying_again(c, jobs, w.0, w.1)),
        ),
    ];
    Ok((figs, paid))
}

/// OPS-039, CALL-024, BIZ-053: calls, complaints per 20 routable requests, cards, sign-ups of
/// `trade` (all with `None`) and requests started per week in `w`.
async fn people_figs(
    c: &mut PgConnection,
    (jobs, trade): (&[Uuid], Option<&str>),
    w: Window,
) -> Result<Vec<Fig>, OwnerToolError> {
    let calls = read!(calls_answered(c, jobs, w));
    let complaints = read!(complaints_for_jobs(c, jobs, w));
    let cards = read!(cards_approved(c, jobs, w));
    let signups = read!(signup_routes(c, w, trade));
    let in_trade = |t: Option<&str>| trade.is_none_or(|want| t == Some(want));
    let routable: usize = read!(routable_jobs(c, w))
        .iter()
        .filter(|t| in_trade(Some(&t.trade)))
        .map(|t| t.jobs.len())
        .sum();
    let started: i64 = read!(requests_per_week(c, w))
        .iter()
        .filter(|r| in_trade(r.trade.as_deref()))
        .map(|r| r.requests)
        .sum();
    let routable = i64::try_from(routable).map_err(db)?;
    let n = signups.signups;
    Ok(vec![
        share(M::MaskedCallsAnswered, part(calls.answered, calls.calls)),
        num(M::ComplaintsPer20Jobs, ratio(complaints * 20, routable)),
        share(
            M::CardsApprovedWithoutEdit,
            part(cards.without_edit, cards.approved),
        ),
        num(M::RequestsPerWeek, ratio(started, weeks(w))),
        share(
            M::SignupsActiveIn15Min,
            part(signups.active_within_15_minutes_approx, n),
        ),
        share(
            M::SignupsToLetterOrReview,
            part(signups.letter_or_review, n),
        ),
    ])
}

/// Every figure of one scope (a trade's jobs, or all with `None`) in `w`; the total adds intake
/// completion and Casey's minutes per paid introduction.
async fn scope_figs(
    c: &mut PgConnection,
    (jobs, trade): (&[Uuid], Option<&str>),
    w: Window,
    set_aside: u16,
) -> Result<Vec<Fig>, OwnerToolError> {
    let mut figs = quote_figs(c, jobs, w).await?;
    figs.extend(pick_figs(c, jobs, w).await?);
    let (money, paid) = money_figs(c, jobs, w, set_aside).await?;
    figs.extend(money);
    figs.extend(people_figs(c, (jobs, trade), w).await?);
    if trade.is_none() {
        let intake = read!(intake_completion(c, w));
        figs.push(share(
            M::IntakeReachedCard,
            part(intake.reached_card, intake.started),
        ));
        figs.push(minutes_fig(c, w, paid).await?);
    }
    Ok(figs)
}

/// OPS-039: the weekly minutes entries of every Arizona week in `w`, per paid introduction.
async fn minutes_fig(c: &mut PgConnection, w: Window, paid: i64) -> Result<Fig, OwnerToolError> {
    let (mut recruiting, mut other, mut at) = (0_i64, 0_i64, w.0);
    while at < w.1 {
        if let Some(m) = read!(minutes_for_week(c, at)) {
            recruiting += i64::from(m.recruiting());
            other += i64::from(m.other());
        }
        at += WEEK;
    }
    Ok(fig(
        M::MinutesPerIntroduction,
        ratio(recruiting + other, paid),
        |v| format!("{v:.1} minutes ({recruiting} recruiting, {other} everything else)"),
    ))
}

/// OPS-039, D-4280: a figure with its color word from T621's judge.
fn cell(f: Fig, pilot_week: u32, lines: &ScorecardLines) -> Cell {
    let judged = f
        .value
        .map_or(Color::Uncolored, |v| color(f.measure, v, pilot_week, lines));
    let color = match judged {
        Color::Green => Some("green"),
        Color::Amber => Some("amber"),
        Color::Red => Some("red"),
        Color::Uncolored => None,
    };
    Cell {
        text: f.text,
        color,
    }
}

/// OPS-039, INT-047, BIZ-053, BIZ-054 (D-4202): the source, step and answer counts of `w`, in
/// [`LISTS`] order; total only.
async fn counts(c: &mut PgConnection, w: Window) -> Result<[Counts; 4], OwnerToolError> {
    let requests = read!(requests_by_source(c, w))
        .into_iter()
        .map(|r| (r.source, r.requests));
    let none = |s: Option<String>| s.unwrap_or_else(|| "none".to_owned());
    let signups = read!(signups_by_source(c, w, None)).into_iter().map(|s| {
        (
            format!("link {}, answer {}", none(s.g_code), none(s.heard_from)),
            s.signups,
        )
    });
    let stops = read!(intake_completion(c, w))
        .stopped_at
        .into_iter()
        .map(|s| (s.step, s.requests));
    let mut answers: Counts = Vec::new();
    for a in read!(leaving_answers(c, w)) {
        let asked = if a.asked_by == AskedBy::Left {
            "Business left"
        } else {
            "Business paused"
        };
        let key = format!(
            "{asked}: {}",
            a.choice.map_or("Skipped the question", |r| r.label())
        );
        match answers.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => answers.push((key, 1)),
        }
    }
    Ok([
        requests.collect(),
        signups.collect(),
        stops.collect(),
        answers,
    ])
}

/// OPS-047: T622's figures as the page words them.
fn panel_trade(f: TradeFigures) -> PanelTrade {
    let per_pick = f.quotes_per_pick.iter().map(|b| {
        let words = b.quotes_per_pick_tenths().map_or_else(
            || format!("{} quotes, no pick", b.quotes),
            |t| format!("{}.{}", t / 10, t % 10),
        );
        (b.business, words)
    });
    PanelTrade {
        minutes: f.median_quote_minutes.map_or_else(
            || "No data yet".to_owned(),
            |d| format!("{:.1} minutes", d.as_secs_f64() / 60.0),
        ),
        per_pick: per_pick.collect(),
        not_a_fit: f.not_a_fit.iter().map(|(r, n)| (r.code(), *n)).collect(),
        refunds: f.refund_reasons,
        trade: f.trade,
    }
}

/// The page's week and pilot windows, the Arizona month's start and the pilot week at `now`.
fn windows(
    now: Timestamp,
    start: Timestamp,
) -> Result<(Window, Window, Timestamp, u32), OwnerToolError> {
    let az = arizona_time(now);
    let monday = i64::from(az.date().weekday().to_monday_zero_offset());
    let today = az.start_of_day().map_err(db)?.timestamp();
    let week = (today - SignedDuration::from_hours(24 * monday), now);
    let month = az
        .first_of_month()
        .and_then(|d| d.start_of_day())
        .map_err(db)?;
    let pilot_week = now.duration_since(start).as_secs().max(0) / WEEK.as_secs() + 1;
    let pilot_week = u32::try_from(pilot_week).map_err(db)?;
    Ok((week, (start, now), month.timestamp(), pilot_week))
}

/// One scope per trade after the total, each row the week's and the pilot's figure.
async fn scopes(
    c: &mut PgConnection,
    all: &[Uuid],
    (week, pilot): (Window, Window),
    inputs: &ScorecardInputs,
    pilot_week: u32,
) -> Result<Vec<Scope>, OwnerToolError> {
    let trades = read!(jobs_by_trade(c, pilot.1));
    let named = trades
        .iter()
        .map(|t| (t.jobs.as_slice(), Some(t.trade.as_str())));
    let mut scopes = Vec::with_capacity(trades.len() + 1);
    for scope in std::iter::once((all, None)).chain(named) {
        let w = scope_figs(c, scope, week, inputs.set_aside_basis_points).await?;
        let p = scope_figs(c, scope, pilot, inputs.set_aside_basis_points).await?;
        let rows = w.into_iter().zip(p).map(|(w, p)| Row {
            label: label(w.measure),
            week: cell(w, pilot_week, &inputs.lines),
            pilot: cell(p, pilot_week, &inputs.lines),
        });
        let title = scope
            .1
            .map_or_else(|| "All trades".to_owned(), |t| format!("Trade: {t}"));
        scopes.push(Scope {
            title,
            rows: rows.collect(),
        });
    }
    Ok(scopes)
}

/// OPS-039, D-458: this month's paid introductions with their color, and the ladder rung.
async fn ladder(
    c: &mut PgConnection,
    all: &[Uuid],
    (month, now): Window,
    inputs: &ScorecardInputs,
    pilot_week: u32,
) -> Result<(Cell, &'static str), OwnerToolError> {
    let paid = read!(ledger_totals(c, all, month, now)).map_or(0, |t| t.paid_introductions);
    let rung = match ladder_rung(u32::try_from(paid).unwrap_or(u32::MAX), &inputs.lines) {
        LadderRung::Below => "below the pilot target",
        LadderRung::Pilot => "at the pilot target",
        LadderRung::Pace20k => "at the $20,000 a year target",
        LadderRung::Pace50k => "at the $50,000 a year target",
    };
    let paid = whole(M::PaidIntroductionsThisMonth, paid);
    Ok((cell(paid, pilot_week, &inputs.lines), rung))
}

/// OPS-039, CALL-024, OPS-046, OPS-047: the owner's pilot scorecard at `now`, every measure per
/// trade and in total for the week and the pilot to date, colored against `inputs.lines`, with
/// the minutes form (`csrf` its token) and what `notice` says of the last form sent. Reads only.
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read or render.
pub async fn owner_scorecard_page(
    c: &mut PgConnection,
    inputs: &ScorecardInputs,
    (csrf, notice): (&str, Option<&MinutesNotice>),
    now: Timestamp,
) -> Result<String, OwnerToolError> {
    let (week, pilot, month, pilot_week) = windows(now, inputs.pilot_start)?;
    let mut all: Vec<Uuid> = read!(jobs_by_trade(c, now))
        .into_iter()
        .flat_map(|t| t.jobs)
        .collect();
    all.sort_unstable();
    all.dedup();
    let scopes = scopes(c, &all, (week, pilot), inputs, pilot_week).await?;
    let (paid_month, rung) = ladder(c, &all, (month, now), inputs, pilot_week).await?;
    let (lists_week, lists_pilot) = (counts(c, week).await?, counts(c, pilot).await?);
    let lists = LISTS.into_iter().zip(lists_week).zip(lists_pilot);
    let pushed: Vec<&str> = inputs.pushed.iter().map(String::as_str).collect();
    let mut panels = Vec::with_capacity(2);
    for (when, w) in [("This week", week), ("Pilot to date", pilot)] {
        let figures = read!(trade_figures(c, &pushed, w));
        panels.push((when, figures.into_iter().map(panel_trade).collect()));
    }
    ScorecardView {
        pilot_week,
        scopes,
        ladder: inputs.lines.ladder(),
        paid_month,
        rung,
        lists: lists.map(|((t, w), p)| (t, w, p)).collect(),
        email: read!(email_panel(c, now)),
        panels,
        csrf,
        minutes_route: OWNER_SCORECARD_MINUTES_ROUTE,
        saved: notice == Some(&MinutesNotice::Saved),
        refused: match notice {
            Some(MinutesNotice::Refused(form)) => Some(form),
            _ => None,
        },
    }
    .render()
    .map_err(db)
}
