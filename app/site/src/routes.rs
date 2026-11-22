//! The site's router and the banner read every site page makes (WEB-001, WEB-007, T101).
//!
//! [`site_routes`] mounts the compiled-in files of [`crate::assets`] at `/static/site/<name>` with
//! one `nest_service` and no route call of its own (D-3106: the security route scan reads
//! every route call as a page route); an unlisted name answers 404 and a method other than `GET` or `HEAD` 405 with
//! `Allow`. Each later site task merges its own page router in `server::build_router`. Every site
//! page extends `templates/site_layout.html` and sets its `banner_shown` field from
//! [`banner_shown`] before it renders (WEB-007, D-1843, D-3144), or from [`banner_shown_on`] on
//! a connection the page already holds (D-4098).
//!
//! Caching and compression (T616, WEB-030, D-4252, D-5318): the file service is wrapped, inside
//! out, in a `CompressionLayer` that gzips or Brotli-compresses only `text/css` and
//! `image/svg+xml` (static files only: no page passes through it, so no CSRF token is ever
//! compressed, the BREACH rule), then in [`revalidate`], which sees the coding that layer chose
//! and gives the strong `ETag` its per-coding suffix and answers a matching `If-None-Match` 304.

use axum::Router;
use axum::extract::Request;
use axum::http::{Extensions, HeaderMap, HeaderValue, StatusCode, Uri, Version, header};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sqlx::{PgConnection, PgPool};
use tower_http::compression::CompressionLayer;

use crate::assets;

/// WEB-001: the site's router: the static files at `/static/site/<name>` (T101), compressed when
/// they are text and revalidated by `ETag` (WEB-030, D-4252). Site pages added by later tasks
/// merge their own routers beside it in `server::build_router`.
pub fn site_routes() -> Router {
    let files = get(asset)
        .layer(CompressionLayer::new().compress_when(compressible))
        .layer(from_fn(revalidate));
    Router::new().nest_service("/static/site", files)
}

/// `GET /static/site/<name>`: a listed file with its MIME type, its cache rule and a strong
/// `ETag` of the first 16 hex digits of its SHA256, else 404 (WEB-001, SEC-013, WEB-030). The
/// stylesheet keeps its hour, so the next page's stylesheet comes from the browser's cache and a
/// page's `autofocus` is not held back by a fetch (A11Y-030, D-3901).
async fn asset(uri: Uri) -> Response {
    let name = uri.path().strip_prefix('/').unwrap_or_default();
    match (assets::find(name), assets::sha256(name)) {
        (Some(file), Some(digest)) => (
            [
                (header::CONTENT_TYPE, file.mime.to_owned()),
                (header::CACHE_CONTROL, file.cache_control().to_owned()),
                (
                    header::ETAG,
                    format!("\"{}\"", digest.get(..16).unwrap_or(digest)),
                ),
            ],
            file.bytes,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// WEB-030, D-4252: only a successful stylesheet or SVG answer is compressed; fonts, photos and
/// PNGs are compressed already.
fn compressible(status: StatusCode, _: Version, headers: &HeaderMap, _: &Extensions) -> bool {
    let kind = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    status == StatusCode::OK && (kind.starts_with("text/css") || kind.starts_with("image/svg+xml"))
}

/// WEB-030, D-5318: outside the compression, so it sees the chosen `Content-Encoding`: the
/// `ETag` gains `-gzip` or `-br` for a compressed answer, `Vary: Accept-Encoding` is set, and an
/// `If-None-Match` naming the tag (or `*`) gets a 304 with the `ETag`, `Cache-Control` and `Vary`
/// and no body. An answer with no `ETag` (a 404, a 405) passes unchanged.
async fn revalidate(request: Request, next: Next) -> Response {
    let wanted = request.headers().get(header::IF_NONE_MATCH).cloned();
    let mut response = next.run(request).await;
    let Some(tag) = coded_etag(response.headers()) else {
        return response;
    };
    let headers = response.headers_mut();
    headers.insert(header::ETAG, tag.clone());
    if !headers.contains_key(header::VARY) {
        headers.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    if wanted.is_some_and(|list| names_tag(&list, &tag)) {
        return not_modified(response.headers());
    }
    response
}

/// The handler's `ETag` with the suffix of the answer's `Content-Encoding` (none, `gzip`, `br`),
/// or `None` when the answer has no `ETag`.
fn coded_etag(headers: &HeaderMap) -> Option<HeaderValue> {
    let plain = headers.get(header::ETAG)?.to_str().ok()?;
    let coding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok());
    let Some(coding) = coding else {
        return HeaderValue::from_str(plain).ok();
    };
    let digits = plain.trim_matches('"');
    HeaderValue::from_str(&format!("\"{digits}-{coding}\"")).ok()
}

/// Whether an `If-None-Match` list names `tag` or is `*`, by the weak comparison RFC 9110
/// section 13.1.2 sets for it (a `W/` prefix is ignored).
fn names_tag(list: &HeaderValue, tag: &HeaderValue) -> bool {
    let Ok(list) = list.to_str() else {
        return false;
    };
    let tag = tag.to_str().unwrap_or_default();
    list.split(',')
        .map(str::trim)
        .any(|entry| entry == "*" || entry.strip_prefix("W/").unwrap_or(entry) == tag)
}

/// The 304: no body, and only the `ETag`, `Cache-Control` and `Vary` of the full answer.
fn not_modified(full: &HeaderMap) -> Response {
    let mut answer = StatusCode::NOT_MODIFIED.into_response();
    for name in [header::ETAG, header::CACHE_CONTROL, header::VARY] {
        if let Some(value) = full.get(&name) {
            answer.headers_mut().insert(name, value.clone());
        }
    }
    answer
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
