//! TD-PRIV-23 (PRIV-031; T315, T448, D-2205, D-3265, D-3592): the "Download my data" masking
//! catches a bare domain and a phone number split across lines or any named separator, and keeps
//! decimals, money and abbreviations.
//! The existing download cases are `data_download::tests` (TD-PRIV-15), unchanged. Fake data only.

use sqlx::types::Uuid;

use crate::data_download::{HeldItem, Section, download_file, mask_contact_details};
use crate::homeowner_portal::OnFile;

/// TD-PRIV-23: a bare domain and a `www.` address are masked, with a path or wrapping punctuation.
#[test]
fn td_priv_23_a_bare_domain_is_masked() {
    assert_eq!(
        mask_contact_details("Call acmeplumbing.example"),
        "Call [removed]"
    );
    assert_eq!(mask_contact_details("see www.example.com"), "see [removed]");
    for words in [
        "Visit Acme-Plumbing.Example/quotes today",
        "(acmeplumbing.example)",
        "Our site is shop.acme.example.",
    ] {
        let masked = mask_contact_details(words);
        assert!(masked.contains("[removed]"), "{words}");
        assert!(!masked.to_ascii_lowercase().contains("example"), "{words}");
    }
}

/// TD-PRIV-23: a web address wrapped in brackets or quotes, after a label, with a possessive, a
/// port or a query is masked whole.
#[test]
fn td_priv_23_a_wrapped_or_suffixed_address_is_masked() {
    for words in [
        "(https://acmeplumbing.example)",
        "\"https://acmeplumbing.example\"",
        "Site:acmeplumbing.example",
        "acmeplumbing.example's",
        "acmeplumbing.example\u{2019}s",
        "acmeplumbing.example:8080",
        "acmeplumbing.example?ref=1",
        "https://acmeplumbing.example/quotes#top",
    ] {
        assert_eq!(mask_contact_details(words), "[removed]", "{words}");
    }
    assert_eq!(
        mask_contact_details("See (https://acmeplumbing.example) by 5 p.m."),
        "See [removed] by 5 p.m."
    );
}

/// TD-PRIV-23: a phone number broken by line breaks (with or without the separators already
/// allowed) is masked, and the line breaks around it stay.
#[test]
fn td_priv_23_a_number_split_across_lines_is_masked() {
    assert_eq!(mask_contact_details("928\n555\n0142"), "[removed]");
    assert_eq!(
        mask_contact_details("Call\n928-\n555-\r\n0142\nthanks"),
        "Call\n[removed]\nthanks"
    );
    assert_eq!(
        mask_contact_details("Sizes\n24\n36\n48"),
        "Sizes\n24\n36\n48"
    );
}

/// TD-PRIV-23: decimals, money and abbreviations are never read as a domain or a number.
#[test]
fn td_priv_23_decimals_money_and_titles_are_kept() {
    for words in [
        "3.5 hours",
        "$1,250.00",
        "Dr. Lee",
        "about 3.5 hours, $1,250.00 total, ask Dr. Lee",
        "e.g. a new valve, done by 5 p.m.",
        "version 2.0 of the plan",
    ] {
        assert_eq!(mask_contact_details(words), words);
    }
}

/// TD-PRIV-23: in the file, a quote's bare domain and a message's split number are masked, and
/// the requests section is left as written.
#[test]
fn td_priv_23_the_file_masks_both_in_quotes_and_messages() {
    let her = OnFile::new(Uuid::from_u128(7), "+19285550100");
    let at = jiff::Timestamp::from_second(1_800_000_000).expect("time");
    let item = |section, words: &str| HeldItem {
        contact: Uuid::from_u128(7),
        section,
        at,
        words: words.to_owned(),
    };
    let items = [
        item(
            Section::Quotes,
            "Acme: $1,250.00, 3.5 hours. Call acmeplumbing.example",
        ),
        item(Section::Messages, "Text 928\n555\n0142"),
        item(Section::Requests, "Leaky faucet, about 3.5 hours"),
    ];
    let file = download_file(&her, &items);
    assert!(file.contains("Acme: $1,250.00, 3.5 hours. Call [removed]"));
    assert!(file.contains("Text [removed]"));
    assert!(file.contains("Leaky faucet, about 3.5 hours"));
    assert!(!file.contains("acmeplumbing") && !file.contains("0142"));
}

/// PRIV-031, D-3592: a fake 555 number split by a slash, an underscore, a tab, a non-breaking
/// space, an en dash or an em dash is masked; six digits with the same separators are not.
#[test]
fn a_number_split_by_any_named_separator_is_masked() {
    for sep in ["/", "_", "\t", "\u{a0}", "\u{2013}", "\u{2014}"] {
        let number = ["928", "555", "0100"].join(sep);
        assert_eq!(
            mask_contact_details(&format!("Call {number} now")),
            "Call [removed] now",
            "{sep:?}"
        );
        let short = ["92", "55", "01"].join(sep);
        let kept = format!("Size {short} only");
        assert_eq!(mask_contact_details(&kept), kept, "{sep:?}");
    }
}

/// PRIV-031, D-3592: a minus sign, a figure dash and fullwidth digits also join a run.
#[test]
fn a_minus_sign_or_fullwidth_digits_are_masked() {
    assert_eq!(
        mask_contact_details("928\u{2212}555\u{2212}0100"),
        "[removed]"
    );
    assert_eq!(
        mask_contact_details("928\u{2012}555\u{2012}0100"),
        "[removed]"
    );
    assert_eq!(
        mask_contact_details("\u{ff19}\u{ff12}\u{ff18} 555 0100"),
        "[removed]"
    );
}

/// PRIV-031, D-3592: a comma, colon or dollar sign still ends a run, so money and durations stay.
#[test]
fn money_and_durations_are_never_joined() {
    for words in ["$12,500.00, 3.5 hours", "Acme: $1,250.00, 3.5 hours"] {
        assert_eq!(mask_contact_details(words), words);
    }
}
