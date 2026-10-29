# Pre-build engineering audit: Ask Hava, October 28, 2026

**Auditor:** Claude (Fable 5.1) as principal architect, Rust engineer, security architect, QA director and technical program manager, with five independent Opus reviewers who saw only their own scope and not each other's work. **Commit audited:** `debbd3e` on `main`. **Decisions:** D-479 (governance and tooling), D-480 (security and privacy requirements). **Verdict:** `docs/build-readiness.md`.

Every finding below was checked against the files before it was accepted; a reviewer's claim that did not hold is not listed. Each finding has a severity, where it lives, what is wrong, the fix, and how we will know it is fixed. "Done here" means this audit's pull request carries the fix; "before the first build task" means it is on the readiness list; "escalated" means it needs Casey's word.

## 1. What was audited

| Artefact | Size | Method |
|---|---|---|
| `docs/spec/*.md` | 646 requirements, 20 files | traceability run, cross-reference of IDs to tasks and test entries, independent consistency sweep |
| `kit/tasks.json`, `kit/prompts/` | 171 tasks, 164 prompts | graph analysis (cycles, stage order, chain depth, coverage, budgets), independent sequencing review |
| `docs/architecture.md`, `docs/test-design.md`, `docs/threat-model.md` | 336, 550, 197 lines | independent Rust and security reviews against ASVS 5.0 L2 |
| `BUILD_BRIEF.md`, `CLAUDE.md`, `docs/constitution.md`, `docs/sources.md` | governance core | enforcement classification: ruleset, CI, runner, deny rule, or document only |
| `runner/`, `scripts/`, `.github/workflows/`, `.claude/settings.json` | the harness | code read, drills run, live `kit/state.json` and ruleset inspected |
| Baseline | `bash scripts/smoke.sh` | green: fingerprint, 9 plan checks, 380 tests |

## 2. Summary

The blueprint is unusually complete: every live requirement has a task, 635 of 636 have a test-design entry, the graph is acyclic, no two concurrent tasks share a file, and the runner proves "done" from merged checks and a tree hash. The two independent sweeps before this audit (D-477, D-478) had already removed the contradictions a check could not see.

What the audit found is of a different kind: rules that exist only as prose, checks that pass by doing nothing, and a few places where the plan assumes a file or a process that nothing creates. None of it is a reason to change direction. All of it is a reason not to start building yet.

**Counts:** Critical 7 (all fixed here or converted to a named task before the first build task), High 34, Medium 38, Low 18.

## 3. Governance and runner (reviewer C, verified)

### G-C1 Critical: the runner's code never runs live
`runner/cycle.py` has an `Env` interface and no implementation, no `__main__`, no caller. The live runner is a Sonnet session reading `BUILD_BRIEF.md` Appendix A prose, and the state record shows it skipping the code's rules (D-171 ignored a lease; K2 and K2b model evidence "not independently verified"; P1 and P2 skipped the reviewer, D-166). So every "runner-side" rule is in practice document-only.
**Fix (before the first build task, harness task T174 in the readiness list):** `runner/__main__.py` plus `runner/github_env.py` (git and GitHub REST, `--no-renames` diffs); Appendix A cut to "run `python3 -m runner`, do the task it hands you, run `python3 -m runner --after`"; every state commit stamped with the runner code SHA.
**Resolved when:** a rehearsal session holding a live lease exits with `lease_live` from code, and every state commit carries `runner_sha`.

### G-C2 Critical: Claude acts as Casey on GitHub
Sessions push and comment under the `casey451` identity (`BUILD_BRIEF.md:227`). The `inbox` workflow's owner filter therefore proves nothing: a session could post "approved to build" or "go live". A plan-change PR from Casey cannot be told from one a session opened, and a required review is impossible because an author cannot approve their own PR.
**Done here:** `inbox.yml` ignores comments performed through a GitHub App; deny rules block `gh issue comment`, the comments, statuses, check-runs and rulesets APIs from sessions. These are pattern rules: a determined session could route around them.
**Escalated (E-1):** run Claude under its own identity (the Claude GitHub App as a bot, which the backup runner already does), then add `CODEOWNERS` with a required `casey451` review on locked paths.
**Resolved when:** a comment posted from a session on an owner issue produces no inbox file (drill), and the ruleset shows a required review that a session cannot satisfy.

