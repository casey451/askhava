//! The answer for a path no route serves (T615; WEB-027, WEB-007, A11Y-003, D-4252).
//!
//! [`not_found`] is the app router's one fallback (`server::build_router`): a `GET` or `HEAD` on an
//! unknown path answers 404 with `templates/not_found.html`, a site page in the site layout (the
//! header links home and to Help, the footer follows), with `Cache-Control: no-store`; any other
//! method gets a bare 404 that reads nothing. The server's layers wrap the fallback, so the 404
//! carries every security header the other pages carry (SEC-012). A scanner's thousand paths must
//! not cost a thousand database reads, so the WEB-007 banner is read through a [`BannerCache`] of
//! its own, kept [`BANNER_WINDOW`]: at most one read a minute, and while that read is in flight the
//! other requests use the last answer (hidden before the first). A route another crate serves,
//! and the `/static/site/` mount's own empty 404, are never reached by the fallback.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use askama::Template;
use axum::http::{Method, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{MethodRouter, any};
use sqlx::PgPool;

use crate::routes::banner_shown;

/// WEB-007, D-4252: how long the 404 page keeps the banner answer it read.
pub(crate) const BANNER_WINDOW: Duration = Duration::from_secs(60);

/// The 404 page (WEB-027, A11Y-003): one `h1` and one sentence in the site layout.
#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundPage {
    banner_shown: bool,
}

/// WEB-007, D-4252: the banner answer the 404 page last read and when, shared with nothing else.
#[derive(Debug)]
pub(crate) struct BannerCache {
    window: Duration,
    kept: Mutex<Option<(Instant, bool)>>,
}

impl BannerCache {
    /// An empty cache keeping an answer for `window`.
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            window,
            kept: Mutex::new(None),
        }
    }

    /// The kept answer while it is fresh at `now`, or while another request is reading it; `None`
    /// tells this caller to read, after the slot is stamped `now` so no other request reads too.
    pub(crate) fn claim(&self, now: Instant) -> Option<bool> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        match *kept {
            Some((at, shown)) if now.saturating_duration_since(at) < self.window => Some(shown),
            previous => {
                *kept = Some((now, previous.is_some_and(|(_, shown)| shown)));
                None
            }
        }
    }

    /// Keeps `shown`, read at `at`.
    pub(crate) fn keep(&self, at: Instant, shown: bool) {
        *self.kept.lock().unwrap_or_else(PoisonError::into_inner) = Some((at, shown));
    }

    /// WEB-007: the banner answer, read from `pool` only when the kept one is stale.
    async fn shown(&self, pool: &PgPool) -> bool {
        let now = Instant::now();
        match self.claim(now) {
            Some(shown) => shown,
            None => {
                let shown = banner_shown(pool).await;
                self.keep(now, shown);
                shown
            }
        }
    }
}

/// WEB-027, WEB-007, A11Y-003, D-4252: the app router's fallback: the 404 page for `GET` and
/// `HEAD`, a bare 404 for any other method, the banner read through one [`BannerCache`].
pub fn not_found(pool: PgPool) -> MethodRouter {
    let cache = Arc::new(BannerCache::new(BANNER_WINDOW));
    any(move |method: Method| {
        let (pool, cache) = (pool.clone(), Arc::clone(&cache));
        async move { answer(&method, &cache, &pool).await }
    })
}

/// The fallback's answer to `method`: the page, or the bare 404 that reads nothing.
pub(crate) async fn answer(method: &Method, cache: &BannerCache, pool: &PgPool) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }
    page(cache.shown(pool).await)
}

