//! Retry steps, call-back due times, the daily limit and the email layout (TD-TXT-05, TD-OPS-14,
//! TD-BIZ-26, TD-A11Y-02; TXT-014, TXT-015, INT-023, OPS-038, SEC-028, A11Y-034). Fake data only.

use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;

use crate::email::{DailyLimit, MailboxKind, Notice, PostalAddress, Recipient, next_reset, render};
use crate::retries::{NO_RUSH_CALL_BACK, Step, Urgency, call_back_due, next_step};
use crate::templates::{Audience, Language};

const LAYOUT: &str = include_str!("../../templates/email_layout.html");

fn at(s: &str) -> Timestamp {
    s.parse().expect("time")
}

fn postal() -> PostalAddress {
    PostalAddress::new(
        "PO Box 1234, Lake Havasu City, AZ 86405",
        MailboxKind::PoBox,
    )
    .expect("box")
}

fn to(audience: Audience, language: Language) -> Recipient<'static> {
    Recipient {
        id: Uuid::from_u128(7),
        audience,
        language,
        manage_url: "https://askhava.com/portal",
    }
}

/// TD-TXT-05, TXT-014, TXT-015: 3 retries 10, 20 and 30 minutes after the first failure, then
/// give up; nothing is retried past 30 minutes.
#[test]
fn td_txt_05_three_retries_over_thirty_minutes_then_give_up() {
    let first = at("2026-11-02T17:00:00Z");
    let steps: Vec<Step> = (1..=5).map(|n| next_step(first, n)).collect();
    assert_eq!(
        steps,
        [
            Step::RetryAt(at("2026-11-02T17:10:00Z")),
            Step::RetryAt(at("2026-11-02T17:20:00Z")),
            Step::RetryAt(at("2026-11-02T17:30:00Z")),
            Step::GiveUp,
            Step::GiveUp,
        ]
    );
    assert_eq!(next_step(first, 0), Step::GiveUp);
}

/// TD-TXT-05, INT-023: an urgent call-back is due in 24 hours, a no-rush one in 4 days.
#[test]
fn td_txt_05_call_back_due_24_hours_urgent_4_days_otherwise() {
    let now = at("2026-11-02T17:00:00Z");
    assert_eq!(
        call_back_due(now, Urgency::Urgent, NO_RUSH_CALL_BACK),
        at("2026-11-03T17:00:00Z")
    );
    assert_eq!(
        call_back_due(now, Urgency::NoRush, NO_RUSH_CALL_BACK),
        at("2026-11-06T17:00:00Z")
    );
    let two_days = SignedDuration::from_hours(48);
    assert_eq!(
        call_back_due(now, Urgency::NoRush, two_days),
        at("2026-11-04T17:00:00Z")
    );
}

/// TD-OPS-14, OPS-038: at 91 of 100 bulk email waits, at 90 it still goes; the reset is the next
/// midnight UTC; a zero limit or a share outside 1 to 100 is refused.
#[test]
fn td_ops_14_bulk_waits_above_ninety_percent() {
    let limit = DailyLimit::new(100, 90).expect("limit");
    assert!(limit.holds_bulk(91));
    assert!(!limit.holds_bulk(90));
    assert_eq!(
        next_reset(at("2026-11-02T23:59:59Z")),
        at("2026-11-03T00:00:00Z")
    );
    assert_eq!(
        next_reset(at("2026-11-03T00:00:00Z")),
        at("2026-11-04T00:00:00Z")
    );
    assert!(DailyLimit::new(0, 90).is_err());
    assert!(DailyLimit::new(100, 0).is_err());
    assert!(DailyLimit::new(100, 101).is_err());
}

/// TD-BIZ-26, SEC-028: the postal address is never empty and only a box or private mailbox.
#[test]
fn td_biz_26_postal_address_is_a_box_never_empty() {
    assert!(PostalAddress::new("", MailboxKind::PoBox).is_err());
    assert!(PostalAddress::new("12 Test Way, Lake Havasu City", MailboxKind::PoBox).is_err());
    assert!(PostalAddress::new("12 Test Way", MailboxKind::PrivateMailbox).is_err());
    assert!(PostalAddress::new("P.O. Box 9", MailboxKind::PoBox).is_ok());
    assert!(PostalAddress::new("100 Test Way PMB 12", MailboxKind::PrivateMailbox).is_ok());
}

/// Each audience and language with its why-line, manage label and `lang` (SEC-028, A11Y-034).
#[rustfmt::skip]
const FOOTERS: [(Audience, Language, &str, &str, &str); 4] = [
    (Audience::Business, Language::En, "because your business signed up for job requests from Ask Hava.", "Manage email notices", "en"),
    (Audience::Homeowner, Language::En, "because you asked Ask Hava for quotes.", "Manage email notices", "en"),
    (Audience::Business, Language::Es, "su negocio se inscribió", "Administrar avisos por correo", "es"),
    (Audience::Homeowner, Language::Es, "pidió cotizaciones a Ask Hava.", "Administrar avisos por correo", "es"),
];

/// TD-BIZ-26, TD-A11Y-02, SEC-028, A11Y-034: every audience and language gets its why-line,
/// "Manage email notices" (or its Spanish) linked to the portal, the LLC address and `lang`, in
/// both parts.
#[test]
fn td_biz_26_every_footer_has_address_link_and_why_line() {
    for (audience, language, why, manage, lang) in FOOTERS {
        let r =
            render(&postal(), &to(audience, language), Notice::StillOpen(1042)).expect("render");
        for part in [&r.html, &r.plain] {
            assert!(part.contains(why), "{lang} {audience:?}: why-line");
            assert!(part.contains(manage), "{lang} {audience:?}: manage link");
            assert!(
                part.contains("Ask Hava LLC, PO Box 1234"),
                "{lang}: postal address"
            );
            assert!(
                part.contains("https://askhava.com/portal"),
                "{lang}: portal link"
            );
        }
        assert!(
            r.html
                .starts_with(&format!("<!DOCTYPE html>\n<html lang=\"{lang}\">"))
        );
        assert!(r.subject.starts_with("Ask Hava: "));
        assert!(
            r.html.find("1042") < r.html.find(why),
            "content comes before the footer"
        );
    }
}

/// TD-BIZ-26, SEC-028: the layout and every notice line hold no promotion words.
#[test]
fn td_biz_26_notice_emails_contain_no_promotion() {
    let banned = [
        "refer",
        "invite",
        "new feature",
        "discount",
        "offer",
        "survey",
        "facebook",
        "deal",
        "free trial",
    ];
    let notices = [
        Notice::TextFailed("Ask Hava: fake text"),
        Notice::OptedOut,
        Notice::StillOpen(7),
    ];
    for language in [Language::En, Language::Es] {
        for notice in notices {
            let r = render(&postal(), &to(Audience::Homeowner, language), notice).expect("render");
            let all = format!("{} {} {}", r.subject, r.plain, LAYOUT).to_lowercase();
            for word in banned {
                assert!(!all.contains(word), "promotion word {word:?} in {notice:?}");
            }
        }
    }
}

/// TD-A11Y-02, A11Y-034: the layout's root carries `lang` and escapes what it is given.
#[test]
fn td_a11y_02_layout_root_has_lang_and_escapes() {
    assert!(LAYOUT.contains("<html lang=\"{{ lang }}\">"));
    let r = render(
        &postal(),
        &to(Audience::Homeowner, Language::En),
        Notice::TextFailed("<b>x</b>"),
    )
    .expect("render");
    assert!(!r.html.contains("<b>x</b>"));
    assert!(r.plain.contains("<b>x</b>"));
}
