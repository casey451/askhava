//! TD-SEC-25 (SEC-046): code outside `privacy` cannot build a session proof by struct literal; the fields are private.

use askhava_privacy::api::{HomeownerSessionProof, OwnerSessionProof};

fn main() {
    let _homeowner = HomeownerSessionProof {
        contact_id: Default::default(),
    };
    let _owner = OwnerSessionProof {
        owner_id: Default::default(),
    };
}
