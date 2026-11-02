//! The pure parts of the charge, case by case (TD-FEE-11, TD-FEE-20, TD-FEE-21, TD-FEE-24,
//! TD-FEE-32 unit cases; FEE-016, FEE-018, FEE-020, FEE-037, FEE-038, FEE-047; D-153, D-270):
//! the attempt labels, what each Stripe answer means for an attempt and for a new press, and
//! the timer clocks. The database cases are `tests/integration/tests/charge_attempts.rs`.

use jiff::Timestamp;
use sqlx::types::Uuid;

use crate::api::{
    AttemptStatus, CARD_FIX_SECONDS, ChargeOutcome, EndReason, STRIPE_DOWN_SECONDS, attempt_label,
    intent_label,
};
use crate::handlers::{due_after, time_left};

/// Every stored attempt code; tests name statuses by code, since only `status.rs` may act on a
/// status variant (SEC-031).
const CODES: [&str; 6] = [
    "sent",
    "processing",
    "succeeded",
    "declined",
    "needs_action",
    "unreachable",
];

fn of(code: &str) -> AttemptStatus {
    AttemptStatus::from_code(code).expect("a stored code")
}

/// Arizona wall-clock time on fake days in November 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(askhava_core::api::arizona())
        .expect("zoned")
        .timestamp()
}

/// TD-FEE-11, TD-FEE-32 (FEE-016, FEE-047): the intent key is pick-<id>-intent and each attempt
/// is pick-<id>-try<n>, no personal data.
#[test]
fn td_fee_11_labels_are_per_pick_and_per_try() {
    let pick = Uuid::from_u128(0x1042);
    assert_eq!(intent_label(pick), format!("pick-{pick}-intent"));
    assert_eq!(attempt_label(pick, 1), format!("pick-{pick}-try1"));
    assert_eq!(attempt_label(pick, 2), format!("pick-{pick}-try2"));
}

/// Every stored code reads back, and no other text does.
#[test]
fn attempt_codes_round_trip() {
    for code in CODES {
        assert_eq!(of(code).code(), code);
    }
    assert!(AttemptStatus::from_code("paid").is_none());
}

/// TD-FEE-20 (FEE-037, D-153): Stripe's answers map to attempt statuses; an unknown one decides
/// nothing, so it never lets a second try go.
#[test]
fn td_fee_20_stripe_answers_and_new_tries() {
    let map = [
        ("succeeded", "succeeded"),
        ("processing", "processing"),
        ("requires_capture", "processing"),
        ("requires_action", "needs_action"),
        ("requires_payment_method", "declined"),
        ("requires_confirmation", "declined"),
        ("canceled", "declined"),
        ("something_new", "sent"),
    ];
    let read = |stripe: &str| AttemptStatus::from_intent(stripe).code();
    for (stripe, code) in map {
        assert_eq!(read(stripe), code, "{stripe}");
    }
    let blocks = |code: &str| of(code).blocks_new_try();
    assert_eq!(blocks("sent"), Some(ChargeOutcome::Pending));
    assert_eq!(blocks("processing"), Some(ChargeOutcome::Pending));
    assert_eq!(blocks("succeeded"), Some(ChargeOutcome::AlreadyPaid));
    for code in ["declined", "needs_action", "unreachable"] {
        assert_eq!(blocks(code), None, "{code}");
    }
    assert!(!of("sent").settles(of("processing")));
    assert!(of("succeeded").settles(of("needs_action")));
    assert!(!of("declined").settles(of("declined")));
}

/// FEE-018, FEE-020: which answers open the card-fix window, end the Stripe release, may still
/// change, and what the page shows for each.
#[test]
fn attempt_effects_and_outcomes() {
    let fix = Some(az(3, 11, 0));
    for code in CODES {
        let status = of(code);
        let card_fix = ["declined", "needs_action"].contains(&code);
        assert_eq!(status.needs_card_fix(), card_fix, "{code}");
        let quiet = ["sent", "unreachable"].contains(&code);
        assert_eq!(status.stripe_answered(), !quiet, "{code}");
        assert_eq!(status.paid(), code == "succeeded", "{code}");
        let undecided = ["sent", "processing"].contains(&code);
        assert_eq!(status.undecided(), undecided, "{code}");
        assert_eq!(status.needs_approval(), code == "needs_action", "{code}");
    }
    let outcome = |code: &str| of(code).outcome(fix, "pi_FakeT062");
    assert_eq!(outcome("sent"), ChargeOutcome::Pending);
    assert_eq!(outcome("processing"), ChargeOutcome::Pending);
    let paid = ChargeOutcome::Succeeded("pi_FakeT062".to_owned());
    assert_eq!(outcome("succeeded"), paid);
    assert_eq!(outcome("declined"), ChargeOutcome::Declined(fix));
    assert_eq!(outcome("needs_action"), ChargeOutcome::NeedsApproval(fix));
    assert_eq!(outcome("unreachable"), ChargeOutcome::StripeDown);
    assert_eq!(EndReason::StripeDown.code(), "stripe-down");
    assert_eq!(EndReason::CardFix.code(), "card-fix");
    assert_eq!(EndReason::FeeLimit.code(), "fee-limit");
}

