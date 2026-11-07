//! TD-PICK-48 pure cases (PICK-022, PICK-024, PRIV-005, BIZ-037, BIZ-043; D-1739): the released
//! view's refusal rule over every pick status, every seat in the business and both business
//! pairings, released or not; and the same rule on the database, through `released_view`'s
//! crate-private `viewable` (the vault is opened only after it, and `read_released` is tested in
//! `privacy`'s `realm_tests`). Fake IDs only.

use askhava_businesses::api::{
    Acting, Roles, add_member, add_owner, create, remove_member, set_roles,
};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::api::rules::{PickStatus, shows_release};
use crate::api::{Seat, ViewFacts, may_view};
use crate::released_view::viewable;

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

/// A business with its owner, a "Confirm and billing" member and a "Cards and quotes"-only member:
/// `(business, [owner, billing, cards])`.
async fn team(db: &mut PgConnection) -> (Uuid, [Uuid; 3]) {
    let now = Timestamp::now();
    let business = create(db, false, now).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(1), now)
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let mut add = async |mailbox, (cards, billing)| {
        let roles = Roles::new(cards, billing).expect("roles");
        add_member(db, by, Uuid::from_u128(mailbox), roles, now)
            .await
            .expect("member")
    };
    let billing = add(2, (false, true)).await;
    let cards = add(3, (true, false)).await;
    (business, [owner, billing, cards])
}

/// A pick of `business` at `status`; with `bound`, its release row bound to that contact and
/// email choice, as `release`'s claim writes it.
async fn pick(
    db: &mut PgConnection,
    (business, status): (Uuid, PickStatus),
    bound: Option<(Uuid, bool)>,
) -> Uuid {
    let job = askhava_jobs::api::create(db, Timestamp::now(), false, Actor::System)
        .await
        .expect("job");
    let id = sqlx::query_scalar!(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) \
         VALUES ($1, $2, 1, 'page', $3, now()) RETURNING id",
        job,
        business,
        status.code(),
    )
    .fetch_one(&mut *db)
    .await
    .expect("pick");
    if let Some((contact, email_chosen)) = bound {
        sqlx::query!(
            "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
             contact_id, mail_chosen) VALUES ($1, $2, $3, 'paid', now(), $4, $5)",
            id,
            job,
            business,
            contact,
            email_chosen,
        )
        .execute(&mut *db)
        .await
        .expect("release");
    }
    id
}

/// TD-PICK-48 on the database: the owner and a "Confirm and billing" member get the binding of a
/// released paid or free pick (the contact the release row names and its email choice); a
/// "Cards and quotes"-only member, another business's person, a pick with no release row, a pick
/// whose status left paid or free and an unknown pick get nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_48_only_the_owner_or_billing_member_gets_the_bound_contact(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (business, [owner, billing, cards]) = team(&mut db).await;
    let (other, [stranger, ..]) = team(&mut db).await;
    let (her, decoy) = (Uuid::from_u128(0x0253_0a01), Uuid::from_u128(0x0253_0a02));
    let paid = pick(&mut db, (business, PickStatus::Paid), Some((her, false))).await;
    let free = pick(&mut db, (business, PickStatus::Free), Some((her, true))).await;
    pick(&mut db, (other, PickStatus::Paid), Some((decoy, true))).await;
    for (who, id, email) in [
        (owner, paid, false),
        (billing, paid, false),
        (owner, free, true),
    ] {
        let bound = viewable(&mut db, (who, business), id).await.expect("read");
        let bound = bound.expect("shown");
        assert_eq!(
            (bound.contact, bound.email_chosen),
            (her, email),
            "the bound contact"
        );
    }
    let unreleased = pick(&mut db, (business, PickStatus::Paid), None).await;
    let no_show = pick(&mut db, (business, PickStatus::NoShow), Some((her, true))).await;
    let waiting = pick(
        &mut db,
        (business, PickStatus::AwaitingConfirm),
        Some((her, true)),
    )
    .await;
    let refused = [
        ((cards, business), paid),
        ((stranger, other), paid),
        ((stranger, business), paid),
        ((owner, other), paid),
        ((owner, business), unreleased),
        ((owner, business), no_show),
        ((owner, business), waiting),
        ((owner, business), Uuid::from_u128(0x0253_0aff)),
    ];
    for (who, id) in refused {
        let bound = viewable(&mut db, who, id).await.expect("read");
        assert!(bound.is_none(), "{who:?} on {id}");
    }
}

/// TD-PICK-48 (BIZ-037, BIZ-043): a member removed or demoted after the release is refused at the
/// next view; the role is read at each view, never kept.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_48_a_member_removed_or_demoted_after_the_release_is_refused(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (business, [owner, billing, _]) = team(&mut db).await;
    let by = Acting {
        business,
        member: owner,
    };
    let roles = Roles::new(true, true).expect("roles");
    let leaver = add_member(&mut db, by, Uuid::from_u128(4), roles, Timestamp::now())
        .await
        .expect("member");
    let id = pick(
        &mut db,
        (business, PickStatus::Paid),
        Some((Uuid::from_u128(0x0253_0a03), false)),
    )
    .await;
    for who in [billing, leaver] {
        assert!(
            viewable(&mut db, (who, business), id)
                .await
                .expect("read")
                .is_some()
        );
    }
    let cards_only = Roles::new(true, false).expect("roles");
    set_roles(&mut db, by, billing, cards_only)
        .await
        .expect("demote");
    remove_member(&mut db, by, leaver, Timestamp::now())
        .await
        .expect("remove");
    for who in [billing, leaver] {
        let bound = viewable(&mut db, (who, business), id).await.expect("read");
        assert!(bound.is_none(), "{who}");
    }
}