### G-C3 Critical: required checks run from the branch they test
`ci.yml`, `twin.yml` and `a11y.yml` trigger `on: push` and run the branch's copy, so a PR can add `if: false`, drop a step, or add a workflow whose job is named `Ci`; the ruleset's required checks carry no `integration_id`, so a forged commit status named `Ci` would also satisfy it.
**Done here:** `.github/workflows/governance.yml` runs on `workflow_run` of `ci`, which always uses `main`'s workflow file and `main`'s copy of `scripts/check_plan_lock.py` and `scripts/check_pr_change.py` against the head commit, and posts a `Governance` check run on that commit.
**Escalated (E-2, three taps):** add `Governance` to the ruleset's required checks, set each required check's source to GitHub Actions (`integration_id` 15368), and turn on "Require branches to be up to date".
**Resolved when:** a test PR with `if: false` on `Ci` cannot merge, and the ruleset JSON shows the four checks with the Actions integration and `strict: true`.

### G-H1 High: owner gates cleared on negations
`runner/outbox.py` matched a substring, so "don't go live yet" cleared Gate C and "I have NOT approved to build yet" cleared Gate P (reproduced).
**Done here:** a go is a short reply (12 words or fewer) carrying the phrase with no negation word; tests cover the negations.

### G-H2 High: skipped checks count as passed
The Rust, audit and npm steps run only `if [ -f Cargo.toml ]` (and lock files); `Cargo.toml` and the lock files are shared paths, so a task could delete one and merge green.
**Done here:** `Ci` fails if any of `Cargo.toml`, `Cargo.lock`, `package-lock.json`, `rust-toolchain.toml`, `deny.toml`, `tests/coverage_floor.json` exists on `main` but not on the branch.

### G-H3 High: no task owned `twin.yml`, so `Twin` could never become real
Also reviewer B's C1: `twin/` does not exist on `main`; `Twin` runs a smoke copy of the old simulator that exits 0; T005 to T007 own `twin/` files nothing creates; the hard-failing gate (D-107, D-108) was never queued.
**Done here:** new harness task T172 (stage 1, before T005) brings the twin into `twin/`, writes `gate.py`, the waivers and self-tests, and switches `twin.yml` to the gate; T074 also owns `twin.yml` for the `rules_bridge` switch.
**Resolved when:** T172's report shows a deliberately broken waiver turning `Twin` red.

### G-H4 High: the architecture's boundary sections were open to any task
`docs/architecture.md` is a shared path; sections 3 to 6 (ownership, layers, kernel, state machine) could be rewritten by any task, and T009's architecture test would have read its oracle from the prose.
**Done here:** `kit/plan.lock` fingerprints those sections; `Ci` and `Governance` fail a change without a lock rewrite and a decision; `kit/layers.json` (protected) is the machine-readable layer table T009's test reads; on a task branch the lock may be rewritten only by a harness task or the owner of a changed locked file.

### G-H5 High: renames hid deletions
`git diff --name-only` with rename detection lists a moved file under its new name, so a protected test moved into an owned path looked like an addition.
**Done here:** `--no-renames` in `check_pr_change.py`, with a test; the runner's real `Env` (G-C1) must do the same.

### G-H6 High: any push to `claude/task/**` gets a PR with auto-merge
A worker subagent shares the runner's credentials and can push before the path check and review.
**Before the first build task (with G-C1):** the runner writes `reviewed_sha` and the lease to `claude/kit-state` before pushing; `auto-pr` enables auto-merge only when the lease names the task and `reviewed_sha` equals the head.

### G-H7 High: branches need not be up to date to merge
Ruleset `strict_required_status_checks_policy: false`; a stale branch can merge an untested combination, caught only afterwards by the tree check. **Escalated (E-2).**

### G-H8 High: path rules were skipped by renaming the branch
Only `claude/task/T###-` branches got the ownership check. **Done here:** a non-task branch touching protected or locked paths must carry a `Plan-Change: D-###` trailer naming a new decision entry. With G-C2 open, this proves intent, not authorship.

### G-M1 Medium: nothing protects `claude/kit-state`
No ruleset; force-push and delete are blocked only by local patterns; three workflows push to the branch the lease lives on. **Escalated (E-2):** a ruleset on `claude/kit-state` blocking deletion and non-fast-forward pushes. **Before the first build task:** `scripts/check_state.py` schema check at runner start; the lease moved to its own ref.

### G-M2 Medium: the decisions log was append-only on paper
**Done here:** `check_pr_change.py` requires the base's `D-###` entries to be a byte-identical, ordered prefix of the head's.