/// TD-FEE-20 (FEE-016, FEE-037, D-153): an undecided or bank-approval attempt takes any answer;
/// a decline takes only a success (a success from Stripe always wins); a call that never left
/// takes any answer Stripe gives; a success never changes.
#[test]
fn td_fee_20_which_answers_may_replace_a_stored_status() {
    for from in CODES {
        for to in CODES {
            let open = ["sent", "processing", "needs_action"].contains(&from);
            let want = match from {
                "succeeded" => false,
                "declined" => to == "succeeded",
                "unreachable" => !["sent", "unreachable"].contains(&to),
                _ => open,
            };
            assert_eq!(of(from).may_become(of(to)), want, "{from} -> {to}");
        }
    }
}

/// TD-FEE-20 (FEE-019, FEE-037): an intent that still wants a card while a sent confirm may be
/// on its way leaves it sent; after that, or for any other stored status, it is a decline; a
/// refused confirm reads the intent again and an answer that decides nothing is a decline.
#[test]
fn td_fee_20_asking_while_a_confirm_is_in_flight_and_a_refused_confirm() {
    let asked = |stored: &str, stripe: &str, in_flight: bool| {
        AttemptStatus::asked(of(stored), stripe, in_flight).code()
    };
    assert_eq!(asked("sent", "requires_payment_method", true), "sent");
    assert_eq!(asked("sent", "requires_payment_method", false), "declined");
    assert_eq!(
        asked("processing", "requires_payment_method", true),
        "declined"
    );
    assert_eq!(asked("sent", "succeeded", true), "succeeded");
    let refused = |stripe: Option<&str>| AttemptStatus::after_refusal(stripe).code();
    assert_eq!(refused(Some("succeeded")), "succeeded");
    assert_eq!(refused(Some("something_new")), "declined");
    assert_eq!(refused(Some("requires_payment_method")), "declined");
    assert_eq!(refused(None), "declined");
}

/// TD-FEE-11, TD-FEE-24 (FEE-020, PICK-044, D-270): a decline at 7:30 p.m. has 30 minutes that
/// evening and 30 from 8:00 a.m.; on an emergency job at 22:00 it has 60 clock minutes; the
/// Stripe release is 4 clock hours.
#[test]
fn td_fee_24_card_fix_and_release_clocks() {
    assert_eq!(
        due_after(az(3, 19, 30), true, CARD_FIX_SECONDS),
        az(4, 8, 30)
    );
    assert_eq!(
        due_after(az(3, 22, 0), false, CARD_FIX_SECONDS),
        az(3, 23, 0)
    );
    assert_eq!(
        due_after(az(3, 22, 0), false, STRIPE_DOWN_SECONDS),
        az(4, 2, 0)
    );
}

/// TD-FEE-20 (FEE-038, D-135): the time left, counted in the timer's own clock, resumes to the
/// same deadline when nothing else passes; business time left skips the night.
#[test]
fn td_fee_20_time_left_skips_the_night_and_resumes() {
    let (paused_at, due) = (az(3, 19, 45), az(4, 8, 30));
    let left = time_left(paused_at, due, true);
    assert_eq!(left, 45 * 60);
    assert_eq!(due_after(paused_at, true, left), due);
    assert_eq!(due_after(az(5, 10, 0), true, left), az(5, 10, 45));
    assert_eq!(time_left(az(3, 22, 10), az(3, 23, 0), false), 50 * 60);
    assert_eq!(time_left(az(3, 23, 30), az(3, 23, 0), false), 0);
    assert_eq!(time_left(az(4, 9, 0), due, true), 0);
}
