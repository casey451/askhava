//! The weekly digest and the owner-portal flags (OPS-027, OPS-030, OPS-031, OPS-033, OPS-034,
//! OPS-041, BIZ-054, CON-016; D-219, D-3880, D-3882, T524).
//!
//! [`weekly_digest`] lists every OPS-030 item for a half-open window as counts and job numbers,
//! read only through other modules' readers; it computes no input itself, and an item with no
//! reader yet (unreadable business texts) is reported as such, never queried (D-3880).
//! [`owner_flags`] applies the [`OwnerFlagLimits`] settings to the monthly inputs; a flag changes
//! nothing. [`owner_digest_view`] is what the owner page shows (T815 serves it at
//! [`OWNER_DIGEST_ROUTE`]): the digest, the flags, the help requests due in the next 4 days, and
//! PROBLEM reply words only under an [`OwnerSessionProof`] (PRIV-006). No result holds a
//! homeowner contact detail or vault ID (PRIV-028). The test-restore action names are passed in,
//! since `portals` may not call `recovery` (D-4932).

use askhava_businesses::api::{AskedBy, LeaveReason, leaving_answers, license_paused};
use askhava_core::api::{OwnerFlagLimits, arizona, arizona_time};
use askhava_jobs::api::{CHECK_IN_ANSWER, PROBLEM_ALERT, PROBLEM_SAFETY_ALERT, job_number};
use askhava_ops::api::{DigestAlert, audit_subjects, digest_alerts, latest_audit};
use askhava_ops::api::{open_help_requests, system_status_changes, trade_active_counts};
use askhava_privacy::api::{OwnerSessionProof, problem_words};
use jiff::{SignedDuration, Span, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

/// The owner-portal path of the digest view, the weekly email's one portal link (D-5319).
pub const OWNER_DIGEST_ROUTE: &str = "/owner/digest";
/// OPS-030: one digest covers the week before it.
pub const DIGEST_WEEK: SignedDuration = SignedDuration::from_hours(7 * 24);
/// OPS-033: the owner portal lists what falls due this far ahead.
pub const DUE_AHEAD: SignedDuration = SignedDuration::from_hours(4 * 24);
/// The subject kind of a PROBLEM alert: the inbox row its words are kept for (JOB-016).
const INBOX: &str = "messaging.inbox";

/// The digest could not read an input; nothing is sent or shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DigestError {
    /// A reader failed.
    #[error("a digest input could not be read")]
    Database,
}

/// One OPS-030 item: its label, its count (`None` when no reader exists yet, D-3880) and its job
/// numbers or code-name details (trades, answers, due times); never a contact detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestLine {
    /// The item's label.
    pub item: &'static str,
    /// How many, or `None` for an input with no reader.
    pub count: Option<i64>,
    /// Job numbers, trade or answer codes, or Arizona due times.
    pub refs: Vec<String>,
}

/// OPS-030's digest of `[from, to)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    /// The window's start.
    pub from: Timestamp,
    /// The window's end.
    pub to: Timestamp,
    /// Every OPS-030 item, in the requirement's order.
    pub lines: Vec<DigestLine>,
}

/// One OPS-027 crowded trade, with its quotes per win in tenths (`None` with no win).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrowdedTrade {
    /// The trade's code name.
    pub trade: String,
    /// Its active businesses.
    pub active: u32,
    /// Its median quotes per job last calendar month, in tenths.
    pub median_tenths: u32,
    /// Confirmed quotes per win last calendar month, in tenths.
    pub quotes_per_win_tenths: Option<i64>,
}

/// The owner flags for Casey's monthly review; none acts on anything (OPS-027, OPS-031, OPS-034).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnerFlags {
    /// OPS-027: crowded trades.
    pub crowded: Vec<CrowdedTrade>,
    /// OPS-031: trades over the NO-or-OTHER share in each of the last months.
    pub no_or_other: Vec<String>,
    /// OPS-034: businesses with their lapsed picks in the window.
    pub lapsed: Vec<(Uuid, i64)>,
    /// OPS-034: each flagged homeowner's request count, never her phone or vault ID.
    pub repeat_requests: Vec<i64>,
}

/// One PROBLEM reply's kept words with its job number, shown only under the owner's proof.
pub struct ProblemReply {
    /// The job's number.
    pub number: i64,
    /// The scrubbed words.
    pub words: String,
}

