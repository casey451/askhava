//! The home pages' texting number and consent line render together or not at all (T725; WEB-017,
//! CON-005, WORD-013, TXT-003, D-4709, D-5402).
//!
//! The English home page is rendered with no database through `pages::home_page` for every
//! combination of banner, WEB-028 numbers and texting value; the number shows exactly when the
//! consent line does, and neither shows with no texting value. The line is read from
//! `docs/copy/site-copy.md` by its heading, never by line number, its citation dropped, and found
//! sentence by sentence in the page's text content as T161's check reads it (tags stripped,
//! entities decoded, whitespace collapsed, each sentence without its closing full stop). The
//! Spanish home page is checked the same way against the Spanish block. Fake numbers only.

use crate::pages::{Shown, SitePages, es_home_page, home_page, shown_number};

/// The repository root, a run-time base; the copy file is `docs/copy/site-copy.md` under it.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The English consent line's heading (D-4709).
const ENGLISH: &str = "## Home · texting consent line (homeowner, fixed)";

/// The Spanish consent line's heading (D-5402, D-5341 (1)).
const SPANISH: &str = "## Spanish home · texting consent line (homeowner, fixed)";

/// A fake configured homeowner number, as `server` passes it.
const FAKE: &str = "+19285550101";

/// The quoted words of the copy block under `heading`, its trailing citation dropped.
fn line(heading: &str) -> String {
    let copy = std::fs::read_to_string(format!("{ROOT}/docs/copy/site-copy.md")).expect("copy");
    let mut found = copy.lines().skip_while(|l| *l != heading).skip(1);
    let words = found.find_map(|l| l.strip_prefix("> ")).expect(heading);
    let cited = words.ends_with(')').then(|| words.rfind(" (")).flatten();
    cited.map_or(words, |at| &words[..at]).trim().to_owned()
}

/// The sentences of `words`, each without its closing full stop.
fn sentences(words: &str) -> Vec<String> {
    words
        .split_inclusive(". ")
        .map(|s| s.trim().trim_end_matches('.').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The text content of `html`: tags stripped, entities decoded, whitespace collapsed.
fn page_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => in_tag = true,
            (true, '>') => (in_tag, _) = (false, text.push(' ')),
            (false, c) => text.push(c),
            (true, _) => {}
        }
    }
    let decoded = [
        ("&#39;", "'"),
        ("&#x27;", "'"),
        ("&quot;", "\""),
        ("&amp;", "&"),
    ]
    .iter()
    .fold(text, |out, (entity, c)| out.replace(entity, c));
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether every sentence of `line` shows in `text`, in order.
fn shows(text: &str, line: &str) -> bool {
    let mut from = 0;
    sentences(line).iter().all(|sentence| {
        text[from..]
            .find(sentence.as_str())
            .map(|at| from += at + sentence.len())
            .is_some()
    })
}

/// The texting values each page is rendered with: none, and the fake number as shown.
fn values() -> [Option<String>; 2] {
    [None, Some(shown_number(FAKE))]
}

/// WEB-017, CON-005, D-4709: on the English home page, for every banner, numbers and texting
/// value, the number shows exactly when the consent line does, and neither with no value.
#[test]
fn web_017_the_number_and_its_consent_line_show_together() {
    let (consent, pages) = (line(ENGLISH), SitePages::default());
    assert!(
        consent.contains("Reply STOP"),
        "the line was read: {consent}"
    );
    for texting in values() {
        for banner in [false, true] {
            for numbers in [None, Some(Shown::fake())] {
                let html = home_page(
                    &pages,
                    banner,
                    numbers,
                    (texting.clone(), "fake".to_owned()),
                );
                let text = page_text(&html.expect("renders"));
                let (number, line) = (text.contains("555-0101"), shows(&text, &consent));
                assert_eq!(number, line, "{texting:?} {banner}: {text}");
                assert_eq!(number, texting.is_some(), "{texting:?} {banner}");
                assert_eq!(
                    text.contains("Reply STOP"),
                    texting.is_some(),
                    "{texting:?}"
                );
            }
        }
    }
}

/// WEB-017, WORD-013, TXT-003, D-5402: the Spanish home page shows the number exactly when its
/// Spanish consent line (with PARAR) is there, and neither with no value.
#[test]
fn word_013_the_spanish_number_and_its_consent_line_show_together() {
    let (consent, pages) = (line(SPANISH), SitePages::default());
    assert!(
        consent.contains("PARAR"),
        "the Spanish line was read: {consent}"
    );
    for texting in values() {
        for banner_shown in [false, true] {
            let page = es_home_page(&pages, banner_shown, texting.clone());
            let text = page_text(&page.expect("es"));
            let (number, line) = (text.contains("555-0101"), shows(&text, &consent));
            assert_eq!(number, line, "{texting:?} {banner_shown}: {text}");
            assert_eq!(number, texting.is_some(), "{texting:?} {banner_shown}");
            assert_eq!(text.contains("PARAR"), texting.is_some(), "{texting:?}");
        }
    }
}

/// WEB-017: a configured number shows as "(928) 555-0101"; any other form shows as given.
#[test]
fn web_017_the_number_shows_in_its_local_form() {
    assert_eq!(shown_number(FAKE), "(928) 555-0101");
    assert_eq!(shown_number("5550101"), "5550101");
}
