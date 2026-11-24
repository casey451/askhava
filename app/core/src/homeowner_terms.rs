//! The published homeowner Terms, compiled in once (TERMS-016, PICK-016, D-4727): a page or text
//! pick accepts them, stored with their version and SHA256; `server` and `intake` both read them
//! here, so there is one `include_str!` and one parse of the "Version <v>" line.

/// The exact text of `docs/legal/homeowner-terms.md`.
const HOMEOWNER_TERMS: &str = include_str!("../../../docs/legal/homeowner-terms.md");

/// The published homeowner Terms as `(version, text)` (TERMS-016, PICK-016): the first word after
/// "Version " on its version line (else `unversioned`) and the whole file, exactly as published.
#[must_use]
pub fn homeowner_terms() -> (&'static str, &'static str) {
    (version_of(HOMEOWNER_TERMS), HOMEOWNER_TERMS)
}

/// The version `text` names on its first "Version <v>" line, or `unversioned`.
fn version_of(text: &str) -> &str {
    text.lines()
        .find_map(|l| l.strip_prefix("Version "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("unversioned")
}

#[cfg(test)]
mod tests {
    use super::{homeowner_terms, version_of};

    #[test]
    fn terms_016_the_published_terms_name_a_version() {
        assert_eq!(version_of("T\nVersion 1.2 (draft)"), "1.2");
        assert_eq!(version_of("no line"), "unversioned");
        let (version, text) = homeowner_terms();
        assert!(version != "unversioned" && text.contains(&format!("Version {version}")));
    }
}
