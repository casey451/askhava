# Reconciliation of the ISO 27001 readiness gap analysis

**Status:** draft for owner review (D-2740). **Input:** "Ask Hava — ISO 27001 Security Readiness and Codebase Gap Analysis", prepared November 10, 2026 against `main` `f8231346`, shared by Casey on November 11, 2026. The report itself is not stored in the repository; it was read as data. **Checked against:** `main` `f8231346` (unchanged since the report), `kit/tasks.json` on `main` (362 tasks), the open plan branch `origin/claude/plan-review-fixes-3` (T368 to T374, **proposed, PR open**), and live `claude/kit-state` `kit/state.json` updated `2026-11-11T01:18:35Z` (207 DONE, 168 QUEUED, 1 IN_PROGRESS (T157), 1 SKIPPED (GATE-A), no MERGE_PENDING). Statuses move; re-read both files before acting on a row.

**Test evidence used.** No test was run for this document. "Verified" below means the control's code and its test exist on `main` and the last recorded nightly run was green: `kit/nightly.json` on `claude/kit-state` records `nightly-main` success (Ci, Twin, Ally, named mutants) on `e4743224` (an ancestor of `main`, 18 commits behind) at `2026-11-10T17:23:48Z`, and stage proofs 1 to 3 are recorded in `kit/state.json` (`stage_proofs`). That is build-environment evidence only: **no row is operating evidence from a live or test-host environment**, because none exists yet (stage 6 is queued).

## Classes

- **EXISTING_AND_VERIFIED** — code and a passing test exist on `main` (build environment only).
- **EXISTING_BUT_NOT_PROVEN** — the design exists; operating evidence is missing (named).
- **ALREADY_QUEUED** — a current task owns it; do not recreate.
- **NEW_TECHNICAL_WORK** — not owned by any task; a task is proposed below (not created).
- **OWNER_ACTION_OR_EXTERNAL** — only Casey, counsel or an outside party can close it.
- **NOT_APPLICABLE** — justified; needs owner approval.

One primary class per finding; secondary owners are in the notes.

## Summary

