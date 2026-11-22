//! WEB-030, D-4252, D-5318 (T616): the site files' cache rules, the font hash pins, the strong
//! per-coding `ETag` with its 304, and gzip or Brotli for the stylesheet and the mark only.

use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::Response;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use tower_http::decompression::Decompression;

use crate::api::{not_found, site_routes};
use crate::assets;

/// Each font file with the SHA256 its name is pinned to: a font served for a year as
/// `immutable` must take a new name when its bytes change (WEB-030, D-4252).
const FONT_PINS: [(&str, &str); 7] = [
    (
        "fonts/gelasio-latin-400-normal.woff2",
        "68e2b704c5624ba84e70d826e33c5fc08d75eb452f834d869abdecc39f3b26ac",
    ),
    (
        "fonts/gelasio-latin-500-normal.woff2",
        "4d7df92976b7d7ec1c5630c69cc9946c566520fe9f193142856d021a64df7493",
    ),
    (
        "fonts/gelasio-latin-500-italic.woff2",
        "4e135d1e726512bf9aebe34b09b833887350329ba48456a49013be7893d3f8f0",
    ),
    (
        "fonts/dm-sans-latin-400-normal.woff2",
        "a71e519e44faaa2a518544f31c899590cd80076d09814d015b69e64dd9202128",
    ),
    (
        "fonts/dm-sans-latin-500-normal.woff2",
        "720546bd1109304dfb6076e22b74c2afeaca0b38c819d019eccb1a2e91e6f0be",
    ),
    (
        "fonts/dm-sans-latin-700-normal.woff2",
        "52b34c81a087fe1de2aa984d16f3a7e70f502eb5a6c69c51d88ec0741b0aa0c4",
    ),
    (
        "fonts/dm-sans-latin-wght-normal.woff2",
        "9fea608a947e67020c33cad9a6fe3d60c54119dfb8cff87768a8117a15ed7543",
    ),
];

/// The files that may change under the same name, so each is sent `no-cache`.
const REVALIDATED: [&str; 5] = [
    "photos/hero-kitchen.webp",
    "photos/hero-kitchen-phone.webp",
    "brand/mark.svg",
    "brand/touch-icon.png",
    "brand/share.png",
];

/// A request for `path` with these headers.
fn request(method: Method, path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).expect("request")
}

/// `router`'s answer to `request`: the status, the headers and the raw body.
async fn answer(router: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response: Response = router.oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let body = to_bytes(body, 1 << 22).await.expect("body");
    (parts.status, parts.headers, body.to_vec())
}

/// A GET of `/static/site/<name>` on the site router with these headers.
async fn get(name: &str, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Vec<u8>) {
    let path = format!("/static/site/{name}");
    answer(site_routes(), request(Method::GET, &path, headers)).await
}

/// One header's value as text, or "" when absent.
fn value(headers: &HeaderMap, name: header::HeaderName) -> &str {
    headers.get(name).map_or("", |v| v.to_str().expect("text"))
}

/// The plain `ETag` the route gives `name`: its SHA256's first 16 hex digits, quoted.
fn plain_tag(name: &str) -> String {
    let digest = assets::sha256(name).expect("listed");
    format!("\"{}\"", &digest[..16])
}

/// WEB-030: fonts a year `immutable`, the stylesheet its hour (D-3901), the rest `no-cache`,
/// each with a strong `ETag` and `Vary: Accept-Encoding`.
#[tokio::test]
async fn each_file_carries_its_cache_rule() {
    let mut cases: Vec<(&str, &str)> = FONT_PINS
        .iter()
        .map(|(name, _)| (*name, "public, max-age=31536000, immutable"))
        .collect();
    cases.push(("site.css", "public, max-age=3600"));
    cases.extend(REVALIDATED.iter().map(|name| (*name, "no-cache")));
    for (name, rule) in cases {
        let (status, headers, _) = get(name, &[]).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_eq!(value(&headers, header::CACHE_CONTROL), rule, "{name}");
        assert_eq!(value(&headers, header::ETAG), plain_tag(name), "{name}");
        assert_eq!(value(&headers, header::VARY), "accept-encoding", "{name}");
    }
}

/// WEB-030, D-4252: each font's bytes are pinned by hash under its name.
#[test]
fn every_font_is_pinned_to_its_hash() {
    for (name, pin) in FONT_PINS {
        assert_eq!(
            assets::sha256(name),
            Some(pin),
            "{name}: new bytes need a new name"
        );
    }
    assert_eq!(assets::sha256("fonts/none.woff2"), None);
}

/// WEB-030, D-4252, D-5358: every `fonts/` file the site lists, each served a year as
/// `immutable`, has a hash pin, and every pin names a listed file.
#[test]
fn every_listed_font_has_a_pin() {
    let mut listed: Vec<&str> = assets::names()
        .filter(|name| name.starts_with("fonts/"))
        .collect();
    let mut pinned: Vec<&str> = FONT_PINS.iter().map(|(name, _)| *name).collect();
    listed.sort_unstable();
    pinned.sort_unstable();
    assert_eq!(listed, pinned, "a listed font needs a pin in FONT_PINS");
}

