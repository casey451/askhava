//! The homeowner's private feedback page at `/f/{token}` (JOB-017, T230, D-083, D-466, D-493).
//!
//! The feedback request texted with the move to Done (`jobs::api::FEEDBACK_REQUEST`, T097) links
//! here with the job's quote-page link token (`jobs::api::quote_token::resolve`): the page opens
//! only while the job is Done (`JobStatus::takes_feedback`) and the token is live and not 30 days
//! past the close (PICK-011's rule); anything else is a bare 404 (SEC-037). The homeowner writes to
//! Ask Hava in one box and sees one share box, "Ask Hava may show what I wrote on askhava.com with
//! my first name and town", unticked, asked of everyone, with no reward offered. `POST` stores the
//! words and the tick exactly as sent through `jobs::api::feedback::store` (one per job; a second
//! send shows the thanks and stores nothing; a job that left Done after the page opened is a 404
//! and stores nothing). What may appear on askhava.com is
//! `jobs::api::feedback::shown`: ticked, not removed for one of the four JOB-017 reasons, not from
//! the owner, a relative of the owner or a business. This page never shows other feedback and no
//! page says shown feedback is all the feedback received. No name, phone or address is read. The
//! page is `noindex`, `no-store`, behind the cross-site refusal, and its form carries the
//! pre-session CSRF token (SEC-042).

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, LinkToken};
use askhava_jobs::api::feedback::{MAX_CHARS, given, store};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{current, job_number};
use axum::Router;
use axum::extract::{FromRef, Path, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum_extra::extract::cookie::CookieJar;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::quote_page::token_expired;
use crate::security::{CsrfField, CsrfForm, WebSecurity, no_store, refuse_cross_site};
use crate::session::{csrf_token, ensure_pre_session};

/// The feedback page's route; the token is the job's quote-page link token (JOB-017).
pub const FEEDBACK_ROUTE: &str = "/f/{token}";

/// The page's own stylesheet: the share box drawn with a dark 2px edge and a filled centre when
/// ticked, so its edge is at least 3:1 against the page (A11Y-007; the site's look is T101's).
pub const FEEDBACK_CSS_ROUTE: &str = "/static/portals/feedback.css";

/// The stylesheet [`FEEDBACK_CSS_ROUTE`] serves (same origin, so the CSP allows it, SEC-013).
const FEEDBACK_CSS: &str = "#feedback-share{appearance:none;width:24px;height:24px;margin:0 8px 0 0;\
vertical-align:middle;border:2px solid #1f1d1b;border-radius:3px;background:#fff}\
#feedback-share:checked{background:#1f1d1b;box-shadow:inset 0 0 0 4px #fff}\
#feedback-body{display:block;width:100%;max-width:40rem;box-sizing:border-box}\n";

/// JOB-017, WORD-001: the share box's label, word for word, with the display name and the public
/// domain from the brand file (D-1932, T294).
#[must_use]
pub fn share_label() -> String {
    let brand = askhava_core::api::brand();
    format!(
        "{} may show what I wrote on {} with my first name and town",
        brand.display_name(),
        brand.domain()
    )
}

/// The page's state: the pool, the clock and the cross-site and CSRF refusals' event log.
#[derive(Clone)]
struct FeedbackPage {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<FeedbackPage> for WebSecurity {
    fn from_ref(page: &FeedbackPage) -> Self {
        page.security.clone()
    }
}

/// The posted form: the words, the share box (absent when unticked) and the CSRF token.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedbackForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    body: String,
    share: Option<String>,
}

impl CsrfField for FeedbackForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// Why a send was not stored; the page says so in its alert summary and beside the box
/// (A11Y-011, A11Y-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Problem {
    /// Nothing but spaces was written.
    Empty,
    /// More than [`MAX_CHARS`] characters.
    TooLong,
}

impl Problem {
    /// The line the page shows for this problem.
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Empty => "Please write something before you send.",
            Self::TooLong => "Keep it to 2,000 characters or fewer.",
        }
    }
}

