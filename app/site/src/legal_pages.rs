//! The legal pages and the two data pages (T221; WEB-014, TERMS-021, TERMS-023, AI-018, D-3144,
//! D-3155, D-3844, D-3878).
//!
//! [`legal_routes`] serves `/terms` (the homeowner terms, then the business terms, each its own
//! section with its anchor and version line), `/privacy` and `/sms-terms` from the drafts in
//! `docs/legal/`, compiled in with `include_str!` by literal path (a missing document fails the
//! build) and rendered once from Markdown with `pulldown-cmark` when the router is built; raw HTML
//! in a draft is shown as text (SEC-011). [`accessibility_statement`] renders T115's statement
//! the same way. `/your-data` and `/business-data`, with their Spanish pages under `/es/`, are
//! written in their templates; they show AI-018's switched-off sentence while
//! `intake::api::ai_on` is false. Every page extends `templates/site_layout.html` (WORD-002 footer)
//! and sets `banner_shown` first (WEB-007). GET only; nothing is written.

use std::sync::Arc;

use askama::Template;
use askhava_core::api::Environment;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use sqlx::PgPool;

use crate::routes::banner_shown;

/// Both terms documents on one page (D-3144).
pub const TERMS_PATH: &str = "/terms";
/// The privacy policy (WEB-014).
pub const PRIVACY_PATH: &str = "/privacy";
/// The SMS terms (WEB-014).
pub const SMS_TERMS_PATH: &str = "/sms-terms";
/// The homeowner data page (TERMS-023).
pub const YOUR_DATA_PATH: &str = "/your-data";
/// The Spanish homeowner data page (TERMS-023, D-3844).
pub const ES_YOUR_DATA_PATH: &str = "/es/your-data";
/// The business data page (TERMS-021).
pub const BUSINESS_DATA_PATH: &str = "/business-data";
/// The Spanish business data page (TERMS-021, D-3844).
pub const ES_BUSINESS_DATA_PATH: &str = "/es/business-data";

/// One drafted legal document, rendered: its title, draft note, version line and HTML body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalDoc {
    /// The anchor of its section on a page that holds more than one document.
    pub anchor: &'static str,
    /// The document's `# ` heading.
    pub title: String,
    /// The first line: "Draft prepared with AI research; not reviewed by a lawyer." (TERMS-028).
    pub draft: String,
    /// The second line: the version and its date (TERMS-028).
    pub version: String,
    /// The rest, as HTML; its headings sit `shift` levels below the Markdown's.
    pub body: LegalHtml,
}

/// SEC-011: the HTML of a compiled-in `docs/legal/` draft, its raw HTML already turned into text.
/// Only [`LegalDoc::render`] builds one, so Askama prints it as HTML without a `|safe` in any
/// template, and nothing a homeowner or business typed can reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalHtml(String);