/// D-5318: the plain, gzip and Brotli answers of the stylesheet and the mark carry three
/// different tags, the suffix taken from the coding actually sent, weights and `identity`
/// included.
#[tokio::test]
async fn each_coding_has_its_own_tag() {
    for name in ["site.css", "brand/mark.svg"] {
        let plain = plain_tag(name);
        let digits = plain.trim_matches('"');
        for (accept, coding, tag) in [
            ("identity", "", plain.clone()),
            ("gzip", "gzip", format!("\"{digits}-gzip\"")),
            ("br", "br", format!("\"{digits}-br\"")),
            ("br;q=0.5, gzip;q=1", "gzip", format!("\"{digits}-gzip\"")),
            ("gzip;q=0.5, br", "br", format!("\"{digits}-br\"")),
        ] {
            let (status, headers, _) = get(name, &[("accept-encoding", accept)]).await;
            assert_eq!(status, StatusCode::OK, "{name} {accept}");
            assert_eq!(
                value(&headers, header::CONTENT_ENCODING),
                coding,
                "{accept}"
            );
            assert_eq!(value(&headers, header::ETAG), tag, "{name} {accept}");
            assert_eq!(value(&headers, header::VARY), "accept-encoding", "{accept}");
        }
    }
}

/// D-5318: for each coding, sending the tag back gets a 304 with the `ETag`, `Cache-Control`
/// and `Vary` and no body; another coding's tag gets the full answer.
#[tokio::test]
async fn a_matching_tag_gets_304_for_each_coding() {
    for accept in ["identity", "gzip", "br"] {
        let (_, first, _) = get("site.css", &[("accept-encoding", accept)]).await;
        let tag = value(&first, header::ETAG).to_owned();
        let weak = format!("\"other\", W/{tag}");
        for sent in [tag.as_str(), weak.as_str(), "*"] {
            let again = [("accept-encoding", accept), ("if-none-match", sent)];
            let (status, headers, body) = get("site.css", &again).await;
            assert_eq!(status, StatusCode::NOT_MODIFIED, "{accept} {sent}");
            assert!(body.is_empty(), "{accept}");
            assert_eq!(value(&headers, header::ETAG), tag);
            assert_eq!(
                value(&headers, header::CACHE_CONTROL),
                "public, max-age=3600"
            );
            assert_eq!(value(&headers, header::VARY), "accept-encoding");
            assert_eq!(value(&headers, header::CONTENT_ENCODING), "", "{accept}");
        }
        let other = if accept == "br" { "gzip" } else { "br" };
        let crossed = [("accept-encoding", other), ("if-none-match", tag.as_str())];
        assert_eq!(
            get("site.css", &crossed).await.0,
            StatusCode::OK,
            "{accept}"
        );
    }
}

/// D-5318: a `HEAD` carries the same tag and no body, and revalidates with a 304 too.
#[tokio::test]
async fn a_head_request_carries_the_tag() {
    let head = |headers: &[(&str, &str)]| {
        request(
            Method::HEAD,
            "/static/site/fonts/gelasio-latin-400-normal.woff2",
            headers,
        )
    };
    let (status, headers, body) = answer(site_routes(), head(&[])).await;
    let tag = plain_tag("fonts/gelasio-latin-400-normal.woff2");
    assert_eq!((status, body.is_empty()), (StatusCode::OK, true));
    assert_eq!(value(&headers, header::ETAG), tag);
    let again = head(&[("if-none-match", tag.as_str())]);
    assert_eq!(
        answer(site_routes(), again).await.0,
        StatusCode::NOT_MODIFIED
    );
}

/// WEB-030: the gzip and Brotli stylesheet and mark decode to the listed bytes and are smaller.
#[tokio::test]
async fn gzip_and_brotli_decode_to_the_same_bytes() {
    for name in ["site.css", "brand/mark.svg"] {
        let listed = assets::find(name).expect("listed").bytes;
        for accept in ["gzip", "br"] {
            let (_, _, sent) = get(name, &[("accept-encoding", accept)]).await;
            assert!(sent.len() < listed.len(), "{name} {accept}");
            let path = format!("/static/site/{name}");
            let unpacking = Decompression::new(site_routes());
            let request = request(Method::GET, &path, &[("accept-encoding", accept)]);
            let response = unpacking.oneshot(request).await.expect("answer");
            let body = to_bytes(Body::new(response.into_body()), 1 << 22).await;
            assert_eq!(body.expect("decodes").as_ref(), listed, "{name} {accept}");
        }
    }
}

/// WEB-030: a font, a photo and a PNG are already compressed and are sent as listed.
#[tokio::test]
async fn a_font_photo_or_png_is_never_compressed() {
    let accept = [("accept-encoding", "br, gzip")];
    for name in [
        "fonts/dm-sans-latin-wght-normal.woff2",
        "photos/hero-kitchen.webp",
        "brand/share.png",
    ] {
        let (status, headers, body) = get(name, &accept).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_eq!(value(&headers, header::CONTENT_ENCODING), "", "{name}");
        assert_eq!(body, assets::find(name).expect("listed").bytes, "{name}");
    }
}

/// WEB-030, D-4252 (BREACH): a page beside the site files, here the 404 page, is never
/// compressed and carries no file `ETag`, so a page's CSRF token is never sent compressed.
#[tokio::test]
async fn a_page_is_never_compressed() {
    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(200))
        .connect_lazy("postgres://127.0.0.1:9/no_database")
        .expect("lazy pool");
    let router = site_routes().fallback_service(not_found(pool));
    let page = request(
        Method::GET,
        "/no-such-page",
        &[("accept-encoding", "br, gzip")],
    );
    let (status, headers, body) = answer(router, page).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value(&headers, header::CONTENT_ENCODING), "");
    assert_eq!(value(&headers, header::ETAG), "");
    assert!(String::from_utf8(body).expect("plain html").contains("<h1"));
}
