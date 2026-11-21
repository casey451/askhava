//! The served owner refund review, audit log viewer and health page (SEC-006, SEC-034, SEC-042,
//! SEC-043, SEC-048, FEE-026, FEE-050, OPS-008, OPS-010; T304, D-2128, D-2204, D-3155, D-3844,
//! D-3890, D-5159).
//!
//! [`routes`] serves `portals`' pages (T205, T476) over the pool: `GET` [`OWNER_REFUND_ROUTE`]
//! (pick P's review step), `POST` [`OWNER_REFUND_PRESS_ROUTE`] (`portals::owner_refund`: after a
//! fresh factor one Stripe refund and one ledger row, a disputed charge blocked), `POST`
//! [`OWNER_EVIDENCE_ROUTE`] (`portals::owner_dispute_evidence` with the route's pick and the
//! form's `dp_` ID, which records the countering fee once), `GET` [`OWNER_AUDIT_LOG_ROUTE`]
//! (`?after=` a row ID) and `GET` [`OWNER_HEALTH_ROUTE`]. The layers and their order are
//! `owner_portal`'s: every response is `no-store`; every `POST` passes `refuse_cross_site` and
//! the CSRF form check before the owner-session check (`owner_signin::require_owner`), so a
//! cross-site or wrong-token `POST` is a 403 with one `csrf_rejected` event that changes
//! nothing; no session, another realm's or an ended one gets `owner_signed_out`, the 401 linking
//! to `/owner/sign-in`. A press without a code from the last 5 minutes is a 401 with the review
//! step naming the step-up page (`portals` logs the `authz_denied` event); a refused press (no
//! charge, another pick's dispute, no Stripe part) is a 400 with the step again; a `{pick}` that
//! is no UUID is the owner's bare 404. The refund uses the Stripe hooks' own client and
//! `ASKHAVA_ID_SECRET` from the Confirm settings, the secret the worker's retry derives its keys
//! from (D-3890); without either the routes are still served and a press changes nothing.
//! [`SecretReport`] is what `main` tells the health page about the settings: flags per secret
//! name, the SEC-036 account identifiers and the email limits, never a secret's value, length
//! or hash (OPS-008).

use std::sync::Arc;

use askhava_core::api::{Clock, Settings, SystemClock, Vendor};
use askhava_payments::api::{OwnerSubmission, RefundOutcome};
use askhava_portals::api::{
    AccountIds, ButtonForm, ConfirmSettings, CookieJar, CsrfField, CsrfForm, DEFAULT_QUOTED_WITHIN,
    EvidenceForm, HealthInputs, OWNER_AUDIT_LOG_ROUTE, OWNER_EVIDENCE_ROUTE, OWNER_HEALTH_ROUTE,
    OWNER_REFUND_PRESS_ROUTE, OWNER_REFUND_ROUTE, OwnerPrincipal, OwnerToolError, ReviewNotice,
    SecretLine, WebSecurity, csrf_token, no_store, owner_audit_log_page, owner_dispute_evidence,
    owner_health_page, owner_refund, owner_refund_page, refuse_cross_site,
};
use axum::Router;
use axum::extract::{Extension, FromRef, Path, RawQuery, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::owner_signin::{OwnerSite, note_owner_site, require_owner};
use crate::release_sink::StripeHooks;

/// The secrets a running app needs (SEC-001, OPS-008): setting names only.
const REQUIRED_SECRETS: [&str; 6] = [
    "ASKHAVA_DATABASE_URL",
    "ASKHAVA_ID_SECRET",
    "ASKHAVA_RESEND_API_KEY",
    "ASKHAVA_STRIPE_SECRET_KEY",
    "ASKHAVA_STRIPE_WEBHOOK_SECRET",
    "ASKHAVA_TWILIO_AUTH_TOKEN",
];
const EMAIL_DAILY: (&str, u64) = ("ASKHAVA_EMAIL_DAILY_LIMIT", 100);
const EMAIL_MONTHLY: (&str, u64) = ("ASKHAVA_EMAIL_MONTHLY_LIMIT", 3_000);

/// OPS-008, SEC-036, D-3155, D-3844: what the health page is told about this process's settings:
/// each required secret's flags, the four account identifiers (not secrets), the email limits
/// and the spot-check cadence. It holds no secret's value, length or hash.
#[derive(Debug, Clone)]
pub struct SecretReport {
    secrets: Vec<SecretLine>,
    accounts: AccountIds,
    email_limits: (u64, u64),
    spot_days: u32,
}

impl Default for SecretReport {
    /// "Not checked": no secret line, no account, the default limits and cadence; what a caller
    /// that passes no report gets (D-3890).
    fn default() -> Self {
        Self {
            secrets: Vec::new(),
            accounts: AccountIds::default(),
            email_limits: (EMAIL_DAILY.1, EMAIL_MONTHLY.1),
            spot_days: askhava_core::api::DEFAULT_SPOT_CHECK_EVERY_DAYS,
        }
    }
}

impl SecretReport {
    /// OPS-008: the report of `settings` and the process's `pairs`. `settings` parsed and passed
    /// its environment checks at start (`Settings::from_pairs`, SEC-002), so a secret that is set
    /// is also shown parsed and environment-matched; one not set shows "No" three times. Only
    /// whether a value is empty is read here. Limit (D-5128, D-5132): outside the Stripe secret
    /// key, the start check is the SEC-002 marker scan alone, so "environment-matched" does not
    /// prove a token belongs to this environment's account.
    #[must_use]
    pub fn from_settings<I>(settings: &Settings, pairs: I) -> Self
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let pairs: Vec<(String, String)> = pairs.into_iter().collect();
        let value = |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim());
        let line = |name: &&str| {
            let present = value(name).is_some_and(|v| !v.is_empty());
            let (parsed, env_matched) = (present, present);
            SecretLine {
                name: (*name).to_owned(),
                present,
                parsed,
                env_matched,
            }
        };
        let limit = |(name, default): (&str, u64)| {
            let set = value(name).and_then(|v| v.parse().ok());
            set.unwrap_or(default)
        };
        let account = |vendor| settings.account(vendor).map(str::to_owned);
        Self {
            secrets: REQUIRED_SECRETS.iter().map(line).collect(),
            accounts: AccountIds {
                twilio_account_sid: account(Vendor::Twilio),
                stripe_account_id: account(Vendor::Stripe),
                resend_domain: account(Vendor::Resend),
                storage_bucket: account(Vendor::Storage),
            },
            email_limits: (limit(EMAIL_DAILY), limit(EMAIL_MONTHLY)),
            spot_days: settings.spot_check_every_days(),
        }
    }
}

