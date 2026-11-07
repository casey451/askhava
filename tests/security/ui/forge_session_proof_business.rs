//! TD-PICK-48, TD-SEC-25 (SEC-046, D-1739): code outside `privacy` cannot build a business session proof by struct literal (its fields are private, so none can be named), `Default` or `Clone`.

use askhava_privacy::api::BusinessSessionProof;

fn copy(proof: BusinessSessionProof) -> (BusinessSessionProof, BusinessSessionProof) {
    (proof.clone(), proof)
}

fn main() {
    let _literal = BusinessSessionProof {};
    let _default: BusinessSessionProof = Default::default();
    let _ = copy;
}
