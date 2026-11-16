//! The site's static files, compiled into the binary (WEB-001, SEC-013, T101, D-3106).
//!
//! The runtime image keeps only the built `.js` files (`Dockerfile`), so the stylesheet, and the
//! fonts and photos T166 and T216 add, live here as a fixed list of name, MIME type and bytes
//! (`include_bytes!`). A new file is one more [`Asset`] line in [`ASSETS`]; nothing is read from
//! disk at run time and no name outside the list is ever served.
//!
//! The fonts (T166, WEB-027, D-3331) are Gelasio 400 and 500 and DM Sans 400, 500 and 700, copied
//! from `docs/design/fonts/` (SIL Open Font License; the license texts sit beside the originals
//! there) and served at `/static/site/fonts/<name>` as `font/woff2`, so no page loads a font host.

/// One file served at `/static/site/<name>`.
pub(crate) struct Asset {
    /// The file name after `/static/site/`.
    pub(crate) name: &'static str,
    /// The `Content-Type` it is served with.
    pub(crate) mime: &'static str,
    /// The file's bytes, compiled in.
    pub(crate) bytes: &'static [u8],
}

/// Every file the site serves (WEB-001: the daylight-and-evergreen stylesheet; WEB-027: its
/// self-hosted fonts).
const ASSETS: &[Asset] = &[
    Asset {
        name: "site.css",
        mime: "text/css; charset=utf-8",
        bytes: include_bytes!("../static/site.css"),
    },
    Asset {
        name: "fonts/gelasio-latin-400-normal.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/gelasio-latin-400-normal.woff2"),
    },
    Asset {
        name: "fonts/gelasio-latin-500-normal.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/gelasio-latin-500-normal.woff2"),
    },
    Asset {
        name: "fonts/dm-sans-latin-400-normal.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/dm-sans-latin-400-normal.woff2"),
    },
    Asset {
        name: "fonts/dm-sans-latin-500-normal.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/dm-sans-latin-500-normal.woff2"),
    },
    Asset {
        name: "fonts/dm-sans-latin-700-normal.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/dm-sans-latin-700-normal.woff2"),
    },
];

/// The listed file called `name`, or `None` (the route answers 404).
pub(crate) fn find(name: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.name == name)
}

#[cfg(test)]
mod tests {
    use super::find;

    /// The five font files `site.css` names in its `@font-face` rules.
    const FONTS: [&str; 5] = [
        "gelasio-latin-400-normal.woff2",
        "gelasio-latin-500-normal.woff2",
        "dm-sans-latin-400-normal.woff2",
        "dm-sans-latin-500-normal.woff2",
        "dm-sans-latin-700-normal.woff2",
    ];

    /// WEB-027: each font is listed as WOFF2 under `fonts/`, holds WOFF2 bytes and is named by
    /// the stylesheet with a relative URL, so it loads from the site, never a font host.
    #[test]
    fn every_font_is_listed_as_woff2_and_named_by_the_stylesheet() {
        let css = std::str::from_utf8(find("site.css").expect("listed").bytes).expect("utf-8");
        for file in FONTS {
            let font = find(&format!("fonts/{file}")).expect(file);
            assert_eq!(font.mime, "font/woff2", "{file}");
            assert!(font.bytes.starts_with(b"wOF2"), "{file}");
            assert!(css.contains(&format!("url(\"fonts/{file}\")")), "{file}");
        }
        assert!(!css.contains("http"), "the stylesheet names no other host");
    }

    /// WEB-001: the stylesheet is listed as CSS and holds the evergreen link colour.
    #[test]
    fn the_stylesheet_is_listed_as_css() {
        let css = find("site.css").expect("listed");
        assert_eq!(css.mime, "text/css; charset=utf-8");
        let text = std::str::from_utf8(css.bytes).expect("utf-8");
        assert!(text.contains("--evergreen: #245849"));
    }

    /// An unlisted name, a path with a folder and an empty name are never found.
    #[test]
    fn an_unlisted_name_is_not_found() {
        for name in [
            "",
            "site.js",
            "../site.css",
            "static/site.css",
            "SITE.CSS",
            "fonts/",
            "fonts/../site.css",
            "gelasio-latin-400-normal.woff2",
        ] {
            assert!(find(name).is_none(), "{name}");
        }
    }
}
