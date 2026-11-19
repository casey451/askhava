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
//!
//! T484 (OPS-028, OPS-035, CON-035, PICK-048; D-3753, D-3890): each pushed critical alert about
//! an inbound text offers the bridge press ([`owner_bridge_press`]), which only queues one
//! `calls::api::OWNER_BRIDGE_KIND` task (alert and press IDs, one per press) for `server`'s
//! handler. The settings page records an opt-out a homeowner asked for by any other contact
//! ([`owner_record_opt_out`], `messaging::api::suppress` with `RevokeChannel::Owner`, confirmed on
//! screen, never by text) and files Stripe's written answer ([`owner_file_stripe_answer`],
//! `quotes::api::file_stripe_answer`), showing the newest filing. All three need the SEC-043 fresh
//! factor. `server` (T310) serves their `POST`s.
//!
//! T407 (CALL-019, CALL-022, SEC-043, D-275, D-384, D-3134): the settings page shows the stored
//! "safety voice call" switch, off at launch, with the attorney approval in force while on.
//! [`owner_safety_call`] turns it on (with the approval's date and reference) or off through
//! `calls::api`, each after the SEC-043 fresh factor; a refusal names what is missing: the
//! approval, or the CALL-022 items the given `Conditions` leave false (production passes
//! `calls::api::SAFETY_CONDITIONS`).
//!
//! T480 (TXT-039, TXT-043, SEC-043, D-427, D-3753, D-3844): the settings page shows the stored
//! "Spanish conversations" switch, off at launch, with the review and campaign note in force while
//! on, and lists each Spanish template's current version for the review record.
//! [`owner_spanish`] turns it on (with the review's date, reviewer and versions, the campaign
//! note's date and reference and the Spanish sample) or off through `messaging::api`, each after
//! the SEC-043 fresh factor; a refusal names each missing record.

use askama::Template;
use askhava_businesses::api::{needs_review_signups, review_target_line};
use askhava_calls::api::{
    Conditions, OWNER_BRIDGE_KIND, SwitchChange, safety_approval, turn_safety_switch_off,
    turn_safety_switch_on, unmet_conditions,
};
use askhava_catalog::api::{CatalogError, category, category_list, set_category_enabled};
use askhava_core::api::{NewTask, brand, enqueue};
use askhava_messaging::api::{
    Audience, CONSENT_KEEP_YEARS, LaunchChannel, ModeStateError, NumberState, OutboxError,
    RevokeChannel, Week, WeekRate, alert_percent, launch_channel, load_lines, record_restore,
    record_suspension, set_launch_channel, suppress, switch_off, switch_on, weekly_optout_rates,
};
use askhava_messaging::api::{
    Missing, SpanishChange, SpanishRecords, spanish_switch, spanish_template_versions,
    turn_spanish_off, turn_spanish_on,
};
use askhava_ops::api::{
    Actor, AuditEntry, AuditRow, Change, DELIVERY_ALERT, Switch, acknowledge_pushed_alert,
    alert_subject, audit, audit_once, banner_shown, clear_banner, export_audit, is_paused,
    pushed_alerts, set_switch,
};
use askhava_payments::api::{
    DisputeView, FeeMode, MoneyEventError, OwnerSubmission, PickRefundState, RefundOutcome,
    RefundStripe, RefundTrigger, disputes_of, fee_terms_in_force, record_pick_owner_submission,
    refund_state, request_refund, switch_fee_mode,
};
use askhava_privacy::api::Vault;
use askhava_quotes::api::{AutoConfirmError, file_stripe_answer, latest_stripe_answer};
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
/// OPS-028: the bridge press for one pushed critical alert (alert and press IDs in the body).
pub const OWNER_BRIDGE_ROUTE: &str = "/owner/call-homeowner";
/// CON-035: the owner-recorded opt-out (the number in the body).
pub const OWNER_OPT_OUT_ROUTE: &str = "/owner/opt-out";
/// PICK-048: the Stripe written answer filing.
pub const OWNER_STRIPE_ANSWER_ROUTE: &str = "/owner/stripe-answer";
/// CALL-019, CALL-022, SEC-043: the safety voice call switch (`to` and the approval in the body).
pub const OWNER_SAFETY_CALL_ROUTE: &str = "/owner/safety-call";
/// The "Spanish conversations" switch `POST` (TXT-039, T480).
pub const OWNER_SPANISH_ROUTE: &str = "/owner/spanish-conversations";
/// OPS-028: the audit action of one queued bridge press, subject the alert.
pub const OWNER_BRIDGE_ACTION: &str = "portals.owner_bridge_press";
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
        fresh_factor(conn, principal, (action, change.posts_to()), now).await?;
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

