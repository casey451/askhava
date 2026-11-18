//! The site's router and the banner read every site page makes (WEB-001, WEB-007, T101).
//!
//! [`site_routes`] mounts the compiled-in files of [`crate::assets`] at `/static/site/<name>` with
//! one `nest_service` and no route call of its own (D-3106: the security route scan reads
//! every route call as a page route); an unlisted name answers 404 and a method other than `GET` or `HEAD` 405 with
//! `Allow`. Each later site task merges its own page router in `server::build_router`. Every site
//! page extends `templates/site_layout.html` and sets its `banner_shown` field from
//! [`banner_shown`] before it renders (WEB-007, D-1843, D-3144), or from [`banner_shown_on`] on
//! a connection the page already holds (D-4098).

use axum::Router;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sqlx::{PgConnection, PgPool};

use crate::assets;

/// WEB-001: the site's router: the static files at `/static/site/<name>` (T101). Site pages added
/// by later tasks merge their own routers beside it in `server::build_router`.
pub fn site_routes() -> Router {
    Router::new().nest_service("/static/site", get(asset))
}

/// `GET /static/site/<name>`: a listed file with its MIME type, else 404 (WEB-001, SEC-013). The
/// browser keeps a file for an hour, so the next page's stylesheet comes from its cache and a
/// page's `autofocus` is not held back by a fetch (A11Y-030, D-3901).
async fn asset(uri: Uri) -> Response {
    let name = uri.path().strip_prefix('/').unwrap_or_default();
    match assets::find(name) {
        Some(file) => (
            [
                (header::CONTENT_TYPE, file.mime),
                (header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            file.bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// WEB-007, D-1843: whether a site page shows the status banner, read from `ops::api` before the
/// page renders. The banner is hidden by default, so a failed read shows the page without it and
/// logs one warning with no values (R5).
pub async fn banner_shown(pool: &PgPool) -> bool {
    match pool.acquire().await {
        Ok(mut db) => banner_shown_on(&mut db).await,
        Err(_) => banner_hidden(),
    }
}

/// WEB-007, D-1843, D-4098: [`banner_shown`] on a connection the page already holds, so a page
/// that reads more (the home page's numbers) takes one pooled connection, not two. The same
/// fail-closed rule: a failed read hides the banner and logs one warning with no values (R5).
pub async fn banner_shown_on(db: &mut PgConnection) -> bool {
    match askhava_ops::api::banner_shown(db).await {
        Ok(shown) => shown,
        Err(_) => banner_hidden(),
    }
}

/// The failed-read answer: one warning, no values, no banner (R5).
fn banner_hidden() -> bool {
    tracing::warn!("status banner read failed; the page shows no banner");
    false
}

#[cfg(test)]
mod tests {
    use askama::Template;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode, header};
    use tower::ServiceExt;

    use super::site_routes;

    /// WEB-007's sentence, word for word (`docs/spec/website.md:11`).
    const BANNER: &str = "Texting is temporarily unavailable. You can still use the chat on this site and your quote page.";

    /// A site page as T102 and later tasks build them: it extends the site layout.
    #[derive(Template)]
    #[template(
        source = "{% extends \"site_layout.html\" %}{% block title %}Sample page{% endblock %}{% block main %}<h1>Sample page</h1><p lang=\"es\">Hola</p>{% endblock %}",
        ext = "html"
    )]
    struct SamplePage {
        banner_shown: bool,
    }

    /// A GET or POST to `path` on the site router: the status, the `Allow` header and the body.
    async fn answer(method: Method, path: &str) -> (StatusCode, String, String) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty());
        let response = site_routes()
            .oneshot(request.expect("request"))
            .await
            .expect("answer");
        let status = response.status();
        let allow = response
            .headers()
            .get(header::ALLOW)
            .map_or(String::new(), |v| v.to_str().unwrap_or_default().to_owned());
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, allow, String::from_utf8_lossy(&body).into_owned())
    }

    /// TD-WEB-01 (WEB-007): the banner is hidden by default and shows WEB-007's sentence above the
    /// header when `ops` says it is on.
    #[test]
    fn td_web_01_banner_hidden_by_default_and_shown_when_on() {
        let hidden = SamplePage {
            banner_shown: false,
        }
        .render()
        .expect("renders");
        assert!(!hidden.contains(BANNER));
        assert!(!hidden.contains("site-banner"));
        let shown = SamplePage { banner_shown: true }.render().expect("renders");
        let banner = shown.find(BANNER).expect("banner shown");
        assert!(banner < shown.find("<header").expect("header"));
    }

    /// TD-WEB-01 (WEB-001, A11Y-003, A11Y-004, A11Y-017): a site page is plain server HTML with
    /// `lang`, a skip link into `main`, one `h1`, the help link in the header, the stylesheet and no
    /// script.
    #[test]
    fn td_web_01_site_page_needs_no_script() {
        let html = SamplePage {
            banner_shown: false,
        }
        .render()
        .expect("renders");
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("<a class=\"skip-link\" href=\"#main\">Skip to main content</a>"));
        assert!(html.contains("<main id=\"main\" tabindex=\"-1\">"));
        assert_eq!(html.matches("<h1").count(), 1);
        assert!(html.contains("<p lang=\"es\">Hola</p>"));
        let header =
            &html[html.find("<header").expect("header")..html.find("</header>").expect("end")];
        assert!(header.contains("<a href=\"/help\">Help</a>"));
        assert!(html.contains("<link rel=\"stylesheet\" href=\"/static/site/site.css\">"));
        assert!(!html.contains("<script"));
    }

    /// WEB-001: the stylesheet is served as CSS; an unknown name is a 404.
    #[tokio::test]
    async fn the_stylesheet_is_served_and_unknown_names_are_not() {
        let (status, _, body) = answer(Method::GET, "/static/site/site.css").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("--evergreen: #245849"));
        for path in [
            "/static/site/nope.css",
            "/static/site/",
            "/static/site/static/site.css",
        ] {
            assert_eq!(
                answer(Method::GET, path).await.0,
                StatusCode::NOT_FOUND,
                "{path}"
            );
        }
    }

    /// Another method on a static file is a 405 naming `GET` in `Allow`.
    #[tokio::test]
    async fn another_method_is_405_with_allow() {
        let (status, allow, _) = answer(Method::POST, "/static/site/site.css").await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert!(allow.contains("GET"), "{allow}");
    }
}
