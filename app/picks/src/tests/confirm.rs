//! The pure Confirm-window rules (TD-PICK-09, TD-PICK-15, TD-JOB-03, TD-PICK-33, TD-PICK-28;
//! PICK-019, PICK-020, PICK-033, PICK-034, PICK-035, PICK-037). Fake job numbers and links only.

use askhava_licensing::api::LookupOutcome;
use askhava_messaging::api::{Language, Registry};
use jiff::Timestamp;

use crate::confirm::{
    BusinessWord, ConfirmReply, ConfirmSettings, EXPIRED_TEMPLATE, FEE_REPLY_TEMPLATE, FeeBasis,
    LicenseAtPick, PICK_NOTICE_EMERGENCY_TEMPLATE, PICK_NOTICE_TEMPLATE, WINDOW_KEY,
    business_reply, confirm_by, fee_line, license_at_pick, read_business_word,
};
use crate::status::{confirm_backstop_minutes, pick_lost};
use askhava_core::api::SettingsError;
use askhava_jobs::api::JobStatus;

/// The English fee line, which every caller in these tests has.
fn en(fee_cents: i64, basis: Option<FeeBasis>) -> String {
    fee_line(fee_cents, basis, Language::En).expect("a fee line")
}

fn window(value: &str) -> Result<ConfirmSettings, SettingsError> {
    ConfirmSettings::from_pairs([
        ("ASKHAVA_OTHER".to_owned(), "9".to_owned()),
        (WINDOW_KEY.to_owned(), value.to_owned()),
    ])
}

/// Arizona wall-clock time (UTC-7 all year).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(-7)))
        .expect("time")
        .timestamp()
}

/// TD-PICK-09 (PICK-019, D-482): a notice sent at 7:41 p.m. counts 19 minutes that day and the
/// rest from 08:00, ending 11:41 the next day; the window is a setting, so 3 business hours end
/// at 10:41 with no code change.
#[test]
fn td_pick_09_window_counts_business_hours_and_is_a_setting() {
    let four = ConfirmSettings::default();
    assert_eq!(four.window_minutes, 240);
    assert_eq!(confirm_by(az(9, 19, 41), four), az(10, 11, 41));
    let three = ConfirmSettings {
        window_minutes: 180,
    };
    assert_eq!(confirm_by(az(9, 19, 41), three), az(10, 10, 41));
    assert_eq!(confirm_by(az(9, 9, 0), four), az(9, 13, 0));
}

/// TD-PICK-09 (PICK-019, D-345, D-1503): the window is the `ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES`
/// setting, 240 when absent or empty; a value that is not 1 to 3,600 stops startup naming only the
/// key; `jobs`' backstop length is read from `jobs`, not copied.
#[test]
fn td_pick_09_the_window_is_read_from_the_settings() {
    assert_eq!(
        ConfirmSettings::from_pairs([]),
        Ok(ConfirmSettings::default())
    );
    assert_eq!(window(" 180 ").map(|s| s.window_minutes), Ok(180));
    assert_eq!(window("").map(|s| s.window_minutes), Ok(240));
    for bad in ["0", "3601", "-5", "four", "2.5"] {
        assert_eq!(
            window(bad),
            Err(SettingsError::Invalid(WINDOW_KEY)),
            "{bad}"
        );
    }
    assert_eq!(confirm_backstop_minutes(), 240);
}

/// PICK-019, PICK-044, D-1503: a pick is dead once its job went back to Quotes ready or ended,
/// never after a Confirm moved it on.
#[test]
fn td_pick_09_a_returned_job_loses_its_pick() {
    assert!(pick_lost(JobStatus::QuotesReady));
    assert!(pick_lost(JobStatus::Closed));
    assert!(pick_lost(JobStatus::Cancelled));
    assert!(!pick_lost(JobStatus::AwaitingConfirm));
    assert!(!pick_lost(JobStatus::PaymentPending));
    assert!(!pick_lost(JobStatus::Connected));
}

/// TD-JOB-03 (PICK-019, D-482): a pick notice at 7:30 p.m. has its 4 business hours end at
/// 11:30 a.m.; one sent at night counts from 8 a.m.
#[test]
fn td_job_03_a_pick_at_7_30_pm_ends_at_11_30_am() {
    let four = ConfirmSettings::default();
    assert_eq!(confirm_by(az(9, 19, 30), four), az(10, 11, 30));
    assert_eq!(confirm_by(az(9, 23, 0), four), az(10, 12, 0));
}

/// TD-PICK-09 (PICK-020, D-056): the live lookup decides at the pick; when it is down a daily
/// Active result under 24 hours old is used, otherwise the pick waits; an inactive license is
/// ineligible whatever the daily result.
#[test]
fn td_pick_09_license_at_the_pick() {
    use LicenseAtPick::{Eligible, Ineligible, Waits};
    assert_eq!(license_at_pick(LookupOutcome::Active, false), Eligible);
    assert_eq!(license_at_pick(LookupOutcome::Inactive, true), Ineligible);
    assert_eq!(license_at_pick(LookupOutcome::Unknown, true), Eligible);
    assert_eq!(license_at_pick(LookupOutcome::Unknown, false), Waits);
}

