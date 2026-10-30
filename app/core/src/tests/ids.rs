//! Derived IDs, link tokens and webhook event keys (TD-CARD-06, TD-PICK-12, TD-SEC-16, TD-SEC-18;
//! CARD-017, PICK-028, SEC-017, SEC-037, SEC-039).
//!
//! Example cases: IDs are stable, differ by kind, job, business and secret; a token changes with
//! its salt; a secret, salt or token of the wrong shape is refused; nothing secret reaches
//! `Debug`; event keys keep Twilio pairs apart. The database cases (re-issue, revoke, 404,
//! exactly-once) are in `tests/integration/tests/webhook_dedupe.rs`. All values are fake.

use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::ids::{
    IdError, IdKind, IdSecret, LinkKind, LinkSalt, LinkToken, card_id, derive_id, link_token,
    pick_id, quote_id,
};
use crate::webhooks::{Dedupe, EventKey, WebhookError};

/// A fake 32-byte secret, version 1, for environment `env`.
pub(super) fn secret(env: &str) -> IdSecret {
    IdSecret::new(
        1,
        SecretString::from(format!("fake-id-secret-for-tests-{env:0>8}")),
    )
    .unwrap()
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// TD-CARD-06: card IDs are stable, differ across jobs and businesses, and need the secret.
#[test]
fn td_card_06_card_ids_are_stable_distinct_and_keyed() {
    let s = secret("test");
    assert_eq!(card_id(&s, id(1), id(2)), card_id(&s, id(1), id(2)));
    assert_ne!(card_id(&s, id(1), id(2)), card_id(&s, id(3), id(2)));
    assert_ne!(card_id(&s, id(1), id(2)), card_id(&s, id(1), id(4)));
    assert_ne!(
        card_id(&s, id(1), id(2)),
        card_id(&secret("other"), id(1), id(2))
    );
    assert_ne!(card_id(&s, id(1), id(2)), card_id(&s, id(2), id(1)));
}

/// TD-PICK-12: quote, pick and link IDs are derived and stable, and never share a card's ID.
#[test]
fn td_pick_12_quote_pick_and_link_ids_are_derived_and_stable() {
    let s = secret("test");
    let quote = quote_id(&s, id(1), id(2));
    assert_eq!(quote, quote_id(&s, id(1), id(2)));
    assert_ne!(quote, card_id(&s, id(1), id(2)));
    assert_eq!(pick_id(&s, id(1), quote), pick_id(&s, id(1), quote));
    assert_eq!(
        pick_id(&s, id(1), quote),
        derive_id(&s, IdKind::Pick, &[id(1), quote])
    );
    let salt = LinkSalt::from_bytes(&[7; 16]).unwrap();
    let link = link_token(&s, LinkKind::QuotePage, id(1), &salt);
    assert_eq!(link, link_token(&s, LinkKind::QuotePage, id(1), &salt));
    assert_ne!(link, link_token(&s, LinkKind::Action, id(1), &salt));
}

/// TD-SEC-16: the same job in two environments gives two tokens; a new salt gives a new token;
/// the secret version is part of the token.
#[test]
fn td_sec_16_tokens_differ_by_environment_salt_and_version() {
    let salt = LinkSalt::generate().unwrap();
    let test = link_token(&secret("test"), LinkKind::QuotePage, id(9), &salt);
    let live = link_token(&secret("live"), LinkKind::QuotePage, id(9), &salt);
    assert_ne!(test, live);
    let reissued = link_token(
        &secret("test"),
        LinkKind::QuotePage,
        id(9),
        &LinkSalt::generate().unwrap(),
    );
    assert_ne!(test, reissued);
    let key = || SecretString::from("fake-id-secret-for-tests-00000000".to_owned());
    let v2 = IdSecret::new(2, key()).unwrap();
    let v1 = IdSecret::new(1, key()).unwrap();
    assert_ne!(
        link_token(&v1, LinkKind::Photo, id(9), &salt),
        link_token(&v2, LinkKind::Photo, id(9), &salt)
    );
    assert_ne!(LinkSalt::generate().unwrap(), LinkSalt::generate().unwrap());
}

/// TD-SEC-16: a token parses back, its hash is stable, and a token of the wrong shape is refused.
#[test]
fn td_sec_16_tokens_parse_hash_and_refuse_bad_shapes() {
    let token = link_token(
        &secret("test"),
        LinkKind::QuotePage,
        id(5),
        &LinkSalt::from_bytes(&[1; 16]).unwrap(),
    );
    let parsed = LinkToken::parse(token.as_str()).unwrap();
    assert_eq!(parsed.hash(), token.hash());
    for bad in [
        "",
        "abc",
        &token.as_str().to_uppercase(),
        &format!("{}0", token.as_str()),
    ] {
        assert_eq!(LinkToken::parse(bad), Err(IdError::BadToken));
    }
    assert_eq!(LinkSalt::from_bytes(&[0; 15]), Err(IdError::BadSalt));
}

/// SEC-037: a short or unversioned secret is refused, and no secret, token or salt reaches
/// `Debug`.
#[test]
fn weak_secrets_are_refused_and_nothing_secret_is_printed() {
    let short = SecretString::from("fake-short".to_owned());
    assert_eq!(IdSecret::new(1, short).unwrap_err(), IdError::WeakSecret);
    let unversioned = SecretString::from("fake-id-secret-for-tests-00000000".to_owned());
    assert_eq!(
        IdSecret::new(0, unversioned).unwrap_err(),
        IdError::WeakSecret
    );
    let s = secret("test");
    assert!(!format!("{s:?}").contains("fake-id-secret"));
    let token = link_token(
        &s,
        LinkKind::Action,
        id(3),
        &LinkSalt::from_bytes(&[2; 16]).unwrap(),
    );
    assert!(!format!("{token:?}").contains(token.as_str()));
    let salt = LinkSalt::from_bytes(&[171; 16]).unwrap();
    let shown = format!("{salt:?}");
    assert_eq!(shown, "LinkSalt([redacted])");
    assert!(!shown.contains("171") && !shown.contains("ab"));
}

/// TD-SEC-18: Twilio status pairs and voice steps are distinct keys; bad parts are refused.
#[test]
fn td_sec_18_event_keys_keep_pairs_apart() {
    let sent = EventKey::pair("SM00000000000000000000000000000001", "sent").unwrap();
    let delivered = EventKey::pair("SM00000000000000000000000000000001", "delivered").unwrap();
    assert_ne!(sent, delivered);
    assert_eq!(sent.as_str(), "SM00000000000000000000000000000001:sent");
    assert_eq!(EventKey::new("evt_fake_1").unwrap().as_str(), "evt_fake_1");
    for bad in ["", "a:b", "has space", &"x".repeat(129)] {
        assert_eq!(EventKey::new(bad), Err(WebhookError::BadKey));
        assert_eq!(EventKey::pair("CA1", bad), Err(WebhookError::BadKey));
    }
}

/// SEC-017: a returned row is the first delivery, none a repeat.
#[test]
fn dedupe_reads_the_returning_row() {
    assert!(Dedupe::from_returning(Some("evt_fake_1")).is_first());
    assert_eq!(Dedupe::from_returning::<&str>(None), Dedupe::Repeat);
}
