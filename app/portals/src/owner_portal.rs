//! The owner portal's home and settings pages and the changes their forms post (T108; FEE-007,
//! LIC-002, OPS-003, OPS-035, SEC-043, CON-024, BIZ-027; D-1843, D-1871, D-2128, D-3882, D-3890).
//!
//! [`owner_home_page`] shows the pushed alerts with the safe-mode acknowledgement, the pause
//! switches (all, charging, texting and one per trade; no town has a record ID to scope one,
//! D-3181), the fee mode read from `payments.fee_terms`, the homeowner line's texting switch,
//! launch channel and number state (`messaging::api`), the status banner's off switch, each of the
//! last [`OPT_OUT_WEEKS`] Arizona weeks' opt-out rates by audience, the needs-review sign-ups with
//! their decision target and the consent-record retention line. [`owner_settings_page`] holds
//! the category switches. Every form carries the CSRF token and a [`ChangeForm`]; no route
//! constant has a path parameter, record IDs go in the form body (D-3890). [`parse_change`] reads
//! a form for its route and [`apply_owner_change`] makes the change through the owning crate,
//! which logs it, after the SEC-043 fresh factor for the fee mode and every settings change. No
//! page or error holds a contact detail. `server` (T310) serves these behind the owner session.
//!
//! T205 (FEE-026, FEE-050, SEC-006, D-229, D-4506, D-4481): [`owner_refund_page`] is pick P's
//! refund review step, listing only P's disputes (`payments::api::disputes_of`);
//! [`owner_refund`] refunds P once through `request_refund` with [`RefundTrigger::Owner`] (a
//! disputed charge is blocked there), and [`owner_dispute_evidence`] records Casey's own evidence
//! submission for one of P's disputes through `record_pick_owner_submission`, its audit row in the
//! same transaction, refusing another pick's. Both need the SEC-043 fresh factor and audit with
//! the owner as actor and P as subject. [`owner_audit_log_page`] lists the audit log with each
//! row's actor. `server` (T304) serves these.

use askama::Template;
use askhava_businesses::api::{needs_review_signups, review_target_line};
use askhava_catalog::api::{CatalogError, category, category_list, set_category_enabled};
use askhava_messaging::api::{
    Audience, CONSENT_KEEP_YEARS, LaunchChannel, ModeStateError, NumberState, Week, WeekRate,
    alert_percent, launch_channel, load_lines, record_restore, record_suspension,
    set_launch_channel, switch_off, switch_on, weekly_optout_rates,
};
use askhava_ops::api::{
    Actor, AuditEntry, AuditRow, Change, DELIVERY_ALERT, Switch, acknowledge_pushed_alert, audit,
    audit_once, banner_shown, clear_banner, export_audit, is_paused, pushed_alerts, set_switch,
};
use askhava_payments::api::{
    DisputeView, FeeMode, MoneyEventError, OwnerSubmission, PickRefundState, RefundOutcome,
    RefundStripe, RefundTrigger, disputes_of, fee_terms_in_force, record_pick_owner_submission,
    refund_state, request_refund, switch_fee_mode,
};
use jiff::{Timestamp, tz};
use secrecy::SecretString;
use serde::Deserialize;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::owner_auth::{OwnerAction, OwnerAuthError, OwnerPrincipal, require_owner_step_up};
use crate::owner_step_up_page::OWNER_STEP_UP_ROUTE;

