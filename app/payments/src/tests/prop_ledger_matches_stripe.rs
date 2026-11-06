//! TD-FEE-16 (property layer; FEE-029, FEE-030, FEE-051, D-153): for random sequences of charges,
//! refunds and disputes, with or without a known pick, an earlier ledger entry or a closed job,
//! one reconcile run by `plan_charge` leaves the ledger's gross charges, refunds and disputes equal
//! to Stripe's to the cent; a closed-job or second charge is always refunded unless disputed or
//! already refunded, an open pick's own charge never is, a charge naming no pick never enters the
//! ledger, and a second run appends nothing. The database cases are
//! `tests/integration/tests/reconcile.rs`.

use proptest::prelude::*;
use sqlx::types::Uuid;

use crate::api::{
    DISPUTE_FEE_CENTS, EntryKind, MoneyTotals, MoneyView, PickFacts, RefundWhy, StripeCharge,
    plan_charge,
};

/// One Stripe charge and what Ask Hava knew before the run.
#[derive(Debug, Clone)]
struct Case {
    charge: StripeCharge,
    pick: Option<PickFacts>,
    has_charge: bool,
    has_refund: bool,
}

fn case() -> impl Strategy<Value = Case> {
    let amount = prop_oneof![Just(2500_i64), Just(7500), Just(19_900)];
    (amount, 0..4_u8, any::<[bool; 6]>(), 0..300_i64).prop_map(|(amount, n, b, fee)| {
        let refunded = if b[0] { amount } else { 0 };
        let pick = b[1].then_some(PickFacts {
            pick: Uuid::from_u128(u128::from(n) + 1),
            business: Uuid::from_u128(0xb067),
            job: Uuid::from_u128(u128::from(n) + 100),
            own_intent: b[2],
            ended: b[3],
        });
        Case {
            charge: StripeCharge {
                id: format!("ch_Fake{n}{amount}"),
                payment_intent: format!("pi_Fake{n}"),
                pick: pick.map(|p| p.pick),
                amount_cents: amount,
                refunded_cents: refunded,
                dispute: b[4].then(|| (format!("dp_Fake{n}"), amount)),
                balance_transaction: format!("txn_Fake{n}"),
                fee_cents: fee,
            },
            pick,
            has_charge: b[5],
            has_refund: b[5] && refunded > 0,
        }
    })
}

/// Runs the plan against a model ledger and a model Stripe; returns the ledger lines.
fn run(case: &mut Case) -> Vec<(EntryKind, i64)> {
    let c = &mut case.charge;
    let mut ledger = Vec::new();
    if case.has_charge {
        ledger.push((EntryKind::Charge, c.amount_cents));
    }
    if case.has_refund {
        ledger.push((EntryKind::Refund, c.refunded_cents));
    }
    if let Some((_, amount)) = &c.dispute {
        ledger.push((EntryKind::Dispute, amount + DISPUTE_FEE_CENTS));
    }
    let plan = plan_charge(c, case.pick, (case.has_charge, case.has_refund));
    ledger.extend(plan.lines.iter().copied());
    if plan.refund.is_some() {
        c.refunded_cents = c.amount_cents;
        ledger.push((EntryKind::Refund, c.amount_cents));
    }
    let again = plan_charge(c, case.pick, (true, c.refunded_cents > 0));
    let fee = usize::from(c.fee_cents > 0 && case.pick.is_some());
    assert_eq!(
        again.lines.len(),
        fee,
        "a second run appends only the keyed fee"
    );
    assert_eq!(again.refund, None, "a second run refunds nothing");
    ledger
}

