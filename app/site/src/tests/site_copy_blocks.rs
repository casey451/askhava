//! The copy panel's page check (WORD-029, D-474, D-491, D-3890, D-4669): every `## Home ·` and
//! `## Footer ·` block of `docs/copy/site-copy.md` shows on the home page as the copy file words
//! it.
//!
//! The home page is rendered with no database through `pages::home_page`: no banner, no numbers,
//! the default `SitePages` and a fake request-box token; its market names come from
//! `markets.yaml` as the page reads them. Each block is found by its heading, never by line
//! number, and matched against the page's text content (tags stripped, entities decoded,
//! whitespace collapsed): its trailing requirement citation in parentheses is dropped, and its
//! sentences are found in order, each without its closing full stop. While the start line's block
//! names the texting number, only its chat sentence is matched, as the page shows it before
//! Gate C (D-3890 (3), WEB-017). The copy file is read at run time from the repository root, as
//! `app/server/tests/home_request_box.rs` reads it (D-4692 (7)).

use crate::pages::{SitePages, home_page};

/// The repository root, a run-time base; the copy file is `docs/copy/site-copy.md` under it.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The blocks the home page does not show, each with its reason (D-4669, D-4709).
const SKIPPED: [(&str, &str); 2] = [
    (
        "Home · texting consent line",
        "CON-005: shown only beside the texting number after Gate C (WEB-017); \
         app/site/src/tests/home_texting_line.rs (T725, D-4709) checks it with the number",
    ),
    (
        "Home · business line",
        "WEB-024: belongs to the business page and sign-up and follows the business-texting \
         switch (T202, T526)",
    ),
];

/// The heading of a block shown only with the WEB-028 numbers, which this render leaves out.
const NUMBERS_ONLY: &str = "numbers";

/// The start line's heading (D-3890 (3)).
const START_LINE: &str = "Home · start line";

/// One block of the copy file: its heading (after `## `) and its quoted words, lines joined.
struct Block {
    heading: String,
    words: String,
}

/// The `## Home ·` and `## Footer ·` blocks of the copy file `text`, in file order.
fn blocks(text: &str) -> Vec<Block> {
    let mut found: Vec<Block> = Vec::new();
    let mut current: Option<Block> = None;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            found.extend(current.take());
            let page_block = heading.starts_with("Home · ") || heading.starts_with("Footer · ");
            current = page_block.then(|| Block {
                heading: heading.split(" (").next().unwrap_or(heading).to_owned(),
                words: String::new(),
            });
        } else if let (Some(block), Some(quote)) = (current.as_mut(), line.strip_prefix("> ")) {
            if !block.words.is_empty() {
                block.words.push(' ');
            }
            block.words.push_str(quote.trim());
        }
    }
    found.extend(current);
    found
}

/// `words` without a trailing requirement citation such as "(CON-003)" or "(WEB-022, WEB-021)".
fn without_citation(words: &str) -> &str {
    let Some(open) = words.rfind(" (") else {
        return words;
    };
    let inner = &words[open + 2..];
    let id = inner.split([',', ';', ')']).next().unwrap_or("");
    let cited = words.ends_with(')')
        && id.split_once('-').is_some_and(|(area, n)| {
            area.len() >= 2
                && area.chars().all(|c| c.is_ascii_uppercase())
                && !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
        });
    if cited { &words[..open] } else { words }
}

/// The sentences of `words`, each without its closing full stop.
fn sentences(words: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = words.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if matches!(c, '.' | '?' | '!') && chars.peek().is_none_or(|n| n.is_whitespace()) {
            out.push(std::mem::take(&mut current));
        }
    }
    out.push(current);
    out.into_iter()
        .map(|s| s.trim().trim_end_matches('.').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The sentences of `block` to find on the page: its citation dropped and, for the start line
/// while it names the texting number, its chat sentence only (D-3890 (3)).
fn expected(block: &Block) -> Vec<String> {
    let all = sentences(without_citation(&block.words));
    let names_number = block.words.chars().any(|c| c.is_ascii_digit())
        || block.words.to_lowercase().contains("text");
    if block.heading == START_LINE && names_number {
        all.into_iter().filter(|s| s.contains("chat")).collect()
    } else {
        all
    }
}

/// The text content of `html`: tags stripped, entities decoded, whitespace collapsed.
fn page_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => in_tag = true,
            (true, '>') => {
                in_tag = false;
                text.push(' ');
            }
            (false, c) => text.push(c),
            (true, _) => {}
        }
    }
    decoded(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The character references askama's HTML escaper writes, and what each stands for.
const ENTITIES: [(&str, &str); 8] = [
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&#34;", "\""),
    ("&#39;", "'"),
    ("&#x27;", "'"),
    ("&nbsp;", " "),
    ("&amp;", "&"),
];

/// `text` with the escaper's character references decoded, `&amp;` last.
fn decoded(text: &str) -> String {
    ENTITIES
        .iter()
        .fold(text.to_owned(), |out, (entity, c)| out.replace(entity, c))
}

/// The first of `block`'s sentences missing from `text` in order, or `None` when all show.
fn missing(text: &str, block: &Block) -> Option<String> {
    let mut from = 0;
    for sentence in expected(block) {
        match text[from..].find(&sentence) {
            Some(at) => from += at + sentence.len(),
            None => return Some(sentence),
        }
    }
    None
}

/// Why `block` is not checked on this render, or `None` when it is.
fn skip_reason(block: &Block) -> Option<&'static str> {
    if block.heading.contains(NUMBERS_ONLY) {
        return Some("WEB-028: shown only with the computed numbers, which this render leaves out");
    }
    SKIPPED
        .iter()
        .find(|(heading, _)| block.heading == *heading)
        .map(|(_, reason)| *reason)
}

