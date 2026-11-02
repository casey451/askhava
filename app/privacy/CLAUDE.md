# askhava-privacy: the contact vault

## Purpose

The layer-2 crate that holds every homeowner contact detail (name, phone, email, street address, on-site contact), each sealed on its own under `VAULT_KEY`, and the only code that opens one (PRIV-001, PRIV-002). Every other record refers to a contact by its ID. Plaintext leaves only through `api`, each read audit-logged with its reader and purpose (PRIV-003, PRIV-004); the release and portal-realm reads need proof types only this crate's once-per-process mints make (SEC-030, SEC-046). It is also the only code that reads raw homeowner text (the inbound reader) and the only maker of `ScrubbedText`, the one text type `ai` accepts (AI-002 to AI-004, AI-020, PRIV-026). T022 built the vault, T023 the reader and scrubber; photos and purges are later tasks'.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the public surface: re-exports, the capability types `ReleaseProof`, `ReleaseBasis`, `ReleaseMint` (`mint(pick_id, contact_id, basis)`), `release_mint()`, `HomeownerSessionProof`, `OwnerSessionProof`, `SessionMint` (`homeowner`, `owner`), `session_mint()`; the reads `read_contact`, `read_for_release`, `read_own_contact`, `read_for_owner`; the inline `realm_tests` (D-751).
- `src/scrub.rs`: `ScrubbedText` (`as_str`), `scrub`, `CommonWords`, `DEFAULT_COMMON_WORDS`, `Known`, `NameMemory`, `REMOVED`, `is_format` (the one Cf list, which `ai`'s screen also uses, D-1362); the crate-private `normalize`.
- `src/reader.rs`: `read_inbound`, `InboundWords`, `Flags`, `Inbound`; `read_inbound_email`, `SignedDelivery`, `EmailContext`, `InboundEmail`, `InboundError`, `EmailAuth`, `Standing`, `SenderIndex`, `email_index`, `WEBHOOK_TOLERANCE_SECS`.
- `src/models.rs`: `VaultError`, `Field`, `Reader`, `Purpose`, `OwnerPurpose`, `Grant`, `GRANTS`, `grant_for`, `NewContact`, `ContactDetails`, `IndexPurpose`, `normalize_phone`.
- `src/vault.rs`: `Vault` (`new`, `seal_version`, `index_version`, `blind_index`), `store_contact`, `find_by_phone`, `rotate_batch`, `check_read_rates`, the crate-private `read` and `refuse`; `RATE_MULTIPLE`, `BASELINE_HOURS`, `BASELINE_FLOOR`.
- Migration `0036_privacy_vault.sql`: the `privacy` schema and grants, `privacy.sealed_version()`, `privacy.contact` (sealed columns, generated `oldest_key_version`), `privacy.contact_index` (blind index), `privacy.read_count` (D-749).
- `sqlx.toml`: `'TIMESTAMPTZ'` reads as `jiff_sqlx::Timestamp`.

## Rules it enforces

- PRIV-001: contact fields exist only as `privacy.contact` columns; `store_contact` is the way in.
- PRIV-002, PRIV-024: `Vault::seal`/`open` (AES-GCM, 256-bit key, random nonce, key version and associated data `privacy.contact|<column>|<id>|v<version>`); `Vault::new` refuses keys shared between `VAULT_KEY` and `VAULT_INDEX_KEY` or inside the database credential, plain, percent-encoded, unpadded or URL-safe (D-752); a blind-index unique violation from a race is `PhoneTaken`; only `rotate_batch` locks a row; `blind_index` derives a key per `IndexPurpose` (`homeowner-phone|vN`); `rotate_batch` moves rows and `homeowner-phone` digests (the only purpose `reseal` rebuilds, D-753) to the active versions while both stay readable.
- PRIV-003, PRIV-004: `vault::read` writes one `ops.audit_log` row per field (`vault_read.<reader>.<purpose>`) and the hourly count in the read's transaction, before returning plaintext, and refuses (`InTransaction`) on a connection already inside a transaction, by sqlx's counter or a raw `BEGIN` (`ensure_bare`'s transaction-local probe, D-753); `read_for_release` consumes its proof, opens only the contact the proof names, and adds a row with subject `picks.pick` and the proof's pick ID (D-752, D-753); `models::GRANTS` lists each reader, purpose and the fields it may open (D-093, D-749); the phone is never released (PICK-023).
- SEC-030, SEC-046: proofs have private fields and no `Clone` or `Default`; the mints are not `Clone` and each is taken once per process (`AtomicBool`, a second call panics).
- SEC-047: `vault::refuse` refuses a connection inside a transaction first (D-753), then alerts at once on a read outside every grant, subject kind `vault_read.<reader>.<purpose>` and the contact (D-752); `check_read_rates` rounds its time down to the UTC hour and alerts once per hour and action above 3 times the 14-day baseline, floored at 1 read an hour (D-750, D-753).
- AI-002, AI-003, AI-020: `scrub::scrub` (NFKC, Cf stripped, other-script digits; names after the trigger phrases unless the first word is common, sign-offs, digit runs of 7+ in digits, number words, compound and "double" forms and "o"/"l" look-alikes, emails, spelled emails, handles, links, bare domains, streets, house numbers before a capitalised name, gate codes, every vault-name word and every remembered word of a stated name; D-800, D-803).
- AI-004: `ScrubbedText`'s field is private and it has no `Default`, `From` or public constructor.
- PRIV-026, D-093: `reader::read_inbound` (flags on the raw text, then the scrub) and `read_inbound_email` (Svix signature and 5-minute window, `OptOutOnly` unless SPF, DKIM and DMARC all pass, a missing auth block failing, HELP and START cleared for `OptOutOnly`, an opt-out word alone on the first line opting out, `business|`/`homeowner|` email blind index; D-802, D-804). It does not deduplicate: callers must drop a repeated `svix-id` before calling `read_inbound_email` (D-804). Raw body storage waits for `privacy.message_body` (D-801).

## Never break

- A sealed value copied to another row or column never opens: `tests/integration/tests/vault.rs::a_sealed_value_opens_only_in_its_own_place`.
- No contact-field column outside `privacy`: `no_column_outside_privacy_holds_a_contact_field`.
- Every plaintext read is audited with reader, purpose and field: `every_plaintext_read_is_audited_with_reader_and_purpose`.
- The vault keys are separate from each other and from the database credential: `the_vault_keys_are_separate_from_the_database_credential`.
- Only the readers of `docs/architecture.md` section 8 name a read function: `tests/security/tests/vault_readers.rs`.
- A homeowner session reads only its own record; a release never opens the phone: `src/api.rs` `realm_tests`.
- No `Debug` on `Vault`; every plaintext is a `SecretString`; `VaultError` carries no value (PRIV-016).
- Only `realm_tests::the_session_mint_is_taken_once` calls `session_mint()` in this crate's unit tests, and only T192's test calls `release_mint()` (D-751).
- No crate but `privacy` builds `ScrubbedText`: `tests/security/tests/scrubbed_text_compile_fail.rs`.
- The corpus keeps at least 200 cases and 99% recall: `src/tests/scrub.rs::the_corpus_scores_at_least_99_percent_recall`.
- "I'm having a leak" keeps every word; short commands survive: `src/tests/scrub.rs`.
- An inbound email failing SPF, DKIM or DMARC, or without an auth block, may only opt out (no HELP or START flag); a bad or stale signature is refused: `tests/integration/tests/read_inbound.rs`.

## How to change

- Add a reader or purpose: a `Reader`/`Purpose` variant, a `GRANTS` entry with its fields and a `vault_read.<reader>.<purpose>` action, `docs/architecture.md` section 8, and the scan's `READERS` in `tests/security/tests/vault_readers.rs`; a realm read takes its proof type, never a bare reader.
- Rotate a key: add `2:<base64>` before `1:…` in the variable, run `rotate_batch` until it returns 0, then drop `1:…` (`the_rotation_drill_moves_every_row_to_v2`).
- Add a contact field: a sealed `bytea` column through a new migration (`-- privacy (T###)` first line, adding it to `oldest_key_version`), a `Field` variant and its `ContactDetails` slot, and the grants that may open it.
- After changing a query, regenerate `.sqlx/` (`cargo sqlx prepare --workspace -- --all-targets --all-features`).
- Scrubber miss found: add the case to `CONTACTS` in `src/tests/scrub.rs`, then the pattern in `scrub.rs` (one `mark_*` pass per kind); a new trigger phrase or street suffix is a line in its list; never lower the 99% bar.
- Inbound email payload differs from D-802's assumed shape: change only the private `Envelope`/`Received`/`Results` types in `reader.rs` and the test bodies in `read_inbound.rs`.
- Run `cargo test -p askhava-privacy`, `cargo test -p askhava-integration --test read_inbound`, `cargo test -p askhava-security --test scrubbed_text_compile_fail`, `cargo test -p askhava-integration --test vault`, `cargo test -p askhava-security --test vault_readers` and `bash scripts/smoke.sh`.

## Tests that guard it

- `tests/integration/tests/vault.rs` (TD-PRIV-01, TD-PRIV-09 rotation cases, TD-SEC-26 vault cases).
- `tests/security/tests/vault_readers.rs` (TD-PRIV-01 reader case, TD-SEC-25 business-realm case).
- `src/api.rs` `realm_tests` (TD-SEC-25 own-record case, SEC-030, SEC-046).
- `src/tests/scrub.rs`, `src/tests/prop_scrub_props.rs` (TD-AI-01, TD-AI-05, TD-AI-09); `tests/integration/tests/read_inbound.rs` (TD-PRIV-11 reader cases); `tests/security/tests/scrubbed_text_compile_fail.rs` (TD-AI-01 compile-fail).
- Compile-fail cases: T223 (session proofs), T192 (release proof and the release-mint once test); architecture scan for the mint names: `tests/integration/tests/architecture.rs` (TD-SEC-10).
