//! T387 (D-3049, D-3059): deterministic examples for `askhava-payments` branches that a randomized
//! property test reaches only sometimes, or no test reaches, so the crate's branch coverage is the
//! same on every run (BUILD_BRIEF.md:481). The property suites stay randomized; these pin fixed
//! inputs. llvm-cov counts a function's branches from its single best-covered instantiation, and
//! this unit-test binary compiles its own copy of the crate, so each test here drives every arm of
//! its function itself rather than only the arm other binaries miss. Every ID here is fake.

use sqlx::types::Uuid;

use askhava_adapters::api::AdapterError;

use crate::api::{
    CardError, CustomerId, EntryKind, PickFacts, Refund, RefundWhy, StripeCharge, StripeRefund,
    plan_charge,
};

/// A $75 charge on a known pick, with Stripe's processing fee `fee_cents`.
fn charge_on_pick(fee_cents: i64) -> (StripeCharge, PickFacts) {
    let pick = PickFacts {
        pick: Uuid::from_u128(0x387_0001),
        business: Uuid::from_u128(0x387_0002),
        job: Uuid::from_u128(0x387_0003),
        own_intent: true,
        ended: false,
    };
    let charge = StripeCharge {
        id: "ch_FakeT387".to_owned(),
        payment_intent: "pi_FakeT387".to_owned(),
        pick: Some(pick.pick),
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: "txn_FakeT387".to_owned(),
        fee_cents,
    };
    (charge, pick)
}

/// FEE-029, FEE-051, FEE-028 (TD-FEE-33): a charge on a known pick whose balance transaction
/// carries no Stripe fee gets its `charge` entry and no `processing-fee` line (a $0 ledger line of
/// a charged kind is never written). This is the `fee_cents > 0` false arm of `plan_charge`, which
/// `prop_ledger_matches_stripe` draws only about once in 600 cases (the flap of D-3049).
#[test]
fn a_known_pick_charge_with_no_stripe_fee_appends_no_fee_line() {
    let (charge, pick) = charge_on_pick(0);
    let plan = plan_charge(&charge, Some(pick), (false, false));
    assert_eq!(plan.lines, vec![(EntryKind::Charge, 7500)]);
    assert_eq!(plan.refund, None);
    assert!(plan.unknown);
    let (charge, pick) = charge_on_pick(248);
    let plan = plan_charge(&charge, Some(pick), (true, false));
    assert_eq!(plan.lines, vec![(EntryKind::ProcessingFee, 248)]);
    assert!(!plan.unknown);
    let ended = PickFacts {
        ended: true,
        ..pick
    };
    let plan = plan_charge(&charge, Some(ended), (true, false));
    assert_eq!(plan.refund, Some(RefundWhy::ClosedJob));
}

/// FEE-029, FEE-030, FEE-027: a charge whose dispute ID is not a `dp_…` is malformed, so the
/// reconcile never matches it; a `dp_…` with a positive amount is kept. Both arms of the dispute
/// check run in this one binary.
#[test]
fn a_charge_with_a_malformed_dispute_id_is_not_well_formed() {
    let (good, _) = charge_on_pick(0);
    let disputed = StripeCharge {
        dispute: Some(("dp_FakeT387".to_owned(), 7500)),
        ..good.clone()
    };
    assert!(disputed.is_well_formed());
    let malformed = StripeCharge {
        dispute: Some(("dx_FakeT387".to_owned(), 7500)),
        ..good
    };
    assert!(!malformed.is_well_formed());
}

/// Stripe's refund object `id` of `intent`, `succeeded` for the whole $75 in `usd`.
fn stripe_refund(id: &str, intent: Option<&str>) -> StripeRefund {
    StripeRefund {
        id: id.to_owned(),
        status: "succeeded".to_owned(),
        amount: Some(7500),
        currency: Some("usd".to_owned()),
        payment_intent: intent.map(str::to_owned),
    }
}

/// FEE-026, FEE-028 (D-1753, D-2303): `Refund::checked` refuses an ID that is not a `re_…` or is
/// only the prefix (`InvalidId`), an unknown status or a missing PaymentIntent (`Decode`), and a
/// refund for another PaymentIntent than the one asked about (`Decode`), so none is ever booked;
/// the asked one, or one read with nothing asked, is kept. Every arm runs in this one binary.
#[test]
fn checked_refuses_every_malformed_or_foreign_refund() {
    let asked = Some("pi_FakeT387");
    for id in ["ch_FakeT387", "re_"] {
        let refused = Refund::checked(stripe_refund(id, Some("pi_FakeT387")), asked);
        assert_eq!(refused.err(), Some(AdapterError::InvalidId), "{id}");
    }
    let unknown = StripeRefund {
        status: "reversed".to_owned(),
        ..stripe_refund("re_FakeT387", Some("pi_FakeT387"))
    };
    assert_eq!(
        Refund::checked(unknown, asked).err(),
        Some(AdapterError::Decode)
    );
    let no_intent = stripe_refund("re_FakeT387", None);
    assert_eq!(
        Refund::checked(no_intent, asked).err(),
        Some(AdapterError::Decode)
    );
    let other = stripe_refund("re_FakeT387", Some("pi_FakeT387other"));
    assert_eq!(
        Refund::checked(other.clone(), asked).err(),
        Some(AdapterError::Decode)
    );
    let read = Refund::checked(other, None).expect("refund");
    assert_eq!(read.payment_intent, "pi_FakeT387other");
    let own = Refund::checked(stripe_refund("re_FakeT387", Some("pi_FakeT387")), asked);
    assert_eq!(own.expect("refund").amount_cents, Some(7500));
}

/// FEE-014, FEE-036 (D-1409): a Stripe customer ID's tail may hold letters, digits and `_` (the
/// table's `'^cus_[A-Za-z0-9_]{1,200}$'`); any other character, or no tail, is `BadField`.
#[test]
fn a_customer_id_takes_letters_digits_and_underscores_only() {
    assert!(CustomerId::parse("cus_Fake_T387").is_ok());
    for bad in ["cus_Fake-T387", "cus_", "pm_FakeT387"] {
        assert!(
            matches!(CustomerId::parse(bad), Err(CardError::BadField)),
            "{bad}"
        );
    }
}
