//! The built browser files at `/static/<crate>/` (T362, D-2633; PICK-009, A11Y-033, SEC-012).
//!
//! `npm run build` compiles each `app/<crate>/static/*.ts` to a `.js` beside it, and the image
//! keeps only the `.js`, at `/srv/askhava/static/<crate>/` (D-1281). [`router`] serves one
//! tower-http `ServeDir` per crate in [`CRATES`], each mounted as a nested service (never a
//! one-line route and never a path constant, so `tests/security/tests/auth_matrix.rs` reads no
//! route from it; its `/static` door is `Asset`). `build_router` merges it under the shared
//! security headers. `ServeDir` sets the MIME type from the extension (`.js` as JavaScript, CSS,
//! fonts, images), refuses `..` plain or percent-encoded, and answers 405 to any method but `GET`
//! and `HEAD`; a directory answers 404 (no listing, no `index.html`), and so does an unknown
//! crate (no mount). A content-hashed name (`name.<8+ hex>.ext`) is cached for a year; every
//! other answer is revalidated. `portals`' own `/static/portals/feedback.css` route stays
//! `portals`' (D-3106). Tests: `tests/static_files_route.rs` (TD-SEC-62).

use std::path::PathBuf;

use askhava_core::api::{Environment, Settings};
use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use tower_http::services::ServeDir;

/// The image's folder of built files, one `<crate>/` folder each (the Dockerfile, D-1281).
pub const IMAGE_DIR: &str = "/srv/askhava/static";

/// The crates whose built files are served; a crate that adds a `static/` script adds its name.
pub const CRATES: [&str; 1] = ["portals"];

/// The answer for a content-hashed name: its bytes never change under that name.
const LONG_CACHE: &str = "public, max-age=31536000, immutable";

/// Every other answer: the browser asks again before reusing it.
const REVALIDATE: &str = "no-cache";

/// Where the built files are (PICK-009, A11Y-033, D-2633).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticRoot {
    /// `<dir>/<crate>/<file>`: the image's layout, or a test's temporary folder.
    Built(PathBuf),
    /// `<repo>/app/<crate>/static/<file>`: the repository in dev and CI, after `npm run build`.
    Repo(PathBuf),
}

impl StaticRoot {
    /// `ASKHAVA_STATIC_DIR` when set; else [`IMAGE_DIR`] in test and live, and the working
    /// directory's repository layout in dev and CI (D-2633).
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        match (settings.static_dir(), settings.environment()) {
            (Some(dir), _) => Self::Built(PathBuf::from(dir)),
            (None, Environment::Test | Environment::Live) => Self::Built(PathBuf::from(IMAGE_DIR)),
            (None, Environment::Dev | Environment::Ci) => Self::Repo(PathBuf::from(".")),
        }
    }

    /// The folder served at `/static/<krate>/`.
    #[must_use]
    pub fn folder(&self, krate: &str) -> PathBuf {
        match self {
            Self::Built(dir) => dir.join(krate),
            Self::Repo(repo) => repo.join("app").join(krate).join("static"),
        }
    }
}

impl Default for StaticRoot {
    /// The image's folder, [`IMAGE_DIR`].
    fn default() -> Self {
        Self::Built(PathBuf::from(IMAGE_DIR))
    }
}

/// `/static/<crate>/` for every crate in [`CRATES`], served from `root` with no directory
/// answer and the cache rule of [`content_hashed`] (PICK-009, A11Y-033, SEC-012, D-2633).
pub fn router(root: &StaticRoot) -> Router {
    CRATES
        .iter()
        .fold(Router::new(), |router, krate| {
            let mount = format!("/static/{krate}");
            let files = ServeDir::new(root.folder(krate)).append_index_html_on_directories(false);
            router.nest_service(&mount, files)
        })
        .layer(axum::middleware::from_fn(cache_control))
}

/// Middleware: a year's cache for a found content-hashed file, `no-cache` for anything else.
async fn cache_control(request: Request, next: Next) -> Response {
    let hashed = content_hashed(request.uri().path());
    let mut response = next.run(request).await;
    let value = if hashed && response.status().is_success() {
        LONG_CACHE
    } else {
        REVALIDATE
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

/// Whether the path's last segment is `name.<hash>.ext` with a hash of at least 8 hex digits.
#[must_use]
pub fn content_hashed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or_default();
    let parts: Vec<&str> = name.split('.').collect();
    match parts.as_slice() {
        [stem, .., hash, ext] if !stem.is_empty() && !ext.is_empty() => {
            hash.len() >= 8 && hash.bytes().all(|b| b.is_ascii_hexdigit())
        }
        _ => false,
    }
}
