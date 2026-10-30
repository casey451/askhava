//! Property tests of derived IDs and link tokens (TD-CARD-06, TD-PICK-12, TD-SEC-16; CARD-017,
//! PICK-028, SEC-037; rust-production R17).
//!
//! For random record IDs, salts and secrets: an ID is a function of its inputs; distinct inputs
//! or secrets give distinct IDs; and knowing the secret and the record ID alone, with no salt or
//! a guessed one, never reproduces a stored token's hash. All values are fake.

use proptest::prelude::*;
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::ids::{IdKind, IdSecret, LinkKind, LinkSalt, derive_id, link_token};

fn secret_from(seed: u64) -> IdSecret {
    IdSecret::new(1, SecretString::from(format!("fake-id-secret-{seed:0>20}"))).unwrap()
}

proptest! {
    /// TD-CARD-06, TD-PICK-12: the same inputs give the same ID; another job, business or secret
    /// gives another.
    #[test]
    fn ids_are_stable_and_distinct(
        job in any::<u128>(), other in any::<u128>(), business in any::<u128>(), seed in any::<u64>(),
    ) {
        prop_assume!(job != other);
        let s = secret_from(seed);
        for kind in [IdKind::Card, IdKind::Quote, IdKind::Pick] {
            let parts = [Uuid::from_u128(job), Uuid::from_u128(business)];
            let id = derive_id(&s, kind, &parts);
            prop_assert_eq!(id, derive_id(&s, kind, &parts));
            prop_assert_ne!(id, derive_id(&s, kind, &[Uuid::from_u128(other), parts[1]]));
            prop_assert_ne!(id, derive_id(&secret_from(seed.wrapping_add(1)), kind, &parts));
        }
    }

    /// TD-SEC-16: knowing `ID_SECRET` and the job alone (no salt, a zero salt, or another salt)
    /// never reproduces the stored token hash; the stored salt always does (replay).
    #[test]
    fn secret_and_job_alone_never_reproduce_a_token(
        job in any::<u128>(), salt in any::<[u8; 16]>(), guess in any::<[u8; 16]>(), seed in any::<u64>(),
    ) {
        prop_assume!(salt != guess);
        let s = secret_from(seed);
        let job = Uuid::from_u128(job);
        let stored = link_token(&s, LinkKind::QuotePage, job, &LinkSalt::from_bytes(&salt).unwrap()).hash();
        let replayed = link_token(&s, LinkKind::QuotePage, job, &LinkSalt::from_bytes(&salt).unwrap());
        prop_assert_eq!(replayed.hash(), stored);
        let guessed = link_token(&s, LinkKind::QuotePage, job, &LinkSalt::from_bytes(&guess).unwrap());
        prop_assert_ne!(guessed.hash(), stored);
        prop_assert_ne!(derive_id(&s, IdKind::Card, &[job]).as_bytes().to_vec(), stored[..16].to_vec());
    }
}