/// TD-PICK-15 (PICK-034, D-196): a $125-a-month pick shows the 4-month basis; a $160-weekly pick
/// shows the cap; a one-time job shows the fee alone.
#[test]
fn td_pick_15_recurring_fee_lines() {
    let monthly = FeeBasis {
        monthly_cents: 12_500,
        basis_cents: 50_000,
        capped: false,
    };
    assert_eq!(
        en(7_500, Some(monthly)),
        "Fee $75 (ongoing services count as 4 months: $125 x 4 = $500)"
    );
    let weekly = FeeBasis {
        monthly_cents: 69_333,
        basis_cents: 277_333,
        capped: true,
    };
    assert_eq!(
        en(7_500, Some(weekly)),
        "Fee $75 (ongoing services count as 4 months, capped at $75)"
    );
    let small = FeeBasis {
        monthly_cents: 8_667,
        basis_cents: 34_668,
        capped: false,
    };
    assert_eq!(
        en(2_500, Some(small)),
        "Fee $25 (ongoing services count as 4 months: $86.67 x 4 = $346.68)"
    );
    let big = FeeBasis {
        monthly_cents: 40_000,
        basis_cents: 160_000,
        capped: false,
    };
    assert!(en(19_900, Some(big)).ends_with("$400 x 4 = $1,600)"));
    assert_eq!(en(19_900, None), "Fee $199");
}

/// PICK-034, TXT-003, D-1503: a Spanish reply carries a Spanish fee line with the same amounts;
/// no fee, even a capped one, has no fee line.
#[test]
fn td_pick_15_the_fee_line_follows_the_language_and_a_zero_fee_has_none() {
    let monthly = FeeBasis {
        monthly_cents: 12_500,
        basis_cents: 50_000,
        capped: false,
    };
    let capped = FeeBasis {
        capped: true,
        ..monthly
    };
    assert_eq!(
        fee_line(7_500, Some(monthly), Language::Es).as_deref(),
        Some("Tarifa $75 (los servicios continuos cuentan como 4 meses: $125 x 4 = $500)")
    );
    assert_eq!(
        fee_line(7_500, Some(capped), Language::Es).as_deref(),
        Some("Tarifa $75 (los servicios continuos cuentan como 4 meses, con tope de $75)")
    );
    assert_eq!(
        fee_line(2_500, None, Language::Es).as_deref(),
        Some("Tarifa $25")
    );
    assert_eq!(fee_line(0, Some(capped), Language::En), None);
    assert_eq!(fee_line(0, None, Language::Es), None);
    assert_eq!(fee_line(-100, None, Language::En), None);
}

/// TD-PICK-15 (PICK-033, PICK-035, D-259, D-365): YES and CONFIRM, in English or Spanish, with or
/// without the job number, read; a linked phone gets the fee and link (never a confirm or a
/// charge), after the window the expired reply; an unlinked phone or another business nothing.
#[test]
fn td_pick_15_yes_and_confirm_by_text() {
    use BusinessWord::{Confirm, Yes};
    assert_eq!(read_business_word("YES 1042"), Some((Yes, Some(1042))));
    assert_eq!(read_business_word("sí 1042"), Some((Yes, Some(1042))));
    assert_eq!(read_business_word("Yes"), Some((Yes, None)));
    assert_eq!(
        read_business_word("CONFIRM 1042"),
        Some((Confirm, Some(1042)))
    );
    assert_eq!(
        read_business_word("confirmar 1042"),
        Some((Confirm, Some(1042)))
    );
    assert_eq!(read_business_word("1042 YES"), None);
    assert_eq!(read_business_word("YES please"), None);
    assert_eq!(read_business_word("YES 1042 now"), None);
    assert_eq!(read_business_word(""), None);
    assert_eq!(read_business_word("YES 1042."), Some((Yes, Some(1042))));
    assert_eq!(read_business_word("Confirm! "), Some((Confirm, None)));
    assert_eq!(read_business_word("."), None);
    assert_eq!(business_reply(true, true), ConfirmReply::FeeAndLink);
    assert_eq!(business_reply(true, false), ConfirmReply::Expired);
    assert_eq!(business_reply(false, true), ConfirmReply::Nothing);
    assert_eq!(business_reply(false, false), ConfirmReply::Nothing);
}

fn english(id: &str) -> String {
    let registry = Registry::embedded().expect("registry");
    let (text, _) = registry.template(id, Language::En).expect("English");
    assert!(
        registry.template(id, Language::Es).is_some(),
        "{id} Spanish"
    );
    text.to_owned()
}

/// TD-PICK-33 (PICK-037, PICK-033, D-359): the pick notice subjects and the fee reply equal their
/// spec text in English, each with a Spanish template; the expired reply says the quote can't be
/// picked again (PICK-035).
#[test]
fn td_pick_33_templates_equal_the_spec_text() {
    assert_eq!(
        english(PICK_NOTICE_TEMPLATE),
        "Ask Hava: A homeowner picked your quote for job {job}. Accept by {time}."
    );
    assert_eq!(
        english(PICK_NOTICE_EMERGENCY_TEMPLATE),
        "Ask Hava: EMERGENCY: a homeowner picked your quote for job {job}. Accept by {time}."
    );
    let fee = english(FEE_REPLY_TEMPLATE).replace("{number}", &en(7_500, None));
    assert_eq!(fee, "Ask Hava: Fee $75. Confirm and pay here: {link}.");
    let expired = english(EXPIRED_TEMPLATE);
    assert!(expired.contains("expired") && expired.contains("can't be picked again"));
}

/// TD-PICK-28 (PICK-019): the fee line the review step shows carries no confirm end time.
#[test]
fn td_pick_28_the_review_fee_line_has_no_end_time() {
    let basis = FeeBasis {
        monthly_cents: 12_500,
        basis_cents: 50_000,
        capped: false,
    };
    for line in [en(7_500, Some(basis)), en(2_500, None)] {
        assert!(!line.contains(':') || line.contains("4 months:"), "{line}");
        assert!(!line.contains("a.m.") && !line.contains("p.m.") && !line.contains("by"));
    }
}