impl LegalHtml {
    /// The HTML as text, for tests and checks.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LegalHtml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl askama::filters::HtmlSafe for LegalHtml {}

impl LegalDoc {
    /// TERMS-028: renders `source` (draft note, version line, `# ` title, then the text), its
    /// headings moved down `shift` levels, raw HTML shown as text (SEC-011).
    fn render(anchor: &'static str, source: &str, shift: usize) -> Self {
        let mut lines = source.splitn(3, '\n');
        let draft = lines.next().unwrap_or_default().trim().to_owned();
        let version = lines.next().unwrap_or_default().trim().to_owned();
        let rest = lines.next().unwrap_or_default().trim_start();
        let (title, text) = match rest.strip_prefix("# ") {
            Some(titled) => titled.split_once('\n').unwrap_or((titled, "")),
            None => ("", rest),
        };
        let events =
            Parser::new_ext(text, Options::ENABLE_TABLES).map(|event| shifted(event, shift));
        let mut body = String::new();
        html::push_html(&mut body, events);
        Self {
            anchor,
            title: title.trim().to_owned(),
            draft,
            version,
            body: LegalHtml(body),
        }
    }
}

/// A Markdown event with its heading moved down `shift` levels (at most `h6`) and raw HTML as text.
fn shifted(event: Event<'_>, shift: usize) -> Event<'_> {
    let down = |level: HeadingLevel| {
        HeadingLevel::try_from((level as usize + shift).min(6)).unwrap_or(HeadingLevel::H6)
    };
    match event {
        Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) => Event::Start(Tag::Heading {
            level: down(level),
            id,
            classes,
            attrs,
        }),
        Event::End(TagEnd::Heading(level)) => Event::End(TagEnd::Heading(down(level))),
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(CowStr::from(raw.into_string())),
        other => other,
    }
}

/// D-3155: the accessibility statement for T115's `/accessibility` page, rendered as the legal
/// pages are.
#[must_use]
pub fn accessibility_statement() -> LegalDoc {
    let source = include_str!("../../../docs/legal/accessibility-statement.md");
    LegalDoc::render("accessibility-statement", source, 0)
}

/// The three legal pages' documents, rendered once (WEB-014, D-3144).
struct Docs {
    terms: [LegalDoc; 2],
    privacy: LegalDoc,
    sms: LegalDoc,
}

impl Docs {
    fn render() -> Self {
        let homeowner = include_str!("../../../docs/legal/homeowner-terms.md");
        let business = include_str!("../../../docs/legal/business-terms.md");
        let privacy = include_str!("../../../docs/legal/privacy-policy.md");
        let sms = include_str!("../../../docs/legal/sms-terms.md");
        Self {
            terms: [
                LegalDoc::render("homeowner-terms", homeowner, 1),
                LegalDoc::render("business-terms", business, 1),
            ],
            privacy: LegalDoc::render("privacy-policy", privacy, 0),
            sms: LegalDoc::render("sms-terms", sms, 0),
        }
    }

    /// `/terms`: one page titled "Terms", the homeowner terms first (D-3144).
    fn terms(&self) -> (&str, &[LegalDoc]) {
        ("Terms", self.terms.as_slice())
    }

    fn privacy(&self) -> (&str, &[LegalDoc]) {
        (&self.privacy.title, std::slice::from_ref(&self.privacy))
    }

    fn sms(&self) -> (&str, &[LegalDoc]) {
        (&self.sms.title, std::slice::from_ref(&self.sms))
    }
}

/// A page of drafted legal documents (WEB-014, D-3144).
#[derive(Template)]
#[template(path = "legal.html")]
struct LegalPage<'a> {
    banner_shown: bool,
    title: &'a str,
    docs: &'a [LegalDoc],
}

/// TERMS-023, AI-018: the homeowner data page in English, or Spanish when `es`.
#[derive(Template)]
#[template(path = "your_data.html")]
struct YourDataPage {
    banner_shown: bool,
    es: bool,
    ai_on: bool,
}

/// TERMS-021, AI-018: the business data page in English, or Spanish when `es`.
#[derive(Template)]
#[template(path = "business_data.html")]
struct BusinessDataPage {
    banner_shown: bool,
    es: bool,
    ai_on: bool,
}

/// Which data page a route serves.
#[derive(Clone, Copy)]
enum DataPage {
    Homeowner,
    Business,
}

/// WEB-014, TERMS-021, TERMS-023: the legal and data pages' router over `pool`; `env` is the
/// environment the AI switch is read in (AI-018). The documents are rendered here, once.
pub fn legal_routes(pool: PgPool, env: Environment) -> Router {
    let docs = Arc::new(Docs::render());
    let legal = |pick: fn(&Docs) -> (&str, &[LegalDoc])| {
        let (pool, docs) = (pool.clone(), Arc::clone(&docs));
        move || async move {
            let (title, shown) = pick(&docs);
            let shown_banner = banner_shown(&pool).await;
            respond(
                LegalPage {
                    banner_shown: shown_banner,
                    title,
                    docs: shown,
                }
                .render(),
            )
        }
    };
    let data = |page: DataPage, es: bool| {
        let pool = pool.clone();
        move || async move { respond(data_html(&pool, env, page, es).await) }
    };
    Router::new()
        .route(TERMS_PATH, get(legal(Docs::terms)))
        .route(PRIVACY_PATH, get(legal(Docs::privacy)))
        .route(SMS_TERMS_PATH, get(legal(Docs::sms)))
        .route(YOUR_DATA_PATH, get(data(DataPage::Homeowner, false)))
        .route(ES_YOUR_DATA_PATH, get(data(DataPage::Homeowner, true)))
        .route(BUSINESS_DATA_PATH, get(data(DataPage::Business, false)))
        .route(ES_BUSINESS_DATA_PATH, get(data(DataPage::Business, true)))
}

