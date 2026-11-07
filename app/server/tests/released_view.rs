//! TD-PICK-48 (PICK-024, PRIV-003, SEC-046; D-1739, D-1895) with the real session mint:
//! `server::session_minter()` hands every caller in the process the one `privacy` session mint
//! (`server` is the one test home allowed to name it, `tests/integration/tests/architecture.rs`),
//! and `picks::api::released_view` with a business session proof it makes answers an unknown pick
//! with the one not-found answer and opens nothing.
//!
//! `server` code may name no module's schema, so this file seeds nothing a module API cannot make
//! (TD-SEC-10), and no API yet moves a pick to paid or free or writes a release row (D-1564,
//! D-1895). The released view's database cases are therefore split where the data can be made:
//! the rule on stored picks, release rows and team roles in `app/picks/src/tests/released_view.rs`,
//! and the vault read with a business proof in `privacy`'s `realm_tests`. Fake IDs only.

use askhava_core::api::MIGRATOR;
use askhava_ops::api::export_audit;
use askhava_picks::api::{ViewAnswer, released_view};
use askhava_privacy::api::Vault;
use askhava_server::session_minter;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

fn vault() -> Vault {
    let key = |k: &str| SecretString::from(format!("1:{k}"));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let (seal, index) = (
        "ERERERERERERERERERERERERERERERERERERERERERE=",
        "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI=",
    );
    Vault::new(&key(seal), &key(index), &url).expect("vault")
}

/// SEC-046, D-488: every call gets the same mint, so the session mint is taken once per process
/// however often the composition or a test asks; its proof names the person and the business.
#[test]
fn the_session_minter_is_one_mint_per_process() {
    assert!(std::ptr::eq(session_minter(), session_minter()));
    let (person, business) = (Uuid::from_u128(0x0253_0c01), Uuid::from_u128(0x0253_0c02));
    let proof = session_minter().business(person, business);
    assert_eq!((proof.person(), proof.business()), (person, business));
}

/// TD-PICK-48: a business session proof passing a pick ID with no release gets the one not-found
/// answer, and nothing is read from the vault.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_48_an_unreleased_pick_is_not_found_and_opens_nothing(pool: PgPool) {
    let (person, business) = (Uuid::from_u128(0x0253_0c03), Uuid::from_u128(0x0253_0c04));
    let proof = session_minter().business(person, business);
    let pick = Uuid::from_u128(0x0253_0cff);
    let answer = released_view(&pool, &vault(), &proof, pick)
        .await
        .expect("view");
    assert!(matches!(answer, ViewAnswer::NotFound));
    let mut db = pool.acquire().await.expect("connection");
    let rows = export_audit(&mut db, 0, 100).await.expect("export");
    let reads = rows.iter().filter(|r| r.action.starts_with("vault_read."));
    assert_eq!(reads.count(), 0, "no vault read");
}