/// SEC-043: the owner's fresh second factor for `action` on `route`; a refusal is a security event
/// and changes nothing.
async fn fresh_factor(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (action, route): (OwnerAction, &str),
    now: Timestamp,
) -> Result<(), OwnerToolError> {
    match require_owner_step_up(conn, principal, (action, route), now).await {
        Err(OwnerAuthError::NeedsFreshFactor) => Err(OwnerToolError::NeedsFreshFactor),
        other => other.map_err(store),
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
    /// OPS-028: a critical alert about an inbound text, which offers the bridge press.
    bridge: bool,
    /// A fresh press ID for this render's bridge form, so a resubmitted form queues nothing more.
    press: String,
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
    brand: &'a str,
    routes: [&'static str; 8],
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
    notice: Option<&'static str>,
    opt_out_route: &'static str,
    stripe_route: &'static str,
    stripe: Option<StripeRow>,
    safety_route: &'static str,
    safety: Option<(String, String)>,
    unmet: Vec<&'static str>,
    spanish_route: &'static str,
    spanish: Option<SpanishRow>,
    spanish_missing: Vec<&'static str>,
    spanish_versions: Vec<(String, String)>,
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
            bridge: bridgeable(&a.subject.severity, a.subject.subject_kind.as_deref()),
            press: Uuid::new_v4().to_string(),
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
            OWNER_BRIDGE_ROUTE,
        ],
        brand: brand().display_name(),
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
    owner_settings_page_with(conn, csrf, None).await
}

/// LIC-002, CON-035, PICK-048, D-3753: the owner settings page with the on-screen confirmation of
/// the change just made, if any: the category switches, the opt-out form, the Stripe-answer form
/// and the newest filed answer, and the safety voice call switch (CALL-019, T407).
///
/// # Errors
///
/// [`OwnerToolError::Database`] for a failed read or render.
pub async fn owner_settings_page_with(
    conn: &mut PgConnection,
    csrf: &str,
    done: Option<SettingsNotice>,
) -> Result<String, OwnerToolError> {
    let stripe = latest_stripe_answer(conn).await.map_err(store)?;
    let safety = safety_approval(conn).await.map_err(store)?;
    let spanish = spanish_switch(conn).await.map_err(store)?;
    let versions = spanish_template_versions().map_err(store)?;
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
        notice: done.map(SettingsNotice::line),
        safety_route: OWNER_SAFETY_CALL_ROUTE,
        safety: safety.map(|a| (a.date.strftime("%B %-d, %Y").to_string(), a.reference)),
        unmet: done.map(SettingsNotice::unmet).unwrap_or_default(),
        spanish_route: OWNER_SPANISH_ROUTE,
        spanish: spanish.in_force().map(|f| SpanishRow {
            reviewed_on: f.reviewed_on.strftime("%B %-d, %Y").to_string(),
            reviewer: f.reviewer.clone(),
            note_on: f.note_on.strftime("%B %-d, %Y").to_string(),
            note_reference: f.note_reference.clone(),
        }),
        spanish_missing: done
            .map(SettingsNotice::spanish_missing)
            .unwrap_or_default(),
        spanish_versions: versions.into_iter().collect(),
        opt_out_route: OWNER_OPT_OUT_ROUTE,
        stripe_route: OWNER_STRIPE_ANSWER_ROUTE,
        stripe: stripe.map(|a| StripeRow {
            answered_on: a.answered_on,
            reference: a.reference,
            allows: a.allows,
            filed: arizona(a.filed_at),
        }),
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
    standing: &'static str,
    open: bool,
}

fn dollars(cents: i64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

fn dispute_row(d: DisputeView) -> DisputeRow {
    let standing = match (d.submitted_at, d.auto_submit) {
        (Some(_), _) => "The evidence is in.",
        (None, true) => "We send the evidence 2 days before the due date.",
        (None, false) => "You need to counter or accept this one in Stripe.",
    };
    DisputeRow {
        open: d.submitted_at.is_none(),
        amount: dollars(d.amount_cents),
        due: arizona(d.due_at),
        fee: dollars(d.fee_at_stake_cents),
        standing,
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

/// OPS-028: whether an alert of `severity` about a `subject_kind` record offers the bridge press:
/// a critical alert about an inbound text, whose sender the bridge can dial.
fn bridgeable(severity: &str, subject_kind: Option<&str>) -> bool {
    severity == "critical" && subject_kind == Some("messaging.inbox")
}

/// The newest filed Stripe written answer as shown (PICK-048).
#[derive(Debug, Clone)]
struct StripeRow {
    answered_on: String,
    reference: String,
    allows: bool,
    filed: String,
}

/// The settings page's on-screen confirmation of the change just made (CON-035, PICK-048).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsNotice {
    /// The number is now on the suppression list.
    OptOutListed,
    /// The number was already on the list; nothing changed.
    OptOutAlready,
    /// Stripe's written answer was filed.
    StripeAnswerFiled,
    /// The safety voice call is now on (or already was).
    SafetyCallOn,
    /// The safety voice call is now off (or already was).
    SafetyCallOff,
    /// Not turned on: no approval date and reference (CALL-019).
    SafetyCallNoApproval,
    /// Not turned on: these CALL-022 items are not met.
    SafetyCallUnmet(Conditions),
    /// Spanish conversations are now on, with the records given.
    SpanishOn,
    /// Spanish conversations are now off (or already were).
    SpanishOff,
    /// Spanish conversations not turned on: these records are missing (TXT-039, TXT-043).
    SpanishMissing(SpanishGaps),
}

impl SettingsNotice {
    /// The confirmation line.
    fn line(self) -> &'static str {
        match self {
            Self::OptOutListed => "Done. We won't text or auto-call that number again.",
            Self::OptOutAlready => "That number was already off our texts. Nothing changed.",
            Self::StripeAnswerFiled => "Saved. Stripe's answer is below.",
            Self::SafetyCallOn => "Saved. The safety voice call is on.",
            Self::SafetyCallOff => "Saved. The safety voice call is off.",
            Self::SafetyCallNoApproval => {
                "Not turned on. Enter the date and reference of your attorney's written approval."
            }
            Self::SafetyCallUnmet(_) => {
                "Not turned on. These parts of the safety call aren't built and tested yet:"
            }
            Self::SpanishOn => "Saved. Spanish conversations are on.",
            Self::SpanishOff => "Saved. Spanish conversations are off.",
            Self::SpanishMissing(_) => {
                "Spanish conversations are still off. Here's what's missing:"
            }
        }
    }

    /// Whether the change was refused, so `server` answers 400 (CALL-019, CALL-022).
    #[must_use]
    pub fn refused(self) -> bool {
        matches!(
            self,
            Self::SafetyCallNoApproval | Self::SafetyCallUnmet(_) | Self::SpanishMissing(_)
        )
    }

    /// TXT-039, TXT-043: the missing records a Spanish refusal names, in words; empty otherwise.
    fn spanish_missing(self) -> Vec<&'static str> {
        let Self::SpanishMissing(gaps) = self else {
            return Vec::new();
        };
        gaps.items().into_iter().map(missing_words).collect()
    }

    /// CALL-022: the unmet items a refusal names, in words; empty for any other notice.
    fn unmet(self) -> Vec<&'static str> {
        let Self::SafetyCallUnmet(met) = self else {
            return Vec::new();
        };
        unmet_conditions(&met)
            .into_iter()
            .map(condition_words)
            .collect()
    }
}