### G-M3 Medium: `#[allow]`, wildcard arms, `unsafe`, TODOs were reader-enforced
**Done here:** `scripts/check_rust_rules.py` in `Ci` and the smoke test; clippy runs with `-F clippy::allow_attributes_without_reason`, which source code cannot override.

### G-M4 Medium: a new crate needed only a document entry
**Done here:** `kit/allowed_crates.txt` (protected) and `scripts/check_crates.py` fail `Ci` on any direct dependency not listed.

### G-M5 Medium: a red `main` had no way out in code
`main_red` was set and never cleared; `FIX-MAIN` was not a task; nightly results were not read. **Done here:** `nightly-main` records `kit/nightly.json` on `claude/kit-state`; `docs/governance.md` section 5 defines the protocol. **Before the first build task (T174):** implement it in the runner with a drill.

### G-M6 Medium: state cannot resume a session with zero loss
Missing: per-attempt history (model, evidence, head SHA, checks, reviewer), an open-deviations register, the usage reading (`usage.last_reading` is null), the plan-lock hash at claim time, `main`'s SHA and smoke result at cycle start, owner answers as an append-only file. **Before the first build task (T174).**

### G-L1 to G-L4 Low
`docs/sources.md:29` contradicted section 16 (done here; T001 removed from the graph); `CLAUDE.md` 200-line rule unchecked (done here in `check_code_docs.py`); reviewer models unverified (T174); usage reading undefined (T174: worker-transcript token totals per task in `usage.by_task`).

## 4. Task graph and sequencing (reviewer B, verified)

### B-C1 Critical: `Twin` never becomes hard-failing — see G-H3 (done here, T172).

### B-C2 Critical: mutation and coverage tooling had no owner
`scripts/run_named_mutants.py` and `tests/coverage_floor.json` were kit outputs nothing created; T169 was told to measure against a file T056 would create later. **Done here:** T169 creates `coverage_floor.json`; T056 owns `run_named_mutants.py` and the `ci.yml` step.

### B-H1 High: the coverage gate landed after 50 build tasks, including the vault and licensing
T169 depended on T053 only for `a11y.yml`. **Done here:** T169 split: T169 (Ci coverage, sqlx check, per-crate clippy; depends on T009 and T168) and T173 (Ally, the e2e job, the Dockerfile npm stage); T022 and T034 now depend on T169.

### B-H2 High: the end-to-end journeys never ran in a required check
No workflow boots the server and runs `tests/e2e`. **Done here:** T173 adds the Ci e2e job (server, worker, `stand_ins/server.py`, Playwright).

### B-H3 High: a stage-2 mutant could only be killed in stage 3
"License check at pick skipped" needed TD-PICK-09 (T063, stage 3). **Done here:** moved to T073.

### B-H4 High: only stage 3 had a machine-checkable exit
**Done here:** `docs/build-readiness.md` section 3 names an exit test per stage; each cleanup prompt names it. **Before the first build task (T174):** `scripts/stage_exit.py --stage N` reading `kit/nightly.json` and a test-results file, run by the runner before the next stage; the runner dispatches `nightly-main` when a cleanup task merges.

### B-H5 High: cleanup tasks were barriers with an impossible scope
They owned all of `app/` with a budget of one file and 200 lines, and every next-stage task depended on them. **Done here:** cleanup tasks are scan-driven (`cargo machete`, duplicate scan, `check_rust_rules.py`, `check_code_docs.py`, a commented-out-code grep), own `app/` and `migrations/`, budget 14 files; done means zero findings and the stage exit test recorded. **Open:** gating the next stage on `stage_exit` rather than on every stage task (T174), so a blocked mid-stage task does not halt stages 3 to 7.

### B-H6 High: the first whole-codebase audit came at task 136 of 171, and nothing acted on its findings
**Before the first build task:** stage-boundary Opus audits of each stage's money, privacy and security diff inside `stage_exit`; a FIX task queued per Critical or High finding; audit files with a machine-readable findings table.

### B-H7 High: context budgets are understated
D-119 counts every read as 1% whatever its size; 126 tasks read `docs/test-design.md` (170 KB), 153 read `docs/architecture.md` (56 KB). Reviewer B's estimate puts T150 near 145k tokens against an 18% budget; T147 to T149, T151, T104, T156, T160 and T162 over 100k. **Before the first build task (plan change P-1):** `gen_prompts.py` pastes only the task's own requirement sentences, TD rows and decision lines, and computes `context_pct` from bytes; `--check` prints a token estimate per prompt.