/// The owner home page (`GET`).
pub const OWNER_HOME_ROUTE: &str = "/owner/home";
/// The owner settings page (`GET`): the category switches.
pub const OWNER_SETTINGS_ROUTE: &str = "/owner/settings";
/// A pause switch's `POST` (OPS-003).
pub const OWNER_PAUSE_ROUTE: &str = "/owner/pause";
/// The fee-mode switch's `POST` (FEE-007).
pub const OWNER_FEE_MODE_ROUTE: &str = "/owner/fee-mode";
/// A category switch's `POST` (LIC-002).
pub const OWNER_CATEGORY_ROUTE: &str = "/owner/category";
/// The homeowner line's `POST`: texting, launch channel, suspension and restore (D-1843).
pub const OWNER_TEXTING_ROUTE: &str = "/owner/texting";
/// The status banner's off switch `POST` (D-1843).
pub const OWNER_BANNER_ROUTE: &str = "/owner/banner";
/// The safe-mode acknowledgement of a pushed alert `POST` (OPS-001).
pub const OWNER_ALERT_ACK_ROUTE: &str = "/owner/alert-ack";
/// Pick `{pick}`'s refund review step (`GET`, T205; FEE-026, FEE-050, SEC-006, D-3890).
pub const OWNER_REFUND_ROUTE: &str = "/owner/refunds/{pick}";
/// The review step's refund press (`POST`, no body but the CSRF field; FEE-026, SEC-043).
pub const OWNER_REFUND_PRESS_ROUTE: &str = "/owner/refunds/{pick}/refund";
/// The review step's "I submitted this dispute's evidence in Stripe myself" press (`POST`, an
/// [`EvidenceForm`]; FEE-050, D-229, D-4506).
pub const OWNER_EVIDENCE_ROUTE: &str = "/owner/refunds/{pick}/evidence";
/// The audit log viewer (`GET`, `?after=<row ID>`; SEC-006, OPS-010).
pub const OWNER_AUDIT_LOG_ROUTE: &str = "/owner/audit-log";
/// The audit action of the owner's refund press, about the pick (SEC-006, D-074).
pub const OWNER_REFUND_ACTION: &str = "portals.owner_refund";
/// The audit action of the owner's own evidence submission, about the pick (SEC-006, D-4481).
pub const OWNER_EVIDENCE_ACTION: &str = "portals.owner_dispute_evidence";
/// How many audit rows one page of the viewer lists.
pub const AUDIT_PAGE_ROWS: i64 = 100;
/// How many finished Arizona weeks of opt-out rates the home page lists (CON-024).
pub const OPT_OUT_WEEKS: usize = 4;

/// A pause switch the owner turns on or off (OPS-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PauseSwitch {
    /// Everything.
    All,
    /// Charging.
    Charging,
    /// Texting.
    Texting,
    /// One trade, by its category code.
    Trade(String),
}

/// One owner change, parsed from its form (OPS-003, FEE-007, LIC-002, OPS-001, D-1843).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerChange {
    /// Turns a pause switch on (`true`) or off.
    Pause(PauseSwitch, bool),
    /// Switches the fee mode.
    FeeMode(FeeMode),
    /// Switches the category with this code on or off.
    Category(String, bool),
    /// Switches homeowner texting on or off.
    HomeownerTexting(bool),
    /// Sets the homeowner launch channel.
    LaunchChannel(LaunchChannel),
    /// Records the provider's suspension of the homeowner number, from now.
    NumberSuspended,
    /// Records the provider's restore of the homeowner number.
    NumberRestored,
    /// Hides the status banner.
    BannerOff,
    /// Acknowledges the pushed alert with this ID.
    AcknowledgeAlert(Uuid),
}

impl OwnerChange {
    /// The `POST` route the change's form posts to.
    #[must_use]
    pub fn posts_to(&self) -> &'static str {
        match self {
            Self::Pause(..) => OWNER_PAUSE_ROUTE,
            Self::FeeMode(_) => OWNER_FEE_MODE_ROUTE,
            Self::Category(..) => OWNER_CATEGORY_ROUTE,
            Self::BannerOff => OWNER_BANNER_ROUTE,
            Self::AcknowledgeAlert(_) => OWNER_ALERT_ACK_ROUTE,
            _ => OWNER_TEXTING_ROUTE,
        }
    }

    /// SEC-043: the fee mode and every settings change need a fresh factor; a pause or an
    /// acknowledgement does not, so Casey can stop things at once (SEC-006).
    #[must_use]
    pub fn needs_fresh_factor(&self) -> Option<OwnerAction> {
        match self {
            Self::Pause(..) | Self::AcknowledgeAlert(_) => None,
            Self::FeeMode(_) => Some(OwnerAction::FeeMode),
            _ => Some(OwnerAction::Settings),
        }
    }
}

