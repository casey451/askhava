//! Pages from the brand file (TD-WORD-37; WORD-001, WORD-002, WEB-001, SEC-025; T294, D-1932): under
//! the embedded brand the layout's title suffix and home link, the result page's contact card, the
//! share-box label, the sign-up notice lines, the "Why was I charged?" lead line and the emailed
//! sign-in site are byte-identical to the literals they replaced. Fake request numbers, a fake
//! 64-hex token and fake IDs only.

use askama::Template;
use askhava_payments::api::{ChargeReceipt, Tier};
use jiff::Timestamp;
use sqlx::types::Uuid;

use crate::business_auth::site;
use crate::business_signup::notice_line;
use crate::feedback::{FeedbackView, share_label};
use crate::pick_flow::{PageLanguage, result_page};
use crate::receipts_page::{Basis, WhyCharged, why_charged_page};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// TD-WORD-37 (WORD-001, WEB-001): `base.html`'s title suffix and home link carry the brand.
#[test]
fn td_word_37_layout_title_and_home_link_are_todays() {
    let html = FeedbackView::form(1042, TOKEN.to_owned(), "csrf-sample".to_owned())
        .render()
        .expect("form renders");
    assert!(html.contains("<title>Feedback on request 1042 | Ask Hava</title>"));
    assert!(html.contains("<header>\n<p><a href=\"/\">Ask Hava</a></p>\n</header>"));
    assert!(html.contains("<label for=\"feedback-share\">Ask Hava may show what I wrote on askhava.com with my first name and town</label>"));
    assert_eq!(
        share_label(),
        "Ask Hava may show what I wrote on askhava.com with my first name and town"
    );
}

/// TD-WORD-37 (WORD-001): the result page's contact card, its file name and the share line.
#[test]
fn td_word_37_contact_card_link_is_todays() {
    let html = result_page(1042, 2, TOKEN.to_owned(), None).expect("result renders");
    assert!(html.contains(
        "<p><a href=\"data:text/vcard;charset=utf-8,BEGIN%3AVCARD%0D%0AVERSION%3A3.0%0D%0AFN%3AAsk%20Hava%0D%0AORG%3AAsk%20Hava%20LLC%0D%0AURL%3Ahttps%3A%2F%2Faskhava.com%0D%0AEND%3AVCARD%0D%0A\" download=\"ask-hava.vcf\">Add Ask Hava to your contacts</a></p>\n<p>Know someone who needs quotes? Send them askhava.com.</p>"
    ));
}

/// TD-WORD-37 (SEC-025): the emailed sign-in link is on the brand's domain only.
#[test]
fn td_word_37_sign_in_site_is_the_brand_domain() {
    assert_eq!(site(), "https://askhava.com");
}

/// TD-WORD-37 (WORD-001): the sign-up notice lines while texting is on, English and Spanish.
#[test]
fn td_word_37_notice_lines_are_todays() {
    assert_eq!(
        notice_line(true, PageLanguage::En),
        (
            "Choose how you get job requests and picks: email, text or both. Ask Hava never calls you with a recording.".to_owned(),
            "en"
        )
    );
    assert_eq!(
        notice_line(true, PageLanguage::Es),
        (
            "Elija cómo recibir las solicitudes de trabajo y las selecciones: correo electrónico, texto o ambos. Ask Hava nunca le llama con una grabación.".to_owned(),
            "es"
        )
    );
}

/// TD-WORD-37 (WORD-001, WORD-002): the "Why was I charged?" on-site lead line and contractor line.
#[test]
fn td_word_37_why_charged_lines_are_todays() {
    let facts = WhyCharged {
        receipt: ChargeReceipt {
            pick: Uuid::nil(),
            business: Uuid::nil(),
            job: Uuid::nil(),
            fee_cents: 2500,
            tier: Some(Tier::Small),
            charged_at: Timestamp::UNIX_EPOCH,
        },
        job_number: 1042,
        quote: "$80.00 visit fee".to_owned(),
        basis: Basis::OnSite,
    };
    let html = why_charged_page(&facts).expect("page renders");
    assert!(html.contains("You set your price after seeing the job, so Ask Hava used the usual visit fee for this kind of work. That makes it a small job."));
    assert!(html.contains(
        "<li>Ask Hava is not a contractor and does not perform, bid on or supervise work.</li>"
    ));
}