/// The owner tools' state: the refund's Stripe hooks and ID secret, when both are configured.
#[derive(Clone)]
struct Tools {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    refund: Option<(StripeHooks, SecretString)>,
    report: Arc<SecretReport>,
}

impl FromRef<Tools> for WebSecurity {
    fn from_ref(tools: &Tools) -> Self {
        tools.security.clone()
    }
}

/// The evidence press with its CSRF token; a missing field reaches the CSRF check (D-3878).
#[derive(serde::Deserialize)]
struct EvidenceBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    dispute: String,
}

impl CsrfField for EvidenceBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The routes `build_router` merges (D-2128): the refund takes `stripe`'s client and
/// `confirm`'s `ASKHAVA_ID_SECRET` (D-3890), and with either missing refunds nothing; `report`
/// is what the health page shows of the settings.
pub fn routes(
    pool: PgPool,
    stripe: &Option<StripeHooks>,
    confirm: &Option<ConfirmSettings>,
    report: SecretReport,
) -> Router {
    let refund = stripe.as_ref().zip(confirm.as_ref());
    let refund = refund.map(|(hooks, confirm)| (Arc::clone(hooks), confirm.secret.clone()));
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Tools {
        pool,
        clock,
        security: security.clone(),
        refund,
        report: Arc::new(report),
    };
    Router::new()
        .route(OWNER_REFUND_ROUTE, get(refund_page))
        .route(OWNER_REFUND_PRESS_ROUTE, post(refund_press))
        .route(OWNER_EVIDENCE_ROUTE, post(evidence_press))
        .route(OWNER_AUDIT_LOG_ROUTE, get(audit_log))
        .route(OWNER_HEALTH_ROUTE, get(health))
        .route_layer(from_fn(note_owner_site))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "owner tools failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `built` as a page answering `status`, else the bare 500.
fn page(built: Result<String, OwnerToolError>, status: StatusCode) -> Response {
    match built {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("owner page"),
    }
}

/// SEC-005, SEC-006, SEC-048: the live owner at `now` and the route's pick, else the owner
/// door's answer; a `{pick}` that is no UUID is the owner's bare 404.
async fn owner(
    tools: &Tools,
    jar: &CookieJar,
    site: &OwnerSite,
    pick: &str,
) -> Result<(OwnerPrincipal, Uuid, Timestamp), Box<Response>> {
    let now = tools.clock.now();
    let principal = require_owner(&tools.pool, jar, site, now).await?;
    match Uuid::try_parse(pick) {
        Ok(pick) => Ok((principal, pick, now)),
        Err(_) => Err(Box::new(StatusCode::NOT_FOUND.into_response())),
    }
}

/// FEE-026, FEE-050: `pick`'s review step for `jar` with `notice`, answering `status`. Reads only.
async fn review(
    tools: &Tools,
    jar: &CookieJar,
    pick: Uuid,
    notice: Option<ReviewNotice>,
    status: StatusCode,
) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    page(
        owner_refund_page(&mut db, &csrf, pick, notice).await,
        status,
    )
}