/// The page's template (`templates/feedback.html`).
#[derive(Template)]
#[template(path = "feedback.html")]
pub(crate) struct FeedbackView {
    /// The request (job) number the homeowner was texted.
    pub(crate) request: i64,
    /// The link token the form posts back to.
    pub(crate) token: String,
    /// Feedback is stored: the thanks shows instead of the form.
    pub(crate) sent: bool,
    /// The form's CSRF token (SEC-042).
    pub(crate) csrf: String,
    /// The words sent back after a problem, so nothing typed is lost.
    pub(crate) body: String,
    /// The share box as sent back after a problem; unticked on first view (JOB-017).
    pub(crate) share: bool,
    /// What stopped the send, if anything ([`Problem::message`]).
    pub(crate) error: Option<&'static str>,
    /// The share box's label ([`share_label`]).
    pub(crate) label: String,
}

impl FeedbackView {
    /// JOB-017: the first view of the form for request `request` at link `token`: empty, share
    /// box unticked.
    pub(crate) fn form(request: i64, token: String, csrf: String) -> Self {
        Self {
            request,
            token,
            sent: false,
            csrf,
            body: String::new(),
            share: false,
            error: None,
            label: share_label(),
        }
    }
}

/// JOB-017: what is wrong with `body` before it is stored, if anything.
pub(crate) fn check(body: &str) -> Option<Problem> {
    let words = body.trim();
    if words.is_empty() {
        Some(Problem::Empty)
    } else if words.chars().count() > MAX_CHARS {
        Some(Problem::TooLong)
    } else {
        None
    }
}

