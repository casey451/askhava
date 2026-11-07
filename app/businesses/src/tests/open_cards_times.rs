//! Each person's open-cards email times (CARD-022, D-491, D-492; T085), on a fresh database with
//! fake IDs only: 5 p.m. by default, at most two times, off, per person, own business only.

use jiff::Timestamp;
use sqlx::types::Uuid;

use crate::api::{
    Acting, BusinessesError, OpenCardsTime, OpenCardsTimes, Roles, add_member, add_owner, create,
    open_cards_times, set_open_cards_times,
};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

#[test]
fn card_022_at_most_two_times_and_a_repeat_counts_once() {
    use OpenCardsTime::{EightAm, FivePm, Noon};
    assert_eq!(OpenCardsTimes::new(&[EightAm, Noon, FivePm]), None);
    let two = OpenCardsTimes::new(&[FivePm, EightAm, FivePm]).expect("two");
    assert_eq!(two.times(), vec![EightAm, FivePm]);
    assert_eq!(OpenCardsTimes::DEFAULT.times(), vec![FivePm]);
    assert!(OpenCardsTimes::new(&[]).expect("off").times().is_empty());
    assert_eq!(OpenCardsTime::parse("noon"), Some(Noon));
    assert_eq!(OpenCardsTime::parse("9am"), None);
    assert_eq!(EightAm.minute(), 480);
}

/// CARD-022, D-492: each person keeps their own times; another business's member is refused.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn card_022_times_are_stored_per_person(mut db: Db) {
    let business = create(&mut db, false, at(0)).await.expect("business");
    let owner = add_owner(&mut db, business, Uuid::from_u128(0xc1), at(0))
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let roles = Roles::new(true, false).expect("roles");
    let member = add_member(&mut db, owner, Uuid::from_u128(0xc2), roles, at(0))
        .await
        .expect("member");
    let first = open_cards_times(&mut db, member).await.expect("read");
    assert_eq!(first, OpenCardsTimes::DEFAULT, "5 p.m. by default");
    let morning = OpenCardsTimes::new(&[OpenCardsTime::EightAm, OpenCardsTime::Noon]).expect("two");
    set_open_cards_times(&mut db, owner, morning, at(1))
        .await
        .expect("owner sets");
    let off = OpenCardsTimes::new(&[]).expect("off");
    let me = Acting { business, member };
    set_open_cards_times(&mut db, me, off, at(2))
        .await
        .expect("off");
    set_open_cards_times(&mut db, owner, morning, at(3))
        .await
        .expect("a repeat replaces");
    assert_eq!(
        open_cards_times(&mut db, owner.member).await.expect("r"),
        morning
    );
    assert_eq!(open_cards_times(&mut db, member).await.expect("r"), off);
    let other = create(&mut db, false, at(0)).await.expect("other");
    let stranger = Acting {
        business: other,
        member,
    };
    let refused = set_open_cards_times(&mut db, stranger, morning, at(4)).await;
    assert_eq!(refused, Err(BusinessesError::UnknownBusiness));
    assert_eq!(open_cards_times(&mut db, member).await.expect("r"), off);
}
