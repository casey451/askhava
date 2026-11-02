//! The public face of `askhava-privacy`: the contact vault and the capabilities that open it (D-481, D-488).
//!
//! Plaintext leaves the vault only through the read functions here, each audit-logged with its
//! reader and purpose (PRIV-003, PRIV-004). `messaging` and `calls` read with [`read_contact`]
//! under a registered grant; the release needs a [`ReleaseProof`] (SEC-030), the homeowner realm a
//! [`HomeownerSessionProof`] and the owner realm an [`OwnerSessionProof`] with a purpose (SEC-046).
//! The proofs have private fields and no `Clone` or `Default`, so no other crate can build one;
//! each is made only by its mint, and each mint can be taken once per process: [`release_mint`]
//! by `server` for `payments` (T189), [`session_mint`] by `server` for `portals`' session
//! extractors. The architecture test fails any other crate that names a mint (TD-SEC-10, D-597).
//! Raw homeowner text enters only through [`read_inbound`] and [`read_inbound_email`] (PRIV-026),
//! and [`scrub`] is the only maker of [`ScrubbedText`] (AI-002 to AI-004, AI-020).

use std::sync::atomic::{AtomicBool, Ordering};

use askhava_ops::api::Actor;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::vault;

pub use crate::models::{
    ContactDetails, Field, GRANTS, Grant, IndexPurpose, NewContact, OwnerPurpose, Purpose, Reader,
    VaultError, grant_for, normalize_phone,
};
pub use crate::reader::{
    EmailAuth, EmailContext, Flags, Inbound, InboundEmail, InboundError, InboundWords, SenderIndex,
    SignedDelivery, Standing, WEBHOOK_TOLERANCE_SECS, email_index, read_inbound,
    read_inbound_email,
};
pub use crate::scrub::{
    CommonWords, DEFAULT_COMMON_WORDS, Known, NameMemory, REMOVED, ScrubbedText, is_format, scrub,
};
pub use crate::vault::{
    BASELINE_FLOOR, BASELINE_HOURS, RATE_MULTIPLE, Vault, check_read_rates, find_by_phone,
    rotate_batch, store_contact,
};

static RELEASE_MINT_TAKEN: AtomicBool = AtomicBool::new(false);
static SESSION_MINT_TAKEN: AtomicBool = AtomicBool::new(false);

/// Why a release is allowed (PRIV-005, D-477).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseBasis {
    /// A signature-checked Stripe event for a succeeded charge.
    PaidCharge,
    /// A recorded fee decision that no fee is due.
    NoFeeDue,
}

/// Proof that pick `pick_id` may release the details of its homeowner's contact `contact_id`;
/// only [`ReleaseMint::mint`] makes one, and only `payments` holds the mint. The proof names the
/// contact, so it opens that contact and no other (SEC-030, D-481, D-753).
#[derive(Debug)]
pub struct ReleaseProof {
    pick_id: Uuid,
    contact_id: Uuid,
    basis: ReleaseBasis,
}

impl ReleaseProof {
    /// The pick this proof releases.
    #[must_use]
    pub fn pick_id(&self) -> Uuid {
        self.pick_id
    }

    /// The homeowner contact this proof opens (D-753).
    #[must_use]
    pub fn contact_id(&self) -> Uuid {
        self.contact_id
    }

    /// Why the release is allowed.
    #[must_use]
    pub fn basis(&self) -> ReleaseBasis {
        self.basis
    }
}

/// The one maker of [`ReleaseProof`]s, not `Clone`; `server` takes it once and hands it to
/// `payments` (SEC-030, D-488).
#[derive(Debug)]
pub struct ReleaseMint {
    _private: (),
}

impl ReleaseMint {
    /// A proof for pick `pick_id` and its homeowner's contact `contact_id`; `payments` calls it
    /// only after a verified succeeded charge or a no-fee decision, never on a redirect (SEC-030,
    /// D-753).
    #[must_use]
    pub fn mint(&self, pick_id: Uuid, contact_id: Uuid, basis: ReleaseBasis) -> ReleaseProof {
        ReleaseProof {
            pick_id,
            contact_id,
            basis,
        }
    }
}