/// The body of every owner change form: what to change, the record or choice it names and, for a
/// switch, `on` or `off`. The CSRF field is read by `server`'s `CsrfForm`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChangeForm {
    /// The change's name, such as `pause_trade`.
    pub change: String,
    /// The category code, alert ID or choice the change names, if any.
    #[serde(default)]
    pub target: String,
    /// `on` or `off` for a switch.
    #[serde(default)]
    pub to: String,
}

/// Why an owner change was not made. Carries no value (PRIV-016).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OwnerToolError {
    /// SEC-043: no passkey or TOTP in the last 5 minutes; nothing changed.
    #[error("a fresh second factor is needed")]
    NeedsFreshFactor,
    /// The record named is unknown, or the change is refused by its rules.
    #[error("the change is refused")]
    Refused,
    /// A database call failed.
    #[error("the owner portal database call failed")]
    Database,
}

fn store<E>(_: E) -> OwnerToolError {
    OwnerToolError::Database
}

/// The change `form` names, or `None` when it is unknown or does not belong to `route`.
#[must_use]
pub fn parse_change(route: &str, form: &ChangeForm) -> Option<OwnerChange> {
    let on = match form.to.as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };
    let target = form.target.trim();
    let change = match form.change.as_str() {
        "pause_all" => OwnerChange::Pause(PauseSwitch::All, on?),
        "pause_charging" => OwnerChange::Pause(PauseSwitch::Charging, on?),
        "pause_texting" => OwnerChange::Pause(PauseSwitch::Texting, on?),
        "pause_trade" if !target.is_empty() => {
            OwnerChange::Pause(PauseSwitch::Trade(target.to_owned()), on?)
        }
        "fee_mode" => OwnerChange::FeeMode(match target {
            "tiers" => FeeMode::Tiers,
            "flat" => FeeMode::Flat,
            _ => return None,
        }),
        "category" if !target.is_empty() => OwnerChange::Category(target.to_owned(), on?),
        "homeowner_texting" => OwnerChange::HomeownerTexting(on?),
        "launch_channel" => OwnerChange::LaunchChannel(match target {
            "sms" => LaunchChannel::Sms,
            "web_and_email" => LaunchChannel::WebAndEmail,
            _ => return None,
        }),
        "number_suspended" => OwnerChange::NumberSuspended,
        "number_restored" => OwnerChange::NumberRestored,
        "banner_off" => OwnerChange::BannerOff,
        "acknowledge" => OwnerChange::AcknowledgeAlert(Uuid::parse_str(target).ok()?),
        _ => return None,
    };
    (change.posts_to() == route).then_some(change)
}

/// OPS-003, FEE-007, LIC-002, OPS-001, SEC-043, D-1843, D-1871: makes `change` for the signed-in
/// owner at `now` through the crate that owns it, which logs it with the owner as actor; true
/// when something changed, false for a repeat. The fee mode and each settings change first need a
/// fresh factor (a refusal is a security event and changes nothing).
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] or
/// [`OwnerToolError::Database`].
pub async fn apply_owner_change(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    change: OwnerChange,
    now: Timestamp,
) -> Result<bool, OwnerToolError> {
    if let Some(action) = change.needs_fresh_factor() {
        let gate = require_owner_step_up(conn, principal, (action, change.posts_to()), now).await;
        match gate {
            Err(OwnerAuthError::NeedsFreshFactor) => return Err(OwnerToolError::NeedsFreshFactor),
            other => other.map_err(store)?,
        }
    }
    let owner = principal.owner();
    let by = (owner, now);
    let home = Audience::Homeowner;
    match change {
        OwnerChange::Pause(switch, on) => pause(conn, switch, on, owner).await,
        OwnerChange::FeeMode(to) => Ok(switch_fee_mode(conn, owner, to)
            .await
            .map_err(store)?
            .is_some()),
        OwnerChange::Category(code, on) => {
            set_category_enabled(conn, &code, on, Actor::Owner(owner))
                .await
                .map_err(|e| refused_or_store(e == CatalogError::Database))
        }
        OwnerChange::HomeownerTexting(true) => texting(switch_on(conn, home, by).await),
        OwnerChange::HomeownerTexting(false) => texting(switch_off(conn, home, by).await),
        OwnerChange::LaunchChannel(to) => texting(set_launch_channel(conn, home, to, by).await),
        OwnerChange::NumberSuspended => texting(record_suspension(conn, home, now, by).await),
        OwnerChange::NumberRestored => texting(record_restore(conn, home, by).await),
        OwnerChange::BannerOff => clear_banner(conn, owner, now).await.map_err(store),
        OwnerChange::AcknowledgeAlert(alert) => acknowledge_pushed_alert(conn, alert, owner, now)
            .await
            .map_err(store),
    }
}

