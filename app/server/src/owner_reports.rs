//! The served owner reports: the pilot scorecard with its weekly minutes form, the monthly intake
//! report, and the money view with its expense entry and monthly CSV (OPS-039, OPS-040, FEE-046,
//! FEE-028, SEC-005, SEC-034, SEC-042, SEC-048; T305, D-2128, D-3890, D-4897, D-5318).
//!
//! [`routes`] serves `portals`' pages: `GET` [`OWNER_SCORECARD_ROUTE`], [`OWNER_INTAKE_REPORT_ROUTE`],
//! [`OWNER_MONEY_ROUTE`] and [`OWNER_MONEY_CSV_ROUTE`] (each reads only; the month a
//! `?month=YYYY-MM` query value, this Arizona month when absent or unreadable) and the `POST`s
//! [`OWNER_SCORECARD_MINUTES_ROUTE`] and [`OWNER_EXPENSE_ROUTE`]. The layers are `owner_ai_routes`':
//! every response is `no-store`; every `POST` passes `refuse_cross_site` and the CSRF form check
//! (each refusal a 403 and a security event) before the owner-session check; no owner session, or
//! another realm's, gets `owner_signed_out` (a 401 linking to `/owner/sign-in`) and changes
//! nothing. The values the pages take from `Settings` travel in [`ReportSettings`], which `main`
//! passes through `RouterParts::with_report_settings`; without it the defaults hold. The owner help
//! page is `portals`' `owner_help_routes`, merged in `owner_pages`, not here (D-5318).

use std::sync::Arc;

use askhava_core::api::{Clock, Settings, SystemClock, arizona_time};
use askhava_portals::api::{
    CookieJar, CsrfField, CsrfForm, ExpenseAnswer, ExpenseForm, LinesError, MinutesForm,
    MinutesNotice, OWNER_EXPENSE_ROUTE, OWNER_INTAKE_REPORT_ROUTE, OWNER_MONEY_CSV_ROUTE,
    OWNER_MONEY_ROUTE, OWNER_SCORECARD_MINUTES_ROUTE, OWNER_SCORECARD_ROUTE, OwnerToolError,
    ScorecardInputs, ScorecardLines, WebSecurity, csrf_token, intake_report_page,
    money_view_csv_file, money_view_page, month_start, no_store, owner_record_minutes,
    owner_scorecard_page, post_expense, refuse_cross_site,
};
use axum::Router;
use axum::extract::{Extension, FromRef, Query, State};
use axum::http::{StatusCode, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;
use sqlx::PgPool;

use crate::owner_signin::{OwnerSite, note_owner_site, require_owner};

/// What the owner reports take from `Settings` (OPS-039, OPS-040, OPS-047, FEE-046; D-3890 (2),
/// D-5318 (2)): the set-aside basis points, the golden-set target, the scorecard's lines, the
/// Gate C launch instant the pilot starts at (D-019) and the pushed trades.
#[derive(Clone, Debug)]
pub struct ReportSettings {
    set_aside: u16,
    golden_target: u32,
    lines: ScorecardLines,
    launch: Option<Timestamp>,
    pushed: Vec<String>,
}

impl ReportSettings {
    /// The values `settings` holds (OPS-039, OPS-040, FEE-046).
    ///
    /// # Errors
    ///
    /// [`LinesError`] when the `ASKHAVA_SCORECARD_*` lines do not parse (a swapped pair).
    pub fn from_settings(settings: &Settings) -> Result<Self, LinesError> {
        Ok(Self {
            set_aside: settings.sales_tax_set_aside_basis_points(),
            golden_target: settings.golden_target_percent(),
            lines: ScorecardLines::from_pairs(settings.scorecard_lines())?,
            launch: settings.launch_date(),
            pushed: settings.pilot_pushed_trades().to_vec(),
        })
    }

    /// OPS-039, D-019: the scorecard's inputs at `now`; with no launch date set (dev, CI) the
    /// pilot starts at the start of the current Arizona week (Monday, D-5318).
    fn inputs(&self, now: Timestamp) -> Result<ScorecardInputs, OwnerToolError> {
        let pilot_start = match self.launch {
            Some(launch) => launch,
            None => arizona_monday(now)?,
        };
        Ok(ScorecardInputs {
            lines: self.lines.clone(),
            set_aside_basis_points: self.set_aside,
            pilot_start,
            pushed: self.pushed.clone(),
        })
    }
}

impl Default for ReportSettings {
    /// Every value at its `Settings` default: no line or key set, no launch date, no pushed trade.
    fn default() -> Self {
        Self {
            set_aside: Settings::DEFAULT_SET_ASIDE_BASIS_POINTS,
            golden_target: askhava_core::api::DEFAULT_GOLDEN_TARGET_PERCENT,
            lines: ScorecardLines::from_pairs(Settings::default_scorecard_lines()).unwrap_or_else(
                |_| unreachable!("the default lines parse (scorecard_lines tests)"),
            ),
            launch: None,
            pushed: Vec::new(),
        }
    }
}

/// The start of the Arizona week (Monday 00:00) holding `now`.
fn arizona_monday(now: Timestamp) -> Result<Timestamp, OwnerToolError> {
    let az = arizona_time(now);
    let back = i64::from(az.date().weekday().to_monday_zero_offset());
    let today = az.start_of_day().map_err(|_| OwnerToolError::Database)?;
    Ok(today.timestamp() - SignedDuration::from_hours(24 * back))
}

#[derive(Clone)]
struct Reports {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    settings: Arc<ReportSettings>,
}

impl FromRef<Reports> for WebSecurity {
    fn from_ref(reports: &Reports) -> Self {
        reports.security.clone()
    }
}

/// The `?month=YYYY-MM` query of the money view, the CSV and the intake report (D-3890).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MonthBody {
    #[serde(default)]
    month: Option<String>,
}