/// The home page's text content as the page renders with no database.
fn home_text() -> String {
    let html = home_page(&SitePages::default(), false, None, "fake-token".to_owned())
        .expect("home page renders");
    page_text(&html)
}

/// The copy file's page blocks, read from the repository.
fn copy_blocks() -> Vec<Block> {
    let path = format!("{ROOT}/docs/copy/site-copy.md");
    blocks(&std::fs::read_to_string(&path).expect("docs/copy/site-copy.md"))
}

/// WORD-029, D-474, D-4669: every home and footer block of `docs/copy/site-copy.md` shows on the
/// home page sentence by sentence in order; the two placed elsewhere are skipped with their
/// reasons, and each skipped heading still exists in the file.
#[test]
fn word_029_every_home_and_footer_block_shows_on_the_page() {
    let (text, blocks) = (home_text(), copy_blocks());
    let mut checked = 0;
    for block in &blocks {
        assert!(
            !block.words.is_empty(),
            "{}: no quoted words",
            block.heading
        );
        if let Some(reason) = skip_reason(block) {
            println!("skipped {}: {reason}", block.heading);
            continue;
        }
        if let Some(sentence) = missing(&text, block) {
            panic!("{}: not on the home page: {sentence}", block.heading);
        }
        checked += 1;
    }
    for (heading, _) in SKIPPED {
        assert!(
            blocks.iter().any(|b| b.heading == heading),
            "{heading} gone"
        );
    }
    assert!(blocks.iter().any(|b| b.heading.starts_with("Footer · ")));
    assert!(checked >= 20, "only {checked} blocks checked");
}

/// WORD-029, D-4669: the check fails a changed word, a sentence out of order and a dropped
/// sentence, and drops only a requirement citation.
#[test]
fn word_029_the_block_check_fails_planted_differences() {
    let text = page_text("<p>First one.</p><p>Second &amp; last <a href=\"/x\">link</a></p>");
    let block = |words: &str| Block {
        heading: "Home · planted".to_owned(),
        words: words.to_owned(),
    };
    for (words, gap) in [
        ("First one. Second & last link (WEB-004)", None),
        (
            "First one. Second & first link",
            Some("Second & first link"),
        ),
        ("Second & last link. First one", Some("First one")),
        ("First one. A third one.", Some("A third one")),
    ] {
        assert_eq!(missing(&text, &block(words)).as_deref(), gap, "{words}");
    }
    assert_eq!(without_citation("Pick one (or none)"), "Pick one (or none)");
    assert_eq!(without_citation("Go. (CON-005; carrier rules)"), "Go.");
    assert_eq!(decoded("it&#x27;s &#39;a&#39; &amp;lt;"), "it's 'a' &lt;");
}

/// D-3890 (3), WEB-017: while the start line names the texting number, only its chat sentence is
/// matched; without a number, the whole block is.
#[test]
fn d_3890_the_start_line_matches_its_chat_sentence_while_it_names_the_number() {
    let block = |words: &str| Block {
        heading: START_LINE.to_owned(),
        words: words.to_owned(),
    };
    let with_number = block("Text 555-0100 to start. Or start in the chat on this site.");
    assert_eq!(
        expected(&with_number),
        ["Or start in the chat on this site"]
    );
    let chat_only = block("Start in the chat on this site. You don't need to know the trade.");
    assert_eq!(expected(&chat_only).len(), 2);
    let numbers = Block {
        heading: "Home · numbers heading".to_owned(),
        words: "Requests so far".to_owned(),
    };
    assert!(skip_reason(&numbers).is_some());
}