/// CALL-022: one condition's code name in words for the settings page.
fn condition_words(code: &'static str) -> &'static str {
    match code {
        "narrowed_trigger" => {
            "Only gas, carbon monoxide, smoke, fire, burning, spark and shock words start a call."
        }
        "recent_only" => {
            "A call goes only to the number that texted, within 30 minutes of the text."
        }
        "script" => {
            "The recorded message runs under 30 seconds. It says to press 9 to stop our calls."
        }
        "other_number" => "The call comes from a number other than our main number.",
        "list_checked" => "Before every call, we check the person hasn't asked us to stop.",
        "consent_text" => "From the first day the call is on, the welcome text mentions it.",
        "logged" => {
            "Each call is logged: the text that started it, the message version, the time, the result and any key pressed."
        }
        other => other,
    }
}

/// The records a Spanish turn-on lacked, as a set that is `Copy` (TXT-039, TXT-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanishGaps(u8);

/// Every [`Missing`] item in the order the page names them.
const MISSING_ORDER: [Missing; 6] = [
    Missing::ReviewDate,
    Missing::Reviewer,
    Missing::ReviewedTemplates,
    Missing::NoteDate,
    Missing::NoteReference,
    Missing::Sample,
];

impl SpanishGaps {
    /// The set holding `items`.
    #[must_use]
    pub fn of(items: &[Missing]) -> Self {
        let bits = MISSING_ORDER
            .iter()
            .enumerate()
            .filter(|(_, m)| items.contains(m))
            .fold(0u8, |acc, (i, _)| acc | (1 << i));
        Self(bits)
    }

    /// The items in the set, in the page's order.
    #[must_use]
    pub fn items(self) -> Vec<Missing> {
        MISSING_ORDER
            .iter()
            .enumerate()
            .filter(|(i, _)| self.0 & (1 << i) != 0)
            .map(|(_, m)| *m)
            .collect()
    }
}

/// TXT-039, TXT-043: one missing record in words for the settings page.
fn missing_words(item: Missing) -> &'static str {
    match item {
        Missing::ReviewDate => "The date of the professional translation review.",
        Missing::Reviewer => "The reviewer's name (200 characters at most).",
        Missing::ReviewedTemplates => {
            "A review that lists every Spanish text below at the version shown. A text added or changed since the review needs a new one."
        }
        Missing::NoteDate => {
            "The date you updated the campaign description to say texts are in English or Spanish."
        }
        Missing::NoteReference => {
            "The reference for that campaign update (200 characters at most)."
        }
        Missing::Sample => "A Spanish sample message (1,600 characters at most).",
    }
}