/// Takes the release mint; once per process (SEC-030, D-488).
///
/// # Panics
///
/// On a second call in the same process, so no second minter can exist.
#[must_use]
pub fn release_mint() -> ReleaseMint {
    assert!(
        !RELEASE_MINT_TAKEN.swap(true, Ordering::SeqCst),
        "the release mint was already taken"
    );
    ReleaseMint { _private: () }
}

/// A signed-in homeowner's session, naming their own contact; only [`SessionMint::homeowner`]
/// makes one, in `portals`' homeowner session extractor (SEC-046).
#[derive(Debug)]
pub struct HomeownerSessionProof {
    contact_id: Uuid,
}

/// Casey's signed-in owner session; only [`SessionMint::owner`] makes one, in `portals`' owner
/// session extractor (SEC-046).
#[derive(Debug)]
pub struct OwnerSessionProof {
    owner_id: Uuid,
}

/// The one maker of session proofs, not `Clone`; `server` takes it once and hands it to `portals`
/// (SEC-046, D-488).
#[derive(Debug)]
pub struct SessionMint {
    _private: (),
}

impl SessionMint {
    /// The proof for a verified homeowner session on contact `contact_id` (SEC-046).
    #[must_use]
    pub fn homeowner(&self, contact_id: Uuid) -> HomeownerSessionProof {
        HomeownerSessionProof { contact_id }
    }

    /// The proof for Casey's verified owner session (SEC-046).
    #[must_use]
    pub fn owner(&self, owner_id: Uuid) -> OwnerSessionProof {
        OwnerSessionProof { owner_id }
    }
}

/// Takes the session mint; once per process (SEC-046, D-488).
///
/// # Panics
///
/// On a second call in the same process.
#[must_use]
pub fn session_mint() -> SessionMint {
    assert!(
        !SESSION_MINT_TAKEN.swap(true, Ordering::SeqCst),
        "the session mint was already taken"
    );
    SessionMint { _private: () }
}

/// Reads `fields` of contact `id` for `messaging` or `calls` (PRIV-003, PRIV-004). The reader,
/// purpose and every field must be a registered grant; a realm or release reader, or anything
/// unregistered, is refused and raises a High alert at once (SEC-047).
///
/// # Errors
///
/// [`VaultError::UnregisteredPurpose`], [`VaultError::NotFound`], [`VaultError::Sealed`],
/// [`VaultError::Audit`] or [`VaultError::Database`].
pub async fn read_contact(
    db: &mut PgConnection,
    vault: &Vault,
    (reader, purpose): (Reader, Purpose),
    id: Uuid,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    let service = matches!(reader, Reader::Messaging | Reader::Calls);
    match grant_for(reader, purpose, fields).filter(|_| service) {
        Some(grant) => vault::read(db, vault, (grant, Actor::System, None), id, fields).await,
        None => vault::refuse(db, (reader, purpose), id).await,
    }
}

/// Reads the release fields of the contact `proof` names, for the pick it names, consuming the
/// proof so it opens one release of one contact only (D-753); besides the per-field rows, an audit
/// row with subject `picks.pick` and the proof's pick ID ties the read to its pick (SEC-030,
/// PRIV-003, PRIV-005, D-752). The phone is never released (PICK-023).
///
/// # Errors
///
/// As [`read_contact`], and [`VaultError::InTransaction`].
pub async fn read_for_release(
    db: &mut PgConnection,
    vault: &Vault,
    proof: ReleaseProof,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    let (access, id) = ((Reader::Picks, Purpose::Release), proof.contact_id);
    match grant_for(access.0, access.1, fields) {
        Some(grant) => {
            let gate = (grant, Actor::System, Some(proof.pick_id));
            vault::read(db, vault, gate, id, fields).await
        }
        None => vault::refuse(db, access, id).await,
    }
}

