//! TD-SEC-11 (SEC-030): code outside `privacy` cannot build a release proof or its mint with `Default`; neither implements it.

use askhava_privacy::api::{ReleaseMint, ReleaseProof};

fn main() {
    let _proof = ReleaseProof::default();
    let _mint: ReleaseMint = Default::default();
}