fn refused_or_store(database: bool) -> OwnerToolError {
    if database {
        OwnerToolError::Database
    } else {
        OwnerToolError::Refused
    }
}

fn texting(r: Result<bool, ModeStateError>) -> Result<bool, OwnerToolError> {
    r.map_err(|e| refused_or_store(e == ModeStateError::Database))
}

/// OPS-003: flips one pause switch, a trade's scoped by its stored category's record ID.
async fn pause(
    conn: &mut PgConnection,
    switch: PauseSwitch,
    on: bool,
    owner: Uuid,
) -> Result<bool, OwnerToolError> {
    let switch = match switch {
        PauseSwitch::All => Switch::All,
        PauseSwitch::Charging => Switch::Charging,
        PauseSwitch::Texting => Switch::Texting,
        PauseSwitch::Trade(code) => {
            let stored = category(conn, &code).await.map_err(store)?;
            Switch::Trade(stored.ok_or(OwnerToolError::Refused)?.id)
        }
    };
    set_switch(conn, switch, on, Actor::Owner(owner))
        .await
        .map_err(store)
}

/// One row of a switch list: its form target, its label and whether it is on.
#[derive(Debug, Clone)]
struct SwitchRow {
    change: &'static str,
    target: String,
    label: String,
    on: bool,
}

/// One pushed alert as listed (IDs, code names and Arizona times only).
#[derive(Debug, Clone)]
struct AlertRow {
    id: String,
    kind: String,
    severity: String,
    subject: Option<String>,
    raised: String,
    pushed: String,
    acknowledged: Option<String>,
    /// OPS-025, D-4283: a text-delivery alert, which only the delivery path acknowledges, so the
    /// page offers no button for it.
    delivery: bool,
}

/// One audience's week of opt-outs (CON-024).
#[derive(Debug, Clone)]
struct RateRow {
    week: String,
    audience: &'static str,
    line: String,
    alerts: bool,
    limit: i64,
}

/// One needs-review sign-up and its target line (BIZ-027).
#[derive(Debug, Clone)]
struct ReviewRow {
    business: String,
    target: String,
}

#[derive(Template)]
#[template(path = "owner_home.html")]
struct HomeView<'a> {
    csrf: &'a str,
    routes: [&'static str; 7],
    alerts: Vec<AlertRow>,
    pauses: Vec<SwitchRow>,
    flat: bool,
    texting_on: bool,
    sms_launch: bool,
    number: &'static str,
    suspended: bool,
    banner: bool,
    rates: Vec<RateRow>,
    reviews: Vec<ReviewRow>,
    keep_years: i16,
}

#[derive(Template)]
#[template(path = "owner_settings.html")]
struct SettingsView<'a> {
    csrf: &'a str,
    category_route: &'static str,
    step_up_route: &'static str,
    home_route: &'static str,
    categories: Vec<SwitchRow>,
}

fn arizona(at: Timestamp) -> String {
    at.to_zoned(tz::TimeZone::fixed(tz::offset(-7)))
        .strftime("%b %-d, %-I:%M %p")
        .to_string()
}

/// CON-024: the rate line of one audience's week, such as "3 of 100 texted opted out (3.0%)".
fn rate_row(week: Week, rate: WeekRate) -> RateRow {
    let line = if rate.texted == 0 {
        "No texts sent.".to_owned()
    } else {
        let tenths = rate.opted_out * 1000 / rate.texted;
        format!(
            "We texted {}; {} opted out ({}.{}%).",
            rate.texted,
            rate.opted_out,
            tenths / 10,
            tenths % 10
        )
    };
    RateRow {
        week: format!("Week of {}", week.monday().strftime("%b %-d")),
        audience: match rate.audience {
            Audience::Homeowner => "Homeowners",
            Audience::Business => "Businesses",
        },
        line,
        alerts: rate.alerts(),
        limit: alert_percent(rate.audience),
    }
}

