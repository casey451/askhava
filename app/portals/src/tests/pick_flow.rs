//! The pick review step and the pick result as rendered (TD-PICK-08, TD-PICK-28, TD-PICK-30,
//! TD-PICK-24, TD-A11Y-02; PICK-016, PICK-017, PICK-045, PICK-054, A11Y-014, A11Y-031). Fake
//! request numbers and a fake 64-hex token only.

use askama::Template;

use askhava_jobs::api::JobStatus;

use crate::pick_flow::{PageLanguage, ReviewView, result_page, takes_pick};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn review(language: PageLanguage) -> String {
    ReviewView::review(1042, 2, TOKEN.to_owned(), "csrf-sample".to_owned())
        .in_language(language)
        .render()
        .expect("review renders")
}

/// The text between the final Pick button and the end of its form.
fn beside_pick(html: &str) -> &str {
    let start = html.find("id=\"final-pick\"").expect("final Pick button");
    let rest = &html[start..];
    let end = rest.find("</form>").expect("form end");
    &rest[..end]
}

#[test]
fn td_pick_08_review_says_what_happens_next_with_go_back() {
    let html = review(PageLanguage::En);
    for line in [
        "You're picking one business for this request. If it falls through, you can pick another quote.",
        "You hire the business directly. Ask Hava isn't part of your agreement with it.",
        "or reply 2 YES",
        "After you press Pick, we text a 6-digit code to your phone.",
    ] {
        assert!(html.contains(line), "missing: {line}");
    }
    let back = format!("<a href=\"/q/{TOKEN}?#quote-2\">Go back</a>");
    assert!(html.contains(&back), "Go back returns to the card");
    assert!(html.contains(&format!("action=\"/q/{TOKEN}/pick\"")));
}

#[test]
fn td_pick_30_only_the_terms_line_beside_pick_in_english_and_spanish() {
    let cases = [
        (
            PageLanguage::En,
            "lang=\"en\">By picking, you agree to the <a href=\"/terms\">Terms</a></p>",
        ),
        (
            PageLanguage::Es,
            "lang=\"es\">Al elegir, usted acepta los <a href=\"/terms\">Términos</a></p>",
        ),
    ];
    for (language, line) in cases {
        let html = review(language);
        let beside = beside_pick(&html);
        assert!(beside.contains(line), "Terms line beside Pick: {beside}");
        assert_eq!(beside.matches("<p").count(), 1, "nothing else beside Pick");
        for banned in ["$100", "liability", "Liability", "key terms", "Key terms"] {
            assert!(!html.contains(banned), "pick page shows {banned}");
        }
    }
}

#[test]
fn td_pick_28_review_shows_no_confirm_end_time() {
    let html = review(PageLanguage::En);
    for banned in ["until", "confirm by", " a.m.", " p.m.", "4 business hours"] {
        assert!(!html.contains(banned), "review shows an end time: {banned}");
    }
}

#[test]
fn td_a11y_02_final_pick_button_never_disabled() {
    let html = review(PageLanguage::En);
    assert!(!html.contains("disabled"), "A11Y-031: no disabled submit");
    assert!(html.contains("novalidate"));
    assert!(html.contains("name=\"csrf\" value=\"csrf-sample\""));
}

#[test]
fn td_a11y_02_final_pick_before_the_code_step_says_nothing_was_picked() {
    let mut view = ReviewView::review(1042, 2, TOKEN.to_owned(), "csrf-sample".to_owned());
    view.not_open = true;
    let html = view.render().expect("review renders");
    assert!(
        html.contains("Nothing was picked."),
        "D-1251: the press is not silent"
    );
    assert!(
        html.contains("id=\"final-pick\""),
        "the review stays usable"
    );
    assert!(!html.contains("disabled"), "A11Y-031: no disabled submit");
    assert!(!review(PageLanguage::En).contains("Nothing was picked"));
}

#[test]
fn pick_012_pick_032_a_pick_is_taken_until_the_business_confirms() {
    assert!(
        takes_pick(JobStatus::Open),
        "PICK-012: from the first quote"
    );
    assert!(takes_pick(JobStatus::QuotesReady));
    assert!(
        takes_pick(JobStatus::AwaitingConfirm),
        "PICK-032: replaces the waiting pick"
    );
    for status in [
        JobStatus::Intake,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ] {
        assert!(!takes_pick(status), "{status:?}");
    }
    for status in JobStatus::ALL.into_iter().filter(|s| s.is_final()) {
        assert!(!takes_pick(status), "{status:?}");
    }
}

#[test]
fn a11y_014_result_heading_carries_autofocus_and_the_after_done_lines() {
    let html = result_page(1042, 2, TOKEN.to_owned(), None).expect("result renders");
    let heading =
        "<h1 id=\"pick-result\" tabindex=\"-1\" autofocus>Done. The business has your pick.</h1>";
    assert!(html.contains(heading), "focusable result heading");
    for line in [
        "We'll text you when they accept. You can still pick another quote on",
        ">Add Ask Hava to your contacts</a>",
        "URL%3Ahttps%3A%2F%2Faskhava.com",
        "Know someone who needs quotes? Send them askhava.com.",
    ] {
        assert!(html.contains(line), "missing: {line}");
    }
    assert!(!html.contains("final-pick"), "no second Pick on the result");
    assert!(
        !html.contains("so you know it's us"),
        "PICK-054: no texting number yet"
    );
    for banned in ["reward", "Reward"] {
        assert!(!html.contains(banned), "result shows {banned}");
    }
}

#[test]
fn pick_045_result_shows_the_acceptance_end_time_when_given() {
    let time = "6:10 p.m. Arizona time".to_owned();
    let html = result_page(1042, 2, TOKEN.to_owned(), Some(time)).expect("result renders");
    assert!(html.contains("The business has until 6:10 p.m. Arizona time to accept."));
    let none = result_page(1042, 2, TOKEN.to_owned(), None).expect("result renders");
    assert!(
        !none.contains("id=\"accept-by\""),
        "no end time without a value"
    );
}
