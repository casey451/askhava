# askhava-privacy: the contact vault

## Purpose

The layer-2 crate that holds every homeowner contact detail (name, phone, email, street address, on-site contact), each sealed on its own under `VAULT_KEY`, and the only code that opens one (PRIV-001, PRIV-002). Every other record refers to a contact by its ID. Plaintext leaves only through `api`, each read audit-logged with its reader and purpose (PRIV-003, PRIV-004); the release and portal-realm reads need proof types only this crate's once-per-process mints make (SEC-030, SEC-046). It is also the only code that reads raw homeowner text (the inbound reader) and the only maker of `ScrubbedText`, the one text type `ai` accepts (AI-002 to AI-004, AI-020, PRIV-026). T022 built the vault, T023 the reader and scrubber, T090 the photo checks, seal and links; T092 the retention holds, the photo and contact purges and "Delete my data".

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the public surface: re-exports, the capability types `ReleaseProof`, `ReleaseBasis`, `ReleaseMint` (`mint(pick_id, contact_id, basis)`), `release_mint()`, `HomeownerSessionProof`, `OwnerSessionProof`, `BusinessSessionProof` (T253: `person`, `business`), `SessionMint` (`homeowner`, `owner`, `business`), `session_mint()`; the reads `read_contact`, `read_for_release`, `read_for_pick_code` (T246), `read_own_contact`, `read_for_owner`, and the re-export of `read_released` (T253); the inline `realm_tests` (D-751).
- `src/scrub.rs`: `ScrubbedText` (`as_str`), `scrub`, `CommonWords`, `DEFAULT_COMMON_WORDS`, `Known`, `NameMemory`, `REMOVED`, `is_format` (the one Cf list, which `ai`'s screen also uses, D-1362); the crate-private `normalize`.
- `src/reader.rs`: `read_inbound`, `InboundWords`, `Flags`, `Inbound`; `read_inbound_email`, `SignedDelivery`, `EmailContext`, `InboundEmail`, `InboundError`, `EmailAuth`, `Standing`, `SenderIndex`, `email_index`, `WEBHOOK_TOLERANCE_SECS`.
- `src/models.rs`: `VaultError`, `Field`, `Reader`, `Purpose`, `OwnerPurpose`, `Grant`, `GRANTS`, `grant_for`, `NewContact`, `ContactDetails`, `IndexPurpose`, `normalize_phone`.
- `src/photos.rs` (T090): `sniff`, `clean`, `CleanPhoto`, `PhotoKind`, `PhotoSeal`, `store_photo`, `issue_link`, `photo_path`, `open_link`; the limits `MAX_PHOTO_BYTES`, `MAX_SIDE`, `MAX_PIXELS`, `MAX_ALLOC`, `DECODE_PERMITS`, `DECODE_TIMEOUT`, `LINK_TTL_SECS` (`PhotoError` is in `models.rs`).
- Migration `0125_privacy_photos.sql` (T090): `privacy.photo` (job, key version, storage name; never the bytes) and `privacy.photo_link` (token SHA256, photo, expiry); SELECT and INSERT only.
- `src/purge.rs` (T092): `place_hold`, `release_hold`, `is_held`, `purge_photos`, `purge_contact`, `delete_my_data`, `DELETE_CONFIRMATION`. `src/handlers.rs` (T092): `run_purge_due`, `run_purge_due_after` (a re-check that just queued complaint events waits an hour more), `run_complaint_hold`, `purge_target`, `RECHECK_KIND`, `PURGE_KIND`. `HoldSource` and `PurgeStage` are in `models.rs`.
- Migration `0127_privacy_retention_purge.sql` (T092): `privacy.retention_hold` (only `released_at` updatable) and DELETE on `privacy.photo` and `privacy.photo_link`.
- `src/readback_reader.rs` (T077): `read_for_readback` (`Readback`: `first_name`, `street`), the CALL-005 read under `(Calls, CallReadback)`; only `calls` calls it (D-1686).
- `src/released_reader.rs` (T253, D-1739): `read_released(db, vault, &BusinessSessionProof, (pick, contact), fields)` under `(Picks, ReleasedView)`; only `picks`' released view calls it.
- `src/vault.rs`: `Vault` (`new`, `seal_version`, `index_version`, `blind_index`), `store_contact`, `find_by_phone`, `rotate_batch`, `check_read_rates`, the crate-private `read` and `refuse`; `RATE_MULTIPLE`, `BASELINE_HOURS`, `BASELINE_FLOOR`.
- Migration `0036_privacy_vault.sql`: the `privacy` schema and grants, `privacy.sealed_version()`, `privacy.contact` (sealed columns, generated `oldest_key_version`), `privacy.contact_index` (blind index), `privacy.read_count` (D-749).
- `sqlx.toml`: `'TIMESTAMPTZ'` reads as `jiff_sqlx::Timestamp`.

## Rules it enforces

- PRIV-001: contact fields exist only as `privacy.contact` columns; `store_contact` is the way in.
- PRIV-002, PRIV-024: `Vault::seal`/`open` (AES-GCM, 256-bit key, random nonce, key version and associated data `privacy.contact|<column>|<id>|v<version>`); `Vault::new` refuses keys shared between `VAULT_KEY` and `VAULT_INDEX_KEY` or inside the database credential, plain, percent-encoded, unpadded or URL-safe (D-752); a blind-index unique violation from a race is `PhoneTaken`; only `rotate_batch` locks a row; `blind_index` derives a key per `IndexPurpose` (`homeowner-phone|vN`); `rotate_batch` moves rows and `homeowner-phone` digests (the only purpose `reseal` rebuilds, D-753) to the active versions while both stay readable.
- PRIV-003, PRIV-004: `vault::read` writes one `ops.audit_log` row per field (`vault_read.<reader>.<purpose>`) and the hourly count in the read's transaction, before returning plaintext, and refuses (`InTransaction`) on a connection already inside a transaction, by sqlx's counter or a raw `BEGIN` (`ensure_bare`'s transaction-local probe, D-753); `read_for_release` consumes its proof, opens only the contact the proof names, and adds a row with subject `picks.pick` and the proof's pick ID (D-752, D-753); `models::GRANTS` lists each reader, purpose and the fields it may open (D-093, D-749); the phone is never released (PICK-023).
- PICK-013, PRIV-003, D-1602: `read_for_pick_code(db, vault, contact)` opens only the phone under `(Reader::Picks, Purpose::PickCode)` (`vault_read.picks.pick_code`), audited and counted like every read and refused inside a transaction; only `picks` may name it (`vault_readers.rs`).
- PICK-024, PRIV-003, PRIV-005, SEC-046, D-1739, D-1891, D-1893: `read_released` opens name, address and email (the `(Picks, ReleasedView)` grant, `vault_read.picks.released_view`, no phone) of the contact `picks` passes for a released pick, one row per field with `Actor::Business(person)` and one with subject `picks.pick`; refused inside a transaction; a contact whose name, email and address the purge cleared returns `None` and writes no row. `picks` makes the business and role checks first; only `picks` may name it (`vault_readers.rs`).
- CALL-005, D-026, D-1686: `read_for_readback` returns only the first word of the name and the address before its first comma; the full name and the town never leave the crate.
- SEC-030, SEC-046: proofs have private fields and no `Clone` or `Default`; the mints are not `Clone` and each is taken once per process (`AtomicBool`, a second call panics).
- SEC-047: `vault::refuse` refuses a connection inside a transaction first (D-753), then alerts at once on a read outside every grant, subject kind `vault_read.<reader>.<purpose>` and the contact (D-752); `check_read_rates` rounds its time down to the UTC hour and alerts once per hour and action above 3 times the 14-day baseline, floored at 1 read an hour (D-750, D-753).
- AI-002, AI-003, AI-020: `scrub::scrub` (NFKC, Cf stripped, other-script digits; names after the trigger phrases unless the first word is common, sign-offs, digit runs of 7+ in digits, number words, compound and "double" forms and "o"/"l" look-alikes, emails, spelled emails, handles, links, bare domains, streets, house numbers before a capitalised name, gate codes, every vault-name word and every remembered word of a stated name; D-800, D-803).
- PRIV-013, PRIV-025: `sniff` reads type and size from the bytes (JPEG, PNG, WebP; over 15 MiB `TooLarge`; HEIC `Heic` by the `ftyp` major or any compatible brand within the box, never decoded, answered by the caller with the PRIV-025 reply, T212's template, D-1792; anything else `NotAPhoto`); `clean` waits for one of 2 decode permits and decodes in `spawn_blocking`, both inside one 20-second limit (a stuck decode makes later uploads `Busy`, never a queue), under 8,000 px a side, 40 megapixels and a 256 MiB budget for the pixel buffers it allocates (`peak_bytes`: the frame plus the larger of a quarter-turn copy and the RGB8 copy), all checked from the header before any pixel is decoded (D-1793).
- PRIV-014: `clean` keeps the primary frame, applies the orientation and writes a fresh JPEG, so no metadata or appended bytes survive; only a `CleanPhoto` is stored, screened, sent to AI or carded, and the upload bytes are never stored; photos are served only by `open_link` on a live `/photos/<token>` link (10 minutes, token in the path, PRIV-028).
- PRIV-023: `PhotoSeal` reads `VAULT_KEY` itself (`vault.rs` keeps its keys private, D-1789) but skips `Vault::new`'s key-separation checks (D-752), so it must be built from the same `VAULT_KEY` value a `Vault` has already accepted; it seals with associated data `privacy.photo|<job>|<photo>|v<version>` before the `storage` adapter sees the bytes.
- PRIV-007 to PRIV-009, PRIV-011 (D-079, D-491, D-1799 to D-1803): `purge_contact` clears name, email, address and on-site contact and the email index, keeps the sealed phone and its homeowner-phone index, and audits `contact_purged` once; `purge_photos` deletes a job's photo rows (links cascade; the objects stay until `adapters` can delete, D-1803); `run_purge_due` deletes photos at once, makes the contact stage wait an hour (`RECHECK_KIND`) and then daily while `is_held`, and hands the contact and text stages to `intake` as `PURGE_KIND`; a release stored before its hold wins; `delete_my_data` runs in one transaction and returns the PRIV-011 sentence. Nothing here reads another module's table.
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
- A sealed photo opens only under its own job and photo ID, the bucket holds no readable photo, and an expired link opens nothing: `src/tests/photo_checks.rs`, `tests/integration/tests/photo_links.rs`.
- No crate but `privacy` builds `ScrubbedText`: `tests/security/tests/scrubbed_text_compile_fail.rs`.
- The corpus keeps at least 200 cases and 99% recall: `src/tests/scrub.rs::the_corpus_scores_at_least_99_percent_recall`.
- "I'm having a leak" keeps every word; short commands survive: `src/tests/scrub.rs`.
- An inbound email failing SPF, DKIM or DMARC, or without an auth block, may only opt out (no HELP or START flag); a bad or stale signature is refused: `tests/integration/tests/read_inbound.rs`.

## How to change

- Purge something new at a stage: the module that owns the rows handles `PURGE_KIND` for that stage (register it in `register_purge_handlers`); a new hold source is a `HoldSource` variant, the migration's `CHECK` and a step the composition runs before `run_purge_due` (D-1799).

- Add a reader or purpose: a `Reader`/`Purpose` variant, a `GRANTS` entry with its fields and a `vault_read.<reader>.<purpose>` action, `docs/architecture.md` section 8, and the scan's `READERS` in `tests/security/tests/vault_readers.rs`; a realm read takes its proof type, never a bare reader.
- Rotate a key: add `2:<base64>` before `1:…` in the variable, run `rotate_batch` until it returns 0, then drop `1:…` (`the_rotation_drill_moves_every_row_to_v2`).
- Add a contact field: a sealed `bytea` column through a new migration (`-- privacy (T###)` first line, adding it to `oldest_key_version`), a `Field` variant and its `ContactDetails` slot, and the grants that may open it.
- After changing a query, regenerate `.sqlx/` (`cargo sqlx prepare --workspace -- --all-targets --all-features`).
- Scrubber miss found: add the case to `CONTACTS` in `src/tests/scrub.rs`, then the pattern in `scrub.rs` (one `mark_*` pass per kind); a new trigger phrase or street suffix is a line in its list; never lower the 99% bar.
- Serve photos: the route that calls `open_link` puts the token in the path, uses `MAX_PHOTO_BYTES` as its body limit on upload, answers `LinkDead` with 404 and `no_store`, and logs the route template only (D-1791); it builds `PhotoSeal::new` from the same `VAULT_KEY` its `Vault::new` validated, since `PhotoSeal` repeats none of those checks. `store_photo` writes the object before its row, so a failed insert leaves an orphan `photos/<id>` ciphertext with no row: the photo purge (PRIV-007) must list the bucket's `photos/` objects, not only the table (D-1793).
- Inbound email payload differs from D-802's assumed shape: change only the private `Envelope`/`Received`/`Results` types in `reader.rs` and the test bodies in `read_inbound.rs`.
- Run `cargo test -p askhava-privacy`, `cargo test -p askhava-integration --test read_inbound`, `cargo test -p askhava-security --test scrubbed_text_compile_fail`, `cargo test -p askhava-integration --test vault`, `cargo test -p askhava-security --test vault_readers` and `bash scripts/smoke.sh`.

## Tests that guard it

- `tests/integration/tests/vault.rs` (TD-PRIV-01, TD-PRIV-09 rotation cases, TD-SEC-26 vault cases).
- `tests/security/tests/vault_readers.rs` (TD-PRIV-01 reader case, TD-SEC-25 business-realm case).
- `src/api.rs` `realm_tests` (TD-SEC-25 own-record case, SEC-030, SEC-046).
- `tests/integration/tests/retention_purge.rs` (TD-PRIV-03: stages at 30, 180 and 1,096 days, holds on another job, requests with no job, phone kept with consent, dispute and complaint holds, photos and job text, delete my data).
- `src/tests/photo_checks.rs` (TD-PRIV-04, TD-PRIV-08, TD-PRIV-10 unit cases), `tests/integration/tests/photo_links.rs` (TD-PRIV-04, TD-PRIV-08, TD-PRIV-13 photo-link cases).
- `src/tests/scrub.rs`, `src/tests/prop_scrub_props.rs` (TD-AI-01, TD-AI-05, TD-AI-09); `tests/integration/tests/read_inbound.rs` (TD-PRIV-11 reader cases); `tests/security/tests/scrubbed_text_compile_fail.rs` (TD-AI-01 compile-fail).
- Compile-fail cases: T223 (session proofs), T253 (`tests/security/ui/forge_session_proof_business.rs`, the business session proof), T192 (release proof and the release-mint once test); architecture scan for the mint names: `tests/integration/tests/architecture.rs` (TD-SEC-10).
