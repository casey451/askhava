//! The Svix timestamp check never overflows (PRIV-026, SEC-016, D-4196).
//!
//! `reader::verify` reads the unauthenticated `svix-timestamp` before the signature; an extreme
//! stamp is `Stale` without a panic or wrapped arithmetic, and the 300-second edge is inclusive.

use jiff::Timestamp;
use secrecy::SecretString;

use crate::reader::{InboundError, SignedDelivery, verify};

/// A fake `whsec_` key (base64 of `fake`); the signature never matches it.
const FAKE_SECRET: &str = "whsec_ZmFrZQ==";

/// The outcome of `verify` for a delivery stamped `stamp` checked at `now`.
fn check(stamp: i64, now: Timestamp) -> Result<(), InboundError> {
    let timestamp = stamp.to_string();
    let delivery = SignedDelivery {
        id: "msg_fake",
        timestamp: &timestamp,
        signature: "v1,AAAA",
        body: b"{}",
    };
    verify(&delivery, &SecretString::from(FAKE_SECRET), now)
}

/// PRIV-026, SEC-016: stamps whose gap overflows `i64`, or that are far off, are `Stale`.
#[test]
fn extreme_stamps_are_stale() {
    let now = Timestamp::from_second(1_800_000_000).expect("fake now");
    let wrap = now.as_second().wrapping_add(i64::MIN);
    for stamp in [i64::MIN, i64::MAX, -1, wrap] {
        assert_eq!(check(stamp, now), Err(InboundError::Stale), "stamp {stamp}");
    }
}

/// PRIV-026, SEC-016: 300 seconds away either side reaches the signature check; 301 is stale.
#[test]
fn tolerance_edge_is_inclusive() {
    let now = Timestamp::from_second(1_800_000_000).expect("fake now");
    let at = now.as_second();
    for stamp in [at - 300, at + 300] {
        assert_eq!(
            check(stamp, now),
            Err(InboundError::Signature),
            "stamp {stamp}"
        );
    }
    for stamp in [at - 301, at + 301] {
        assert_eq!(check(stamp, now), Err(InboundError::Stale), "stamp {stamp}");
    }
}
