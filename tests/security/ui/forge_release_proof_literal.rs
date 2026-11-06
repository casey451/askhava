//! TD-SEC-11 (SEC-030): code outside `privacy` cannot build a release proof or its mint by struct literal; the fields are private.

use askhava_privacy::api::{ReleaseBasis, ReleaseMint, ReleaseProof};

fn main() {
    let _proof = ReleaseProof {
        pick_id: Default::default(),
        contact_id: Default::default(),
        basis: ReleaseBasis::PaidCharge,
    };
    let _mint = ReleaseMint { _private: () };
}