/// The Spanish records in force as shown.
#[derive(Debug, Clone)]
struct SpanishRow {
    reviewed_on: String,
    reviewer: String,
    note_on: String,
    note_reference: String,
}

/// The "Spanish conversations" form's body: `on` or `off`, and for `on` the review's date
/// (`YYYY-MM-DD`), reviewer and versions (one `<template ID> <version>` per line), the campaign
/// note's date and reference, and the Spanish sample (TXT-039, TXT-043).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpanishForm {
    /// `on` or `off`.
    pub to: String,
    /// The review's date.
    pub reviewed_on: String,
    /// The reviewer.
    pub reviewer: String,
    /// The reviewed versions, one `<template ID> <version>` per line.
    pub versions: String,
    /// The campaign note's date.
    pub note_on: String,
    /// The campaign note's reference.
    pub note_reference: String,
    /// The Spanish sample message.
    pub sample: String,
}

impl SpanishForm {
    /// The records the form gives; a date that does not parse is missing.
    fn records(&self) -> SpanishRecords {
        let date = |d: &str| d.trim().parse::<jiff::civil::Date>().ok();
        let reviewed = self
            .versions
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                let (id, version) = (words.next()?, words.next()?);
                Some((id.to_owned(), version.to_ascii_lowercase()))
            })
            .collect();
        SpanishRecords {
            reviewed_on: date(&self.reviewed_on),
            reviewer: self.reviewer.clone(),
            reviewed,
            note_on: date(&self.note_on),
            note_reference: self.note_reference.clone(),
            sample: self.sample.clone(),
        }
    }
}