/// CON-024: both audiences' rates for each of the last [`OPT_OUT_WEEKS`] whole weeks before `now`.
async fn rates(conn: &mut PgConnection, now: Timestamp) -> Result<Vec<RateRow>, OwnerToolError> {
    let mut week = Week::containing(now);
    let mut rows = Vec::new();
    for _ in 0..OPT_OUT_WEEKS {
        week = week.previous();
        let r = weekly_optout_rates(conn, week).await.map_err(store)?;
        rows.push(rate_row(week, r.homeowners));
        rows.push(rate_row(week, r.businesses));
    }
    Ok(rows)
}

async fn alert_rows(conn: &mut PgConnection) -> Result<Vec<AlertRow>, OwnerToolError> {
    let alerts = pushed_alerts(conn).await.map_err(store)?;
    Ok(alerts
        .into_iter()
        .map(|a| AlertRow {
            id: a.id.to_string(),
            delivery: a.subject.kind == DELIVERY_ALERT,
            subject: a
                .subject
                .subject_kind
                .zip(a.subject.subject_id)
                .map(|(k, id)| format!("{k} {id}")),
            kind: a.subject.kind,
            severity: a.subject.severity,
            raised: arizona(a.raised_at),
            pushed: arizona(a.pushed_at),
            acknowledged: a.acknowledged_at.map(arizona),
        })
        .collect())
}

/// OPS-003: the all, charging and texting switches, then one per switched-on trade.
async fn pause_rows(conn: &mut PgConnection) -> Result<Vec<SwitchRow>, OwnerToolError> {
    let mut rows = Vec::new();
    for (change, label, switch) in [
        ("pause_all", "everything", Switch::All),
        ("pause_charging", "charging", Switch::Charging),
        ("pause_texting", "texting", Switch::Texting),
    ] {
        let on = is_paused(conn, switch).await.map_err(store)?;
        let target = String::new();
        rows.push(SwitchRow {
            change,
            target,
            label: label.to_owned(),
            on,
        });
    }
    let list = category_list(conn).await.map_err(store)?;
    for listing in list.categories.into_iter().filter(|c| c.enabled) {
        let Some(stored) = category(conn, &listing.code).await.map_err(store)? else {
            continue;
        };
        let on = is_paused(conn, Switch::Trade(stored.id))
            .await
            .map_err(store)?;
        let label = listing.name;
        rows.push(SwitchRow {
            change: "pause_trade",
            target: listing.code,
            label,
            on,
        });
    }
    Ok(rows)
}

async fn review_rows(
    conn: &mut PgConnection,
    review_days: u32,
) -> Result<Vec<ReviewRow>, OwnerToolError> {
    let waiting = needs_review_signups(conn).await.map_err(store)?;
    Ok(waiting
        .into_iter()
        .map(|w| ReviewRow {
            business: w.business.to_string(),
            target: review_target_line(w.review_since, review_days),
        })
        .collect())
}

/// The owner home page at `now`, its forms carrying `csrf`; `review_days` is the BIZ-027 setting
/// (`SignupSettings::review_target_days`). Reads only; writes nothing (OPS-003, OPS-035, CON-024,
/// BIZ-027, FEE-007, D-1843).
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read or render.
pub async fn owner_home_page(
    conn: &mut PgConnection,
    csrf: &str,
    review_days: u32,
    now: Timestamp,
) -> Result<String, OwnerToolError> {
    let terms = fee_terms_in_force(conn).await.map_err(store)?;
    let lines = load_lines(conn, (None, None)).await.map_err(store)?;
    let launch = launch_channel(conn, Audience::Homeowner)
        .await
        .map_err(store)?;
    HomeView {
        csrf,
        routes: [
            OWNER_ALERT_ACK_ROUTE,
            OWNER_PAUSE_ROUTE,
            OWNER_FEE_MODE_ROUTE,
            OWNER_TEXTING_ROUTE,
            OWNER_BANNER_ROUTE,
            OWNER_SETTINGS_ROUTE,
            OWNER_STEP_UP_ROUTE,
        ],
        alerts: alert_rows(conn).await?,
        pauses: pause_rows(conn).await?,
        flat: terms.mode() == FeeMode::Flat,
        texting_on: lines.homeowner.texting_on,
        sms_launch: launch == LaunchChannel::Sms,
        number: match lines.homeowner.state {
            NumberState::Working => "working",
            NumberState::Suspended { .. } => "suspended by the text provider",
            NumberState::Restored { .. } => "restored by the text provider",
        },
        suspended: matches!(lines.homeowner.state, NumberState::Suspended { .. }),
        banner: banner_shown(conn).await.map_err(store)?,
        rates: rates(conn, now).await?,
        reviews: review_rows(conn, review_days).await?,
        keep_years: CONSENT_KEEP_YEARS,
    }
    .render()
    .map_err(store)
}