### B-H8 High: some tasks are too big for one session
44 tasks exceed 600 lines, 12 requirements or 10 files (T147 to T150, T102, T104, T105, T081, T151, T156, T157, T158, T160, T162 to T165, T016, T069 and others). **Before the first build task (P-1):** split at 600 lines, 12 requirements, 10 files; `validate_tasks.py` limits lowered to match.

### B-M1 Medium: model allocation goes beyond D-115
127 of 171 tasks on Opus; 14 Opus tasks have no money, privacy, security or recovery area (T002, T003, T004, T005, T034, T035, T042, T044, T152, T153, T154, T156, T159, T170); the journeys are on Opus though `docs/test-design.md:48` says Sonnet writes them. Low-risk moves to Sonnet with an Opus reviewer: T004, T154, T159, T168, T010, T019, T135, T138, T169, T170, T098, T099, T118, T119, T072, T121, T117, T122, T133, T134. Should be Opus instead: T039 (emergency text, 15 requirements), T032 (timers driving Confirm and refunds), T036 (license imports, a 95% floor area); T040 and T032 need an Opus reviewer. Net about 60% Opus. **Escalated (E-3)** as a one-line choice; the task file then changes in a plan change.

### B-M2 Medium: prompt template gaps
134 of 164 prompts have goal equal to title, so the Goal section is dropped; IDs are listed without their text; no per-task test command; no pointer to the crate layout conventions; three definition-of-done items a worker cannot verify (required checks, coverage before T169, the twin before T172); the fresh-eyes review is duplicated between the template and the runner. **Before the first build task (P-1):** with B-H7.

### B-M3 Medium: progress measurement
`scripts/progress.py` (written in this audit) needed `merged_at` from the runner and test evidence. **Done here:** the runner records `merged_sha` and `merged_at` on DONE; `nightly-main` writes `kit/nightly.json`. **Before the first build task (T174):** a nightly `kit/test_results.json` of TD markers that passed, ancestry (`merge-base --is-ancestor`) instead of timestamps, and a separate "manual or Phase 2" class for A11Y-022 and its kind.

### B-M4 Medium: validator checks to add
Done here: withdrawn requirement references fail. Before the first build task (P-1): tests or an `acceptance` field on every build and harness task; kit outputs must exist once K9 is DONE; mutant "killed by" entries owned by an ancestor; harness-before-code order; goal must differ from title; Opus reviewer for any owner of a cargo-mutants target; every task reads the spec file of each requirement it names (13 miss one today); migration numbers increase in serial order.

### B-M5 Medium: the sentinel no-leak test (T120) sits at position 139, long after the vault
**Before the first build task (P-1):** a database-and-logs sentinel scan in stage 2 (after T022), extended by the journeys later.

### B-L1 to B-L4 Low
Migration numbers out of serial order (0044 after 0051; 0105, 0109, 0110 after 0147 to 0151) — safe for SQLx but fragile; number by serial position in P-1. Tiny sessions (T004 30 lines, T124 100 lines, T030 one requirement) — merge in P-1. T167 and T170 labelled stage 1 but ordered after T008 — relabel in P-1. The copy-panel steps appear in pure-infrastructure prompts; T169's module was "adapters" (fixed here).

## 5. Security and privacy (reviewer E, verified; requirements added under D-480)

The 11 High and 15 Medium findings below are now requirements in `docs/spec/` with test-design entries and owning tasks, so the build inherits them. Section references (S-xx) are what the requirements cite.

### S-H1 High: no independent gate between agent-written code and the live deploy
Merges need only the agent-run checks; the live deploy is Casey pressing Deploy on `main`; sessions act as Casey. A prompt-injected session could merge code that exfiltrates the vault and it would ship at Gate C. **Now OPS-043:** live deploys only a SHA named in a release record Casey signs after an independent Opus security review of the release diff; auto-source off; no bypass actors on the ruleset. **Escalated (E-1, E-2)** for the identity and ruleset parts.

### S-H2 High: the test-control router is switched by an environment variable
It can advance the clock and read delivered codes; one wrong variable in live is account takeover; the startup key check works only for Stripe's prefixed keys. **Now SEC-035, SEC-036:** compiled only under a `test-control` feature the production build never enables; per-run bearer secret even in CI; per-environment account-identifier allow-lists.