/// TERMS-021, TERMS-023, AI-018: a data page, with the AI switch read as it renders.
async fn data_html(
    pool: &PgPool,
    env: Environment,
    page: DataPage,
    es: bool,
) -> Result<String, askama::Error> {
    let banner_shown = banner_shown(pool).await;
    let ai_on = ai_on(pool, env).await;
    match page {
        DataPage::Homeowner => YourDataPage {
            banner_shown,
            es,
            ai_on,
        }
        .render(),
        DataPage::Business => BusinessDataPage {
            banner_shown,
            es,
            ai_on,
        }
        .render(),
    }
}

/// AI-018: whether the AI switch is on; a failed read shows the switched-off wording and logs one
/// warning with no values (R5).
async fn ai_on(pool: &PgPool, env: Environment) -> bool {
    let read = match pool.acquire().await {
        Ok(mut db) => askhava_intake::api::ai_on(&mut db, env).await.ok(),
        Err(_) => None,
    };
    read.unwrap_or_else(|| {
        tracing::warn!("AI switch read failed; the page shows the switched-off wording");
        false
    })
}

/// A rendered page, or a bare 500 that names nothing (R4, R5).
fn respond(page: Result<String, askama::Error>) -> Response {
    match page {
        Ok(html) => Html(html).into_response(),
        Err(_) => {
            tracing::error!("legal page failed to render");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    //! TD-WEB-01, TD-PRIV-06, TD-TERMS-04, TD-WEB-05 (WEB-014, TERMS-021, TERMS-023, AI-018): the
    //! pages as rendered, without a database. Fake data only.

    use askama::Template;

    use super::{
        BusinessDataPage, Docs, LegalDoc, LegalPage, YourDataPage, accessibility_statement,
    };

    /// AI-018's switched-off sentence (`docs/spec/ai.md:22`).
    const AI_OFF: &str = "Our texts are automated. When it's switched on, an AI service helps word our questions and write your job card from your request, after our software removes names, phone numbers, emails and street numbers. You see the card before it goes.";
    /// The start of AI-018's switched-on paragraph.
    const AI_ON: &str =
        "To sort your request and help word our questions, an AI service from Anthropic";
    /// The start of the Spanish switched-off sentence.
    const AI_OFF_ES: &str = "Nuestros mensajes de texto son automáticos.";

    fn homeowner(es: bool, ai_on: bool) -> String {
        let page = YourDataPage {
            banner_shown: false,
            es,
            ai_on,
        };
        page.render().expect("renders")
    }

    fn business(es: bool, ai_on: bool) -> String {
        let page = BusinessDataPage {
            banner_shown: false,
            es,
            ai_on,
        };
        page.render().expect("renders")
    }

    /// TD-TERMS-04 (AI-018): each data page in each language holds the switched-off wording while
    /// the switch is off and the switched-on paragraph while it is on, never both, never
    /// "AI-powered".
    #[test]
    fn td_terms_04_data_pages_follow_the_ai_switch() {
        for render in [homeowner, business] {
            let (off, on) = (render(false, false), render(false, true));
            assert!(off.contains(AI_OFF) && !off.contains(AI_ON));
            assert!(on.contains(AI_ON) && !on.contains(AI_OFF));
            let (es_off, es_on) = (render(true, false), render(true, true));
            assert!(es_off.contains(AI_OFF_ES) && !es_on.contains(AI_OFF_ES));
            assert!(es_on.contains("un servicio de IA de Anthropic lee"));
            for html in [off, on, es_off, es_on] {
                assert!(!html.to_lowercase().contains("ai-powered"));
            }
        }
    }

    /// TD-PRIV-06, TD-WEB-05 (TERMS-021, TERMS-023, A11Y-004): both data pages exist in both
    /// languages, with `lang`, one `h1`, the other language's link and the not-a-contractor footer.
    #[test]
    fn td_priv_06_data_pages_in_both_languages() {
        for (render, en_link, es_link) in [
            (
                homeowner as fn(bool, bool) -> String,
                "/your-data",
                "/es/your-data",
            ),
            (business, "/business-data", "/es/business-data"),
        ] {
            let (en, es) = (render(false, false), render(true, false));
            assert!(en.contains("<html lang=\"en\">") && es.contains("<html lang=\"es\">"));
            assert_eq!(en.matches("<h1").count(), 1);
            assert_eq!(es.matches("<h1").count(), 1);
            assert!(en.contains(&format!("href=\"{es_link}\"")));
            assert!(es.contains(&format!("href=\"{en_link}\"")));
            assert!(
                en.contains("is not a contractor and does not perform, bid on or supervise work.")
            );
            assert!(es.contains("no es contratista y no realiza, cotiza ni supervisa trabajos."));
            assert!(!es.contains("<h2>Homeowners</h2>"));
        }
    }

    /// TD-WEB-01 (WEB-014, D-3144): /terms holds the homeowner terms, then the business terms,
    /// each its own section with its anchor, heading, draft note and version line; section
    /// headings sit below the document's.
    #[test]
    fn td_web_01_terms_page_holds_both_documents_in_order() {
        let docs = Docs::render();
        let (title, shown) = docs.terms();
        let page = LegalPage {
            banner_shown: false,
            title,
            docs: shown,
        };
        let html = page.render().expect("renders");
        let homeowner = html
            .find("<section id=\"homeowner-terms\"")
            .expect("homeowner terms");
        let business = html
            .find("<section id=\"business-terms\"")
            .expect("business terms");
        assert!(homeowner < business);
        assert_eq!(html.matches("<h1").count(), 1);
        assert_eq!(
            html.matches("<p class=\"legal-version\">Version ").count(),
            2
        );
        assert!(html.contains("<h2>Laftly Homeowner Terms</h2>"));
        assert!(html.contains("<h3>1. What Laftly is</h3>"));
        assert!(!html.contains("<h1>Laftly"));
    }

    /// TD-WEB-01 (WEB-014, TERMS-028): the privacy policy and SMS terms are their own pages, the
    /// title the page's `h1`, sections `h2`, the draft note and version line shown, tables kept.
    #[test]
    fn td_web_01_privacy_and_sms_pages_render_their_drafts() {
        let docs = Docs::render();
        for (title, shown) in [docs.privacy(), docs.sms()] {
            let page = LegalPage {
                banner_shown: false,
                title,
                docs: shown,
            };
            let html = page.render().expect("renders");
            assert_eq!(html.matches("<h1").count(), 1);
            assert!(html.contains("Draft prepared with AI research; not reviewed by a lawyer."));
            assert!(html.contains("<h2>1. "));
        }
        assert_eq!(docs.privacy().0, "Laftly Privacy Policy");
        assert!(docs.privacy.body.as_str().contains("<table>"));
        assert!(docs.privacy.body.as_str().contains(AI_OFF));
    }

    /// SEC-011, D-3155: raw HTML in a draft is shown as text; the accessibility statement renders.
    #[test]
    fn raw_html_is_text_and_the_statement_renders() {
        let doc = LegalDoc::render(
            "x",
            "Draft\nVersion 1\n\n# Title\n\n## Part\n\n<script>x</script>\n",
            1,
        );
        assert_eq!(doc.title, "Title");
        assert!(doc.body.as_str().contains("<h3>Part</h3>"));
        assert!(!doc.body.as_str().contains("<script>"));
        let statement = accessibility_statement();
        assert!(statement.title.ends_with("Accessibility Statement"));
        assert!(statement.version.starts_with("Version "));
    }
}
