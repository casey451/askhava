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
//! T613 (D-4246, D-4807) adds Gelasio 500 italic for the one italic accent a page, the latin
//! subset of `@fontsource/gelasio@5.3.0` copied unchanged (the same OFL family and license text);
//! a test pins its served bytes to `docs/design/fonts/gelasio-latin-500-italic.woff2`.
//! T617 (D-4252, D-4863) adds DM Sans from one weight-axis variable file,
//! `fonts/dm-sans-latin-wght-normal.woff2` from `@fontsource-variable/dm-sans@5.3.0` copied
//! unchanged (36,932 bytes against 42,728 for the three static files; license text
//! `docs/design/fonts/OFL-DM-Sans-variable.txt`); the stylesheet's DM Sans faces all name it, and
//! the three static DM Sans files stay listed and served, unused by the stylesheet, because the
//! design-token suite requests each one. A test pins its served bytes to the `docs/design/fonts/` copy.
//!
//! The photos and the mark (T216, WEB-027, D-3331, D-4252) are the home hero, served through
//! `<picture>` as the phone strip `photos/hero-kitchen-phone.webp` and the tablet and desktop
//! `photos/hero-kitchen.webp` (no AVIF yet, D-4630), each 100 KB or less with no metadata, and the brand mark
//! `brand/mark.svg` (no `<title>`, no script), copied from `docs/design/`. They are concept
//! placeholders: licensed photos replace them at the same names and sizes before launch.

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
/// self-hosted fonts, the hero photo crops and the mark).
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
        name: "fonts/gelasio-latin-500-italic.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/gelasio-latin-500-italic.woff2"),
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
    Asset {
        name: "fonts/dm-sans-latin-wght-normal.woff2",
        mime: "font/woff2",
        bytes: include_bytes!("../static/fonts/dm-sans-latin-wght-normal.woff2"),
    },
    Asset {
        name: "photos/hero-kitchen.webp",
        mime: "image/webp",
        bytes: include_bytes!("../static/photos/hero-kitchen.webp"),
    },
    Asset {
        name: "photos/hero-kitchen-phone.webp",
        mime: "image/webp",
        bytes: include_bytes!("../static/photos/hero-kitchen-phone.webp"),
    },
    Asset {
        name: "brand/mark.svg",
        mime: "image/svg+xml",
        bytes: include_bytes!("../static/brand/mark.svg"),
    },
];

/// The listed file called `name`, or `None` (the route answers 404).
pub(crate) fn find(name: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.name == name)
}

#[cfg(test)]
mod tests {
    use super::find;

    /// The four font files `site.css` names in its `@font-face` rules: Gelasio and the one
    /// variable DM Sans file (T617, D-4863).
    const FONTS: [&str; 4] = [
        "gelasio-latin-400-normal.woff2",
        "gelasio-latin-500-normal.woff2",
        "gelasio-latin-500-italic.woff2",
        "dm-sans-latin-wght-normal.woff2",
    ];

    /// The three static DM Sans files, listed and served but unused by the stylesheet (D-4863).
    const STATIC_DM_SANS: [&str; 3] = [
        "dm-sans-latin-400-normal.woff2",
        "dm-sans-latin-500-normal.woff2",
        "dm-sans-latin-700-normal.woff2",
    ];

    /// WEB-027: each font is listed as WOFF2 under `fonts/` and holds WOFF2 bytes; the stylesheet
    /// names the Gelasio files and the variable DM Sans file with a relative URL, so they load
    /// from the site, never a font host, and names no static DM Sans file (D-4252, D-4863).
    #[test]
    fn every_font_is_listed_as_woff2_and_named_by_the_stylesheet() {
        let css = std::str::from_utf8(find("site.css").expect("listed").bytes).expect("utf-8");
        for file in FONTS.iter().chain(&STATIC_DM_SANS) {
            let font = find(&format!("fonts/{file}")).expect(file);
            assert_eq!(font.mime, "font/woff2", "{file}");
            assert!(font.bytes.starts_with(b"wOF2"), "{file}");
            let named = css.contains(&format!("url(\"fonts/{file}\")"));
            assert_eq!(named, FONTS.contains(file), "{file}");
        }
        assert!(!css.contains("http"), "the stylesheet names no other host");
    }

    /// WEB-027, WEB-030, D-4252: the served variable DM Sans font is the reviewed file in
    /// `docs/design/fonts/`, byte for byte, and no larger than the three static files together.
    #[test]
    fn the_variable_dm_sans_font_is_the_reviewed_file() {
        let served = find("fonts/dm-sans-latin-wght-normal.woff2").expect("listed");
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let reviewed =
            std::fs::read(root.join("docs/design/fonts/dm-sans-latin-wght-normal.woff2"))
                .expect("the reviewed font is in docs/design/fonts/");
        assert_eq!(served.bytes, reviewed.as_slice());
        let statics: usize = STATIC_DM_SANS
            .iter()
            .map(|file| find(&format!("fonts/{file}")).expect(file).bytes.len())
            .sum();
        assert!(served.bytes.len() <= statics);
    }

    /// WEB-027, D-4246: the served italic accent font is the reviewed file in `docs/design/fonts/`,
    /// byte for byte.
    #[test]
    fn the_italic_font_is_the_reviewed_file() {
        let served = find("fonts/gelasio-latin-500-italic.woff2").expect("listed");
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let reviewed = std::fs::read(root.join("docs/design/fonts/gelasio-latin-500-italic.woff2"))
            .expect("the reviewed font is in docs/design/fonts/");
        assert_eq!(served.bytes, reviewed.as_slice());
    }

    /// WEB-001: the stylesheet is listed as CSS and holds the evergreen link colour.
    #[test]
    fn the_stylesheet_is_listed_as_css() {
        let css = find("site.css").expect("listed");
        assert_eq!(css.mime, "text/css; charset=utf-8");
        let text = std::str::from_utf8(css.bytes).expect("utf-8");
        assert!(text.contains("--evergreen: #245849"));
    }

    /// WEB-027, D-4252: each hero crop is listed with its image type, holds that format's bytes,
    /// is 100 KB or less and carries no Exif, XMP or GPS block.
    #[test]
    fn every_photo_is_listed_small_and_without_metadata() {
        for (name, mime, magic) in [
            ("photos/hero-kitchen.webp", "image/webp", *b"WEBP"),
            ("photos/hero-kitchen-phone.webp", "image/webp", *b"WEBP"),
        ] {
            let photo = find(name).expect(name);
            assert_eq!(photo.mime, mime, "{name}");
            assert_eq!(photo.bytes[8..12], magic, "{name}");
            assert!(photo.bytes.len() <= 100 * 1024, "{name}");
            for block in [&b"Exif"[..], b"<x:xmpmeta", b"XMP ", b"GPS"] {
                assert!(
                    !photo.bytes.windows(block.len()).any(|w| w == block),
                    "{name}"
                );
            }
        }
    }

    /// WEB-027, D-3890: the mark is listed as SVG, is evergreen, and has no script, title or
    /// metadata.
    #[test]
    fn the_mark_is_a_plain_evergreen_svg() {
        let mark = find("brand/mark.svg").expect("listed");
        assert_eq!(mark.mime, "image/svg+xml");
        let text = std::str::from_utf8(mark.bytes).expect("utf-8");
        assert!(
            text.starts_with("<svg") && text.contains("id=\"mark\"") && text.contains("#245849")
        );
        for gone in ["<script", "<title", "<metadata", " on", "href"] {
            assert!(!text.contains(gone), "{gone}");
        }
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
