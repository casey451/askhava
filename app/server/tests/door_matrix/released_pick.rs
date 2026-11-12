//! The door probe's released pick (T401; D-3106, D-3134; PICK-022, PICK-024, FEE-010, FEE-012):
//! a waiting page pick released on the $0 path through module APIs only, as
//! `tests/integration/tests/no_fee_release_binding.rs` and `src/release_sink.rs` reach it. The
//! business's free first win is stored by `payments`, `payments::api::no_fee_release` clears the
//! pick for its own business and job, `picks::api::settle` moves it to `free`, the proof is minted
//! with `askhava_server::release_minter()` (never a second `privacy` release mint in the process),
//! and `picks::api::release` shows it with the masked pool the probe's router is built with.
//!
//! Every `payments` call of `door_matrix.rs` lives here, and this file names no homeowner mark,
//! so TD-FEE-07 (`tests/security/tests/no_homeowner_charge.rs`) holds for both; the contact the
//! release binds is passed in. Every ID is fake.

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_core::api::Settings;
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{Released, Settled, release, settle};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The masked pool of the router's own fake settings (`super::pairs`).
fn masked() -> MaskedPool {
    let pairs = super::pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let twilio = TwilioConfig::from_settings(&settings, pairs).expect("twilio");
    twilio.pool().clone()
}

/// The $0 free-first-win ledger decision for `business` on `job` (FEE-010, FEE-012).
async fn free_first_win(pool: &PgPool, (business, job): (Uuid, Uuid)) {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t401.free.{job}.{business}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(&mut db, &entry).await.expect("ledger");
}

/// Releases `pick`, the waiting page pick of `business` on `job`, to `contact` on the $0 path
/// (PICK-022, PICK-024, FEE-012, D-1739, D-3106): free first win, `no_fee_release`, settle to
/// `free`, the minted proof and the release, which must show it.
pub(super) async fn release_free(
    pool: &PgPool,
    vault: &Vault,
    (business, job, pick): (Uuid, Uuid, Uuid),
    contact: Uuid,
    now: Timestamp,
) {
    free_first_win(pool, (business, job)).await;
    let mut db = pool.acquire().await.expect("db");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let cleared = cleared.expect("a $0 decision clears the pick");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let settled = settle(&mut tx, &cleared).await.expect("settle");
    tx.commit().await.expect("commit");
    assert_eq!(settled, Settled::Moved(job), "the pick moved to free");
    let proof = askhava_server::release_minter().mint(cleared, contact);
    let shown = release(pool, (vault, &masked()), (proof, false), now).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
}
