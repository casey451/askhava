//! No page loads a third-party script, style, image, frame or font (TD-SEC-04; SEC-013, SEC-034,
//! FEE-014, D-491).
//!
//! Every template and static file under `app/*/templates/` and `app/*/static/` is scanned for a
//! load from another host: a `src` or `href` on a loading tag (`script`, `link`, `img`, `iframe`,
//! `source`, `audio`, `video`, `embed`, `object`), a CSS `url(` or `@import`, and any absolute URL
//! in a script. The one allowed host is Stripe.js from `js.stripe.com` on the Confirm page, which
//! Stripe Elements needs (FEE-014). Plain links (`<a href>`) load nothing and are allowed. The
//! CSP must name no host either, so a load the scan missed is still refused by the browser. Fonts
//! are therefore self-hosted (SEC-013), and contact and photo pages load no third-party script
//! (SEC-034).

use std::path::{Path, PathBuf};

use askhava_portals::api::CONTENT_SECURITY_POLICY;

const LOADING_TAGS: [&str; 9] = [
    "script", "link", "img", "iframe", "source", "audio", "video", "embed", "object",
];
const STRIPE_HOST: &str = "js.stripe.com";

fn is_remote(url: &str) -> bool {
    let url = url.trim().trim_matches(|c| c == '"' || c == '\'');
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("//")
}

fn host_of(url: &str) -> String {
    let url = url.trim().trim_matches(|c| c == '"' || c == '\'');
    let rest = url.split_once("//").map_or(url, |(_, rest)| rest);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    rest[..end].to_ascii_lowercase()
}

/// The attribute values of `src=` and `href=` in one tag's text.
fn tag_urls(tag: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for attribute in ["src=", "href="] {
        for (at, _) in tag.match_indices(attribute) {
            let rest = &tag[at + attribute.len()..];
            let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'');
            let value = match quote {
                Some(q) => rest[1..].split(q).next().unwrap_or(""),
                None => rest.split([' ', '>']).next().unwrap_or(""),
            };
            urls.push(value.to_owned());
        }
    }
    urls
}

/// Every remote URL `text` would load: from loading tags, CSS `url(` and `@import`, and, in a
/// script file, any absolute URL at all.
fn remote_loads(text: &str, is_script: bool) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for tag in text.split('<').skip(1) {
        let name: String = tag
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        if LOADING_TAGS.contains(&name.to_ascii_lowercase().as_str()) {
            found.extend(tag_urls(
                &tag.split('>').next().unwrap_or("").to_ascii_lowercase(),
            ));
        }
    }
    for marker in ["url(", "@import"] {
        for (at, _) in text.match_indices(marker) {
            let rest = text[at + marker.len()..].trim_start();
            found.push(
                rest.split([')', ';', ' ', '\n'])
                    .next()
                    .unwrap_or("")
                    .to_owned(),
            );
        }
    }
    if is_script {
        for (at, _) in text.match_indices("://") {
            let start = text[..at]
                .rfind(['"', '\'', '`', ' ', '('])
                .map_or(0, |s| s + 1);
            let end = text[at..]
                .find(['"', '\'', '`', ' ', ')'])
                .map_or(text.len(), |e| at + e);
            found.push(text[start..end].to_owned());
        }
    }
    found.retain(|url| is_remote(url));
    found
}

/// The remote loads of one file that are not allowed: Stripe.js only on the Confirm page.
fn refused_loads(path: &Path, text: &str) -> Vec<String> {
    let name = path.to_string_lossy().to_ascii_lowercase();
    let is_script = [".ts", ".js", ".mjs"].iter().any(|ext| name.ends_with(ext));
    let confirm_page = name.contains("confirm");
    remote_loads(text, is_script)
        .into_iter()
        .filter(|url| !(confirm_page && host_of(url) == STRIPE_HOST))
        .collect()
}

fn page_files() -> Vec<PathBuf> {
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app");
    let mut stack: Vec<PathBuf> = std::fs::read_dir(&app)
        .expect("app/")
        .filter_map(Result::ok)
        .flat_map(|krate| ["templates", "static"].map(|dir| krate.path().join(dir)))
        .filter(|dir| dir.is_dir())
        .collect();
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .expect("folder")
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// TD-SEC-04 (SEC-013, SEC-034): no template or static file loads from another host, except
/// Stripe.js on the Confirm page; the shared layout is among the files scanned.
#[test]
fn no_page_loads_a_third_party_host() {
    let files = page_files();
    assert!(
        files
            .iter()
            .any(|path| path.ends_with("site/templates/base.html")),
        "the shared layout was scanned"
    );
    let mut refused = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for url in refused_loads(path, &text) {
            refused.push(format!("{}: {url}", path.display()));
        }
    }
    assert!(refused.is_empty(), "third-party loads: {refused:#?}");
}

/// TD-SEC-04 (SEC-013): the scan catches each kind of load and lets plain links and Stripe.js
/// on the Confirm page through, so a clean scan means something.
#[test]
fn the_scan_catches_every_kind_of_load() {
    let page = "app/businesses/templates/job.html";
    let confirm = "app/businesses/templates/confirm.html";
    let stripe = r#"<script src="https://js.stripe.com/v3/"></script>"#;
    let cases = [
        (
            page,
            r#"<script src="https://cdn.example.test/a.js"></script>"#,
            1,
        ),
        (
            page,
            r#"<link rel="stylesheet" href="//fonts.example.test/css">"#,
            1,
        ),
        (page, r#"<IMG SRC='http://pixel.example.test/p.gif'>"#, 1),
        (
            page,
            "<style>@font-face{src:url(https://fonts.example.test/f.woff2)}</style>",
            1,
        ),
        (
            page,
            r#"<style>@import "https://cdn.example.test/x.css";</style>"#,
            1,
        ),
        (
            "app/site/static/site.ts",
            r#"fetch("https://analytics.example.test/hit")"#,
            1,
        ),
        (
            page,
            r#"<a href="https://roc.az.gov/">Check a license</a>"#,
            0,
        ),
        (page, r#"<script src="/static/site.js"></script>"#, 0),
        (confirm, stripe, 0),
        (page, stripe, 1),
    ];
    for (path, text, want) in cases {
        assert_eq!(
            refused_loads(Path::new(path), text).len(),
            want,
            "{path}: {text}"
        );
    }
}

/// TD-SEC-04 (SEC-013): the CSP names no other host for any kind of load.
#[test]
fn the_csp_allows_the_own_origin_only() {
    for directive in CONTENT_SECURITY_POLICY.split(';') {
        let mut words = directive.split_whitespace();
        let name = words.next().expect("directive name");
        for source in words {
            assert!(
                source == "'self'" || source == "'none'",
                "{name} allows {source}"
            );
        }
    }
}
