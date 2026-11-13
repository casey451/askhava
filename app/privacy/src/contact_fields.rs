//! The homeowner's own contact gets her first name and street address (INT-027, PRIV-001,
//! PRIV-002, PRIV-003; T255, D-3431).
//!
//! [`set_name_address`] writes the name-and-address answer onto the contact that already holds her
//! phone, the one the release, the read-back, the released view and the retention purge read, so
//! no second, phone-less contact is ever made. Each value is sealed with `Vault::seal` for that
//! contact's ID and its own column, so it opens only in its own place (PRIV-002). It adds no
//! reader, purpose or grant, reads no field, counts no read and returns no plaintext; it writes one
//! `contact_fields_set` audit row per field, naming the field only (PRIV-003, PRIV-028). The
//! purge keeps the phone (D-079), so this setter cannot tell a purged contact from a live one:
//! the refusal to refill a request the purge treats as ended is `intake`'s `take_name_address`.

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::VaultError;
use crate::vault::Vault;

/// The audit action of each field this setter writes (PRIV-003).
pub const CONTACT_FIELDS_SET: &str = "contact_fields_set";

/// Seals `name` and `address` for contact `contact` and stores them on its row, replacing any
/// earlier answer, with one [`CONTACT_FIELDS_SET`] audit row per field (INT-027, PRIV-001 to
/// PRIV-003, D-3431). One transaction (a savepoint inside the caller's); its `UPDATE` takes the
/// contact row, lock (3) of the retention lock order (D-1839). The phone, email, on-site contact
/// and every index stay as they are; nothing is read or logged.
///
/// # Errors
/// [`VaultError::NotFound`] for an unknown contact or one with no phone (nothing changed, no
/// audit row); [`VaultError::Sealed`] when a value cannot be sealed; [`VaultError::Audit`] when an
/// audit row cannot be written (the update is rolled back); [`VaultError::Database`].
pub async fn set_name_address(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
    name: &SecretString,
    address: &SecretString,
) -> Result<(), VaultError> {
    let name = vault.seal("name", contact, name.expose_secret())?;
    let address = vault.seal("address", contact, address.expose_secret())?;
    let mut tx = sqlx::Acquire::begin(db).await.map_err(db_err)?;
    let updated = sqlx::query!(
        "UPDATE privacy.contact SET name = $2, address = $3 WHERE id = $1 AND phone IS NOT NULL",
        contact,
        name,
        address,
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if updated == 0 {
        return Err(VaultError::NotFound);
    }
    for field in ["name", "address"] {
        let entry = AuditEntry {
            actor: Actor::System,
            action: CONTACT_FIELDS_SET,
            subject_kind: "privacy.contact",
            subject_id: contact,
            change: Change::Field(field),
        };
        audit(&mut tx, &entry)
            .await
            .map_err(|_| VaultError::Audit)?;
    }
    tx.commit().await.map_err(db_err)
}

fn db_err(_: sqlx::Error) -> VaultError {
    VaultError::Database
}

/// TD-PRIV-30 (T255, D-3431): fake data only.
#[cfg(test)]
mod tests {
    use askhava_ops::api::export_audit;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use secrecy::{ExposeSecret, SecretString};
    use sqlx::PgPool;
    use sqlx::types::Uuid;

    use super::{CONTACT_FIELDS_SET, set_name_address};
    use crate::api::{Field, NewContact, Purpose, Reader, purge_contact, read_contact};
    use crate::models::VaultError;
    use crate::vault::{Vault, store_contact};

    fn vault() -> Vault {
        let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
        let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
        Vault::new(&key(0x32), &key(0x42), &url).expect("keys")
    }

    fn s(v: &str) -> SecretString {
        SecretString::from(v.to_owned())
    }

    async fn stored(db: &mut sqlx::PgConnection, vault: &Vault, phone: Option<&str>) -> Uuid {
        let new = NewContact {
            phone: phone.map(s),
            email: Some(s("testa@example.com")),
            name: phone.is_none().then(|| s("Nobody Example")),
            ..NewContact::default()
        };
        store_contact(db, vault, &new).await.expect("store")
    }

    async fn sets(db: &mut sqlx::PgConnection) -> Vec<(Uuid, Option<String>)> {
        let rows = export_audit(db, 0, 1000).await.expect("export");
        rows.into_iter()
            .filter(|r| r.action == CONTACT_FIELDS_SET)
            .map(|r| (r.subject_id, r.field))
            .collect()
    }

    async fn plain(db: &mut sqlx::PgConnection, vault: &Vault, id: Uuid) -> [Option<String>; 4] {
        let read = read_contact(
            db,
            vault,
            (Reader::Messaging, Purpose::Send),
            id,
            &Field::ALL,
        )
        .await
        .expect("read");
        let open = |v: Option<SecretString>| v.map(|v| v.expose_secret().to_owned());
        [
            open(read.name),
            open(read.address),
            open(read.phone),
            open(read.email),
        ]
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_priv_30_unknown_and_phone_less_contacts_are_refused(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let phoneless = stored(&mut db, &vault, None).await;
        for id in [Uuid::from_u128(0xF1), phoneless] {
            let set = set_name_address(&mut db, &vault, id, &s("Testa"), &s("1 Fake St")).await;
            assert!(matches!(set, Err(VaultError::NotFound)));
        }
        assert!(sets(&mut db).await.is_empty());
        let [name, address, ..] = plain(&mut db, &vault, phoneless).await;
        assert_eq!((name.as_deref(), address), (Some("Nobody Example"), None));
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_priv_30_a_stored_contact_gets_both_fields_and_one_row_each(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = stored(&mut db, &vault, Some("928-555-0181")).await;
        let index = "SELECT purpose, digest FROM privacy.contact_index WHERE contact_id = $1 \
                     ORDER BY purpose";
        let before: Vec<(String, Vec<u8>)> = sqlx::query_as(index)
            .bind(id)
            .fetch_all(&mut *db)
            .await
            .expect("index");
        set_name_address(&mut db, &vault, id, &s("Testa"), &s("1 Fake St"))
            .await
            .expect("set");
        let mut rows = sets(&mut db).await;
        rows.sort();
        let want = |f: &str| (id, Some(f.to_owned()));
        assert_eq!(rows, [want("address"), want("name")]);
        set_name_address(&mut db, &vault, id, &s("Testo"), &s("2 Fake Ave"))
            .await
            .expect("again");
        assert_eq!(sets(&mut db).await.len(), 4);
        let got = plain(&mut db, &vault, id).await;
        let want = ["Testo", "2 Fake Ave", "+19285550181", "testa@example.com"];
        assert_eq!(got, want.map(|v| Some(v.to_owned())));
        let after: Vec<(String, Vec<u8>)> = sqlx::query_as(index)
            .bind(id)
            .fetch_all(&mut *db)
            .await
            .expect("index");
        assert_eq!(before, after);
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_priv_30_a_value_copied_to_another_contact_does_not_open(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = stored(&mut db, &vault, Some("928-555-0182")).await;
        let other = stored(&mut db, &vault, Some("928-555-0183")).await;
        set_name_address(&mut db, &vault, id, &s("Testa"), &s("1 Fake St"))
            .await
            .expect("set");
        sqlx::query(
            "UPDATE privacy.contact SET address = (SELECT address FROM privacy.contact \
             WHERE id = $1) WHERE id = $2",
        )
        .bind(id)
        .bind(other)
        .execute(&mut *db)
        .await
        .expect("copy");
        let read = read_contact(
            &mut db,
            &vault,
            (Reader::Messaging, Purpose::Send),
            other,
            &[Field::Address],
        )
        .await;
        assert!(matches!(read, Err(VaultError::Sealed)));
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_priv_30_the_purge_clears_both_fields(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = stored(&mut db, &vault, Some("928-555-0184")).await;
        set_name_address(&mut db, &vault, id, &s("Testa"), &s("1 Fake St"))
            .await
            .expect("set");
        assert!(purge_contact(&mut db, id).await.expect("purge"));
        let [name, address, phone, _] = plain(&mut db, &vault, id).await;
        assert_eq!((name, address), (None, None));
        assert_eq!(phone.as_deref(), Some("+19285550184"));
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_priv_30_a_failed_audit_write_leaves_the_row(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        let id = stored(&mut db, &vault, Some("928-555-0185")).await;
        for sql in [
            "CREATE FUNCTION public.t255_refuse() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN IF NEW.field = 'address' THEN RAISE EXCEPTION 'fake audit failure'; \
             END IF; RETURN NEW; END $$",
            "CREATE TRIGGER t255_refuse BEFORE INSERT ON ops.audit_log FOR EACH ROW \
             EXECUTE FUNCTION public.t255_refuse()",
        ] {
            sqlx::query(sql).execute(&mut *db).await.expect("trigger");
        }
        let set = set_name_address(&mut db, &vault, id, &s("Testa"), &s("1 Fake St")).await;
        assert!(matches!(set, Err(VaultError::Audit)));
        sqlx::query("DROP TRIGGER t255_refuse ON ops.audit_log")
            .execute(&mut *db)
            .await
            .expect("drop");
        assert!(sets(&mut db).await.is_empty());
        let [name, address, ..] = plain(&mut db, &vault, id).await;
        assert_eq!((name, address), (None, None));
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_priv_30_a_broken_connection_is_a_database_error(pool: PgPool) {
        let (vault, mut db) = (vault(), pool.acquire().await.expect("connection"));
        sqlx::query("SET default_transaction_read_only = on")
            .execute(&mut *db)
            .await
            .expect("read only");
        let set =
            set_name_address(&mut db, &vault, Uuid::from_u128(1), &s("T"), &s("1 A St")).await;
        assert!(matches!(set, Err(VaultError::Database)));
    }
}
