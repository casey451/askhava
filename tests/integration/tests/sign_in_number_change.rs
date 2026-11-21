//! SEC-004, CON-019 (D-4039, D-5081 finding F39, D-5117, T777): a sign-in code may be texted
//! only to a line whose current number has its text box ticked.
//!
//! Real PostgreSQL through `businesses::api`: `sign_in_lines` lists a member's ticked extra line;
//! after the line's number changes it does not, so the code goes to the sign-in address, and it
//! lists the line again once the new number's box is ticked; a ticked line a non-owner linked and
//! the owner has not approved is listed; a tick by a removed member does not count. Fake numbers
//! only (555-01xx) and fake IDs.

use askhava_businesses::api::{
    Acting, ConsentChannel, Roles, SenderLink, TeamError, add_member, add_owner, change_phone_at,
    create, link_phone, phone_version, record_consent, record_consent_for, remove_member,
    sign_in_lines,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use ConsentChannel::Text;

/// A sender directory that accepts every number: these tests read no sender row.
struct AnyNumber;

impl SenderLink for AnyNumber {
    async fn link(
        &self,
        _: &mut PgConnection,
        _: (Uuid, Uuid),
        _: &SecretString,
    ) -> Result<(), TeamError> {
        Ok(())
    }
}

/// Noon in Arizona (19:00 UTC) on a fixed day, plus `minute` minutes.
fn at(minute: i64) -> Timestamp {
    let noon: Timestamp = "2026-11-10T19:00:00Z".parse().expect("time");
    noon + jiff::SignedDuration::from_mins(minute)
}

/// A business with its owner (mailbox `seed`) and a member with both roles (`seed + 1`), who
/// may link and change an extra phone (CALL-012).
async fn team(db: &mut PgConnection, seed: u128) -> (Acting, Acting) {
    let business = create(db, false, at(0)).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(seed), at(0)).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let cards = Roles::new(true, true).expect("roles");
    let member = add_member(db, owner, Uuid::from_u128(seed + 1), cards, at(0)).await;
    let cards = Acting {
        business,
        member: member.expect("member"),
    };
    (owner, cards)
}

/// An extra (not main) line `by` links at a fake `number`, its text box ticked by `ticker`.
async fn ticked_line(db: &mut PgConnection, by: Acting, ticker: Acting, number: &str) -> Uuid {
    let number = SecretString::from(number.to_owned());
    let line = link_phone(db, &AnyNumber, (by, false), &number, at(1)).await;
    let line = line.expect("link");
    let ids = (by.business, ticker.member, Some(line));
    record_consent(db, ids, &[Text], true, at(1))
        .await
        .expect("consent");
    line
}

/// The lines `sign_in_lines` lists for current member `who`.
async fn listed(db: &mut PgConnection, who: Acting) -> Vec<Uuid> {
    let lines = sign_in_lines(db, who.member).await.expect("read");
    lines.expect("a current member").texting_lines
}

/// SEC-004, CON-019 (D-5081 F39): a ticked line is listed; after its number changes it is not,
/// so the code goes to the sign-in address; it is listed again once the new number is ticked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_changed_number_gets_no_code_until_its_box_is_ticked(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (_, cards) = team(&mut db, 0x7770).await;
    let line = ticked_line(&mut db, cards, cards, "928-555-0161").await;
    assert_eq!(listed(&mut db, cards).await, [line], "a ticked number");
    let new = SecretString::from("928-555-0162".to_owned());
    let changed = change_phone_at(&mut db, &AnyNumber, (cards, line), &new, at(2)).await;
    changed.expect("change");
    assert_eq!(phone_version(&mut db, cards.business, line).await, Ok(2));
    let after = sign_in_lines(&mut db, cards.member).await.expect("read");
    let after = after.expect("still a member");
    assert!(after.texting_lines.is_empty(), "nobody ticked the new one");
    assert_eq!(after.mailbox, Uuid::from_u128(0x7771), "the code's address");
    let ids = (cards.business, cards.member);
    let again = record_consent_for(&mut db, ids, (line, 2), &[Text], true, at(3)).await;
    assert_eq!(again, Ok(()));
    assert_eq!(
        listed(&mut db, cards).await,
        [line],
        "the new number ticked"
    );
}

/// SEC-004 (D-3562, D-5117): a ticked line a non-owner linked is listed while the owner has not
/// approved it; the owner's approval is no condition for that member's own sign-in code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_ticked_line_the_owner_has_not_approved_is_listed(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (owner, cards) = team(&mut db, 0x7780).await;
    let line = ticked_line(&mut db, cards, cards, "928-555-0163").await;
    let waiting: bool =
        sqlx::query_scalar("SELECT approved_at IS NULL FROM businesses.staff_phone WHERE id = $1")
            .bind(line)
            .fetch_one(&mut *db)
            .await
            .expect("approval");
    assert!(waiting, "the owner has not approved it");
    assert_eq!(listed(&mut db, cards).await, [line]);
    assert!(listed(&mut db, owner).await.is_empty(), "never another's");
}

/// CON-019, SEC-004: a tick by a member since removed does not count, for the same number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_tick_by_a_removed_member_does_not_count(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (owner, cards) = team(&mut db, 0x7790).await;
    let line = ticked_line(&mut db, owner, cards, "928-555-0164").await;
    assert_eq!(listed(&mut db, owner).await, [line], "ticked by a member");
    let removed = remove_member(&mut db, owner, cards.member, at(2)).await;
    removed.expect("removed");
    assert!(listed(&mut db, owner).await.is_empty(), "the tick is void");
    assert_eq!(sign_in_lines(&mut db, cards.member).await, Ok(None));
}