proptest! {
    /// TD-FEE-16 (FEE-029, FEE-030, FEE-051): the ledger matches Stripe to the cent.
    #[test]
    fn td_fee_16_the_ledger_matches_stripe(cases in prop::collection::vec(case(), 1..12)) {
        let mut ledger = Vec::new();
        let mut known = Vec::new();
        for mut case in cases {
            let before = case.charge.clone();
            let plan = plan_charge(&before, case.pick, (case.has_charge, case.has_refund));
            let untouched = before.refunded_cents == 0 && before.dispute.is_none();
            let expected = match case.pick {
                None => untouched.then_some(RefundWhy::NoPick),
                Some(p) if !p.own_intent => untouched.then_some(RefundWhy::SecondCharge),
                Some(p) => (untouched && p.ended).then_some(RefundWhy::ClosedJob),
            };
            prop_assert_eq!(plan.refund, expected);
            let lines = run(&mut case);
            if case.pick.is_some() {
                ledger.extend(lines);
                known.push(case.charge);
            } else {
                prop_assert!(plan.lines.is_empty(), "a charge naming no pick enters no ledger");
            }
        }
        let view = MoneyView::from_entries(ledger.iter().copied());
        prop_assert_eq!(view.gross(), MoneyTotals::of_stripe(known.iter()));
        let fees: i64 = known.iter().map(|c| c.fee_cents).sum();
        prop_assert_eq!(view.processing_fees, fees);
    }
}

/// FEE-030, FEE-051: the money view splits the dispute fee out, counts countering fees and their
/// return, adds nothing for the $0 kinds and nets every Stripe fee.
#[test]
fn the_money_view_shows_each_fee_as_its_own_line() {
    let view = MoneyView::from_entries([
        (EntryKind::Charge, 7500),
        (EntryKind::Charge, 2500),
        (EntryKind::Refund, 2500),
        (EntryKind::Credit, 100),
        (EntryKind::Dispute, 7500 + DISPUTE_FEE_CENTS),
        (EntryKind::CounteringFee, DISPUTE_FEE_CENTS),
        (EntryKind::CounteringFeeReturn, DISPUTE_FEE_CENTS),
        (EntryKind::ProcessingFee, 248),
        (EntryKind::FreeFirstWin, 0),
        (EntryKind::RepeatCustomer, 0),
        (EntryKind::FeeFreeJobType, 0),
        (EntryKind::Payout, 4000),
    ]);
    assert_eq!(view.charges, 10_000);
    assert_eq!(view.disputes, 7500);
    assert_eq!(view.dispute_fees, DISPUTE_FEE_CENTS);
    assert_eq!(view.processing_fees, 248);
    assert_eq!(view.payouts, 4000);
    assert_eq!(
        view.net(),
        10_000 - 2500 - 100 - 7500 - DISPUTE_FEE_CENTS - 248
    );
}

/// FEE-029: a malformed charge is refused before any plan; each refund reason has its code.
#[test]
fn a_malformed_charge_is_not_well_formed() {
    let good = StripeCharge {
        id: "ch_Fake1".to_owned(),
        payment_intent: "pi_Fake1".to_owned(),
        pick: None,
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: Some(("dp_Fake1".to_owned(), 7500)),
        balance_transaction: "txn_Fake1".to_owned(),
        fee_cents: 248,
    };
    assert!(good.is_well_formed());
    let bad = [
        StripeCharge {
            id: "re_Fake1".to_owned(),
            ..good.clone()
        },
        StripeCharge {
            payment_intent: "pi_".to_owned(),
            ..good.clone()
        },
        StripeCharge {
            balance_transaction: "txn_a-b".to_owned(),
            ..good.clone()
        },
        StripeCharge {
            dispute: Some(("dp_Fake1".to_owned(), 0)),
            ..good.clone()
        },
        StripeCharge {
            amount_cents: 0,
            refunded_cents: 0,
            ..good.clone()
        },
        StripeCharge {
            refunded_cents: 7501,
            ..good.clone()
        },
        StripeCharge {
            fee_cents: -1,
            ..good.clone()
        },
    ];
    assert!(bad.iter().all(|c| !c.is_well_formed()));
    let codes = [
        RefundWhy::NoPick,
        RefundWhy::ClosedJob,
        RefundWhy::SecondCharge,
    ]
    .map(RefundWhy::code);
    assert_eq!(codes, ["no-pick", "closed-job", "second-charge"]);
}