/// Reads `fields` of the signed-in homeowner's own contact, the only one the proof names (PRIV-021,
/// SEC-046).
///
/// # Errors
///
/// As [`read_contact`].
pub async fn read_own_contact(
    db: &mut PgConnection,
    vault: &Vault,
    proof: &HomeownerSessionProof,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    let access = (
        Reader::PortalsHomeowner,
        Purpose::OwnRecord,
        Actor::Homeowner(proof.contact_id),
    );
    gated(db, vault, access, proof.contact_id, fields).await
}

/// Reads `fields` of contact `id` for Casey's owner session and `purpose` (SEC-046, OPS-036).
///
/// # Errors
///
/// As [`read_contact`].
pub async fn read_for_owner(
    db: &mut PgConnection,
    vault: &Vault,
    (proof, purpose): (&OwnerSessionProof, OwnerPurpose),
    id: Uuid,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    let access = (
        Reader::PortalsOwner,
        purpose.into(),
        Actor::Owner(proof.owner_id),
    );
    gated(db, vault, access, id, fields).await
}

/// A capability-gated read: the grant still bounds the fields; an unlisted field is refused and
/// alerted (SEC-047).
async fn gated(
    db: &mut PgConnection,
    vault: &Vault,
    (reader, purpose, actor): (Reader, Purpose, Actor),
    id: Uuid,
    fields: &[Field],
) -> Result<ContactDetails, VaultError> {
    match grant_for(reader, purpose, fields) {
        Some(grant) => vault::read(db, vault, (grant, actor, None), id, fields).await,
        None => vault::refuse(db, (reader, purpose), id).await,
    }
}

/// The realm and release reads with their capabilities (TD-SEC-25; SEC-030, SEC-046, PRIV-004).
///
/// Here, not in `src/tests/`, because only this module can build a proof without a mint: the
/// mints are once per process and their names are banned outside `privacy`, `payments`, `server`
/// and `portals` (D-597), and `release_mint()`'s once-test is T192's (D-749). No other test in
/// this crate may call `session_mint()`.
#[cfg(test)]
mod realm_tests {
    use askhava_ops::api::export_audit;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use secrecy::{ExposeSecret, SecretString};
    use sqlx::PgPool;

    use super::*;

    fn vault() -> Vault {
        let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
        let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
        Vault::new(&key(0x31), &key(0x41), &url).expect("keys")
    }

    fn contact(phone: &str) -> NewContact {
        let s = |v: &str| Some(SecretString::from(v.to_owned()));
        NewContact {
            name: s("Ada Example"),
            phone: s(phone),
            address: s("7 Example Way"),
            ..NewContact::default()
        }
    }