/// LIC-002: the owner settings page, one switch per offered category, its forms carrying `csrf`.
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read or render.
pub async fn owner_settings_page(
    conn: &mut PgConnection,
    csrf: &str,
) -> Result<String, OwnerToolError> {
    let list = category_list(conn).await.map_err(store)?;
    let categories = list
        .categories
        .into_iter()
        .map(|c| SwitchRow {
            change: "category",
            label: c.name,
            target: c.code,
            on: c.enabled,
        })
        .collect();
    SettingsView {
        csrf,
        category_route: OWNER_CATEGORY_ROUTE,
        step_up_route: OWNER_STEP_UP_ROUTE,
        home_route: OWNER_HOME_ROUTE,
        categories,
    }
    .render()
    .map_err(store)
}

/// The body of the evidence press: the `dp_` ID of the dispute it names. The pick comes from the
/// route's `{pick}`; the CSRF field is read by `server`'s `CsrfForm` (D-4506).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EvidenceForm {
    /// The dispute's Stripe ID.
    #[serde(default)]
    pub dispute: String,
}

/// What the review step says above its lists after a press (FEE-026, FEE-050, SEC-043).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewNotice {
    /// A refund press came to this.
    Refund(RefundOutcome),
    /// An evidence press came to this.
    Evidence(OwnerSubmission),
    /// A press without a code from the last 5 minutes: nothing changed.
    NeedsFreshFactor,
}

fn route_of(route: &str, pick: Uuid) -> String {
    route.replace("{pick}", &pick.to_string())
}

fn money_error(e: MoneyEventError) -> OwnerToolError {
    refused_or_store(e != MoneyEventError::NothingCharged)
}

/// SEC-043: the fresh-factor gate of a refund-step press on `route`.
async fn refund_gate(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    route: &str,
    now: Timestamp,
) -> Result<(), OwnerToolError> {
    match require_owner_step_up(conn, principal, (OwnerAction::Refund, route), now).await {
        Err(OwnerAuthError::NeedsFreshFactor) => Err(OwnerToolError::NeedsFreshFactor),
        other => other.map_err(store),
    }
}

/// FEE-026, SEC-006, SEC-043, D-3883: after a fresh factor, refunds `pick`'s charge in full once
/// through `payments::api::request_refund` with [`RefundTrigger::Owner`]; a replayed press sends
/// nothing and books no second ledger row, and a disputed charge is `Blocked` there. One owner
/// audit row about the pick, written once however often it is pressed.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] when nothing was charged,
/// or [`OwnerToolError::Database`].
pub async fn owner_refund(
    pool: &PgPool,
    (stripe, secret): (&impl RefundStripe, &SecretString),
    principal: &OwnerPrincipal,
    pick: Uuid,
    now: Timestamp,
) -> Result<RefundOutcome, OwnerToolError> {
    let mut conn = pool.acquire().await.map_err(store)?;
    let route = route_of(OWNER_REFUND_PRESS_ROUTE, pick);
    refund_gate(&mut conn, principal, &route, now).await?;
    let outcome = request_refund(pool, stripe, secret, (pick, RefundTrigger::Owner), now)
        .await
        .map_err(money_error)?;
    let entry = owner_entry(principal, OWNER_REFUND_ACTION, pick, Change::Action);
    audit_once(&mut conn, &entry).await.map_err(store)?;
    Ok(outcome)
}