### S-H3 High: vault key management has no lifecycle
No escrow (a lost host variable makes every backup unreadable), no rotation procedure, no blind-index rebuild design, an undefined export-key location, no nonce policy, and an incident plan that says "rotate" with no procedure. **Now PRIV-024** (inventory, escrow on paper, per-purpose index keys, offline export recipient, rotation drill TD-PRIV-09).

### S-H4 High: quote-page tokens derived from one secret
`HMAC(ID_SECRET, job id)` means the secret plus a job number yields every token, and a link cannot be re-issued. **Now SEC-037:** a per-record random salt in the derivation, stored and streamed, so replay still works and a link can be revoked.

### S-H5 High: masked-call PIN brute force and caller-ID-trusting read-back
10^4 PINs with no attempt limit on shared pool numbers; the read-back went to any "linked" phone. **Now CALL-025:** 3 attempts per call, 6 per number per pool number per day, read-back only with STIR/SHAKEN A attestation.

### S-H6 High: Verify, SMS and voice abuse
Sign-in sends a code to any typed number; forwarded links could trigger voice calls at any hour; no caps, no enumeration rule. **Now PICK-055.**

### S-H7 High: native-code photo decoding beside the keys
No mature pure-Rust HEIC decoder exists; `image` defaults set no dimension cap. **Now PRIV-025:** explicit limits, a semaphore, primary frame only; HEIC not decoded in the app at launch (phones send JPEG by default; a HEIC upload gets a plain reply). TD-PRIV-08's HEIC fixture now expects the refusal.

### S-H8 High: sensitive business changes needed only a session
A stolen 30-day cookie could divert notices and picks. **Now SEC-038** (fresh Verify code within 5 minutes).

### S-H9 High: inbound email was not modelled
Business replies and help emails land in a mailbox with homeowner data outside the vault and no retention rule. **Now PRIV-026** (signed inbound webhook into `privacy`, DMARC results recorded, per-purpose email blind index).

### S-H10 High: owner bootstrap undefined; recovery factors concentrated in one vault
**Now SEC-044** (one-time bootstrap token) and OPS-042 (hardware keys, paper escrow, rehearsed break-glass).

### S-H11 High: off-session auto-confirm could be fired by a spoofable text pick
**Now PICK-056.**

### S-M1 to S-M15 Medium (all now requirements)
Scrubber corpus and Unicode normalization (AI-020); HELP notification scrubbing (PRIV-030); AI output content screen and cost caps (AI-021); Resend and Lob webhook verification, Twilio dedupe keys, Stripe proof checks (SEC-039); client IP and atomic rate limits (SEC-040); realm capability proofs inside `portals` (SEC-046); off-database integrity and retention (PRIV-029); data leaving the vault through the dispute pack, card-email bytes, Twilio bodies and Cloudflare (PRIV-027); logging and Sentry paths (PRIV-028); supply-chain pins and gitleaks range scans (SEC-045); owner session timeout and step-up (SEC-043); vault read-volume alert (SEC-047); SSRF (SEC-041); photo store contradiction (D-094 says Postgres, PRIV-023 says object storage — **before the first build task (P-1):** one decision); PII scanner misses emails and image metadata (**before the first build task (P-1):** `pii_scan.py` rules).

### S-L1 to S-L6 Low
CSRF details and fixation (now SEC-042); recycled phone numbers (P-1: masked addresses until re-confirmed); database TLS and migration credentials (P-1); forged owner answers (G-C2); request smuggling test (P-1); the threat-model review record (this audit is the K4 fresh-eyes review; TB12 inbound email and the new rows are added to the threat model in P-1).

### S-V ASVS 5.0 L2 chapters previously uncovered
V11.1 key management (PRIV-024), V7.5 session abuse (SEC-038, SEC-043), V13.4 debug interfaces (SEC-035), V7.3/V7.4 timeouts and termination (SEC-043), V6.2 passwords (SEC-043), V6.3 enumeration (PICK-055), V16.3 security events (SEC-048), V16.4 log protection (PRIV-029), V1.3 SSRF (SEC-041), V12.3 service TLS (P-1), V15.1 remediation timeframes and SBOM (P-1), V5.1 file handling (PRIV-025), V9 self-contained tokens (SEC-037), V3.5 origin separation (SEC-042), V4.2 message structure (P-1), V8.4 admin interface restriction (optional, P-1).

