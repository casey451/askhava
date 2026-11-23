//! The owner's waiting-phones page (IC-95; BIZ-033, BIZ-038, SEC-038, SEC-009; T397, D-3081,
//! D-3744).
//!
//! [`phone_approval_page`] renders `templates/phone_approval.html` (it extends
//! `business_layout.html`, so it shows the business-only "Get help" link, BIZ-047): one entry per
//! phone waiting for the owner's approval, naming the sign-in address of the member who set its
//! number (read from the vault by the `server` composition and handed in; `None` shows one fixed
//! line), the number's last 4 digits and the Arizona time it was set, never a countdown, with an
//! Approve and a Don't approve form. Each form carries the CSRF token, the phone's line ID and the
//! `number_set_at` shown, so an approval names the number the owner saw (D-3081); a member who
//! changed the number since gets [`PhonesNote::NumberChanged`]. This module reads nothing from the
//! database or the vault and shows no whole number. The routes are served by `server`'s
//! `phone_approval` module behind `business_principal`.

use askama::Template;
use askhava_businesses::api::WaitingPhone;
use askhava_core::api::arizona_time;
use jiff::Timestamp;
use sqlx::types::Uuid;

/// The owner's waiting-phones page (BIZ-033, BIZ-038).
pub const PHONES_ROUTE: &str = "/b/phones";
/// The Approve form's target; the line travels in the form body (D-3081).
pub const APPROVE_PHONE_ROUTE: &str = "/b/phones/approve";
/// The Don't approve form's target; the line travels in the form body (D-3081).
pub const REJECT_PHONE_ROUTE: &str = "/b/phones/reject";

/// One waiting phone as the page shows it: the phone and its member's sign-in address, `None`
/// when the composition has no vault or the member is gone (D-3744, D-3889).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownPhone {
    /// The waiting phone (`businesses::api::waiting_phones`).
    pub phone: WaitingPhone,
    /// The sign-in address of the member who set its number, when it could be read.
    pub set_by: Option<String>,
}

/// A line shown above the list after a press that changed nothing (BIZ-033, D-3081).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhonesNote {
    /// Nothing to say.
    None,
    /// A member changed the number after the page was shown; nothing changed.
    NumberChanged,
    /// Another phone now has the number, so it was not approved.
    NumberTaken,
}

/// One phone's entry in the template.
struct Entry {
    line: String,
    who: Option<String>,
    end: Option<String>,
    set_at: String,
    when: String,
}

#[derive(Template)]
#[template(path = "phone_approval.html")]
struct PhonesView<'a> {
    entries: Vec<Entry>,
    note: PhonesNote,
    csrf: &'a str,
    approve_route: &'a str,
    reject_route: &'a str,
}

/// The time a number was set, in Arizona time, such as "Nov 22, 2026 at 9:05 AM".
fn shown_time(at: Timestamp) -> String {
    arizona_time(at)
        .strftime("%b %-d, %Y at %-I:%M %p")
        .to_string()
}

fn entry(shown: &ShownPhone) -> Entry {
    let at = shown.phone.number_set_at;
    Entry {
        line: shown.phone.line.to_string(),
        who: shown.set_by.clone(),
        end: shown.phone.number_end.clone(),
        set_at: at.to_string(),
        when: shown_time(at),
    }
}

/// BIZ-033, BIZ-038, SEC-009, D-3081: the page listing `phones`, each form carrying `csrf` and the
/// `number_set_at` shown, with `note` above the list.
///
/// # Errors
/// [`askama::Error`] when the template fails.
pub fn phone_approval_page(
    phones: &[ShownPhone],
    note: PhonesNote,
    csrf: &str,
) -> Result<String, askama::Error> {
    PhonesView {
        entries: phones.iter().map(entry).collect(),
        note,
        csrf,
        approve_route: APPROVE_PHONE_ROUTE,
        reject_route: REJECT_PHONE_ROUTE,
    }
    .render()
}

/// A posted phone form's line and `number_set_at`, parsed once (R1); `None` for a bad value.
#[must_use]
pub fn posted_phone(line: &str, set_at: &str) -> Option<(Uuid, Timestamp)> {
    Some((Uuid::try_parse(line).ok()?, set_at.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(end: Option<&str>, set_by: Option<&str>) -> ShownPhone {
        ShownPhone {
            phone: WaitingPhone {
                line: Uuid::from_u128(7),
                set_by: Uuid::from_u128(8),
                number_end: end.map(str::to_owned),
                number_set_at: "2026-11-22T16:05:00Z".parse().expect("time"),
            },
            set_by: set_by.map(str::to_owned),
        }
    }

    #[test]
    fn each_phone_shows_its_member_digits_and_arizona_time() {
        let shown = [phone(Some("0150"), Some("member@example.com"))];
        let html = phone_approval_page(&shown, PhonesNote::None, "tok").expect("page");
        assert!(html.contains("member@example.com") && html.contains("0150"));
        assert!(html.contains("Nov 22, 2026 at 9:05 AM"), "{html}");
        assert!(html.contains("value=\"2026-11-22T16:05:00Z\""), "{html}");
        assert!(html.contains("data-get-help"), "the business layout");
        assert!(!html.contains("number-changed"));
    }

    #[test]
    fn no_address_shows_the_fixed_line_and_notes_show() {
        let shown = [phone(None, None)];
        let html = phone_approval_page(&shown, PhonesNote::NumberChanged, "t").expect("page");
        assert!(html.contains("number-changed") && html.contains("someone on your team"));
        let taken = phone_approval_page(&shown, PhonesNote::NumberTaken, "t").expect("page");
        assert!(taken.contains("number-taken"));
        let empty = phone_approval_page(&[], PhonesNote::None, "t").expect("page");
        assert!(empty.contains("No phones are waiting"));
    }

    #[test]
    fn a_posted_phone_parses_once() {
        let at = "2026-11-22T16:05:00.123456Z";
        let line = Uuid::from_u128(7).to_string();
        let parsed = posted_phone(&line, at).expect("parses");
        assert_eq!(parsed.1.to_string(), at, "the shown time round-trips");
        assert!(posted_phone("x", at).is_none() && posted_phone(&line, "yesterday").is_none());
    }
}
