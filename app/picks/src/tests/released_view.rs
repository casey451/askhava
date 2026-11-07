//! TD-PICK-48 pure cases (PICK-022, PICK-024, PRIV-005, BIZ-037, BIZ-043; D-1739): the released
//! view's refusal rule over every pick status, every seat in the business and both business
//! pairings, released or not. The database cases are `app/server/tests/released_view.rs`.

use crate::api::rules::{PickStatus, shows_release};
use crate::api::{Seat, ViewFacts, may_view};

/// Every seat a signed-in person can hold in the pick's business.
const SEATS: [Seat; 4] = [
    Seat::Absent,
    Seat::Owner,
    Seat::Member {
        confirm_and_billing: true,
    },
    Seat::Member {
        confirm_and_billing: false,
    },
];

/// TD-PICK-48: only a released, paid or free pick of the person's own business, seen by its owner
/// or a "Confirm and billing" member, is shown; every other pairing is the one refusal.
#[test]
fn td_pick_48_only_the_owner_or_billing_member_of_a_released_paid_or_free_pick_sees_it() {
    let pairings = [(true, true), (true, false), (false, true), (false, false)];
    for status in PickStatus::ALL {
        for (seat, (same_business, released)) in SEATS
            .into_iter()
            .flat_map(|seat| pairings.map(|pair| (seat, pair)))
        {
            let facts = ViewFacts {
                same_business,
                released,
                shown_status: shows_release(status),
                seat,
            };
            let paid_or_free = ["paid", "free"].contains(&status.code());
            let seated = matches!(
                seat,
                Seat::Owner
                    | Seat::Member {
                        confirm_and_billing: true
                    }
            );
            let want = paid_or_free && seated && same_business && released;
            assert_eq!(may_view(facts), want, "{facts:?} at {status:?}");
        }
    }
}

/// TD-PICK-48: a "Cards and quotes"-only member, a removed member and another business's person
/// are refused on a paid pick (BIZ-037, BIZ-043).
#[test]
fn td_pick_48_cards_only_removed_and_other_business_are_refused() {
    let paid = |seat, same_business| ViewFacts {
        same_business,
        released: true,
        shown_status: shows_release(PickStatus::Paid),
        seat,
    };
    let cards_only = Seat::Member {
        confirm_and_billing: false,
    };
    assert!(!may_view(paid(cards_only, true)));
    assert!(!may_view(paid(Seat::Absent, true)));
    assert!(!may_view(paid(Seat::Owner, false)));
    assert!(may_view(paid(Seat::Owner, true)));
}

/// PRIV-005, D-1739: a pick shows only while paid or free; a no-show or fraud refund shows nothing.
#[test]
fn td_pick_48_only_paid_and_free_show() {
    let shown: Vec<&str> = PickStatus::ALL
        .into_iter()
        .filter(|s| shows_release(*s))
        .map(PickStatus::code)
        .collect();
    assert_eq!(shown, ["paid", "free"]);
}
