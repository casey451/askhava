//! Email action links: single use, bound to one person and one object, expiring, voided with
//! the member, opened without acting, and a cross-site POST refused (TD-SEC-09 action-link cases,
//! TD-BIZ-15 Hold and binding cases; SEC-025, SEC-026, BIZ-038, PICK-043, A11Y-023).
//!
//! Every ID, address and time is fake.

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create, remove_member};
use askhava_core::api::Claimed;
use askhava_picks::api::HoldAsk;
use askhava_portals::api::{
    ActionKind, ActionLinkError, ActionTarget, OPEN_IN_BROWSER_LINE, UseBy, is_cross_site,
    mint_action_link, open_action_link, run_member_removed, use_action_link,
};
use axum::http::{HeaderMap, HeaderValue};
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("time")
}

fn later(minutes: i64) -> Timestamp {
    start()
        .checked_add(SignedDuration::from_mins(minutes))
        .expect("time")
}

/// One business with its owner, a "Confirm and billing" member and a "Cards and quotes" member.
struct Team {
    business: Uuid,
    owner: Uuid,
    billing: Uuid,
    cards: Uuid,
}

async fn team(db: &mut PgConnection) -> Team {
    let business = create(db, false, start()).await.expect("business");
    let owner = add_owner(db, business, Uuid::from_u128(0xb1), start())
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let billing_roles = Roles::new(false, true).expect("roles");
    let billing = add_member(db, by, Uuid::from_u128(0xb2), billing_roles, start())
        .await
        .expect("billing");
    let cards_roles = Roles::new(true, false).expect("roles");
    let cards = add_member(db, by, Uuid::from_u128(0xb3), cards_roles, start())
        .await
        .expect("cards");
    Team {
        business,
        owner,
        billing,
        cards,
    }
}

fn hold_ask(emergency: bool) -> HoldAsk {
    HoldAsk {
        by: Roles::BOTH,
        emergency,
        held: false,
        away_delayed: false,
    }
}

/// The `(kind, object, token)` of a minted link, read from its path.
fn parts(link: &str) -> (ActionKind, Uuid, String) {
    let rest = link
        .strip_prefix("https://askhava.com/b/a/")
        .expect("askhava.com link");
    let mut it = rest.split('/');
    let kind = ActionKind::from_slug(it.next().expect("kind")).expect("known kind");
    let object = it.next().expect("object").parse().expect("uuid");
    let token = it.next().expect("token").to_owned();
    assert!(it.next().is_none());
    (kind, object, token)
}

