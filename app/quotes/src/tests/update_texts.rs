//! TD-TXT-25 (TXT-040, WORD-024): the homeowner update texts after intake keep short sentences,
//! carry her page link and list no quote details outside the LIST reply. Fake request numbers and
//! links only. TXT-041's Spanish line (the entry's other case) is T162's.

use askhava_messaging::api::{Audience, Language, Registry};

use crate::api::{
    Counts, LIMIT, Problem, Reader, SHORT, Shape, UPDATE_TEXTS, check, first_quote_text, scan,
    sentences, update_text, words,
};

const LINK: &str = "https://askhava.com/q/sample-token";

/// A text of one sentence of `n` words, with the opening and the link.
fn planted(n: usize) -> String {
    let words = vec!["word"; n].join(" ");
    format!("Ask Hava: {words}. See them here: {{link}}")
}

/// TD-TXT-25: every listed template, in English and Spanish, passes the scan.
#[test]
fn td_txt_25_every_update_template_passes() {
    assert_eq!(scan(), Ok(Vec::new()));
}

/// TD-TXT-25: a planted 21-word sentence fails and 20 passes; a sentence no requirement fixes
/// fails at 16 words and passes at 15.
#[test]
fn td_txt_25_planted_21_word_sentence_fails_20_passes() {
    assert_eq!(check(&planted(LIMIT), Shape::Fixed), Vec::new());
    let long = Problem::Long {
        sentence: 0,
        words: LIMIT + 1,
    };
    assert_eq!(check(&planted(LIMIT + 1), Shape::Fixed), vec![long]);
    assert_eq!(check(&planted(LIMIT + 1), Shape::List), vec![long]);
    assert_eq!(check(&planted(SHORT), Shape::Update), Vec::new());
    let over = Problem::Long {
        sentence: 0,
        words: SHORT + 1,
    };
    assert_eq!(check(&planted(SHORT + 1), Shape::Update), vec![over]);
}

/// TD-TXT-25: an update with no page link fails, and one with an amount or a value other than
/// the request number, counts and link lists a quote detail; the LIST reply may do both.
#[test]
fn td_txt_25_link_required_and_no_quote_details() {
    let bare = "Ask Hava: You now have {quoted} quotes for request {job}.";
    assert_eq!(check(bare, Shape::Update), vec![Problem::NoLink]);
    let amount = "Ask Hava: A quote for $190 is in. See it here: {link}";
    assert_eq!(check(amount, Shape::Update), vec![Problem::Details]);
    let named = "Ask Hava: {business} quoted request {job}. See it here: {link}";
    assert_eq!(check(named, Shape::Fixed), vec![Problem::Details]);
    let list = "Ask Hava: Your quotes for request {job}:\n1. {business}: $190.";
    assert_eq!(check(list, Shape::List), Vec::new());
}

/// TD-TXT-25: the opening is not counted, a link or placeholder is one word, and a link's dot
/// does not end a sentence.
#[test]
fn td_txt_25_sentences_and_words() {
    let text = "Ask Hava: Two words. See askhava.com/terms now? Last {link}\nNext line";
    let found = sentences(text);
    assert_eq!(found.len(), 4);
    let counts: Vec<usize> = found.iter().map(|s| words(s)).collect();
    assert_eq!(counts, vec![2, 3, 2, 2]);
}

/// TD-TXT-25: the texts as sent, link filled, pass in both languages.
#[test]
fn td_txt_25_rendered_update_texts_pass() {
    for wanted in [Language::En, Language::Es] {
        let reader = Reader {
            wanted,
            spanish_on: true,
        };
        let first = first_quote_text(reader, "1234", LINK).expect("first");
        assert_eq!(check(&first, Shape::Fixed), Vec::new());
        let counts = Counts {
            job: "1234",
            quoted: 3,
            sent: 7,
        };
        let update = update_text(reader, counts, LINK).expect("update");
        assert_eq!(check(&update, Shape::Update), Vec::new());
    }
}

/// TD-TXT-25: every listed template is a homeowner text.
#[test]
fn td_txt_25_listed_templates_are_homeowner_texts() {
    let registry = Registry::embedded().expect("registry");
    for (id, _) in UPDATE_TEXTS {
        assert_eq!(registry.audience(id), Some(Audience::Homeowner), "{id}");
    }
}
