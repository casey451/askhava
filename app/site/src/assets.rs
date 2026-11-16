//! The site's static files, compiled into the binary (WEB-001, SEC-013, T101, D-3106).
//!
//! The runtime image keeps only the built `.js` files (`Dockerfile`), so the stylesheet, and the
//! fonts and photos T166 and T216 add, live here as a fixed list of name, MIME type and bytes
//! (`include_bytes!`). A new file is one more [`Asset`] line in [`ASSETS`]; nothing is read from
//! disk at run time and no name outside the list is ever served.

/// One file served at `/static/site/<name>`.
pub(crate) struct Asset {
    /// The file name after `/static/site/`.
    pub(crate) name: &'static str,
    /// The `Content-Type` it is served with.
    pub(crate) mime: &'static str,
    /// The file's bytes, compiled in.
    pub(crate) bytes: &'static [u8],
}

/// Every file the site serves (WEB-001: the daylight-and-evergreen stylesheet).
const ASSETS: &[Asset] = &[Asset {
    name: "site.css",
    mime: "text/css; charset=utf-8",
    bytes: include_bytes!("../static/site.css"),
}];

/// The listed file called `name`, or `None` (the route answers 404).
pub(crate) fn find(name: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.name == name)
}

#[cfg(test)]
mod tests {
    use super::find;

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
        for name in ["", "site.js", "../site.css", "static/site.css", "SITE.CSS"] {
            assert!(find(name).is_none(), "{name}");
        }
    }
}
