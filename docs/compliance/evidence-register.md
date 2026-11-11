# Evidence register

**Status:** started (D-2740). One row per piece of evidence. Rows hold **pointers only**: a repository path at a commit SHA, a CI run number, a `kit/state.json` field, or an opaque label (`PRIV-EV-###`) for an item kept in the owner's private, access-controlled evidence store. Never paste the evidence itself if it contains a secret, a fingerprint, an account ID, personal data, a confidential vendor report or incident detail (`README.md`).

**Kinds.** `design` — a spec, plan or document says what should happen. `test` — an automated test passed in the build environment. `operating` — the control was observed working in the test or live environment, or a human record shows it was carried out. Only `operating` rows can move a control in `soa.md` past "Designed, CI-tested".

**Names Gate C reads (D-3080).** A row that closes owner action *n* in `owner-actions.md` names it `OA-<n>` in its Control cell (for example `OA-3 RTO and RPO set`), and the real-phone rehearsal's alert row is `GC-ALERT`. Gate C (T145) opens only when every P0 item 1 to 14 has an `operating` `OA-<n>` row with a date, who recorded it and a pointer, and accepts "go live" only once `GC-ALERT` is recorded (`scripts/gate_c_ready.py`, T395).

## Template

| ID | Control (SoA ref) | Kind | Pointer (path@SHA, run, label) | Environment | Result | Date | Recorded by | Reviewer | Follow-up |
|---|---|---|---|---|---|---|---|---|---|

## Entries

| ID | Control (SoA ref) | Kind | Pointer | Environment | Result | Date | Recorded by | Reviewer | Follow-up |
|---|---|---|---|---|---|---|---|---|---|
| E-001 | 8.25, 8.28, 8.29, 8.33 | test | `claude/kit-state` `kit/nightly.json`: `nightly-main` on `e47432249e16` (Ci, Twin, Ally, named mutants: success) | CI (fake data) | Pass | 2026-11-10 | Claude (D-2740) | — | Next nightly on current `main` |
| E-002 | 8.25, 8.32 | test | `kit/state.json` `stage_proofs` 1 (`5d45d8ef0bee`), 2 (`2ed9f1f259de`), 3 (`ea22387162b9`) | CI (fake data) | Stage exits recorded | 2026-10-30 to 2026-11-06 | runner / overseer | — | Stage 4 to 6 proofs as they land |
| E-003 | 5.3, 8.4, 8.32 | design | `kit/state.json` `ruleset_verified: true`; `docs/governance.md@f8231346` section 2 | GitHub | Ruleset `protect-main` verified by the runner | 2026-11-11 (read) | Claude (D-2740) | — | Open items E-1, E-2 in governance s.2; N-01 |
| E-004 | 8.24, 5.34 | test | `app/privacy/src/vault.rs@f8231346`; `tests/integration/tests/vault.rs@f8231346` | CI (fake data) | Covered by E-001 | 2026-11-10 | Claude (D-2740) | — | TD-PRIV-09 on test project (N-04 if needed); T354 |
| E-005 | 8.15, 8.16, 5.28 | test | `app/server/src/offdb_stream.rs@f8231346`; `tests/integration/tests/offdb_stream.rs@f8231346` | CI (fake data) | Covered by E-001 | 2026-11-10 | Claude (D-2740) | — | Real bucket at T134 |
| E-006 | 8.12, 5.34, 8.15 | test | `tests/security/tests/sentinel_db_logs.rs@f8231346` (T217) | CI (fake data) | Covered by E-001 | 2026-11-10 | Claude (D-2740) | — | T120; N-02 |
| E-007 | 5.15, 8.3, 8.5 | test | `tests/security/tests/auth_matrix.rs`, `owner_auth.rs`, `cookies_csrf.rs`, `rate_limits.rs` `@f8231346` | CI (fake data) | Covered by E-001 | 2026-11-10 | Claude (D-2740) | — | T348; hosted run T136, T138 |
| E-008 | 8.21 | test | `tests/security/tests/webhook_forgery.rs`, `replay.rs`, `idempotency_keys.rs` `@f8231346` | CI (fake data) | Covered by E-001 | 2026-11-10 | Claude (D-2740) | — | T132 against Stripe test mode |
| E-009 | 5.21, 8.8, 5.32 | design | `.github/workflows/ci.yml@f8231346` lines 73–113 (gitleaks, PII scan), 290–307 (cargo-audit, cargo-deny, npm audit) | CI | Run on every `Ci` | 2026-11-11 (read) | Claude (D-2740) | — | T345 pins the 10 remaining tag-pinned `uses:` lines |
| E-010 | 8.10 | test | `tests/integration/tests/retention_purge.rs`, `retention_deletes.rs`, `retention_purge_races.rs` `@f8231346` | CI (fake data) | Covered by E-001 | 2026-11-10 | Claude (D-2740) | — | Vendor side: N-02 |
| E-011 | 5.35, 8.29 | design | `docs/reviews/2026-11-10-pentest-dispositions.md@f8231346` (source-assisted assessment, not a runtime test) | — | Findings PT-01 to PT-04 dispositioned | 2026-11-10 | overseer | — | External runtime test (TECH-15) |

Release records (A-02), restore records (T139, A-01), access reviews, supplier reviews, MFA attestations and incident drills are added here as `operating` rows when they happen.
