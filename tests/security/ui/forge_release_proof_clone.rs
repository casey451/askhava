//! TD-SEC-11 (SEC-030): code outside `privacy` cannot copy a release proof or its mint it was lent; neither implements `Clone`.

use askhava_privacy::api::{ReleaseMint, ReleaseProof};

fn copy_proof(proof: &ReleaseProof) -> ReleaseProof {
    proof.clone()
}

fn copy_mint(mint: &ReleaseMint) -> ReleaseMint {
    mint.clone()
}

fn main() {
    let _ = (copy_proof, copy_mint);
}