/// TXT-039, TXT-043, SEC-043, D-427: after the fresh factor, turns Spanish conversations on (only
/// with every record `messaging::api::turn_spanish_on` asks for) or off, which stores one row and
/// one audit row per change. A refusal is a notice naming each missing record and stores nothing.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] for a `to` other than `on`
/// or `off`, or [`OwnerToolError::Database`].
pub async fn owner_spanish(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    form: &SpanishForm,
    now: Timestamp,
) -> Result<SettingsNotice, OwnerToolError> {
    let route = (OwnerAction::Settings, OWNER_SPANISH_ROUTE);
    fresh_factor(conn, principal, route, now).await?;
    let owner = principal.owner();
    let changed = match form.to.as_str() {
        "off" => turn_spanish_off(conn, owner, now).await,
        "on" => turn_spanish_on(conn, owner, &form.records(), now).await,
        _ => return Err(OwnerToolError::Refused),
    };
    Ok(match changed.map_err(store)? {
        SpanishChange::Refused(gaps) => SettingsNotice::SpanishMissing(SpanishGaps::of(&gaps)),
        _ if form.to == "on" => SettingsNotice::SpanishOn,
        _ => SettingsNotice::SpanishOff,
    })
}

/// The safety voice call form's body: `on` or `off`, and for `on` the approval's date
/// (`YYYY-MM-DD`) and reference (CALL-019).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SafetyCallForm {
    /// `on` or `off`.
    pub to: String,
    /// The date of the attorney's written approval.
    pub approved_on: String,
    /// The approval's reference, 1 to 200 characters.
    pub reference: String,
}

/// CALL-019, CALL-022, SEC-043, D-275, D-384: after the fresh factor, turns the safety voice call
/// on (only with an approval date and reference and every item of `met`; production passes
/// `calls::api::SAFETY_CONDITIONS`) or off through `calls::api`, which stores one row and one
/// audit row per change. A refusal is a notice naming what is missing and stores nothing.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] for a `to` other than `on`
/// or `off`, or [`OwnerToolError::Database`].
pub async fn owner_safety_call(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    (form, met): (&SafetyCallForm, &Conditions),
    now: Timestamp,
) -> Result<SettingsNotice, OwnerToolError> {
    let route = (OwnerAction::SafetyCallSwitch, OWNER_SAFETY_CALL_ROUTE);
    fresh_factor(conn, principal, route, now).await?;
    let owner = principal.owner();
    let changed = match form.to.as_str() {
        "off" => turn_safety_switch_off(conn, owner, now).await,
        "on" => {
            let Ok(date) = form.approved_on.trim().parse::<jiff::civil::Date>() else {
                return Ok(SettingsNotice::SafetyCallNoApproval);
            };
            let approval = askhava_calls::api::Approval {
                date,
                reference: form.reference.clone(),
            };
            turn_safety_switch_on(conn, owner, (&approval, met), now).await
        }
        _ => return Err(OwnerToolError::Refused),
    };
    Ok(match (changed.map_err(store)?, form.to.as_str()) {
        (SwitchChange::NoApproval, _) => SettingsNotice::SafetyCallNoApproval,
        (SwitchChange::Unmet, _) => SettingsNotice::SafetyCallUnmet(*met),
        (_, "on") => SettingsNotice::SafetyCallOn,
        _ => SettingsNotice::SafetyCallOff,
    })
}

/// The body of the bridge press: the alert and this render's press ID. The CSRF field is read by
/// `server`'s `CsrfForm`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BridgeForm {
    /// The pushed critical alert's ID.
    pub alert: String,
    /// The press ID the page rendered into the form.
    pub press: String,
}

/// OPS-028, SEC-043, D-3753, D-3890: the bridge press after the fresh factor: one
/// [`OWNER_BRIDGE_KIND`] task holding the alert and press IDs only (deduplicated per press) and
/// one [`OWNER_BRIDGE_ACTION`] audit row, in one transaction; true when queued, false for a repeat
/// of the same press. Nothing is dialled here: `server`'s handler rings Casey.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] (a bad ID, or an alert that is
/// not critical about an inbound text) or [`OwnerToolError::Database`].
pub async fn owner_bridge_press(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    form: &BridgeForm,
    now: Timestamp,
) -> Result<bool, OwnerToolError> {
    fresh_factor(
        conn,
        principal,
        (OwnerAction::Settings, OWNER_BRIDGE_ROUTE),
        now,
    )
    .await?;
    let parse = |id: &str| Uuid::parse_str(id.trim()).map_err(|_| OwnerToolError::Refused);
    let (alert, press) = (parse(&form.alert)?, parse(&form.press)?);
    let subject = alert_subject(conn, alert).await.map_err(store)?;
    let subject = subject.ok_or(OwnerToolError::Refused)?;
    if !bridgeable(&subject.severity, subject.subject_kind.as_deref()) {
        return Err(OwnerToolError::Refused);
    }
    let payload = serde_json::json!({ "alert": alert, "press": press });
    let mut task = NewTask::new(OWNER_BRIDGE_KIND, payload, now);
    task.dedupe_key = Some(format!("{OWNER_BRIDGE_KIND}:{alert}:{press}"));
    let mut tx = conn.begin().await.map_err(store)?;
    if enqueue(&mut tx, &task).await.map_err(store)?.is_none() {
        return Ok(false);
    }
    let entry = AuditEntry {
        actor: Actor::Owner(principal.owner()),
        action: OWNER_BRIDGE_ACTION,
        subject_kind: "ops.alert",
        subject_id: alert,
        change: Change::Action,
    };
    audit(&mut tx, &entry).await.map_err(store)?;
    tx.commit().await.map_err(store)?;
    Ok(true)
}