/// The weekly minutes form with its CSRF token (OPS-039).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MinutesBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    recruiting: String,
    #[serde(default)]
    other: String,
}

/// The expense entry form with its CSRF token (FEE-046).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpenseBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    spent_on: String,
    #[serde(default)]
    vendor: String,
    #[serde(default)]
    amount: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    note: String,
}

impl CsrfField for MinutesBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl CsrfField for ExpenseBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The routes `build_router` merges, over `pool`, with `settings` (SEC-005, SEC-034, SEC-042).
pub fn routes(pool: PgPool, settings: ReportSettings) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Reports {
        pool,
        clock,
        security: security.clone(),
        settings: Arc::new(settings),
    };
    Router::new()
        .route(OWNER_SCORECARD_ROUTE, get(scorecard))
        .route(OWNER_SCORECARD_MINUTES_ROUTE, post(minutes))
        .route(OWNER_INTAKE_REPORT_ROUTE, get(intake_report))
        .route(OWNER_MONEY_ROUTE, get(money))
        .route(OWNER_MONEY_CSV_ROUTE, get(money_csv))
        .route(OWNER_EXPENSE_ROUTE, post(expense))
        .route_layer(from_fn(note_owner_site))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "owner report failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// A page's answer: `status` with the page, a month outside 2000 to 2999 a bare 400, else a 500.
fn page_answer(status: StatusCode, page: Result<String, OwnerToolError>) -> Response {
    match page {
        Ok(html) => (status, Html(html)).into_response(),
        Err(OwnerToolError::Refused) => StatusCode::BAD_REQUEST.into_response(),
        Err(_) => failed("owner report page"),
    }
}

/// The asked month's first day, this Arizona month when absent or unreadable (D-3890).
fn month_of(query: &MonthBody, now: Timestamp) -> Option<Date> {
    month_start(query.month.as_deref(), now).or_else(|| month_start(None, now))
}

/// OPS-039: the scorecard for the owner in `jar` with `notice`, as `status`. Reads only.
async fn scorecard_answer(
    reports: &Reports,
    jar: &CookieJar,
    notice: Option<&MinutesNotice>,
    status: StatusCode,
) -> Response {
    let now = reports.clock.now();
    let (Some(csrf), Ok(inputs)) = (csrf_token(jar), reports.settings.inputs(now)) else {
        return failed("scorecard inputs");
    };
    let Ok(mut db) = reports.pool.acquire().await else {
        return failed("database");
    };
    let page = owner_scorecard_page(&mut db, &inputs, (&csrf, notice), now).await;
    page_answer(status, page)
}

/// `GET` [`OWNER_SCORECARD_ROUTE`]: the pilot scorecard, for the owner only (OPS-039, SEC-034).
async fn scorecard(
    State(reports): State<Reports>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    let now = reports.clock.now();
    if let Err(refused) = require_owner(&reports.pool, &jar, &site, now).await {
        return *refused;
    }
    scorecard_answer(&reports, &jar, None, StatusCode::OK).await
}

