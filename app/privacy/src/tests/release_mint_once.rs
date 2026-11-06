//! The release mint can be taken once per process (TD-SEC-11; SEC-030, D-488).
//!
//! `server` takes [`release_mint`] once and hands it to `payments`; a second call panics, so no
//! second minter of `ReleaseProof`s can exist. This is the only test in this crate that calls
//! `release_mint()` (D-751): another caller in the same test process would take the mint first.
//! The compile-fail cases that no other crate can forge a proof are in
//! `tests/security/ui/forge_release_proof_*.rs`.

use sqlx::types::Uuid;

use crate::api::{ReleaseBasis, release_mint};

/// TD-SEC-11 (SEC-030, D-488): the first take mints a proof naming its pick and contact; a second
/// take in the same process panics with the expected message.
#[test]
fn the_release_mint_is_taken_once() {
    let mint = release_mint();
    let (pick, contact) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let proof = mint.mint(pick, contact, ReleaseBasis::NoFeeDue);
    assert_eq!(
        (proof.pick_id(), proof.contact_id(), proof.basis()),
        (pick, contact, ReleaseBasis::NoFeeDue)
    );
    let second = std::panic::catch_unwind(release_mint).expect_err("a second mint");
    let message = second.downcast_ref::<String>().map(String::as_str);
    let message = message.or_else(|| second.downcast_ref::<&str>().copied());
    assert_eq!(message, Some("the release mint was already taken"));
}