fn owner_entry(
    principal: &OwnerPrincipal,
    action: &'static str,
    pick: Uuid,
    change: Change,
) -> AuditEntry {
    AuditEntry {
        actor: Actor::Owner(principal.owner()),
        action,
        subject_kind: "picks.pick",
        subject_id: pick,
        change,
    }
}

/// FEE-050, D-229, SEC-006, D-4506: after a fresh factor, records that Casey submitted the
/// evidence of `pick`'s own dispute `form.dispute` in Stripe himself, through
/// `payments::api::record_pick_owner_submission`. `Recorded` writes one owner audit row about the
/// pick in the same transaction; `AlreadyIn` writes nothing more; `NotThisPick` (another pick's
/// dispute, or none) writes nothing at all and the page shows a plain error.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`] or [`OwnerToolError::Database`].
pub async fn owner_dispute_evidence(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (pick, form): (Uuid, &EvidenceForm),
    now: Timestamp,
) -> Result<OwnerSubmission, OwnerToolError> {
    let route = route_of(OWNER_EVIDENCE_ROUTE, pick);
    refund_gate(conn, principal, &route, now).await?;
    let mut tx = conn.begin().await.map_err(store)?;
    let answer = record_pick_owner_submission(&mut tx, pick, form.dispute.trim(), now)
        .await
        .map_err(money_error)?;
    if answer == OwnerSubmission::Recorded {
        let change = Change::Field("submitted_at");
        let entry = owner_entry(principal, OWNER_EVIDENCE_ACTION, pick, change);
        audit(&mut tx, &entry).await.map_err(store)?;
    }
    tx.commit().await.map_err(store)?;
    Ok(answer)
}

/// One of the pick's disputes as the review step lists it (FEE-050).
#[derive(Debug, Clone)]
struct DisputeRow {
    id: String,
    amount: String,
    due: String,
    fee: String,
    status: &'static str,
    open: bool,
}