async fn hold_link(db: &mut PgConnection, t: &Team, pick: Uuid, member: Uuid) -> String {
    let target = ActionTarget {
        kind: ActionKind::Hold,
        object: pick,
        member,
        business: t.business,
    };
    let link = mint_action_link(db, target, Some(hold_ask(false)), (start(), later(120)))
        .await
        .expect("minted");
    link.expose_secret().to_owned()
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_09_a_hold_link_works_once_for_its_pick_and_until_the_window_ends(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let (pick, other_pick) = (Uuid::from_u128(0x1042), Uuid::from_u128(0x1043));
    let link = hold_link(&mut db, &t, pick, t.billing).await;
    let (kind, object, token) = parts(&link);
    assert_eq!((kind, object), (ActionKind::Hold, pick));
    assert_eq!(token.len(), 64, "256 random bits as hex (SEC-026)");
    for _ in 0..3 {
        open_action_link(&mut db, (kind, pick), &token, start())
            .await
            .expect("opening only shows the page and never uses the link");
    }
    let refused = use_action_link(&mut db, (kind, other_pick), &token, later(1)).await;
    assert_eq!(refused, Err(ActionLinkError::Ended), "another pick");
    let refused = use_action_link(&mut db, (ActionKind::Confirm, pick), &token, later(1)).await;
    assert_eq!(refused, Err(ActionLinkError::Ended), "another action");
    let grant = use_action_link(&mut db, (kind, pick), &token, later(1))
        .await
        .expect("first press acts");
    assert_eq!((grant.member, grant.business), (t.billing, t.business));
    let again = use_action_link(&mut db, (kind, pick), &token, later(2)).await;
    assert_eq!(again, Err(ActionLinkError::Ended), "used once");
    let late = hold_link(&mut db, &t, pick, t.owner).await;
    let (_, _, late_token) = parts(&late);
    let ended = use_action_link(&mut db, (kind, pick), &late_token, later(120)).await;
    assert_eq!(ended, Err(ActionLinkError::Ended), "the window ended");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_15_no_hold_link_on_an_emergency_job_or_for_cards_and_quotes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let target = |kind, member| ActionTarget {
        kind,
        object: Uuid::from_u128(0x1042),
        member,
        business: t.business,
    };
    let window = (start(), later(60));
    let emergency = mint_action_link(
        &mut db,
        target(ActionKind::Hold, t.owner),
        Some(hold_ask(true)),
        window,
    )
    .await;
    assert_eq!(emergency.err(), Some(ActionLinkError::NoHold));
    for kind in [ActionKind::Hold, ActionKind::Confirm, ActionKind::Pause] {
        let got = mint_action_link(
            &mut db,
            target(kind, t.cards),
            Some(hold_ask(false)),
            window,
        );
        assert_eq!(
            got.await.err(),
            Some(ActionLinkError::NotAllowed),
            "{kind:?}"
        );
    }
    let approve = mint_action_link(
        &mut db,
        target(ActionKind::Approve, t.billing),
        None,
        window,
    );
    assert_eq!(approve.await.err(), Some(ActionLinkError::NotAllowed));
    let quote = mint_action_link(&mut db, target(ActionKind::Quote, t.cards), None, window);
    assert!(
        quote.await.is_ok(),
        "a Cards and quotes member gets the Quote link"
    );
    let past = mint_action_link(
        &mut db,
        target(ActionKind::Quote, t.cards),
        None,
        (start(), start()),
    );
    assert_eq!(past.await.err(), Some(ActionLinkError::Ended));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_09_removing_a_member_voids_their_action_links(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let t = team(&mut db).await;
    let pick = Uuid::from_u128(0x1042);
    let (kind, _, token) = parts(&hold_link(&mut db, &t, pick, t.billing).await);
    let (_, _, kept) = parts(&hold_link(&mut db, &t, pick, t.owner).await);
    let by = Acting {
        business: t.business,
        member: t.owner,
    };
    remove_member(&mut db, by, t.billing, start())
        .await
        .expect("removed");
    let refused = use_action_link(&mut db, (kind, pick), &token, later(1)).await;
    assert_eq!(
        refused,
        Err(ActionLinkError::NotAllowed),
        "a removed member"
    );
    let task = Claimed {
        id: 1,
        kind: "businesses.member_removed".to_owned(),
        payload: serde_json::json!({"business": t.business, "member": t.billing}),
        attempts: 1,
    };
    run_member_removed(pool.clone(), task)
        .await
        .expect("handled");
    let mut db = pool.acquire().await.expect("db");
    let opened = open_action_link(&mut db, (kind, pick), &token, later(1)).await;
    assert_eq!(
        opened,
        Err(ActionLinkError::Ended),
        "voided with the member"
    );
    let owner = use_action_link(&mut db, (kind, pick), &kept, later(1)).await;
    assert!(owner.is_ok(), "another person's link still works");
}

#[test]
fn td_sec_09_a_cross_site_post_to_an_action_page_is_refused() {
    let mut cross = HeaderMap::new();
    cross.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    assert!(is_cross_site(&cross, Some("askhava.com")));
    let mut same = HeaderMap::new();
    same.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    same.insert("origin", HeaderValue::from_static("https://askhava.com"));
    assert!(!is_cross_site(&same, Some("askhava.com")));
}

#[test]
fn td_biz_15_each_action_needs_what_its_keyword_did() {
    assert_eq!(ActionKind::ALL.len(), 10);
    for kind in ActionKind::ALL {
        assert_eq!(ActionKind::from_slug(kind.slug()), Some(kind));
        let text = kind.link_text("1042");
        assert!(!text.to_lowercase().contains("click"), "A11Y-023: {text}");
        let expected = match kind {
            ActionKind::Quote | ActionKind::Hold => UseBy::LinkAlone,
            ActionKind::Confirm => UseBy::LinkAndCode,
            _ => UseBy::Session,
        };
        assert_eq!(kind.use_by(), expected, "{kind:?}");
    }
    assert_eq!(ActionKind::from_slug("delete"), None);
    assert_eq!(ActionKind::Confirm.link_text("1042"), "Confirm job 1042");
    assert!(OPEN_IN_BROWSER_LINE.contains("Open in browser"));
}
