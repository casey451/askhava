//! T387 (D-3049): deterministic examples for `askhava-payments` branches that a randomized
//! property test reaches only sometimes, or no test reaches, so the crate's branch coverage is the
//! same on every run (BUILD_BRIEF.md:481). The property suites stay randomized; these pin one fixed
//! input each. Every ID here is fake.

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

/// FEE-029, FEE-030: a charge whose `ch_…` has nothing after its prefix is malformed, so the
/// reconcile never matches it.
#[test]
fn a_charge_id_with_no_tail_is_not_well_formed() {
    let (good, _) = charge_on_pick(0);
    assert!(good.is_well_formed());
    let bare = StripeCharge {
        id: "ch_".to_owned(),
        ..good
    };
    assert!(!bare.is_well_formed());
}

/// FEE-026, FEE-028 (D-2303): a refund Stripe reports for another PaymentIntent than the one asked
/// about is refused as `Decode`, never booked; the asked one is read.
#[test]
fn a_refund_for_another_intent_is_refused() {
    let raw = StripeRefund {
        id: "re_FakeT387".to_owned(),
        status: "succeeded".to_owned(),
        amount: Some(7500),
        currency: Some("usd".to_owned()),
        payment_intent: Some("pi_FakeT387other".to_owned()),
    };
    let refused = Refund::checked(raw.clone(), Some("pi_FakeT387"));
    assert_eq!(refused.err(), Some(AdapterError::Decode));
    let read = Refund::checked(raw, Some("pi_FakeT387other")).expect("refund");
    assert_eq!(read.payment_intent, "pi_FakeT387other");
}

/// FEE-009, FEE-036: a Stripe customer ID longer than the table allows (200 characters after
/// `cus_`) is refused; 200 is kept.
#[test]
fn a_customer_id_over_the_table_limit_is_refused() {
    let longest = format!("cus_{}", "A".repeat(200));
    assert!(CustomerId::parse(&longest).is_ok());
    let over = format!("cus_{}", "A".repeat(201));
    assert!(matches!(CustomerId::parse(&over), Err(CardError::BadField)));
}