/// The 404 page with `Cache-Control: no-store`; a failed render is the bare 404 with one error
/// and no values (R4, R5).
fn page(banner_shown: bool) -> Response {
    match (NotFoundPage { banner_shown }).render() {
        Ok(html) => (
            StatusCode::NOT_FOUND,
            [(header::CACHE_CONTROL, "no-store")],
            Html(html),
        )
            .into_response(),
        Err(_) => {
            tracing::error!("not-found page failed to render");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    //! The 404 fallback's page, its 60-second banner cache and the fallback itself (T615; WEB-007,
    //! WEB-027, A11Y-003, D-4252). The pool is lazy and points at no server, so a test that reaches
    //! the banner read sees it fail closed (hidden); every other test answers from the cache.

    use std::time::{Duration, Instant};

    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode, header};
    use axum::response::Response;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::{BANNER_WINDOW, BannerCache, answer, not_found};

    /// WEB-007's sentence, word for word (`docs/spec/website.md:11`).
    const BANNER: &str = "Texting is temporarily unavailable. You can still use the chat on this site and your quote page.";

    /// A pool that never connects: its first acquire fails within a fifth of a second.
    fn no_database() -> PgPool {
        PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(200))
            .connect_lazy("postgres://127.0.0.1:1/none")
            .expect("a lazy pool")
    }

    /// The status, `Cache-Control` and body of `response`.
    async fn parts(response: Response) -> (StatusCode, String, String) {
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map_or(String::new(), |v| v.to_str().unwrap_or_default().to_owned());
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (status, cache, String::from_utf8_lossy(&body).into_owned())
    }

    /// WEB-007, D-4252: an empty cache tells the first caller to read and stamps the slot, so a
    /// second caller inside the window gets the last answer without reading; a kept answer holds for
    /// the window and goes stale after it, when the stale answer still serves callers during the read.
    #[test]
    fn the_banner_cache_reads_at_most_once_a_window() {
        let cache = BannerCache::new(BANNER_WINDOW);
        let start = Instant::now();
        assert_eq!(cache.claim(start), None, "the first caller reads");
        assert_eq!(
            cache.claim(start),
            Some(false),
            "hidden while that read runs"
        );
        cache.keep(start, true);
        let inside = start + BANNER_WINDOW - Duration::from_secs(1);
        assert_eq!(cache.claim(inside), Some(true));
        let after = start + BANNER_WINDOW;
        assert_eq!(cache.claim(after), None, "stale after the window");
        assert_eq!(
            cache.claim(after),
            Some(true),
            "the last answer during the read"
        );
    }

    /// WEB-027, A11Y-003, D-4252: a GET is the 404 page with `no-store`: one `h1` and the copy
    /// file's sentence in the site layout, the header's home and Help links, the footer, no script,
    /// and the banner as the cache holds it.
    #[tokio::test]
    async fn a_get_is_the_styled_404_page_with_the_cached_banner() {
        let pool = no_database();
        for shown in [false, true] {
            let cache = BannerCache::new(BANNER_WINDOW);
            cache.keep(Instant::now(), shown);
            let (status, cache_control, html) =
                parts(answer(&Method::GET, &cache, &pool).await).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(cache_control, "no-store");
            assert_eq!(html.matches("<h1").count(), 1);
            assert!(html.contains("<h1>We can't find that page.</h1>"), "{html}");
            assert!(html.contains("<title>Page not found | "));
            let header =
                &html[html.find("<header").expect("header")..html.find("</header>").expect("end")];
            assert!(
                header.contains("<a href=\"/\">") && header.contains("<a href=\"/help\">Help</a>")
            );
            assert!(html.contains("<footer class=\"site-footer\">"));
            assert!(!html.contains("<script"));
            assert_eq!(html.contains(BANNER), shown);
        }
    }

    /// D-4252: another method is a bare 404 that reads nothing, so the cache is never stamped.
    #[tokio::test]
    async fn another_method_is_a_bare_404_that_reads_nothing() {
        let pool = no_database();
        let cache = BannerCache::new(BANNER_WINDOW);
        for method in [
            Method::POST,
            Method::DELETE,
            Method::from_bytes(b"PROBE").expect("method"),
        ] {
            let (status, cache_control, body) = parts(answer(&method, &cache, &pool).await).await;
            assert_eq!(
                (status, cache_control, body),
                (StatusCode::NOT_FOUND, String::new(), String::new())
            );
        }
        assert_eq!(
            cache.claim(Instant::now()),
            None,
            "nothing was read or stamped"
        );
    }

    /// WEB-007, D-4252: the fallback itself answers a GET with the page, a failed banner read
    /// hiding the banner, and a HEAD with the same 404.
    #[tokio::test]
    async fn the_fallback_answers_get_and_head_with_the_page() {
        let fallback = not_found(no_database());
        for method in [Method::GET, Method::HEAD] {
            let request = Request::builder()
                .method(method.clone())
                .uri("/no-such-page")
                .body(Body::empty())
                .expect("request");
            let response = fallback.clone().oneshot(request).await.expect("answer");
            let (status, cache_control, html) = parts(response).await;
            assert_eq!(
                (status, cache_control.as_str()),
                (StatusCode::NOT_FOUND, "no-store"),
                "{method}"
            );
            assert!(
                method == Method::HEAD
                    || html.contains("We can't find that page.") && !html.contains(BANNER)
            );
        }
    }
}
