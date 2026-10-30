# askhava-privacy: the contact vault

## Purpose

The layer-2 crate that holds every homeowner contact detail (name, phone, email, street address, on-site contact), each sealed on its own under `VAULT_KEY`, and the only code that opens one (PRIV-001, PRIV-002). Every other record refers to a contact by its ID. Plaintext leaves only through `api`, each read audit-logged with its reader and purpose (PRIV-003, PRIV-004); the release and portal-realm reads need proof types only this crate's once-per-process mints make (SEC-030, SEC-046). T022 built the vault; the inbound reader and `ScrubbedText` are T023's, photos and purges later tasks'.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the public surface: re-exports, the capability types `ReleaseProof`, `ReleaseBasis`, `ReleaseMint` (`mint`), `release_mint()`, `HomeownerSessionProof`, `OwnerSessionProof`, `SessionMint` (`homeowner`, `owner`), `session_mint()`; the reads `read_contact`, `read_for_release`, `read_own_contact`, `read_for_owner`; the inline `realm_tests` (D-751).
- `src/models.rs`: `VaultError`, `Field`, `Reader`, `Purpose`, `OwnerPurpose`, `Grant`, `GRANTS`, `grant_for`, `NewContact`, `ContactDetails`, `IndexPurpose`, `normalize_phone`.
- `src/vault.rs`: `Vault` (`new`, `seal_version`, `index_version`, `blind_index`), `store_contact`, `find_by_phone`, `rotate_batch`, `check_read_rates`, the crate-private `read` and `refuse`; `RATE_MULTIPLE`, `BASELINE_HOURS`, `BASELINE_FLOOR`.
- Migration `0036_privacy_vault.sql`: the `privacy` schema and grants, `privacy.sealed_version()`, `privacy.contact` (sealed columns, generated `oldest_key_version`), `privacy.contact_index` (blind index), `privacy.read_count` (D-749).
- `sqlx.toml`: `'TIMESTAMPTZ'` reads as `jiff_sqlx::Timestamp`.

## Rules it enforces

- PRIV-001: contact fields exist only as `privacy.contact` columns; `store_contact` is the way in.
- PRIV-002, PRIV-024: `Vault::seal`/`open` (AES-GCM, 256-bit key, random nonce, key version and associated data `privacy.contact|<column>|<id>|v<version>`); `Vault::new` refuses keys shared between `VAULT_KEY` and `VAULT_INDEX_KEY` or inside the database credential; `blind_index` derives a key per `IndexPurpose` (`homeowner-phone|vN`); `rotate_batch` moves rows and digests to the active versions while both stay readable.
- PRIV-003, PRIV-004: `vault::read` writes one `ops.audit_log` row per field (`vault_read.<reader>.<purpose>`) and the hourly count in the read's transaction, before returning plaintext; `models::GRANTS` lists each reader, purpose and the fields it may open (D-093, D-749); the phone is never released (PICK-023).
- SEC-030, SEC-046: proofs have private fields and no `Clone` or `Default`; the mints are not `Clone` and each is taken once per process (`AtomicBool`, a second call panics).
- SEC-047: `vault::refuse` alerts at once on a read outside every grant; `check_read_rates` alerts once per hour and action above 3 times the 14-day baseline, floored at 1 read an hour (D-750).

## Never break

- A sealed value copied to another row or column never opens: `tests/integration/tests/vault.rs::a_sealed_value_opens_only_in_its_own_place`.
- No contact-field column outside `privacy`: `no_column_outside_privacy_holds_a_contact_field`.
- Every plaintext read is audited with reader, purpose and field: `every_plaintext_read_is_audited_with_reader_and_purpose`.
- The vault keys are separate from each other and from the database credential: `the_vault_keys_are_separate_from_the_database_credential`.
- Only the readers of `docs/architecture.md` section 8 name a read function: `tests/security/tests/vault_readers.rs`.
- A homeowner session reads only its own record; a release never opens the phone: `src/api.rs` `realm_tests`.
- No `Debug` on `Vault`; every plaintext is a `SecretString`; `VaultError` carries no value (PRIV-016).
- Only `realm_tests::the_session_mint_is_taken_once` calls `session_mint()` in this crate's unit tests, and only T192's test calls `release_mint()` (D-751).

## How to change

- Add a reader or purpose: a `Reader`/`Purpose` variant, a `GRANTS` entry with its fields and a `vault_read.<reader>.<purpose>` action, `docs/architecture.md` section 8, and the scan's `READERS` in `tests/security/tests/vault_readers.rs`; a realm read takes its proof type, never a bare reader.
- Rotate a key: add `2:<base64>` before `1:…` in the variable, run `rotate_batch` until it returns 0, then drop `1:…` (`the_rotation_drill_moves_every_row_to_v2`).
- Add a contact field: a sealed `bytea` column through a new migration (`-- privacy (T###)` first line, adding it to `oldest_key_version`), a `Field` variant and its `ContactDetails` slot, and the grants that may open it.
- After changing a query, regenerate `.sqlx/` (`cargo sqlx prepare --workspace -- --all-targets --all-features`).
- Run `cargo test -p askhava-privacy`, `cargo test -p askhava-integration --test vault`, `cargo test -p askhava-security --test vault_readers` and `bash scripts/smoke.sh`.

## Tests that guard it

- `tests/integration/tests/vault.rs` (TD-PRIV-01, TD-PRIV-09 rotation cases, TD-SEC-26 vault cases).
- `tests/security/tests/vault_readers.rs` (TD-PRIV-01 reader case, TD-SEC-25 business-realm case).
- `src/api.rs` `realm_tests` (TD-SEC-25 own-record case, SEC-030, SEC-046).
- Compile-fail cases: T223 (session proofs), T192 (release proof and the release-mint once test); architecture scan for the mint names: `tests/integration/tests/architecture.rs` (TD-SEC-10).