### S-E Operational security before Gate C — now OPS-042.

## 6. Rust architecture (reviewer A, verified; decisions under D-481)

The reviewer checked every pin in `docs/architecture.md` section 11 against crates.io, npm and the Rust release channel (all correct), read automod's source, and computed migration order against task order from `kit/tasks.json`. The design flaws below would each have stopped the first session that reached them.

### A-C1 Critical: `ReleaseProof` broke the layer rule, and its proof was tested with the encapsulation off
The vault read lives in `privacy` (layer 2), which cannot name a `payments` (layer 3) type; and the compile-fail case ran under the `testing` feature, which exposed the private constructor. **Done here (D-481):** `ReleaseProof` and a non-`Clone` `ReleaseMint` in `privacy::api`; the mint created once in `server/src/lib.rs` and handed to `payments`; the architecture test fails on any other crate naming the mint; the trybuild case runs in a crate with no feature.

### A-C2 Critical: the named-mutant runner would have rebuilt the workspace per mutant
About 60 mutants, each on a cold copy with a fresh `target/`: hours per PR. **Done here:** mutate in place with a shared target directory, run only the named tests, PR runs only the changed files' mutants, nightly the full list, 10-minute ceiling (test-design section 4, T056).

### A-H1 High: `rules_bridge` needed a forbidden feature, and `Twin` never built Rust
**Done here:** each `api` re-exports its pure rules as `api::rules`; the bridge is a plain binary with a JSON-schema contract test; T074's `twin.yml` installs the toolchain and builds it from the PR's commit.

### A-H2, A-H3 High: `smoke.sh` and `.sqlx/` would have failed at the first `query!`
No `SQLX_OFFLINE`, an empty database for the online check, `prepare --workspace` skipping test targets, no `sqlx-cli` install. **Done here:** offline builds, `sqlx migrate run`, `--all-targets --all-features`, the `prepare --check` in `smoke.sh` when the CLI is present and in `Ci` at T169 with a pinned prebuilt install.

### A-H4 High: no timestamp type worked with the pinned SQLx
sqlx 0.9.0 has `time` and `chrono` features, no `jiff` (confirmed on crates.io). **Done here:** jiff-sqlx 0.2.0.

### A-H5 High: typescript 7.0.2 conflicts with typescript-eslint 8.71.2
Peer range `>=4.8.4 <6.1.0` (confirmed on npm). **Done here:** typescript 6.0.3.

### A-H6 High: the e2e suite had no CI owner and nothing started the stand-ins — see B-H2 (T173, with the boot sequence now specified).