| Class | Count | IDs |
|---|---|---|
| EXISTING_AND_VERIFIED | 0 | (no finding is fully closed; the report's baseline controls, section "Baseline" below, are verified at code level) |
| EXISTING_BUT_NOT_PROVEN | 6 | ISMS-03, ISMS-07, TECH-07, TECH-21, F-01, F-04 |
| ALREADY_QUEUED | 13 | TECH-03, TECH-04, TECH-05, TECH-06, TECH-08, TECH-09, TECH-11, TECH-13, TECH-14, TECH-19, TECH-20, F-03, F-06 |
| NEW_TECHNICAL_WORK | 2 | TECH-12, TECH-16 (plus N-01 and N-04 as secondary work on TECH-01 and TECH-04) |
| OWNER_ACTION_OR_EXTERNAL | 24 | ISMS-01, 02, 04, 05, 06, 08, 09, 10; TECH-01, 02, 10, 15, 17, 18; OPS-ISMS-01 to 08; F-02, F-05 |
| NOT_APPLICABLE | 1 | TECH-22 (owner approval) |
| **Total** | **46** | |

**Where the report's status differs from what was found:** TECH-03 is mostly queued, not new (SEC-052 and T135 already give the `migrate` service sole hold of the migration URL; `migrations/0110_schema_default_privileges.sql` narrows default privileges). TECH-04's key inventory is already owned (PRIV-024 by T117) and its rotation drill has a test entry (`docs/test-design.md:540`, TD-PRIV-09). TECH-07's tamper alarm already exists in code (`app/server/src/offdb_stream.rs:2`–`8`, `CHAIN_ALERT`, tested in `tests/integration/tests/offdb_stream.rs`). F-03's "make Gate C block without the restore" already holds: Gate C (T145) depends transitively on T109, T206, T139, T138, T140, T120, T121, T345 and T117. TECH-16: `ci.yml` actions are already SHA-pinned and gitleaks is SHA-verified (`.github/workflows/ci.yml:73`–`109`); 15 `uses:` lines across 7 workflows (`backup-runner.yml`, `auto-pr.yml`, `governance.yml`, `inbox.yml`, `nightly-main.yml`, `notify.yml`, `twin.yml`) still follow tags, owned by T345. T350 to T367 are on `main` (merged #439); T368 to T375 are only on the open plan PR #442.

## Baseline controls (report section 2), verified at code level

| Control | Evidence on `main` | Test | Class |
|---|---|---|---|
| Contact vault, AES-256-GCM, versioned keys, audited reads | `app/privacy/src/vault.rs:1`–`13` | `tests/integration/tests/vault.rs`, `tests/security/tests/vault_readers.rs` | EXISTING_AND_VERIFIED (build env) |
| Retention purge and holds | `app/privacy/src/purge.rs:1`–`18` (T092, T274, T276 DONE) | `tests/integration/tests/retention_purge.rs`, `retention_deletes.rs`, `retention_purge_races.rs` | EXISTING_AND_VERIFIED (build env); vendor side not proven |
| Scrubbed logging, Sentry without PII | `app/core/src/logging.rs:1`–`10` (PRIV-016, PRIV-017) | `tests/security/tests/sentinel_db_logs.rs` (T217) | EXISTING_AND_VERIFIED (build env) |
| Audit log and off-database stream with hash chain | `app/ops/src/service.rs:1`–`10`, `app/server/src/offdb_stream.rs:1`–`13` | `tests/integration/tests/offdb_stream.rs` | EXISTING_AND_VERIFIED (build env); real bucket T134 |
| Auth, CSRF, webhooks, idempotency, authorization matrix | `tests/security/tests/` (28 files incl. `auth_matrix.rs`, `cookies_csrf.rs`, `webhook_forgery.rs`, `owner_auth.rs`, `rate_limits.rs`) | same | EXISTING_AND_VERIFIED (build env) |
| CI secret, PII and dependency gates | `.github/workflows/ci.yml:73` (gitleaks), `:113` (PII scan), `:290` (cargo-audit, cargo-deny), `:300` (npm audit) | every `Ci` run; nightly record above | EXISTING_AND_VERIFIED (build env) |
| Change control on the blueprint | `docs/governance.md` sections 1 and 2; `scripts/check_plan_lock.py`; `scripts/check_pr_change.py`; `.github/CODEOWNERS:1`–`20` | `Governance` and `Ci` on every PR | EXISTING_AND_VERIFIED (build env); authorship gap R-14, R-37 |

## A. Management system (ISMS-01 to ISMS-10)

| ID | Class | Evidence and current owner | This PR | What closes it |
|---|---|---|---|---|
| ISMS-01 Scope | OWNER_ACTION_OR_EXTERNAL | None before this PR | `isms-scope.md` draft | Casey approves the scope (decision entry naming the SHA) |
| ISMS-02 Interested parties, obligations | OWNER_ACTION_OR_EXTERNAL | WISP section 4a (`docs/legal/information-security-program.md:40`) is an AI reading, not a legal determination | Listed in `owner-actions.md` | Counsel's dated list of applicable laws, contracts and card/SMS rules; then a register file |
| ISMS-03 Risk method | EXISTING_BUT_NOT_PROVEN | `docs/risk-register.md` (R-IDs, severity, owner, resolution criterion) and `docs/threat-model.md` section 6 (R1 to R20 residual risks); no likelihood/impact scale, appetite or signed acceptance | Method proposed in `isms-scope.md` section 5 | Casey approves the method; the first scored review is recorded (`audit-and-review.md`). Existing R-IDs and history are kept |
| ISMS-04 SoA | OWNER_ACTION_OR_EXTERNAL | None before this PR | `soa.md` skeleton | Casey approves applicability and exclusions |
| ISMS-05 Roles, separation of duties | OWNER_ACTION_OR_EXTERNAL | One person holds every role (`docs/legal/information-security-program.md:10`; OPS-002, `docs/spec/ops-recovery.md:6`; R-11, `docs/risk-register.md:20`) | Roles table in `README.md` | Casey approves the roles and decides on a deputy (`owner-actions.md` P0) |
| ISMS-06 Objectives | OWNER_ACTION_OR_EXTERNAL | None before this PR | `security-objectives.md` draft | Casey approves targets; first monthly scorecard recorded |
| ISMS-07 Document control, evidence index | EXISTING_BUT_NOT_PROVEN | Blueprint versioning by `kit/plan.lock` and the append-only `docs/decisions.md` (machine-checked); policy drafts carry version lines but no approver | `README.md` document-control rule; `evidence-register.md` | First approvals recorded as decisions; evidence register kept current |
| ISMS-08 Internal audit | OWNER_ACTION_OR_EXTERNAL | T125, T140 are AI code audits (QUEUED), not an ISMS audit | `audit-and-review.md` template | An independent person audits the ISMS after controls operate |
| ISMS-09 Management review, corrective action | OWNER_ACTION_OR_EXTERNAL | WISP section 7 yearly note (`docs/legal/information-security-program.md:74`–`76`) | `audit-and-review.md` templates | Casey holds and records the first review |
| ISMS-10 Awareness, competence | OWNER_ACTION_OR_EXTERNAL | WISP section 6 (owner re-reads, `:70`–`72`) | — | Casey's acknowledgement; onboarding rule before anyone else gets access (OPS-ISMS-08) |

## B. Technical and operational controls (TECH-01 to TECH-22)

Task statuses are from live `kit/state.json` (`2026-11-11T01:18:35Z`).

| ID | Class | Existing evidence | Owning tasks (status) | Gap and next step |
|---|---|---|---|---|
| TECH-01 Agent identity vs owner approval | OWNER_ACTION_OR_EXTERNAL | `.github/CODEOWNERS:1`–`20`; R-14 accepted (`docs/risk-register.md:21`); R-37 open: the backup runner's Claude job keeps broad write rights and the push token (`docs/risk-register.md:41`; `docs/for-casey-review.md:27`) | T345 pins actions (QUEUED); no task trims permissions | Casey maps privileges by actor on GitHub, host, Stripe, Twilio. Secondary: **N-01** (proposed) trims the job and runs the negative approval-bypass drill |
| TECH-02 Privileged MFA, recovery, recertification | OWNER_ACTION_OR_EXTERNAL | App side DONE: T086 owner password plus passkey or TOTP, recovery codes (`tests/security/tests/owner_auth.rs`); R-11 criterion (`docs/risk-register.md:20`) | T303 serves owner sign-in (QUEUED) | Casey records vendor-console MFA and recovery (opaque labels only) and a yearly access review |
| TECH-03 Host and DB least privilege | ALREADY_QUEUED | SEC-052 (`docs/spec/security.md:56`); `migrations/0110_schema_default_privileges.sql`; `ops.audit_log` app role without UPDATE/DELETE (`docs/architecture.md:385`); OPS-022 Claude never holds a hosting key (`docs/spec/ops-recovery.md:26`) | T135 (QUEUED), T121 startup key checks (QUEUED) | Proof on the test project at T135; network reachability and exports recorded there |
| TECH-04 Key custody, rotation, recovery | ALREADY_QUEUED | Rotation in code (`app/privacy/src/vault.rs:8`–`13`), TD-PRIV-09 drill (`docs/test-design.md:540`); PRIV-024 key inventory with escrow | T354 photo re-seal (QUEUED), T117 inventory (QUEUED), T109 export key (QUEUED) | Casey holds escrow (owner action). Secondary: **N-04** only if TD-PRIV-09 is not run on the test project. Never record key values or fingerprints |
| TECH-05 Backup and real restore | ALREADY_QUEUED | OPS-013 to OPS-016 (`docs/spec/ops-recovery.md:17`–`20`) | T109, T110, T206, T139 (all QUEUED) | Casey sets RTO/RPO; amendment **A-01** makes T139's record compare against them |
| TECH-06 Alerts, safe mode, worker health | ALREADY_QUEUED | OPS-001, OPS-023 | T105, T106, T204, T319, T320, T327, T366, T367 (all QUEUED) | A real recipient receiving an alert is a Gate C rehearsal item (T145) |
| TECH-07 Tamper-evident operating logs | EXISTING_BUT_NOT_PROVEN | Hash chain and `ops.audit_chain_broken` alert (`app/server/src/offdb_stream.rs:2`–`44`), tested in `tests/integration/tests/offdb_stream.rs`; PRIV-029 write-only prefix key, object lock | T134 real bucket (QUEUED) | Real bucket rights and object lock proven at T134; real log-sink leak check by **N-02** |
| TECH-08 Payment and release invariants | ALREADY_QUEUED | Release only on paid or free (T069 DONE; `tests/security/tests/no_phone_released.rs`, `release_proof_compile_fail.rs`) | T327, T328, T331 to T337, T340 to T344, T352 to T355, T359, T361, T363 (all QUEUED); T297 overload (QUEUED) | Do not add tickets; sixth and seventh audit dispositions own them |
| TECH-09 Auth map, every route | ALREADY_QUEUED | T088 DONE (`tests/security/tests/auth_matrix.rs`), T284 DONE | T348 HTTP probe per served route, T272, T346 (QUEUED); hosted run T136, T138 (QUEUED) | Hosted run at stage 6 |
| TECH-10 PCI scope | OWNER_ACTION_OR_EXTERNAL | FEE-014 card data only at Stripe; Confirm page confirms on-session (`docs/architecture.md:297`–`325`) | T132 Stripe test mode (QUEUED) | Casey confirms the applicable SAQ with Stripe or the acquirer; do not declare SAQ A until eligibility is checked |
| TECH-11 Messaging consent and opt-out | ALREADY_QUEUED | Consent spec `docs/spec/consent.md` | T336, T350, T351, T356, T357, T358, T360 (QUEUED); T373 (proposed, PR open) | Counsel reviews any outbound non-customer use case (owner action) |
| TECH-12 Retention and erasure across vendors | NEW_TECHNICAL_WORK | T092, T274, T276 DONE (purge, photo delete, Twilio body redaction); T120 local sentinel J1 to J4 (QUEUED) | T315, T321 (QUEUED) | **N-02** (proposed): synthetic-marker trace across sandbox vendors on the test project |
| TECH-13 AI data and prompt injection | ALREADY_QUEUED | T025 DONE zero-retention switch; `tests/security/tests/scrubbed_text_compile_fail.rs`; AI-008 | T298, T299, T137, T236 (QUEUED); T297 AI faults (QUEUED) | Casey records the provider's current terms under AI-008 (owner action) |
| TECH-14 Release and rollback control | ALREADY_QUEUED | OPS-022: live deploys only when Casey presses Deploy (`docs/spec/ops-recovery.md:26`) | T135, T141, T145 (QUEUED) | Amendment **A-02**: a release record template in the T141 handover pack |
| TECH-15 External security testing | OWNER_ACTION_OR_EXTERNAL | Nov 9 source-assisted assessment (`docs/reviews/2026-11-10-pentest-dispositions.md:3`, not a runtime test); T138 ZAP baseline (QUEUED) | — | Casey engages a third party before launch; AI audits are not a substitute |
| TECH-16 Supply chain provenance | NEW_TECHNICAL_WORK | Pinned actions in `ci.yml`, gitleaks SHA check, cargo-audit, cargo-deny, npm audit, Dependabot (`.github/dependabot.yml`) | T345, T121 (QUEUED) | **N-03** (proposed): release inventory (SBOM) and artifact digest record |
| TECH-17 Access and secret review cadence | OWNER_ACTION_OR_EXTERNAL | PRIV-024 rotation triggers (T117) | T117 (QUEUED) | Casey runs the review on the cadence in `security-objectives.md` |
| TECH-18 Security exceptions | OWNER_ACTION_OR_EXTERNAL | `docs/threat-model.md` section 6 (R1 to R20, from line 183), e.g. R2 business known-device sign-in, R18 email-only business confirm | — | Casey signs each with an expiry or review trigger (`audit-and-review.md`) |
| TECH-19 Data-subject requests | ALREADY_QUEUED | T092 DONE (delete my data), T087 DONE (homeowner portal) | T307 download my data, T228 business leaving, T315 (QUEUED) | Counsel sets response deadlines (owner action) |
| TECH-20 Capacity and abuse | ALREADY_QUEUED | T089 DONE rate limits; T279 DONE texting switch | T297, T236 (QUEUED) | — |
| TECH-21 Vulnerability management | EXISTING_BUT_NOT_PROVEN | CI scanners above; T138 ZAP (QUEUED) | — | Patch SLA and exception owner proposed in `security-objectives.md`; Casey approves |
| TECH-22 Continuous detection | NOT_APPLICABLE | Report marks it optional; no custom SIEM for a one-person service | — | Casey approves the exclusion; revisit at management review |

## C. Owner and external processes (OPS-ISMS-01 to OPS-ISMS-08)

All eight are **OWNER_ACTION_OR_EXTERNAL**; each is one line in `owner-actions.md`.

| ID | Existing partial evidence |
|---|---|
| OPS-ISMS-01 Applicable law | WISP 4a (`docs/legal/information-security-program.md:40`–`42`) and incident plan section 5 (`docs/legal/incident-response-plan.md:28`–`42`) are AI drafts, not legal findings |
| OPS-ISMS-02 Vendor assessments | WISP section 5 vendor list (`:55`–`68`); T122 account checklist (QUEUED) |
| OPS-ISMS-03 RTO/RPO, contact path, deputy | OPS-002 accepts no backup person; R-11 |
| OPS-ISMS-04 Incident plan legal review | Incident plan is marked "not reviewed by a lawyer" (`docs/legal/incident-response-plan.md:1`) |
| OPS-ISMS-05 Device and account baseline | R-11 criterion only |
| OPS-ISMS-06 Incident drill, internal audit | Incident plan section 8 yearly walk-through (`docs/legal/incident-response-plan.md:52`–`54`) |
| OPS-ISMS-07 Risk acceptance | Threat model section 6; R-14 accepted by D-495 |
| OPS-ISMS-08 Training, on/offboarding | WISP section 6 |

## D. Findings F-01 to F-06

| ID | Class | Notes |
|---|---|---|
| F-01 WISP is not yet an evidenced ISMS | EXISTING_BUT_NOT_PROVEN | This folder starts the ISMS layer; closure needs owner approvals and operating records |
| F-02 Agent review is not independent approval | OWNER_ACTION_OR_EXTERNAL | R-14, R-13 (`docs/risk-register.md:21`, `:23`), R-37; N-01 proposed |
| F-03 Restore unproven | ALREADY_QUEUED | T109, T206, T139; Gate C already depends on them; A-01 adds the RTO/RPO comparison |
| F-04 Privacy evidence across vendors | EXISTING_BUT_NOT_PROVEN | Code controls verified (baseline); N-02 proposed for the vendor boundary |
| F-05 Incident plan needs operator and drill | OWNER_ACTION_OR_EXTERNAL | OPS-ISMS-03, 04, 06 |
| F-06 Known business-rule defects | ALREADY_QUEUED | `docs/reviews/2026-11-10-sixth-audit-dispositions.md`, `docs/reviews/2026-11-10-seventh-audit-dispositions.md`; owners in TECH-08 |

## E. Proposed new technical work (not created; a plan-change session decides)

Every row needs a plan change to `kit/tasks.json` (locked) with a decision entry. None duplicates a current task; each was checked against the titles and goals of T001 to T367 and T368 to T374.

| ID | Closes | Type, reviewer | Owner files (proposed) | Acceptance test | Negative test | Gate timing | Escalation |
|---|---|---|---|---|---|---|---|
| **N-01** | TECH-01, F-02, R-37 | harness, Opus review, Casey code-owner review | `.github/workflows/backup-runner.yml`; a new `tests/test_workflow_permissions.py` | Each job's `permissions:` grants only what its steps use (the test parses every workflow and compares with a reviewed table); one runner cycle on fake data green | From a session token: approving a PR, posting an owner answer that `inbox.yml` accepts, or pushing to `main` is refused; the drill's record is in `docs/reviews/` | P0: before any production privilege, at the latest before T243 | Changes what the runner can do: Casey decides timing (deferred by D-2347) |
| **N-02** | TECH-12, TECH-07, TECH-13, F-04 | build (Python tooling), Opus review | `tests/sandbox_sentinel/`; `docs/phase2/sentinel-trace.md` | On the Railway test project, J1 with unique fake markers; each sink (app logs, Sentry events, Twilio message records after the redaction job, Resend logs, object storage, AI inputs) is queried through its sandbox API and recorded present-and-protected, absent, or deleted | A marker planted in a log line makes the scan fail | P0: stage 6 after T136, before T140 | A marker found outside its allowed sink is a High finding and blocks Gate C |
| **N-03** | TECH-16 | harness, Opus review | `scripts/release_inventory.py` (locked: plan change); a step in the release record (A-02) | From `Cargo.lock` and `package-lock.json` alone (standard library, no new dependency), a CycloneDX-shaped JSON lists every package with version and checksum; the release record carries it and the image digest | A lock entry missing from the output, or a changed checksum, fails the test | P1: before Gate C if Casey agrees, otherwise first post-launch review | Any new tool needs its own decision with license and audit result |
| **N-04** (conditional) | TECH-04 | build, Opus review | `docs/phase2/key-rotation-drill.md` | TD-PRIV-09's v1 to v2 drill, including photos after T354, run once on the test project with fake data; zero v1 rows remain | After dropping v1, a v1-sealed row or photo fails to open (proves the drill's check is real) | P0, stage 6; **only if** TD-PRIV-09 is not already run on the test project | Never record key values or fingerprints |

**Plan amendments (to existing tasks, not new tasks):**

- **A-01** (TECH-05, F-03): T139's record states measured restore time, data-loss window, the snapshot restored and the no-double-send/charge result, against Casey's RTO/RPO once set (`owner-actions.md` P0).
- **A-02** (TECH-14, TECH-16): T141's handover pack gains a one-page release record template: source SHA, `Ci`/`Twin`/`Ally` run IDs, migrations, rollback or no-rollback decision, open vulnerabilities, secret scope, health check, and Casey's go.
