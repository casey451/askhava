//! The homeowner's check-in answer page at `/c/{token}/{answer}` (TXT-034, SEC-026, JOB-015,
//! JOB-028, T231, D-493, D-2414 to D-2417).
//!
//! Each button of the emailed check-in (`messaging::api::render_check_in`) opens this page with the
//! job's quote-page link token and an answer code (`messaging::api::parse_check_in_code`, for
//! example `3-done`). `GET` only shows the answer and its Send button: it changes nothing, so a
//! mail scanner opening every button, twice, records nothing. Only the page's `POST`, carrying the
//! pre-session CSRF token (`ButtonForm`, SEC-042), records the answer through
//! `jobs::api::answer_check_in`, once: a job no longer Connected records nothing and shows the
//! closed line. A malformed, unknown, re-issued or revoked token, a token 30 days past the close
//! (PICK-011) or an unknown code is a bare 404 (SEC-037). The page reads no name, phone or
//! address; it is `noindex`, `no-store` and behind the cross-site refusal (SEC-034, SEC-042).

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, Guarded, LinkToken};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{Answer, Inbound, answer_check_in, current, job_number};
use askhava_messaging::api::{Language, answer_label, parse_check_in_code};
use askhava_ops::api::Actor;
use axum::Router;
use axum::extract::{FromRef, Path, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum_extra::extract::cookie::CookieJar;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::quote_page::token_expired;
use crate::security::{ButtonForm, CsrfForm, WebSecurity, no_store, refuse_cross_site};
use crate::session::{csrf_token, ensure_pre_session};

/// The answer page's route: the job's quote-page link token and the answer code (TXT-034).
pub const CHECK_IN_ROUTE: &str = "/c/{token}/{answer}";

/// The page's state: the pool, the clock and the cross-site and CSRF refusals' event log.
#[derive(Clone)]
struct CheckInPage {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<CheckInPage> for WebSecurity {
    fn from_ref(page: &CheckInPage) -> Self {
        page.security.clone()
    }
}

/// Which view the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    /// The answer and its Send button; nothing recorded yet.
    Ask,
    /// The answer is recorded now.
    Sent,
    /// The job is not waiting for a check-in answer (answered already, or moved on).
    Closed,
}

/// The page's template (`templates/checkin.html`).
#[derive(Template)]
#[template(path = "checkin.html")]
pub(crate) struct CheckInView {
    /// The request (job) number from the email.
    pub(crate) request: i64,
    /// The path the form posts back to.
    pub(crate) action: String,
    /// The answer's words, as on the email's button.
    pub(crate) label: &'static str,
    /// The form's CSRF token (SEC-042).
    pub(crate) csrf: String,
    /// What the page shows.
    pub(crate) screen: Screen,
}

/// JOB-015, JOB-028: the job answer and check-in day an answer code names.
pub(crate) fn read_code(code: &str) -> Option<(Answer, CheckIn, &'static str)> {
    let (day, slug) = parse_check_in_code(code)?;
    let answer = match slug {
        "done" => Answer::Done,
        "ongoing" => Answer::Ongoing,
        "later" => Answer::Later,
        "no" => Answer::No,
        _ => Answer::Other,
    };
    let check = if day == 3 {
        CheckIn::Day3
    } else {
        CheckIn::Day14
    };
    Some((answer, check, slug))
}

/// The check-in answer page's router over `pool`, timed by `clock` (TXT-034, SEC-026): `GET` and
/// `POST` [`CHECK_IN_ROUTE`] behind the cross-site refusal and `noindex`; `no-store` is outermost,
/// so a refusal is `no-store` too (TD-TXT-31: every response).
/// `server::checkin::routes` merges it.
pub fn checkin_routes(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = CheckInPage {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(CHECK_IN_ROUTE, get(show).post(send))
        .with_state(state)
        .layer(from_fn(noindex))
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// `X-Robots-Tag: noindex, nofollow` on every answer-page response (it is private).
async fn noindex(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let value = HeaderValue::from_static("noindex, nofollow");
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-robots-tag"), value);
    response
}

/// What an opened link names: the job, its request number, whether it still takes check-ins.
type Found = (Uuid, i64, bool);

/// The job behind `raw` while its link is live (SEC-037, PICK-011); `None` for any other token.
async fn open_job(page: &CheckInPage, raw: &str) -> Result<Option<Found>, StatusCode> {
    const FAILED: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;
    let Ok(token) = LinkToken::parse(raw) else {
        return Ok(None);
    };
    let mut db = page.pool.acquire().await.map_err(|_| FAILED)?;
    let Some(job) = resolve(&mut db, &token).await.map_err(|_| FAILED)? else {
        return Ok(None);
    };
    let row = current(&mut db, job).await.map_err(|_| FAILED)?;
    if token_expired(row.status.is_final(), row.status_at, page.clock.now()) {
        return Ok(None);
    }
    let request = job_number(&mut db, job).await.map_err(|_| FAILED)?;
    Ok(Some((job, request, row.status.takes_check_ins())))
}

/// Renders `view` with `jar`'s cookies, or a 500 logged without any value.
fn page_response(jar: CookieJar, view: &CheckInView) -> Response {
    match view.render() {
        Ok(html) => (jar, Html(html)).into_response(),
        Err(_) => {
            tracing::warn!("check-in page template failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The view of answer `code` on link `raw` for request `request`.
fn view(request: i64, (raw, code): (&str, &str), slug: &str, csrf: String) -> CheckInView {
    CheckInView {
        request,
        action: format!("/c/{raw}/{code}"),
        label: answer_label(Language::En, slug),
        csrf,
        screen: Screen::Ask,
    }
}

/// `GET /c/{token}/{answer}`: shows the answer and its Send button, or the closed line; a bare 404
/// for a dead link or unknown code (SEC-026, SEC-037). Changes nothing but the pre-session cookie.
async fn show(
    State(page): State<CheckInPage>,
    Path((raw, code)): Path<(String, String)>,
    jar: CookieJar,
) -> Response {
    let Some((_, _, slug)) = read_code(&code) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (request, open) = match open_job(&page, &raw).await {
        Ok(Some((_, request, open))) => (request, open),
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(status) => return status.into_response(),
    };
    let Ok(jar) = ensure_pre_session(jar) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let screen = if open { Screen::Ask } else { Screen::Closed };
    let view = CheckInView {
        screen,
        ..view(request, (&raw, &code), slug, csrf)
    };
    page_response(jar, &view)
}

/// `POST /c/{token}/{answer}`: the CSRF token is checked first (`CsrfForm`), then the link and the
/// code; records the answer once through `answer_check_in` and shows the thanks, or the closed
/// line when the job is no longer Connected (TXT-034, JOB-015, D-2416).
async fn send(
    State(page): State<CheckInPage>,
    Path((raw, code)): Path<(String, String)>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let Some((answer, check, slug)) = read_code(&code) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (job, request) = match open_job(&page, &raw).await {
        Ok(Some((job, request, _))) => (job, request),
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(status) => return status.into_response(),
    };
    let Ok(mut db) = page.pool.acquire().await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    // Read only for PROBLEM, which has no button (D-2415).
    let inbound = Inbound {
        inbox: Uuid::nil(),
        safety: false,
    };
    let given = (answer, check, Contact::Call);
    let at = (page.clock.now(), Actor::System);
    let screen = match answer_check_in(&mut db, job, given, inbound, at).await {
        Ok(Guarded::Applied(_)) => Screen::Sent,
        Ok(Guarded::MovedOn) => Screen::Closed,
        Err(_) => {
            tracing::warn!("check-in answer not recorded");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let view = CheckInView {
        screen,
        ..view(request, (&raw, &code), slug, csrf)
    };
    page_response(jar, &view)
}

#[cfg(test)]
mod tests {
    //! TD-TXT-31 (TXT-034, SEC-026): the codes the email's buttons carry, the page's three views,
    //! and on the database a GET twice changes nothing (the POST: `tests/e2e/checkin.spec.ts`).
    //! Fake token only.

    use askama::Template;
    use askhava_core::api::{IdSecret, ManualClock};
    use askhava_jobs::api::quote_token::issue;
    use askhava_jobs::api::rules::CheckIn;
    use askhava_jobs::api::{Answer, JobStatus, create, current, move_to};
    use askhava_messaging::api::{CHECK_IN_ANSWERS, check_in_code};
    use askhava_ops::api::Actor;
    use axum::extract::{Path, State};
    use axum_extra::extract::cookie::CookieJar;
    use jiff::Timestamp;
    use secrecy::SecretString;
    use sqlx::PgPool;
    use std::sync::Arc;

    use super::{
        ButtonForm, CheckInPage, CheckInView, CsrfForm, Screen, WebSecurity, read_code, send, show,
    };

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn page(screen: Screen) -> String {
        let view = CheckInView {
            request: 1042,
            action: format!("/c/{TOKEN}/3-done"),
            label: "The work is done",
            csrf: "csrf-sample".to_owned(),
            screen,
        };
        view.render().expect("renders")
    }

    #[test]
    fn td_txt_31_every_button_code_reads_and_nothing_else_does() {
        for day in [3, 14] {
            for slug in CHECK_IN_ANSWERS {
                let (_, check, read) = read_code(&check_in_code(day, slug)).expect(slug);
                assert_eq!(read, slug);
                assert_eq!(check == CheckIn::Day3, day == 3);
            }
        }
        assert_eq!(read_code("14-ongoing").map(|r| r.0), Some(Answer::Ongoing));
        assert!(read_code("3-problem").is_none(), "PROBLEM is by reply");
    }

    #[test]
    fn td_txt_31_the_page_shows_the_answer_before_its_one_button() {
        let ask = page(Screen::Ask);
        let answer = ask.find("The work is done").expect("answer");
        let button = ask.find("<button").expect("button");
        assert!(answer < button, "answer first");
        assert_eq!(ask.matches("<button").count(), 1);
        assert!(ask.contains(&format!("action=\"/c/{TOKEN}/3-done\"")));
        assert!(ask.contains("name=\"csrf\" value=\"csrf-sample\""));
        assert!(ask.contains("request 1042"));
        for screen in [Screen::Sent, Screen::Closed] {
            let html = page(screen);
            assert!(!html.contains("<form"), "{screen:?}");
            assert!(html.contains("request 1042"), "{screen:?}");
        }
    }

    fn at(minute: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + minute * 60).expect("time")
    }

    async fn connected_job(pool: &PgPool) -> (sqlx::types::Uuid, String) {
        let mut db = pool.acquire().await.expect("db");
        let job = create(&mut db, at(0), false, Actor::System)
            .await
            .expect("job");
        let path = [
            JobStatus::Intake,
            JobStatus::Open,
            JobStatus::QuotesReady,
            JobStatus::AwaitingConfirm,
            JobStatus::PaymentPending,
            JobStatus::Connected,
        ];
        for (minute, pair) in (1..).zip(path.windows(2)) {
            let moved = move_to(&mut db, job, (pair[0], pair[1]), at(minute), Actor::System);
            assert!(moved.await.expect("move").applied().is_some());
        }
        let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
        let secret = IdSecret::new(1, key).expect("secret");
        let token = issue(&mut db, &secret, job, at(10)).await.expect("token");
        (job, token.as_str().to_owned())
    }

    async fn status_of(pool: &PgPool, job: sqlx::types::Uuid) -> JobStatus {
        let mut db = pool.acquire().await.expect("db");
        current(&mut db, job).await.expect("job").status
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_txt_31_a_get_twice_changes_nothing_and_a_dead_link_is_a_404(pool: PgPool) {
        let (job, token) = connected_job(&pool).await;
        let clock = Arc::new(ManualClock::new(at(20)));
        let security = WebSecurity::new(pool.clone(), clock.clone());
        let page = CheckInPage {
            pool: pool.clone(),
            clock,
            security,
        };
        for code in ["3-done", "3-done", "14-other"] {
            let path = Path((token.clone(), code.to_owned()));
            let reply = show(State(page.clone()), path, CookieJar::new()).await;
            assert_eq!(reply.status(), 200, "{code}");
        }
        assert_eq!(
            status_of(&pool, job).await,
            JobStatus::Connected,
            "GET records nothing"
        );
        for (raw, code) in [
            (token.as_str(), "3-problem"),
            (TOKEN, "3-done"),
            ("bad", "3-done"),
        ] {
            let path = Path((raw.to_owned(), code.to_owned()));
            let reply = show(State(page.clone()), path, CookieJar::new()).await;
            assert_eq!(reply.status(), 404, "{code}");
        }
    }

    async fn post(page: &CheckInPage, token: &str, code: &str) -> String {
        let form: ButtonForm = serde_json::from_str(r#"{"csrf":"sample"}"#).expect("form");
        let path = Path((token.to_owned(), code.to_owned()));
        let reply = send(State(page.clone()), path, CookieJar::new(), CsrfForm(form)).await;
        assert_eq!(reply.status(), 200, "{code}");
        let body = axum::body::to_bytes(reply.into_body(), 1 << 20)
            .await
            .expect("body");
        String::from_utf8(body.to_vec()).expect("utf8")
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_txt_31_the_post_records_the_answer_once(pool: PgPool) {
        let (job, token) = connected_job(&pool).await;
        let clock = Arc::new(ManualClock::new(at(20)));
        let security = WebSecurity::new(pool.clone(), clock.clone());
        let page = CheckInPage {
            pool: pool.clone(),
            clock,
            security,
        };
        assert!(post(&page, &token, "3-later").await.contains("Thanks."));
        assert_eq!(
            status_of(&pool, job).await,
            JobStatus::Connected,
            "LATER moves nothing"
        );
        assert!(post(&page, &token, "3-done").await.contains("Thanks."));
        assert_eq!(status_of(&pool, job).await, JobStatus::Done);
        let again = post(&page, &token, "3-done").await;
        assert!(
            again.contains("nothing to send for request"),
            "recorded once"
        );
        assert_eq!(status_of(&pool, job).await, JobStatus::Done);
    }
}