### A-H7 High: the `testing` feature could not work as written
`pub(crate)` items stay hidden when their module is made `pub`; automod gave `api` one visibility with everything else; the natural import path passed locally and failed only in `Ci`, which `smoke.sh` did not reproduce. **Done here (the reviewer's strongest recommendation, O1):** unit and property tests move in-crate under `src/tests/`, the feature and the two test crates are dropped, automod is dropped, `smoke.sh` runs `cargo build -p server`. 66 owned test paths remapped; T168 narrowed.

### A-H8 High: the worker binary could not see `main.rs`'s composition
**Done here:** `server/src/lib.rs` with `build_router()` and `register_handlers()`.

### A-H9 High: migration-role mechanics undecided; D-092 named Alembic
**Done here:** roles created outside migrations; idempotent grants and default privileges inside; `server migrate` as the release step; append-only tests under `SET ROLE`; `app/core/build.rs` (T012).

### A-H10 High: CI with no cache compiled the workspace about five times per run
Estimated 25 to 40 minutes per run and roughly 7,000 Actions minutes over the build. **Done here:** rust-cache pinned by SHA, prebuilt tools, coverage in its own job, per-crate clippy dropped, 15-minute warm ceiling (T169). **Escalated (E-6):** the Actions-minutes budget on the repository's plan.

### A-M1 to A-M8 Medium (all decided here)
One rustls crypto provider (sqlx `tls-rustls-aws-lc-rs`, installed as the default first); `Settings::from_iter` (edition 2024 `set_var` is unsafe); `too_many_lines` and `excessive_nesting` denied in the workspace lints and every member on `workspace = true`; a `status.rs` module per crate with the wildcard deny on it alone, plus glob-import and `==` scans; same-schema migration order checked by the architecture test, each migration naming its module; the coverage nightly pinned by date over rule and service files; the missing decisions (errors, worker loop, shutdown, pool, middleware features, askama_web, shared layouts, static assets, `#[sqlx::test]` isolation) written into sections 2 and 5; `smoke.sh` made equal to `Ci` on the boundary build and the offline check.

### A-L1 to A-L6 Low
rand 0.10's API change and two rand versions in the tree (T009 may use `getrandom` for codes and tokens; logged then); `missing_docs` now fires only on `api` items since internals are private (a smaller line budget than before); the template escaping scan (done here); the architecture test's helper crates (T009, new-library rule); `postgres:16` unpinned until T169 and no `setup-node` (T173, done here); serde-saphyr 2.0.0 published one day before the cutoff (kept; T009 may fall back to 1.3.0 with a decision).

### A-O Over-engineering verdicts
Test-layer crates with a `testing` feature: no requirement behind them, removed. automod: removed. One crate per module: kept (D-453, SEC-029). Passkeys plus TOTP: kept (D-098), noting it is the third crypto stack. Per-crate clippy runs: removed. Branch coverage on nightly, stand-ins over HTTP, the twin oracle, the SEC-031 scan: kept, each has a requirement.

### Checked and sound
Feature unification as claimed; every pin, date, license and feature name in section 11; Rust 1.99.0 and 1.99.1 dates; SQLx's handling of a lower-numbered migration added later; `FOR UPDATE SKIP LOCKED` with leases and a dedupe key; AES-GCM with associated data and a key version plus the HMAC blind index; `Redacted` and `SecretString`; `deny_unknown_fields`; jiff with the area-code table.

## 7. Spec consistency and traceability (reviewer D, verified; fixes under D-482)

The reviewer sampled more than 40 requirements against their test entries, swept the governance documents for superseded values, and checked module ownership against the architecture's layer table.

### D-H1, D-H2 High: the stage exit table could not be met and left out 23 tasks
J1 needed stage 3 and 4 work (the paid release, the masked call, feedback, photos); J3 needed stage 5 switches; the rows named task ranges. **Done here:** rows built from each task's `stage` value, a reduced J1 at stage 2, full J1 and J2 at stage 4, J3 and J4 at stage 5, a Stage 0.5 row, Gate B and Gate C pass bars.

### D-H3 High: three outcomes for an uncovered request and no state-machine arrow
**Done here:** Intake → Not covered in INT-017, INT-022 and architecture section 6; a duplicate joins the open job (INT-010).

### D-H4 High: `ops` (layer 1) owned work that needs layers 3 to 8, and stored homeowner text
**Done here:** `ops` records IDs-only rows and enqueues events; `messaging`, `portals` and `businesses` do the sending and reading; help-message text lives in `privacy.message_body`.

### D-H5 High: 20 test-design rows asserted withdrawn or replaced behaviour
Each would have failed correct code or passed wrong code (the rental meeting question, a 2-hour Confirm window, a "confirming" text on HOLD, voice calls to businesses, 1-business-day replies, the call window, "Switch to urgent", the weekly spot-check, the old crowding numbers, "four" push kinds, a safety-word complaint alerting at once, a refusal before the ZIP check, paid-only release; TD-INT-17, TD-INT-24 and TD-OPS-17 cited withdrawn IDs). **Done here:** every row rewritten to the current requirement.

### D-M1 to D-M11 Medium (done here unless noted)
The $0-pick change carried into CALL-005, the architecture invariant and TD-PICK-10; the section 6 timeout table completed (HOLD, away, emergency); withdrawn IDs removed from coverage and the architecture's catalog notes; the copy-length rules aligned (15 or fewer where no requirement fixes the sentence, never over 20, spec-fixed sentences exempt); `docs/PREBUILD.md`'s "decided" lines corrected (paid weeks, web-chat consent, 30-minute card, no channel question); the constitution corrected (held-text exception, no business voice call, "usually within 2 days", email new-device code, Gate P as stop 0); the brief's stale lines listed for P-1 (R-12); CARD-021, CARD-022 and OPS-041 owners settled; the A11Y and WORD-029 coverage rule stated; J1 runs the ZIP check first; the storyboard, HANDOFF and the review pack carry supersession banners.

### D-L Low (P-1 unless noted)
Counts in PREBUILD (done); history rows marked (done); the threat model's spot-check cadence (done); INT-010 versus INT-022 on duplicates (done); INT-029's step list missing the category check; BIZ-042's "T1" reference.

### Testability (P-1)
The reviewer proposed testable rewrites for 26 requirements (INT-010, INT-015, INT-038, INT-039, INT-044, LIC-040, CON-010, CON-032, CARD-009, CARD-010, A11Y-001, A11Y-005, A11Y-009, A11Y-018, TXT-005, WORD-009, WORD-013, PICK-052, AI-016, LIC-018, LIC-025, CALL-013, SEC-012, PRIV-019, WEB-015, BIZ-042), most needing a setting or a decision to pin a number. They are recorded in D-482 for the P-1 plan change rather than applied blind.

### Checked and sound
All 646 pre-audit IDs unique; withdrawn IDs kept in place; no superseded term outside the intended tables; the CON-031 consent texts at 295 and 303 GSM-7 characters; the section 8 reader list matches PRIV-004; section 6 matches the JOB and FEE timers it cites; INT-043's table matches its requirements; the fee rows consistent with the worked examples; 60 sampled test rows test their sentence.

## 8. Decisions that were escalated, and how they were settled (D-483)

Casey's answer to the escalations was to decide them on best practice rather than ask him. Each is now a decision; what remains for him is only the hands-on step a machine or a session cannot take.

| # | Decision taken | Casey's hands-on step |
|---|---|---|
| E-1 | Claude runs under its own GitHub identity: the Actions runner (Claude GitHub App) becomes primary, the routine the safety net; `CODEOWNERS` names Casey for the locked blueprint and the harness, so harness pull requests wait for his one-tap review and nothing else does | Run `claude setup-token` and store it as the `CLAUDE_CODE_OAUTH_TOKEN` secret (BUILD_BRIEF.md section 0, step 8); turn on "Require review from Code Owners" in the ruleset |
| E-2 | Yes to all four ruleset settings | Add `Governance` as required; set GitHub Actions as the source on all four checks; turn on "Require branches to be up to date"; add a `claude/kit-state` ruleset (no deletion, no force-push) |
| E-3 | Sonnet by default; Opus writes money, privacy, security and recovery application code; Opus reviews every task in those areas and every harness task (113 Opus, 50 Sonnet, 2 Haiku) | none |
| E-4 | OPS-043 stands | At Gate C, name the reviewed commit |
| E-5 | The three behaviour-changing security rules stand | none |
| E-6 | `Ci` designed for cost; pacing set from the real quota | Read the Actions minutes quota in the repository's billing page once and reply with the number |

## 8a. Independent review of this audit's own changes (D-484)

A fresh Opus reviewer read the pull request's diff and found 16 problems in the audit's own work, from a lock-rule bypass (the change rules read ownership from the branch under test) to gate phrases still matched as substrings and CODEOWNERS paths that would have made seven ordinary tasks wait for Casey. All were verified and fixed; D-484 lists each. The lesson is recorded in the governance file's section 7: the audit's own changes get the same review it asks of every task.

## 9. What this audit changed in the repository (D-479 to D-484)

Added (D-481 too): `docs/governance.md`, `docs/ai-operating-manual.md`, `docs/risk-register.md`, `docs/build-readiness.md`, this report; `scripts/progress.py`, `scripts/check_plan_lock.py`, `scripts/check_pr_change.py`, `scripts/check_rust_rules.py`, `scripts/check_crates.py` with tests; `kit/plan.lock`, `kit/layers.json`, `kit/allowed_crates.txt`; `.github/workflows/governance.yml`; tasks T172 and T173. Changed: `ci.yml` (skip guard, plan lock, Rust rules, crates, clippy forbid), `nightly-main.yml` (result stamp), `inbox.yml` (app-posted comments ignored), `.claude/settings.json` (deny rules), `runner/outbox.py` (gate phrases), `runner/verify.py` (nightly file), `runner/cycle.py` (merged SHA and time), `scripts/validate_tasks.py` (withdrawn IDs, protected files, shared lock), `scripts/check_code_docs.py` (CLAUDE.md limit), `scripts/smoke.sh`, `kit/prompts/TEMPLATE.md`, `kit/tasks.json` (withdrawn IDs dropped, T001 removed, T169 split, T056/T073/T074 ownership, cleanup scope, coverage ordering), `docs/test-design.md` (28 new entries, mutant ownership, TD-PRIV-08), `docs/spec/` (28 new requirements), `docs/sources.md:29`.
