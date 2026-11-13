# askhava-privacy: the contact vault

## Purpose
The layer-2 crate that holds every homeowner contact detail (name, phone, email, street address, on-site contact), each sealed on its own under `VAULT_KEY`, and the only code that opens one (PRIV-001, PRIV-002). Every other record refers to a contact by its ID. Plaintext leaves only through `api`, each read audit-logged with its reader and purpose (PRIV-003, PRIV-004);

## Never break
- A sealed value copied to another row or column never opens: `tests/integration/tests/vault.rs::a_sealed_value_opens_only_in_its_own_place`.
- No contact-field column outside `privacy`: `no_column_outside_privacy_holds_a_contact_field`.
- Every plaintext read is audited with reader, purpose and field: `every_plaintext_read_is_audited_with_reader_and_purpose`.
- The vault keys are separate from each other and from the database credential: `the_vault_keys_are_separate_from_the_database_credential`.
- Only the readers of `docs/architecture.md` section 8 name a read function: `tests/security/tests/vault_readers.rs`.
- A homeowner session reads only its own record; a release never opens the phone: `src/api.rs` `realm_tests`.
- No `Debug` on `Vault`; every plaintext is a `SecretString`; `VaultError` carries no value (PRIV-016).
- Only `realm_tests::the_session_mint_is_taken_once` calls `session_mint()` in this crate's unit tests, and only T192's test calls `release_mint()` (D-751).
- A photo's bytes leave `open_link` only after its open audit committed, never inside a caller's transaction: `tests/integration/tests/photo_open_audit.rs` (TD-PRIV-17).
- A sealed photo opens only under its own job and photo ID, the bucket holds no readable photo, and an expired link opens nothing: `src/tests/photo_checks.rs`, `tests/integration/tests/photo_links.rs`.
- No crate but `privacy` builds `ScrubbedText`: `tests/security/tests/scrubbed_text_compile_fail.rs`.
- An opt-out phrase after a stated name survives the scrub, and `NEVER_A_NAME` holds every word of messaging's opt-out, HELP and START lists: `tests/integration/tests/optout_after_name.rs` (TD-CON-32). `privacy` may not depend on `messaging`, so a word added there is added here.
- The corpus keeps at least 200 cases and 99% recall: `src/tests/scrub.rs::the_corpus_scores_at_least_99_percent_recall`.
- "I'm having a leak" keeps every word; short commands survive: `src/tests/scrub.rs`.
- An inbound email failing SPF, DKIM or DMARC, or without an auth block, may only opt out (no HELP or START flag); a bad or stale signature is refused: `tests/integration/tests/read_inbound.rs`.
- A purged photo's object goes after its rows, an orphan only over a day old, never an object a row names or one outside `photos/`: `tests/integration/tests/retention_deletes.rs` (TD-PRIV-20).
- A hold placement or release always takes the shared retention key first, and nothing in `privacy` takes the exclusive key except `lock_retention_for_purge`; never take it after a homeowner or contact row lock: `tests/integration/tests/retention_purge_races.rs` (TD-PRIV-18).
- `privacy.message_body` stores only a `ScrubbedText`, is read only by `problem_words` with an `OwnerSessionProof` (no business or homeowner read), and loses rows after 30 days and at the job's text stage: `tests/integration/tests/check_in_replies.rs` (TD-JOB-12).
- Only `privacy` and `server` name `read_business_mailbox` or `read_staff_phone`, a phone opens only under `SignInCode`, and `business_contacts.rs` seals only through `BusinessTable`: `tests/security/tests/business_contact_readers.rs`, `tests/integration/tests/business_contacts.rs` (TD-PRIV-28).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/privacy/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/privacy/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
- [JOB-016 TXT-020 PRIV-006 PRIV-008] (owns) `message_body.rs`: `keep_problem_words` (one row per inbox row), `problem_words` (owner proof), the daily `privacy.cut_problem_words` (30 days) and `run_words_purge` (text stage, in `server`'s purge step) (D-3521, D-3522).
- [SEC-004 PRIV-002 PRIV-026 SEC-047] (owns) `business_contacts.rs` (T412, D-3262): business members' sign-in addresses (`privacy.business_mailbox`, by mailbox ID, found by the business `email_index` digest) and texting phones (`privacy.staff_phone`, by line ID), outside `privacy.contact`, `Field` and `GRANTS`; reads audited as `vault_read.server.<purpose>`.
- [PRIV-024] (change) Rotation drill: run `rotate_business_batch` beside `rotate_batch` until both return 0, then drop the old key versions (`td_priv_28_a_key_rotation_keeps_every_value_readable_and_findable`); `DuplicateAddress` means two mailboxes hold one address: delete or merge one, run again (D-3517).
- [PRIV-003 SEC-047] (rules) `reseal` re-indexes a mailbox in a savepoint: a `DuplicateAddress` skip rolls back only the move and still commits its rotation audit row and read count (D-3518).