/// The owner-recorded opt-out's body: the number the homeowner asked, by any other contact, to stop
/// texting. Its `Debug` never prints the number (PRIV-016).
#[derive(Clone, Default, Deserialize)]
pub struct OptOutForm {
    /// The phone number as typed.
    pub phone: String,
}

impl std::fmt::Debug for OptOutForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OptOutForm { phone: [redacted] }")
    }
}

/// CON-035, SEC-043, D-3753: after the fresh factor, lists the number at once through
/// `messaging::api::suppress` with [`RevokeChannel::Owner`] (logged with its channel and time);
/// confirmed on screen by the returned notice, never by text.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] for a number that is not a
/// phone number, or [`OwnerToolError::Database`].
pub async fn owner_record_opt_out(
    conn: &mut PgConnection,
    vault: &Vault,
    principal: &OwnerPrincipal,
    form: &OptOutForm,
    now: Timestamp,
) -> Result<SettingsNotice, OwnerToolError> {
    fresh_factor(
        conn,
        principal,
        (OwnerAction::Settings, OWNER_OPT_OUT_ROUTE),
        now,
    )
    .await?;
    let how = (RevokeChannel::Owner, "owner_recorded");
    match suppress(conn, vault, form.phone.trim(), how, now).await {
        Ok(true) => Ok(SettingsNotice::OptOutListed),
        Ok(false) => Ok(SettingsNotice::OptOutAlready),
        Err(OutboxError::BadPhone) => Err(OwnerToolError::Refused),
        Err(_) => Err(OwnerToolError::Database),
    }
}

/// The Stripe-answer filing's body: the answer's date (`YYYY-MM-DD`), the reference and `yes` or
/// `no` for whether it allows charges without the business present.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripeAnswerForm {
    /// The date of Stripe's written answer.
    pub answered_on: String,
    /// The reference, 1 to 200 characters.
    pub reference: String,
    /// `yes` or `no`.
    pub allows: String,
}

/// PICK-048, SEC-043, D-2783, D-3753: after the fresh factor, files Stripe's written answer once
/// through `quotes::api::file_stripe_answer` at `now`; the newest filing is the one in force.
///
/// # Errors
///
/// [`OwnerToolError::NeedsFreshFactor`], [`OwnerToolError::Refused`] for a bad date, reference or
/// choice, or [`OwnerToolError::Database`].
pub async fn owner_file_stripe_answer(
    conn: &mut PgConnection,
    principal: &OwnerPrincipal,
    form: &StripeAnswerForm,
    now: Timestamp,
) -> Result<SettingsNotice, OwnerToolError> {
    let route = (OwnerAction::StripeAnswer, OWNER_STRIPE_ANSWER_ROUTE);
    fresh_factor(conn, principal, route, now).await?;
    let day = form.answered_on.trim().parse::<jiff::civil::Date>();
    let day = day.map_err(|_| OwnerToolError::Refused)?;
    let allows = match form.allows.as_str() {
        "yes" => true,
        "no" => false,
        _ => return Err(OwnerToolError::Refused),
    };
    match file_stripe_answer(conn, (day, &form.reference, allows), now).await {
        Ok(()) => Ok(SettingsNotice::StripeAnswerFiled),
        Err(AutoConfirmError::BadField) => Err(OwnerToolError::Refused),
        Err(_) => Err(OwnerToolError::Database),
    }
}
