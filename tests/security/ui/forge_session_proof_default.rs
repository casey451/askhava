//! TD-SEC-25 (SEC-046): code outside `privacy` cannot build a session proof with `Default`; neither proof implements it.

use askhava_privacy::api::{HomeownerSessionProof, OwnerSessionProof};

fn main() {
    let _homeowner = HomeownerSessionProof::default();
    let _owner: OwnerSessionProof = Default::default();
}