/// SEC-043, D-3878: the answer to a press `portals` did not make: a stale factor is a 401 with
/// the step naming the step-up page, a refusal a 400 with the step, a database failure a 500.
async fn not_made(tools: &Tools, jar: &CookieJar, pick: Uuid, error: OwnerToolError) -> Response {
    match error {
        OwnerToolError::NeedsFreshFactor => {
            let notice = Some(ReviewNotice::NeedsFreshFactor);
            review(tools, jar, pick, notice, StatusCode::UNAUTHORIZED).await
        }
        OwnerToolError::Refused => review(tools, jar, pick, None, StatusCode::BAD_REQUEST).await,
        OwnerToolError::Database => failed("owner press"),
    }
}

/// `GET` [`OWNER_REFUND_ROUTE`]: pick P's refund review step, for the owner only (SEC-006,
/// SEC-034, FEE-026, FEE-050).
async fn refund_page(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    Path(pick): Path<String>,
    jar: CookieJar,
) -> Response {
    match owner(&tools, &jar, &site, &pick).await {
        Ok((_, pick, _)) => review(&tools, &jar, pick, None, StatusCode::OK).await,
        Err(refused) => *refused,
    }
}

/// `POST` [`OWNER_REFUND_PRESS_ROUTE`] (FEE-026, SEC-006, SEC-043, SEC-048): after the fresh
/// factor, `portals::owner_refund` refunds P's charge once with the hooks' Stripe client and the
/// ID secret; a disputed charge is blocked there, a 400 with the step's error. With no Stripe
/// part nothing is asked or changed.
async fn refund_press(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    Path(pick): Path<String>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let (principal, pick, now) = match owner(&tools, &jar, &site, &pick).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let Some((hooks, secret)) = &tools.refund else {
        return not_made(&tools, &jar, pick, OwnerToolError::Refused).await;
    };
    let parts = (&hooks.stripe, secret);
    match owner_refund(&tools.pool, parts, &principal, pick, now).await {
        Ok(outcome) => {
            let blocked = outcome == RefundOutcome::Blocked;
            let status = [StatusCode::OK, StatusCode::BAD_REQUEST][usize::from(blocked)];
            review(
                &tools,
                &jar,
                pick,
                Some(ReviewNotice::Refund(outcome)),
                status,
            )
            .await
        }
        Err(error) => not_made(&tools, &jar, pick, error).await,
    }
}

/// `POST` [`OWNER_EVIDENCE_ROUTE`] (FEE-050, SEC-043, D-2204, D-4506): "I submitted the evidence
/// myself" for the form's dispute of the route's pick; `portals` records the countering fee once
/// and a second press changes nothing. Another pick's dispute is a 400 with the step's error.
async fn evidence_press(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    Path(pick): Path<String>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<EvidenceBody>,
) -> Response {
    let (principal, pick, now) = match owner(&tools, &jar, &site, &pick).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let form = EvidenceForm {
        dispute: body.dispute,
    };
    let answer = owner_dispute_evidence(&mut db, &principal, (pick, &form), now).await;
    drop(db);
    match answer {
        Ok(answer) => {
            let status = match answer {
                OwnerSubmission::NotThisPick => StatusCode::BAD_REQUEST,
                OwnerSubmission::Recorded | OwnerSubmission::AlreadyIn => StatusCode::OK,
            };
            let notice = Some(ReviewNotice::Evidence(answer));
            review(&tools, &jar, pick, notice, status).await
        }
        Err(error) => not_made(&tools, &jar, pick, error).await,
    }
}

/// `GET` [`OWNER_AUDIT_LOG_ROUTE`]: the audit log viewer after row `?after=` (0 when missing or
/// not a number), for the owner only (SEC-006, SEC-034, OPS-010). Reads only.
async fn audit_log(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    RawQuery(query): RawQuery,
    jar: CookieJar,
) -> Response {
    if let Err(refused) = require_owner(&tools.pool, &jar, &site, tools.clock.now()).await {
        return *refused;
    }
    let query = query.unwrap_or_default();
    let after = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("after="));
    let after = after.and_then(|id| id.parse().ok()).unwrap_or(0);
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    page(owner_audit_log_page(&mut db, after).await, StatusCode::OK)
}