/// What the owner digest page shows (OPS-030, OPS-033, D-5318).
pub struct OwnerDigestView {
    /// The last week's digest.
    pub digest: Digest,
    /// The monthly-review flags.
    pub flags: OwnerFlags,
    /// OPS-033: the open help requests due in the next 4 days, as Arizona times.
    pub due_soon: Vec<String>,
    /// The week's PROBLEM words; empty without a proof.
    pub problem_words: Vec<ProblemReply>,
}

/// The settings the digest and the flags read: the flag numbers and T648's restore audit
/// actions, passed or failed (D-4932).
#[derive(Debug, Clone, Copy)]
pub struct DigestInputs<'a> {
    /// The flag numbers.
    pub limits: OwnerFlagLimits,
    /// The test restore's passed and failed audit actions.
    pub restore_actions: [&'a str; 2],
}

fn read<T, E>(r: Result<T, E>) -> Result<T, DigestError> {
    r.map_err(|_| DigestError::Database)
}

fn line(item: &'static str, count: usize, refs: Vec<String>) -> DigestLine {
    let count = Some(i64::try_from(count).unwrap_or(i64::MAX));
    DigestLine { item, count, refs }
}

/// OPS-030: the alerts of `kinds` in `window`, kept by `keep`.
async fn alerts(
    conn: &mut PgConnection,
    kinds: &[&str],
    (from, to): (Timestamp, Timestamp),
    keep: impl Fn(&DigestAlert) -> bool,
) -> Result<Vec<DigestAlert>, DigestError> {
    let all = read(digest_alerts(conn, kinds, from, to).await)?;
    Ok(all.into_iter().filter(|a| keep(a)).collect())
}

/// OPS-030: the job numbers of `jobs`.
async fn numbers(conn: &mut PgConnection, jobs: &[Uuid]) -> Result<Vec<String>, DigestError> {
    let mut out = Vec::with_capacity(jobs.len());
    for job in jobs {
        out.push(format!("#{}", read(job_number(conn, *job).await)?));
    }
    Ok(out)
}

/// OPS-030, D-3883: automatic activations and pauses (the system actor's status rows), no-shows,
/// license-unknown counts and BIZ-054's leaving answers.
async fn business_lines(
    conn: &mut PgConnection,
    window: (Timestamp, Timestamp),
) -> Result<Vec<DigestLine>, DigestError> {
    let rows = system_status_changes(conn, "businesses.business", window.0, window.1).await;
    let rows = read(rows)?;
    let to = |s: &str| {
        rows.iter()
            .filter(|r| r.to_status.as_deref() == Some(s))
            .count()
    };
    let field = rows
        .iter()
        .filter(|r| r.field.as_deref() == Some("pause_reason"));
    let (activations, pauses) = (to("active"), to("paused") + field.count());
    let shows = read(askhava_picks::api::no_shows(conn, window).await)?;
    let shown: Vec<String> = shows.iter().map(|s| format!("#{}", s.job_number)).collect();
    let list = [
        askhava_licensing::api::LIST_FAILING_ALERT,
        askhava_licensing::api::LIST_STILL_FAILING_ALERT,
    ];
    let failing = alerts(conn, &list, window, |_| true).await?.len();
    let paused = read(license_paused(conn, window).await)?.len();
    let answers = read(leaving_answers(conn, window).await)?;
    let answers: Vec<String> = answers
        .iter()
        .map(|a| answer(a.asked_by, a.choice))
        .collect();
    Ok(vec![
        line("Automatic activations", activations, Vec::new()),
        line("No-shows", shown.len(), shown),
        line("Automatic pauses (no review needed)", pauses, Vec::new()),
        line("License unknown", failing + paused, Vec::new()),
        line("Leaving answers", answers.len(), answers),
    ])
}

/// BIZ-054: one answer as `left` or `paused` and its choice's code; never the box's words.
fn answer(by: AskedBy, choice: Option<LeaveReason>) -> String {
    let by = if by == AskedBy::Left {
        "left"
    } else {
        "paused"
    };
    let choice = match choice {
        None => "no answer",
        Some(LeaveReason::Busy) => "busy for a while",
        Some(LeaveReason::FewJobs) => "too few jobs",
        Some(LeaveReason::NotAFit) => "jobs not a fit",
        Some(LeaveReason::Fee) => "the fee",
        Some(LeaveReason::TooMany) => "too many businesses quoting",
        Some(LeaveReason::Other) => "something else",
    };
    format!("{by}: {choice}")
}

