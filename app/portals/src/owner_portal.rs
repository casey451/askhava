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

use askama::Template;
use askhava_businesses::api::{needs_review_signups, review_target_line};
use askhava_catalog::api::{CatalogError, category, category_list, set_category_enabled};
use askhava_messaging::api::{
    Audience, CONSENT_KEEP_YEARS, LaunchChannel, ModeStateError, NumberState, Week, WeekRate,
    alert_percent, launch_channel, load_lines, record_restore, record_suspension,
    set_launch_channel, switch_off, switch_on, weekly_optout_rates,
};
use askhava_ops::api::{
    Actor, Switch, acknowledge_pushed_alert, banner_shown, clear_banner, is_paused, pushed_alerts,
    set_switch,
};
use askhava_payments::api::{FeeMode, fee_terms_in_force, switch_fee_mode};
use jiff::{Timestamp, tz};
use serde::Deserialize;
use sqlx::PgConnection;
use sqlx::types::Uuid;

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
