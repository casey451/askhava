//! T748: the site help form's answers promise no reply (OPS-036, WORD-029, D-4748). The form
//! keeps no sender address until a later task adds a required email field, so no screen that
//! says the owner has the message may say a reply will come; the 911 sentence stays. PRIV-031's
//! account-ask line is the one exception (D-4772).

use askhava_core::api::brand;

use crate::help::{HelpFormScreen, help_form_page};

/// Words that promise a reply or a reply time.
const PROMISES: [&str; 4] = [
    "Replies",
    "reply within",
    "within 2 days",
    "within 24 hours",
];

/// PRIV-031's line about a copy or delete ask made from the signed-in account, which does reach
/// the homeowner, keeps its 30-day answer (D-4772); every other line is checked.
const DATA_ASK: &str = "Once you're in, ask from your account. We reply within 30 days.";

/// OPS-036, WORD-029, D-4748: every screen that confirms the message promises no reply and
/// keeps the 911 sentence, with the brand from the brand file.
#[test]
fn help_form_screens_promise_no_reply() {
    let got = format!(
        "The {} owner has your message. In an emergency, call 911.",
        brand().display_name()
    );
    for screen in [
        HelpFormScreen::Sent,
        HelpFormScreen::OptedOut,
        HelpFormScreen::NeedsNumber,
    ] {
        let page = help_form_page(screen).expect("page");
        for line in page.lines().filter(|l| !l.contains(DATA_ASK)) {
            for promise in PROMISES {
                assert!(!line.contains(promise), "{screen:?}: {line}");
            }
        }
        assert!(page.contains(&got), "{screen:?}: {page}");
    }
}

/// D-4748: the form's optional email label promises no reply either.
#[test]
fn help_form_email_label_promises_no_reply() {
    let form = include_str!("../../../site/templates/help.html");
    assert!(form.contains("<label for=\"help-email\">Your email (optional)</label>"));
    assert!(!form.contains("for our reply"));
}