/// OPS-030: self-acknowledged alerts, complaints and PROBLEM replies (with their job numbers
/// from the check-in answer rows), disputes, unknown charges and the volume alerts.
async fn alert_lines(
    conn: &mut PgConnection,
    window: (Timestamp, Timestamp),
) -> Result<Vec<DigestLine>, DigestError> {
    use askhava_ops::api::{HEALTH_ALERT, LEDGER_ALERT, VENDOR_ALERT};
    let cleared = [HEALTH_ALERT, VENDOR_ALERT, LEDGER_ALERT];
    let acked = alerts(conn, &cleared, window, |a| a.self_acknowledged).await?;
    let from = window.0;
    let raised = |a: &DigestAlert| a.raised_at >= from;
    let complaints = [
        askhava_businesses::api::DIGEST_ALERT,
        askhava_businesses::api::SAFETY_ALERT,
    ];
    let complaints = alerts(conn, &complaints, window, raised).await?.len();
    let problems = [PROBLEM_ALERT, PROBLEM_SAFETY_ALERT];
    let problems = alerts(conn, &problems, window, raised).await?.len();
    let answered = read(audit_subjects(conn, CHECK_IN_ANSWER, window.0, window.1).await)?;
    let problem_field = |f: &str| f.ends_with("_problem");
    let jobs: Vec<Uuid> = answered
        .iter()
        .filter(|r| r.field.as_deref().is_some_and(problem_field))
        .map(|r| r.subject_id)
        .collect();
    let jobs = numbers(conn, &jobs).await?;
    let dispute = [askhava_payments::api::DISPUTE_ACTION];
    let disputes = alerts(conn, &dispute, window, raised).await?.len();
    let unknown = [askhava_payments::api::UNKNOWN_CHARGE_ALERT];
    let unknown = alerts(conn, &unknown, window, raised).await?.len();
    use askhava_ops::api::{VOLUME_HIGH_ALERT, VOLUME_LOW_ALERT};
    let volume = alerts(conn, &[VOLUME_LOW_ALERT, VOLUME_HIGH_ALERT], window, raised).await?;
    Ok(vec![
        line("Self-acknowledged alerts", acked.len(), Vec::new()),
        line("Complaints", complaints, Vec::new()),
        line("PROBLEM replies", problems, jobs),
        line("Dispute filings", disputes, Vec::new()),
        line("Unknown charges refunded", unknown, Vec::new()),
        DigestLine {
            item: "Unreadable business texts",
            count: None,
            refs: vec!["no reader yet".to_owned()],
        },
        line("Database volume alerts", volume.len(), Vec::new()),
    ])
}

/// OPS-030, OPS-041, CON-016, CON-034: opt-out rates, call-backs due, help requests with their
/// due times, HELP answered automatically, opted-out numbers that wrote again and the
/// not-covered requests by trade.
async fn contact_lines(
    conn: &mut PgConnection,
    window: (Timestamp, Timestamp),
) -> Result<Vec<DigestLine>, DigestError> {
    use askhava_messaging::api::{AUTO_ANSWERED_ACTION, WROTE_AGAIN_ACTION, Week};
    let week = Week::containing(window.1).previous();
    let rates = read(askhava_messaging::api::weekly_optout_rates(conn, week).await)?;
    let rate = |r: askhava_messaging::api::WeekRate| format!("{} of {}", r.opted_out, r.texted);
    let rates = vec![
        format!("homeowners {}", rate(rates.homeowners)),
        format!("businesses {}", rate(rates.businesses)),
    ];
    let kinds = [
        askhava_intake::api::CALL_BACK_ALERT,
        askhava_intake::api::CALL_BACK_URGENT_ALERT,
        askhava_messaging::api::CALL_BACK_ALERT,
    ];
    let open = alerts(conn, &kinds, (Timestamp::UNIX_EPOCH, window.1), |a| {
        a.resolved_at.is_none()
    });
    let call_backs = open.await?.len();
    let help: Vec<String> = read(open_help_requests(conn).await)?
        .into_iter()
        .map(|(_, due)| due_time(due))
        .collect();
    let (from, to) = window;
    let auto = read(audit_subjects(conn, AUTO_ANSWERED_ACTION, from, to).await)?.len();
    let again = read(audit_subjects(conn, WROTE_AGAIN_ACTION, from, to).await)?.len();
    let trades = read(askhava_intake::api::not_covered_by_trade(conn, window).await)?;
    let total: i64 = trades.iter().map(|t| t.requests).sum();
    let trades = trades
        .iter()
        .map(|t| format!("{}: {}", t.trade, t.requests));
    Ok(vec![
        line("Opt-outs (of those texted)", 2, rates),
        line("Call-backs due", call_backs, Vec::new()),
        line("Help requests open (reply by)", help.len(), help),
        line("HELP messages answered automatically", auto, Vec::new()),
        line("Opted-out numbers that wrote again", again, Vec::new()),
        DigestLine {
            item: "Not covered, by trade",
            count: Some(total),
            refs: trades.collect(),
        },
    ])
}