/// The feedback page's router over `pool`, timed by `clock` (JOB-017): `GET` and `POST`
/// [`FEEDBACK_ROUTE`] and its stylesheet behind the cross-site refusal, `no-store` and `noindex`.
/// `server::build_router` merges it.
pub fn feedback_routes(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = FeedbackPage {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(FEEDBACK_ROUTE, get(show).post(send))
        .route(FEEDBACK_CSS_ROUTE, get(stylesheet))
        .with_state(state)
        .layer(from_fn(noindex))
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// `X-Robots-Tag: noindex, nofollow` on every feedback-page response (it is private).
async fn noindex(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let value = HeaderValue::from_static("noindex, nofollow");
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-robots-tag"), value);
    response
}

/// The answer to a read that failed; logged by nothing, carries no value.
const FAILED: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;

/// `GET` [`FEEDBACK_CSS_ROUTE`]: the page's stylesheet (A11Y-007).
async fn stylesheet() -> Response {
    let css = HeaderValue::from_static("text/css; charset=utf-8");
    ([(axum::http::header::CONTENT_TYPE, css)], FEEDBACK_CSS).into_response()
}

/// The job behind `raw` and its request number, when the page is open for it now: a live token
/// on a Done job not 30 days past its close (JOB-017, PICK-011, SEC-037).
async fn open_job(page: &FeedbackPage, raw: &str) -> Result<Option<(Uuid, i64)>, StatusCode> {
    let Ok(token) = LinkToken::parse(raw) else {
        return Ok(None);
    };
    let mut db = page.pool.acquire().await.map_err(|_| FAILED)?;
    let Some(job) = resolve(&mut db, &token).await.map_err(|_| FAILED)? else {
        return Ok(None);
    };
    let row = current(&mut db, job).await.map_err(|_| FAILED)?;
    let expired = token_expired(row.status.is_final(), row.status_at, page.clock.now());
    if !row.status.takes_feedback() || expired {
        return Ok(None);
    }
    let request = job_number(&mut db, job).await.map_err(|_| FAILED)?;
    Ok(Some((job, request)))
}

/// Renders `view` with `jar`'s cookies, or a 500 logged without any value.
fn page_response(jar: CookieJar, view: &FeedbackView) -> Response {
    match view.render() {
        Ok(html) => (jar, Html(html)).into_response(),
        Err(_) => {
            tracing::warn!("feedback page template failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /f/{token}`: the form, or the thanks once feedback is stored; a bare 404 when the page is
/// not open (JOB-017, SEC-037). Changes nothing but the pre-session cookie (SEC-042).
async fn show(
    State(page): State<FeedbackPage>,
    Path(raw): Path<String>,
    jar: CookieJar,
) -> Response {
    let (job, request) = match open_job(&page, &raw).await {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(status) => return status.into_response(),
    };
    let Ok(jar) = ensure_pre_session(jar) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let Ok(mut db) = page.pool.acquire().await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(sent) = given(&mut db, job).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let view = FeedbackView {
        sent,
        ..FeedbackView::form(request, raw.clone(), csrf)
    };
    page_response(jar, &view)
}

/// What a send that passed [`check`] came to (JOB-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kept {
    /// The words and the tick are stored now.
    Stored,
    /// The job already had feedback (a repeat send); nothing new is stored.
    Already,
    /// The job left Done after the page opened, so nothing is stored and the page is gone.
    NotOpen,
}

/// JOB-017: stores job `job`'s `feedback` at `at` and says which [`Kept`] case happened, so a
/// repeat send shows the thanks while a job that left Done in between does not.
async fn keep(
    db: &mut PgConnection,
    job: Uuid,
    feedback: (&str, bool),
    at: Timestamp,
) -> Result<Kept, StatusCode> {
    if store(db, job, feedback, at)
        .await
        .map_err(|_| FAILED)?
        .is_some()
    {
        return Ok(Kept::Stored);
    }
    let already = given(db, job).await.map_err(|_| FAILED)?;
    Ok(if already {
        Kept::Already
    } else {
        Kept::NotOpen
    })
}

/// `POST /f/{token}`: stores the words and the tick as sent, then shows the thanks (also for a
/// repeat send, which stores nothing); a job that left Done after the page opened is a bare 404
/// with nothing stored; an empty or too long send shows the form again with what was typed and the
/// problem (JOB-017, A11Y-018).
async fn send(
    State(page): State<FeedbackPage>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<FeedbackForm>,
) -> Response {
    let (job, request) = match open_job(&page, &raw).await {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(status) => return status.into_response(),
    };
    let share = form.share.is_some();
    let csrf = csrf_token(&jar).unwrap_or_default();
    if let Some(problem) = check(&form.body) {
        let view = FeedbackView {
            body: form.body,
            share,
            error: Some(problem.message()),
            ..FeedbackView::form(request, raw.clone(), csrf)
        };
        return (StatusCode::UNPROCESSABLE_ENTITY, page_response(jar, &view)).into_response();
    }
    let Ok(mut db) = page.pool.acquire().await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match keep(&mut db, job, (form.body.trim(), share), page.clock.now()).await {
        Ok(Kept::Stored | Kept::Already) => {}
        Ok(Kept::NotOpen) => return StatusCode::NOT_FOUND.into_response(),
        Err(status) => {
            tracing::warn!("feedback not stored");
            return status.into_response();
        }
    }
    let view = FeedbackView {
        sent: true,
        ..FeedbackView::form(request, raw.clone(), csrf)
    };
    page_response(jar, &view)
}

#[cfg(test)]
mod tests {
    //! TD-JOB-10 (JOB-017): the page's share box, its words and errors, and the storage rules:
    //! only ticked feedback is shown, a negative one like a positive one, a removal only for the
    //! four reasons and logged, never the owner's, a relative's or a business's. Fake words only.

    use askama::Template;
    use askhava_jobs::api::feedback::{Author, Removal, Shown, mark_author, remove, shown, store};
    use askhava_jobs::api::{JobStatus, create, move_to};
    use askhava_ops::api::{Actor, EXPORT_PAGE_MAX, export_audit};
    use jiff::Timestamp;
    use sqlx::PgConnection;
    use sqlx::types::Uuid;

    use super::{FeedbackView, Kept, MAX_CHARS, Problem, check, keep, share_label};

    type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn form() -> FeedbackView {
        FeedbackView::form(1042, TOKEN.to_owned(), "csrf-sample".to_owned())
    }

    /// Words no page may carry: a claim that shown feedback is all of it, or a reward.
    const BANNED: [&str; 8] = [
        "all feedback",
        "all reviews",
        "every review",
        "all of our",
        "reward",
        "discount",
        "gift card",
        "entry",
    ];

    #[test]
    fn td_job_10_the_share_box_is_unticked_with_the_job_017_label() {
        let html = form().render().expect("form renders");
        assert_eq!(
            share_label(),
            "Ask Hava may show what I wrote on askhava.com with my first name and town"
        );
        assert!(html.contains(&format!(
            "<label for=\"feedback-share\">{}</label>",
            share_label()
        )));
        let start = html.find("id=\"feedback-share\"").expect("share box");
        let tag = &html[start..start + html[start..].find('>').expect("tag end")];
        assert!(!tag.contains("checked"), "unticked on first view");
        assert_eq!(html.matches("type=\"checkbox\"").count(), 1, "one box");
        assert!(html.contains(&format!("action=\"/f/{TOKEN}\"")));
        assert!(html.contains("name=\"csrf\" value=\"csrf-sample\""));
        assert!(html.contains("never shown on quote cards"));
    }

    #[test]
    fn td_job_10_no_page_claims_all_feedback_or_offers_a_reward() {
        let sent = FeedbackView {
            sent: true,
            ..form()
        };
        for html in [form().render(), sent.render()] {
            let text = html.expect("renders").to_lowercase();
            for banned in BANNED {
                assert!(!text.contains(banned), "page says {banned}");
            }
        }
    }

    #[test]
    fn td_job_10_a_bad_send_keeps_the_words_and_tick_and_says_why() {
        assert_eq!(check("   "), Some(Problem::Empty));
        assert_eq!(check(&"a".repeat(MAX_CHARS + 1)), Some(Problem::TooLong));
        assert_eq!(check(&"a".repeat(MAX_CHARS)), None);
        let view = FeedbackView {
            body: "Sample words".to_owned(),
            share: true,
            error: Some(Problem::TooLong.message()),
            ..form()
        };
        let html = view.render().expect("renders");
        assert!(html.contains("<title>Error: "));
        assert!(html.contains("role=\"alert\""));
        assert!(html.contains("<a href=\"#feedback-body\">Keep it to 2,000 characters"));
        assert!(html.contains(">Sample words</textarea>"));
        assert!(html.contains("value=\"yes\" checked"));
        assert!(html.contains("aria-invalid=\"true\""));
        assert_eq!(
            Problem::Empty.message(),
            "Please write something before you send."
        );
    }

    fn at(minute: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + minute * 60).expect("fixed time")
    }

    /// The arrows from Intake to Done (JOB-002).
    const TO_DONE: [JobStatus; 7] = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
        JobStatus::Done,
    ];

    /// A job walked by the system along `path` from Intake.
    async fn job(db: &mut PgConnection, path: &[JobStatus]) -> Uuid {
        let id = create(db, at(0), false, Actor::System).await.expect("job");
        for (i, pair) in path.windows(2).enumerate() {
            let minute = i64::try_from(i).expect("small") + 1;
            let moved = move_to(db, id, (pair[0], pair[1]), at(minute), Actor::System).await;
            assert!(moved.expect("move").applied().is_some(), "moved");
        }
        id
    }

    async fn done_with(db: &mut PgConnection, body: &str, share: bool) -> Uuid {
        let j = job(db, &TO_DONE).await;
        store(db, j, (body, share), at(10))
            .await
            .expect("stored")
            .expect("new")
    }

    async fn audit_actions(db: &mut PgConnection, id: Uuid) -> Vec<String> {
        let rows = export_audit(db, 0, EXPORT_PAGE_MAX).await.expect("audit");
        rows.into_iter()
            .filter(|r| r.subject_id == id)
            .map(|r| r.action)
            .collect()
    }

    fn bodies(rows: &[Shown]) -> Vec<&str> {
        let mut words: Vec<&str> = rows.iter().map(|r| r.body.as_str()).collect();
        words.sort_unstable();
        words
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_job_10_only_ticked_feedback_is_shown_negative_like_positive(mut db: Db) {
        done_with(&mut db, "Sample: great work", true).await;
        let late = done_with(&mut db, "Sample: terrible, late", true).await;
        done_with(&mut db, "Sample: private note", false).await;
        let rows = shown(&mut db).await.expect("shown");
        assert_eq!(
            bodies(&rows),
            ["Sample: great work", "Sample: terrible, late"]
        );
        let job_of_late = rows.iter().find(|r| r.id == late).expect("late").job;
        let again = store(&mut db, job_of_late, ("Sample: again", true), at(13)).await;
        assert_eq!(again.expect("read"), None, "one per job");
        let open = job(&mut db, &TO_DONE[..2]).await;
        let early = store(&mut db, open, ("Sample: early", true), at(14)).await;
        assert_eq!(early.expect("read"), None, "only after Done");
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_job_10_removal_only_for_the_four_reasons_and_logged(mut db: Db) {
        let owner = Actor::Owner(Uuid::nil());
        let reasons = [
            (
                Removal::PersonalDetails,
                "feedback_removed_personal_details",
            ),
            (Removal::Abusive, "feedback_removed_abusive"),
            (Removal::Fake, "feedback_removed_fake"),
            (Removal::Unrelated, "feedback_removed_unrelated"),
        ];
        for (reason, action) in reasons {
            let id = done_with(&mut db, "Sample words", true).await;
            assert!(remove(&mut db, id, reason, (owner, at(20))).await.unwrap());
            let twice = remove(&mut db, id, reason, (owner, at(21))).await.unwrap();
            assert!(!twice, "removed once");
            assert_eq!(audit_actions(&mut db, id).await, [action]);
            assert!(shown(&mut db).await.unwrap().is_empty());
        }
        let private = done_with(&mut db, "Sample unticked", false).await;
        let none = remove(&mut db, private, Removal::Fake, (owner, at(20))).await;
        assert!(
            !none.unwrap(),
            "unticked feedback is never shown, so never removed"
        );
        assert!(audit_actions(&mut db, private).await.is_empty());
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_job_10_owner_relative_or_business_feedback_is_never_shown(mut db: Db) {
        let owner = Actor::Owner(Uuid::nil());
        for author in [Author::Owner, Author::OwnerRelative, Author::Business] {
            let id = done_with(&mut db, "Sample words", true).await;
            assert!(mark_author(&mut db, id, author, owner).await.unwrap());
            assert!(shown(&mut db).await.unwrap().is_empty(), "{author:?}");
            assert_eq!(audit_actions(&mut db, id).await, ["feedback_author_marked"]);
        }
        let unknown = mark_author(&mut db, Uuid::nil(), Author::Owner, owner).await;
        assert!(!unknown.unwrap());
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_job_10_a_repeat_send_is_thanked_but_a_job_gone_from_done_is_not(mut db: Db) {
        let done = job(&mut db, &TO_DONE).await;
        let first = keep(&mut db, done, ("Sample words", true), at(10)).await;
        assert_eq!(first.expect("kept"), Kept::Stored);
        let again = keep(&mut db, done, ("Sample again", false), at(11)).await;
        assert_eq!(again.expect("kept"), Kept::Already);
        let open = job(&mut db, &TO_DONE[..2]).await;
        let gone = keep(&mut db, open, ("Sample words", true), at(12)).await;
        assert_eq!(gone.expect("kept"), Kept::NotOpen);
        assert!(shown(&mut db).await.unwrap().iter().all(|r| r.job == done));
    }
}

#[cfg(test)]
mod brand_tests {
    //! Pages from the brand file (TD-WORD-37; WORD-001, WORD-002, WEB-001, SEC-025; T294, D-1932): under
    //! the embedded brand the layout's title suffix and home link, the result page's contact card, the
    //! share-box label, the sign-up notice lines, the "Why was I charged?" lead line and the emailed
    //! sign-in site are byte-identical to the literals they replaced. Fake request numbers, a fake
    //! 64-hex token and fake IDs only.

    use askama::Template;
    use askhava_payments::api::{ChargeReceipt, Tier};
    use jiff::Timestamp;
    use sqlx::types::Uuid;

    use super::{FeedbackView, share_label};
    use crate::business_auth::site;
    use crate::business_signup::notice_line;
    use crate::pick_flow::{PageLanguage, result_page};
    use crate::receipts_page::{Basis, WhyCharged, why_charged_page};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// TD-WORD-37 (WORD-001, WEB-001): `base.html`'s title suffix and home link carry the brand.
    #[test]
    fn td_word_37_layout_title_and_home_link_are_todays() {
        let html = FeedbackView::form(1042, TOKEN.to_owned(), "csrf-sample".to_owned())
            .render()
            .expect("form renders");
        assert!(html.contains("<title>Feedback on request 1042 | Ask Hava</title>"));
        assert!(html.contains("<header>\n<p><a href=\"/\">Ask Hava</a></p>\n</header>"));
        assert!(html.contains("<label for=\"feedback-share\">Ask Hava may show what I wrote on askhava.com with my first name and town</label>"));
        assert_eq!(
            share_label(),
            "Ask Hava may show what I wrote on askhava.com with my first name and town"
        );
    }

    /// TD-WORD-37 (WORD-001): the result page's contact card, its file name and the share line.
    #[test]
    fn td_word_37_contact_card_link_is_todays() {
        let html = result_page(1042, 2, TOKEN.to_owned(), None).expect("result renders");
        assert!(html.contains(
            "<p><a href=\"data:text/vcard;charset=utf-8,BEGIN%3AVCARD%0D%0AVERSION%3A3.0%0D%0AFN%3AAsk%20Hava%0D%0AORG%3AAsk%20Hava%20LLC%0D%0AURL%3Ahttps%3A%2F%2Faskhava.com%0D%0AEND%3AVCARD%0D%0A\" download=\"ask-hava.vcf\">Add Ask Hava to your contacts</a></p>\n<p>Know someone who needs quotes? Send them askhava.com.</p>"
        ));
    }

    /// TD-WORD-37 (SEC-025): the emailed sign-in link is on the brand's domain only.
    #[test]
    fn td_word_37_sign_in_site_is_the_brand_domain() {
        assert_eq!(site(), "https://askhava.com");
    }

    /// TD-WORD-37 (WORD-001): the sign-up notice lines while texting is on, English and Spanish.
    #[test]
    fn td_word_37_notice_lines_are_todays() {
        assert_eq!(
            notice_line(true, PageLanguage::En),
            (
                "Choose how you get job requests and picks: email, text or both. Ask Hava never calls you with a recording.".to_owned(),
                "en"
            )
        );
        assert_eq!(
            notice_line(true, PageLanguage::Es),
            (
                "Elija cómo recibir las solicitudes de trabajo y las selecciones: correo electrónico, texto o ambos. Ask Hava nunca le llama con una grabación.".to_owned(),
                "es"
            )
        );
    }

    /// TD-WORD-37 (WORD-001, WORD-002): the "Why was I charged?" on-site lead line and contractor line.
    #[test]
    fn td_word_37_why_charged_lines_are_todays() {
        let facts = WhyCharged {
            receipt: ChargeReceipt {
                pick: Uuid::nil(),
                business: Uuid::nil(),
                job: Uuid::nil(),
                fee_cents: 2500,
                tier: Some(Tier::Small),
                charged_at: Timestamp::UNIX_EPOCH,
            },
            job_number: 1042,
            quote: "$80.00 visit fee".to_owned(),
            basis: Basis::OnSite,
        };
        let html = why_charged_page(&facts).expect("page renders");
        assert!(html.contains("You set your price after seeing the job, so Ask Hava used the usual visit fee for this kind of work. That makes it a small job."));
        assert!(html.contains(
            "<li>Ask Hava is not a contractor and does not perform, bid on or supervise work.</li>"
        ));
    }
}
