//! TD-SEC-25 (SEC-046): code outside `privacy` cannot copy a session proof it was handed; neither proof implements `Clone`.

use askhava_privacy::api::{HomeownerSessionProof, OwnerSessionProof};

fn copy_homeowner(proof: HomeownerSessionProof) -> (HomeownerSessionProof, HomeownerSessionProof) {
    (proof.clone(), proof)
}

fn copy_owner(proof: OwnerSessionProof) -> (OwnerSessionProof, OwnerSessionProof) {
    (proof.clone(), proof)
}

fn main() {
    let _ = (copy_homeowner, copy_owner);
}