/// An Arizona due time, as the owner reads it.
fn due_time(at: Timestamp) -> String {
    arizona_time(at)
        .strftime("%a %b %-d, %-I:%M %p")
        .to_string()
}

/// OPS-013, D-4932: the newest test-restore audit row of the two actions, passed or failed.
async fn restore_line(
    conn: &mut PgConnection,
    [passed, failed]: [&str; 2],
) -> Result<DigestLine, DigestError> {
    let ok = read(latest_audit(conn, passed).await)?.map(|a| (a.at, "passed"));
    let bad = read(latest_audit(conn, failed).await)?.map(|a| (a.at, "failed"));
    let refs = ok
        .max(bad)
        .map(|(at, how)| vec![format!("{how} {}", due_time(at))])
        .unwrap_or_default();
    Ok(line("Test restore", refs.len(), refs))
}

/// OPS-030, D-219, D-3880: the digest of `window`, counts and job numbers only.
///
/// # Errors
///
/// [`DigestError::Database`] when a reader fails.
pub async fn weekly_digest(
    conn: &mut PgConnection,
    window: (Timestamp, Timestamp),
    restore_actions: [&str; 2],
) -> Result<Digest, DigestError> {
    let mut lines = business_lines(conn, window).await?;
    lines.extend(alert_lines(conn, window).await?);
    lines.extend(contact_lines(conn, window).await?);
    lines.push(restore_line(conn, restore_actions).await?);
    let (from, to) = window;
    Ok(Digest { from, to, lines })
}

/// The Arizona calendar month `back` months before the one holding `now`, as `[from, to)`.
fn month(now: Timestamp, back: u32) -> Result<(Timestamp, Timestamp), DigestError> {
    let first = arizona_time(now).date().first_of_month();
    let from = read(first.checked_sub(Span::new().months(back)))?;
    let to = read(from.checked_add(Span::new().months(1)))?;
    let at = |d: jiff::civil::Date| read(d.to_zoned(arizona())).map(|z| z.timestamp());
    Ok((at(from)?, at(to)?))
}

/// OPS-027, D-464: whether a trade with `active` businesses and a median of `median_tenths`
/// quotes per job in a calendar month is crowded.
#[must_use]
pub fn is_crowded(active: u32, median_tenths: u32, limits: &OwnerFlagLimits) -> bool {
    active >= limits.crowded_businesses
        && median_tenths >= limits.crowded_median_quotes.saturating_mul(10)
}

/// OPS-031, D-251: whether each of the months (connected jobs, of them NO or OTHER at day 3),
/// at least the setting's number of them, is over the share.
#[must_use]
pub fn is_no_or_other(months: &[(usize, usize)], limits: &OwnerFlagLimits) -> bool {
    let wanted = usize::try_from(limits.no_or_other_months).unwrap_or(usize::MAX);
    let percent = usize::try_from(limits.no_or_other_percent).unwrap_or(usize::MAX);
    months.len() >= wanted
        && months
            .iter()
            .all(|&(connected, no)| connected > 0 && no * 100 > percent * connected)
}

/// OPS-027: the crowded trades last calendar month, with their quotes per win.
async fn crowded(
    conn: &mut PgConnection,
    now: Timestamp,
    limits: &OwnerFlagLimits,
) -> Result<Vec<CrowdedTrade>, DigestError> {
    let last = month(now, 1)?;
    let trades = read(askhava_quotes::api::jobs_by_trade(conn, last.1).await)?;
    let mut out = Vec::new();
    for active in read(trade_active_counts(conn).await)? {
        let Some(t) = trades.iter().find(|t| t.trade == active.trade) else {
            continue;
        };
        let Some(q) = read(askhava_quotes::api::month_quotes(conn, &t.jobs, last).await)? else {
            continue;
        };
        if !is_crowded(active.active, q.median_tenths, limits) {
            continue;
        }
        let wins = read(askhava_picks::api::wins(conn, &t.jobs, last).await)?;
        let per_win = (wins > 0).then(|| q.confirmed_quotes * 10 / wins);
        out.push(CrowdedTrade {
            trade: active.trade,
            active: active.active,
            median_tenths: q.median_tenths,
            quotes_per_win_tenths: per_win,
        });
    }
    Ok(out)
}