    /// TD-SEC-25: a homeowner session reads its own record and no other, audited as that homeowner.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_homeowner_session_reads_only_its_own_record(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let own = store_contact(&mut db, &vault, &contact("928-555-0111"))
            .await
            .expect("own");
        store_contact(&mut db, &vault, &contact("928-555-0112"))
            .await
            .expect("other");
        let proof = HomeownerSessionProof { contact_id: own };
        let read = read_own_contact(&mut db, &vault, &proof, &Field::ALL)
            .await
            .expect("read");
        assert_eq!(read.id, own);
        assert_eq!(
            read.phone.as_ref().map(ExposeSecret::expose_secret),
            Some("+19285550111")
        );
        let rows = export_audit(&mut db, 0, 100).await.expect("export");
        let reads = rows
            .iter()
            .filter(|r| r.action == "vault_read.portals_homeowner.own_record");
        let readers: Vec<_> = reads.map(|r| (r.subject_id, r.actor_id)).collect();
        assert_eq!(readers, [(own, Some(own)); 5]);
    }

    /// SEC-030, PICK-023: a release proof opens name, email, address and on-site contact, never the phone.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_release_never_opens_the_phone(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = store_contact(&mut db, &vault, &contact("928-555-0113"))
            .await
            .expect("store");
        let mint = ReleaseMint { _private: () };
        let proof = mint.mint(Uuid::max(), id, ReleaseBasis::NoFeeDue);
        assert_eq!(
            (proof.pick_id(), proof.contact_id(), proof.basis()),
            (Uuid::max(), id, ReleaseBasis::NoFeeDue)
        );
        let read = read_for_release(&mut db, &vault, proof, &[Field::Name, Field::Address]).await;
        assert_eq!(
            read.expect("release")
                .name
                .map(|n| n.expose_secret().to_owned())
                .as_deref(),
            Some("Ada Example")
        );
        let proof = mint.mint(Uuid::max(), id, ReleaseBasis::PaidCharge);
        let phone = read_for_release(&mut db, &vault, proof, &[Field::Phone]).await;
        assert_eq!(phone.err(), Some(VaultError::UnregisteredPurpose));
    }

    /// SEC-030, PRIV-003, D-752, D-753: a release read is audited against the pick its proof names
    /// and opens only the contact the proof names.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn a_release_read_is_audited_against_its_pick(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = store_contact(&mut db, &vault, &contact("928-555-0115"))
            .await
            .expect("store");
        let pick = Uuid::from_u128(0x22);
        let other = store_contact(&mut db, &vault, &contact("928-555-0116"))
            .await
            .expect("other");
        let proof = ReleaseMint { _private: () }.mint(pick, id, ReleaseBasis::PaidCharge);
        read_for_release(&mut db, &vault, proof, &[Field::Name])
            .await
            .expect("release");
        let rows = export_audit(&mut db, 0, 100).await.expect("export");
        let release: Vec<_> = rows
            .iter()
            .filter(|r| r.action == "vault_read.picks.release")
            .map(|r| (r.subject_kind.as_str(), r.subject_id, r.field.as_deref()))
            .collect();
        assert_eq!(
            release,
            [
                ("privacy.contact", id, Some("name")),
                ("picks.pick", pick, None)
            ]
        );
        assert!(
            rows.iter()
                .filter(|r| r.action.starts_with("vault_read."))
                .all(|r| r.subject_id != other),
            "a proof opens only the contact it names (D-753)"
        );
    }

    /// SEC-046, OPS-036: the owner realm reads with its purpose, audited as the owner, within its fields.
    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn the_owner_realm_reads_with_its_purpose(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = store_contact(&mut db, &vault, &contact("928-555-0114"))
            .await
            .expect("store");
        let proof = OwnerSessionProof {
            owner_id: Uuid::max(),
        };
        for purpose in [OwnerPurpose::CallbackAlert, OwnerPurpose::HelpRequest] {
            let read =
                read_for_owner(&mut db, &vault, (&proof, purpose), id, &[Field::Phone]).await;
            assert!(read.expect("owner read").phone.is_some());
        }
        let street = read_for_owner(
            &mut db,
            &vault,
            (&proof, OwnerPurpose::HelpRequest),
            id,
            &[Field::Address],
        )
        .await;
        assert_eq!(street.err(), Some(VaultError::UnregisteredPurpose));
        let rows = export_audit(&mut db, 0, 100).await.expect("export");
        let owner = rows
            .iter()
            .filter(|r| r.action.starts_with("vault_read.portals_owner."));
        let owners: Vec<_> = owner.map(|r| r.actor_id).collect();
        assert_eq!(owners, [Some(Uuid::max()); 2]);
    }

    /// SEC-046, D-488: the session mint makes both proofs and can be taken once per process.
    #[test]
    fn the_session_mint_is_taken_once() {
        let mint = session_mint();
        let id = Uuid::max();
        assert_eq!(
            (mint.homeowner(id).contact_id, mint.owner(id).owner_id),
            (id, id)
        );
        assert!(
            std::panic::catch_unwind(session_mint).is_err(),
            "a second mint"
        );
    }
}