fn dollars(cents: i64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

fn dispute_row(d: DisputeView) -> DisputeRow {
    let status = match (d.submitted_at, d.auto_submit) {
        (Some(_), _) => "The evidence is in.",
        (None, true) => "We send the evidence 2 days before the due date.",
        (None, false) => "You need to counter or accept this one in Stripe.",
    };
    DisputeRow {
        open: d.submitted_at.is_none(),
        amount: dollars(d.amount_cents),
        due: arizona(d.due_at),
        fee: dollars(d.fee_at_stake_cents),
        status,
        id: d.dispute,
    }
}

#[derive(Template)]
#[template(path = "owner_refund.html")]
struct RefundView<'a> {
    csrf: &'a str,
    pick: String,
    refund_route: String,
    evidence_route: String,
    step_up_route: &'static str,
    state: &'static str,
    may_refund: bool,
    notice: Option<(&'static str, bool)>,
    step_up: bool,
    disputes: Vec<DisputeRow>,
}

/// The notice line and whether it is an error.
fn notice_line(notice: ReviewNotice) -> (&'static str, bool) {
    match notice {
        ReviewNotice::Refund(RefundOutcome::Refunded(_)) => ("Refunded in full.", false),
        ReviewNotice::Refund(RefundOutcome::Unchanged) => (
            "Nothing new was sent. A refund was already done or is on its way.",
            false,
        ),
        ReviewNotice::Refund(RefundOutcome::Blocked) => (
            "Error: This charge is disputed, so it can't be refunded. Nothing was sent.",
            true,
        ),
        ReviewNotice::Refund(RefundOutcome::Failed) => (
            "Not refunded yet. We'll try again every hour and let you know.",
            false,
        ),
        ReviewNotice::Evidence(OwnerSubmission::Recorded) => ("Marked as sent by you.", false),
        ReviewNotice::Evidence(OwnerSubmission::AlreadyIn) => {
            ("That evidence is already in.", false)
        }
        ReviewNotice::Evidence(OwnerSubmission::NotThisPick) => (
            "Error: That dispute is not on this charge. Nothing was changed.",
            true,
        ),
        ReviewNotice::NeedsFreshFactor => (
            "Error: You need a code from the last 5 minutes. Nothing was changed.",
            true,
        ),
    }
}

/// FEE-026: where the refund stands, and whether the refund press is offered.
fn state_line(state: PickRefundState) -> (&'static str, bool) {
    match state {
        PickRefundState::NoRefund => ("No refund yet.", true),
        PickRefundState::InFlight => (
            "A refund is on its way. Stripe has not said it went through.",
            false,
        ),
        PickRefundState::Succeeded => ("Refunded in full.", false),
        PickRefundState::Failed => (
            "The last refund didn't go through. We'll try again every hour.",
            true,
        ),
        PickRefundState::Blocked => ("This charge is disputed, so it can't be refunded.", false),
    }
}

/// FEE-026, FEE-050, SEC-006, D-4506: pick `pick`'s refund review step, its forms carrying `csrf`:
/// where its refund stands, the refund press unless the charge is disputed or refunded or a try is
/// on its way, and only this pick's own disputes with the fee at stake, each not yet in offering
/// "I submitted this dispute's evidence in Stripe myself". Reads only.
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read or render.
pub async fn owner_refund_page(
    conn: &mut PgConnection,
    csrf: &str,
    pick: Uuid,
    notice: Option<ReviewNotice>,
) -> Result<String, OwnerToolError> {
    let state = refund_state(conn, pick).await.map_err(store)?;
    let disputes = disputes_of(conn, pick).await.map_err(store)?;
    let (state, may_refund) = state_line(state);
    RefundView {
        csrf,
        pick: pick.to_string(),
        refund_route: route_of(OWNER_REFUND_PRESS_ROUTE, pick),
        evidence_route: route_of(OWNER_EVIDENCE_ROUTE, pick),
        step_up_route: OWNER_STEP_UP_ROUTE,
        may_refund,
        state,
        step_up: notice == Some(ReviewNotice::NeedsFreshFactor),
        notice: notice.map(notice_line),
        disputes: disputes.into_iter().map(dispute_row).collect(),
    }
    .render()
    .map_err(store)
}

/// One audit row as the viewer lists it: IDs, code names and an Arizona time only (SEC-006).
#[derive(Debug, Clone)]
struct LogRow {
    at: String,
    actor: String,
    action: String,
    subject: String,
    change: String,
}

fn log_row(r: AuditRow) -> LogRow {
    let actor = match (r.actor_kind.as_str(), r.actor_id) {
        ("system", _) => "System".to_owned(),
        ("owner", _) => "Owner".to_owned(),
        (kind, Some(id)) => format!("{kind} {id}"),
        (kind, None) => kind.to_owned(),
    };
    let change = match (r.from_status, r.to_status, r.field) {
        (Some(from), Some(to), _) => format!("{from} to {to}"),
        (_, _, Some(field)) => field,
        _ => String::new(),
    };
    LogRow {
        at: arizona(r.at),
        actor,
        action: r.action,
        subject: format!("{} {}", r.subject_kind, r.subject_id),
        change,
    }
}

#[derive(Template)]
#[template(path = "owner_audit_log.html")]
struct AuditLogView {
    rows: Vec<LogRow>,
    next: Option<String>,
}

/// SEC-006, OPS-010: the audit log viewer, up to [`AUDIT_PAGE_ROWS`] rows after row `after`,
/// oldest first, each with its actor; a full page links to the next. Reads only.
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read or render.
pub async fn owner_audit_log_page(
    conn: &mut PgConnection,
    after: i64,
) -> Result<String, OwnerToolError> {
    let rows = export_audit(conn, after, AUDIT_PAGE_ROWS)
        .await
        .map_err(store)?;
    let full = i64::try_from(rows.len()).is_ok_and(|n| n == AUDIT_PAGE_ROWS);
    let next = rows
        .last()
        .filter(|_| full)
        .map(|r| format!("{OWNER_AUDIT_LOG_ROUTE}?after={}", r.id));
    AuditLogView {
        rows: rows.into_iter().map(log_row).collect(),
        next,
    }
    .render()
    .map_err(store)
}