/// OPS-031: the trades over the NO-or-OTHER share in each of the last calendar months.
async fn no_or_other(
    conn: &mut PgConnection,
    now: Timestamp,
    limits: &OwnerFlagLimits,
) -> Result<Vec<String>, DigestError> {
    let this_month = month(now, 0)?;
    let trades = read(askhava_quotes::api::jobs_by_trade(conn, this_month.0).await)?;
    let mut out = Vec::new();
    for t in trades {
        let mut months = Vec::new();
        for back in 1..=limits.no_or_other_months {
            let window = month(now, back)?;
            let a = askhava_jobs::api::connected_answers(conn, &t.jobs, window).await;
            let a = read(a)?;
            months.push((a.connected.len(), a.no_or_other.len()));
        }
        if is_no_or_other(&months, limits) {
            out.push(t.trade);
        }
    }
    Ok(out)
}

/// OPS-027, OPS-031, OPS-034, D-284: the owner flags at `now`; reads only.
///
/// # Errors
///
/// [`DigestError::Database`] when a reader fails.
pub async fn owner_flags(
    conn: &mut PgConnection,
    now: Timestamp,
    limits: &OwnerFlagLimits,
) -> Result<OwnerFlags, DigestError> {
    let days = i64::from(limits.window_days);
    let window = (now - SignedDuration::from_hours(days * 24), now);
    let lapsed = askhava_picks::api::lapsed_businesses(conn, window, limits.lapsed_picks.into());
    let lapsed = read(lapsed.await)?;
    let min = i64::from(limits.repeat_requests);
    let repeat = read(askhava_intake::api::repeat_requesters(conn, window, min).await)?;
    Ok(OwnerFlags {
        crowded: crowded(conn, now, limits).await?,
        no_or_other: no_or_other(conn, now, limits).await?,
        lapsed,
        repeat_requests: repeat.into_iter().map(|(_, n)| n).collect(),
    })
}

/// JOB-016, PRIV-006: the kept words of the week's PROBLEM alerts, read under `proof` only.
async fn problem_replies(
    conn: &mut PgConnection,
    proof: &OwnerSessionProof,
    window: (Timestamp, Timestamp),
) -> Result<Vec<ProblemReply>, DigestError> {
    let kinds = [PROBLEM_ALERT, PROBLEM_SAFETY_ALERT];
    let mut out = Vec::new();
    for a in alerts(conn, &kinds, window, |a| a.raised_at >= window.0).await? {
        let Some((INBOX, inbox)) = a.subject.as_ref().map(|(k, id)| (k.as_str(), *id)) else {
            continue;
        };
        let Some(kept) = read(problem_words(conn, proof, inbox).await)? else {
            continue;
        };
        let number = read(job_number(conn, kept.job).await)?;
        out.push(ProblemReply {
            number,
            words: kept.words,
        });
    }
    Ok(out)
}

/// OPS-030, OPS-033, D-5318: the owner digest page's view at `now`: the last week's digest, the
/// flags, the help requests due in the next 4 days and, only with `proof`, the PROBLEM words.
///
/// # Errors
///
/// [`DigestError::Database`] when a reader fails.
pub async fn owner_digest_view(
    conn: &mut PgConnection,
    proof: Option<&OwnerSessionProof>,
    now: Timestamp,
    inputs: &DigestInputs<'_>,
) -> Result<OwnerDigestView, DigestError> {
    let window = (now - DIGEST_WEEK, now);
    let digest = weekly_digest(conn, window, inputs.restore_actions).await?;
    let flags = owner_flags(conn, now, &inputs.limits).await?;
    let due_soon = read(open_help_requests(conn).await)?
        .into_iter()
        .filter(|(_, due)| *due < now + DUE_AHEAD)
        .map(|(_, due)| due_time(due))
        .collect();
    let problem_words = match proof {
        Some(proof) => problem_replies(conn, proof, window).await?,
        None => Vec::new(),
    };
    Ok(OwnerDigestView {
        digest,
        flags,
        due_soon,
        problem_words,
    })
}