/// `POST` [`OWNER_SCORECARD_MINUTES_ROUTE`]: saves the week's minutes (OPS-039, SEC-042, D-4897);
/// saved is the page with its saved line, refused the page with the values kept, as a 400.
async fn minutes(
    State(reports): State<Reports>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<MinutesBody>,
) -> Response {
    let now = reports.clock.now();
    let principal = match require_owner(&reports.pool, &jar, &site, now).await {
        Ok(principal) => principal,
        Err(refused) => return *refused,
    };
    let form = MinutesForm {
        recruiting: body.recruiting,
        other: body.other,
    };
    let Ok(mut db) = reports.pool.acquire().await else {
        return failed("database");
    };
    let (notice, status) = match owner_record_minutes(&mut db, &principal, &form, now).await {
        Ok(()) => (MinutesNotice::Saved, StatusCode::OK),
        Err(OwnerToolError::Refused) => (MinutesNotice::Refused(form), StatusCode::BAD_REQUEST),
        Err(_) => return failed("owner minutes"),
    };
    drop(db);
    scorecard_answer(&reports, &jar, Some(&notice), status).await
}

/// `GET` [`OWNER_INTAKE_REPORT_ROUTE`]: the month's intake report against the golden-set target
/// (OPS-040, D-3155, SEC-034). Reads only.
async fn intake_report(
    State(reports): State<Reports>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    Query(query): Query<MonthBody>,
) -> Response {
    let now = reports.clock.now();
    if let Err(refused) = require_owner(&reports.pool, &jar, &site, now).await {
        return *refused;
    }
    let Some(first) = month_of(&query, now) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(mut db) = reports.pool.acquire().await else {
        return failed("database");
    };
    let target = reports.settings.golden_target;
    let page = intake_report_page(&mut db, first, target, now).await;
    page_answer(StatusCode::OK, page)
}

/// `GET` [`OWNER_MONEY_ROUTE`]: the month's money view with an empty expense form (FEE-046,
/// SEC-034). Reads only.
async fn money(
    State(reports): State<Reports>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    Query(query): Query<MonthBody>,
) -> Response {
    let now = reports.clock.now();
    if let Err(refused) = require_owner(&reports.pool, &jar, &site, now).await {
        return *refused;
    }
    let (Some(first), Some(csrf)) = (month_of(&query, now), csrf_token(&jar)) else {
        return failed("money month or csrf token");
    };
    let page = money_view_page(&reports.pool, first, reports.settings.set_aside, &csrf).await;
    page_answer(StatusCode::OK, page)
}

/// `GET` [`OWNER_MONEY_CSV_ROUTE`]: the month's CSV as `payments` makes it, a download
/// (FEE-046, SEC-034). Reads only.
async fn money_csv(
    State(reports): State<Reports>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    Query(query): Query<MonthBody>,
) -> Response {
    let now = reports.clock.now();
    if let Err(refused) = require_owner(&reports.pool, &jar, &site, now).await {
        return *refused;
    }
    let Some(first) = month_of(&query, now) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let name = format!(
        "attachment; filename=\"money-{}.csv\"",
        first.strftime("%Y-%m")
    );
    match money_view_csv_file(&reports.pool, first, reports.settings.set_aside).await {
        Ok(csv) => {
            let kind = (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_owned());
            ([kind, (header::CONTENT_DISPOSITION, name)], csv).into_response()
        }
        Err(OwnerToolError::Refused) => StatusCode::BAD_REQUEST.into_response(),
        Err(_) => failed("money csv"),
    }
}

/// `POST` [`OWNER_EXPENSE_ROUTE`]: stores one expense, never a ledger row, and answers 303 to its
/// month's page, or the page again with the field's error and the values kept, as a 400
/// (FEE-046, FEE-028, SEC-042).
async fn expense(
    State(reports): State<Reports>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<ExpenseBody>,
) -> Response {
    let now = reports.clock.now();
    if let Err(refused) = require_owner(&reports.pool, &jar, &site, now).await {
        return *refused;
    }
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    let form = ExpenseForm {
        spent_on: body.spent_on,
        vendor: body.vendor,
        amount: body.amount,
        category: body.category,
        note: body.note,
    };
    let basis = (reports.settings.set_aside, csrf.as_str());
    match post_expense(&reports.pool, &form, basis, now).await {
        Ok(ExpenseAnswer::Added(to)) => Redirect::to(&to).into_response(),
        Ok(ExpenseAnswer::Refused(html)) => (StatusCode::BAD_REQUEST, Html(html)).into_response(),
        Err(_) => failed("owner expense"),
    }
}