/// `GET` [`OWNER_HEALTH_ROUTE`]: the health page, for the owner only (SEC-006, SEC-034,
/// OPS-008, D-3753): T475's figures, this process's time since start and the worker state
/// `/healthz` reports, and the [`SecretReport`]. Reads only.
async fn health(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    let now = tools.clock.now();
    if let Err(refused) = require_owner(&tools.pool, &jar, &site, now).await {
        return *refused;
    }
    let web = crate::healthz::web_health(&tools.pool).await;
    let report = &tools.report;
    let inputs = HealthInputs {
        email_daily_limit: report.email_limits.0,
        email_monthly_limit: report.email_limits.1,
        quoted_within: DEFAULT_QUOTED_WITHIN,
        spot_check_every_days: report.spot_days,
        web_up_for: now.duration_since(web.started),
        worker_up: !web.state.worker_down,
        secrets: report.secrets.clone(),
        accounts: report.accounts.clone(),
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    page(
        owner_health_page(&mut db, &inputs, now).await,
        StatusCode::OK,
    )
}

#[cfg(test)]
mod tests {
    //! OPS-008: `SecretReport::from_settings` over fake pairs (T304 fix round, D-5132).

    use super::*;

    const SET: [&str; 4] = [
        "ASKHAVA_DATABASE_URL",
        "ASKHAVA_ID_SECRET",
        "ASKHAVA_STRIPE_WEBHOOK_SECRET",
        "ASKHAVA_TWILIO_AUTH_TOKEN",
    ];

    /// Fake dev pairs: four secrets set, the Resend key blank, the Stripe key missing, three of
    /// the four account identifiers, and `extra`.
    fn pairs(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let fixed = [
            ("ASKHAVA_APP_ENV", "dev"),
            ("ASKHAVA_RESEND_API_KEY", "   "),
            ("ASKHAVA_TWILIO_ACCOUNT_SID", "fake-twilio-account"),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "fake-stripe-account"),
            ("ASKHAVA_RESEND_DOMAIN", "mail.example.test"),
        ];
        let set = SET.iter().map(|name| (*name, "fake-value-for-tests"));
        let all = fixed.into_iter().chain(set).chain(extra.iter().copied());
        all.map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
    }

    fn report(extra: &[(&str, &str)]) -> SecretReport {
        let pairs = pairs(extra);
        let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
        SecretReport::from_settings(&settings, pairs)
    }

    /// OPS-008: a missing secret and a blank one show "No" three times, a set one "Yes" three
    /// times (D-5128), in the required-secrets order, and no line holds a value.
    #[test]
    fn a_missing_or_blank_secret_is_never_reported_set() {
        let report = report(&[]);
        let names: Vec<&str> = report.secrets.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, REQUIRED_SECRETS);
        for line in &report.secrets {
            let set = SET.contains(&line.name.as_str());
            assert_eq!(line.present, set, "{} present", line.name);
            assert_eq!(line.parsed, set, "{} parsed", line.name);
            assert_eq!(line.env_matched, set, "{} environment", line.name);
        }
        let not_set = report.secrets.iter().filter(|l| !l.present).count();
        assert_eq!(
            not_set, 2,
            "the missing Stripe key and the blank Resend key"
        );
        assert!(!format!("{report:?}").contains("fake-value-for-tests"));
    }

    /// OPS-008, SEC-036: the account identifiers come from the settings; one not set is `None`.
    #[test]
    fn the_account_identifiers_are_the_settings_own() {
        let accounts = report(&[]).accounts;
        assert_eq!(
            accounts.twilio_account_sid.as_deref(),
            Some("fake-twilio-account")
        );
        assert_eq!(
            accounts.stripe_account_id.as_deref(),
            Some("fake-stripe-account")
        );
        assert_eq!(accounts.resend_domain.as_deref(), Some("mail.example.test"));
        assert_eq!(accounts.storage_bucket, None);
    }

    /// OPS-008, D-5128: the email limits are the `ASKHAVA_EMAIL_*_LIMIT` values; one missing or
    /// not a number is the default (100 a day, 3,000 a month).
    #[test]
    fn the_email_limits_are_read_from_the_pairs() {
        let set = report(&[(EMAIL_DAILY.0, "250"), (EMAIL_MONTHLY.0, " 9000 ")]);
        assert_eq!(set.email_limits, (250, 9_000));
        let odd = report(&[(EMAIL_MONTHLY.0, "many")]);
        assert_eq!(odd.email_limits, (EMAIL_DAILY.1, EMAIL_MONTHLY.1));
        assert_eq!(
            odd.spot_days,
            askhava_core::api::DEFAULT_SPOT_CHECK_EVERY_DAYS
        );
    }
}
