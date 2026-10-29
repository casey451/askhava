# Build process review: throughput, cost and failure modes, October 29, 2026

**Follows:** `docs/audit/prebuild-audit-2026-10-28.md`. **Decisions:** D-485 (plan cleanup), D-486 (runner live and pacing), D-487 (expertise files), D-488 (Rust skeleton trial), D-489 (process optimizations), D-490 (brain proposals adopted). **Casey's instruction:** "Optimise this process and then check the build process for any problems or things we can optimize … make the decisions based on best practices."

This review asks one question of each part of the build loop: where does a cycle lose time, tokens or certainty, and what is the cheapest change that removes the loss without weakening a check.

## 1. The loop as it stands

One task per cycle. A cycle: a runner session starts, reads state, claims a task, hands it to a worker on the task's model, reviews the diff with a fresh reviewer, pushes the branch; `Ci`, `Twin`, `Ally` and `Governance` run; auto-merge squashes; the next cycle marks it done. 173 tasks at the start of the day, 216 after the splits and additions of D-485, D-488 and D-490; longest dependency chain 94 tasks deep. Stage sizes now: 11, 73, 23, 46, 41, 18, 2 (plus the checkpoint and stage 0.5).

## 2. Where cycles were lost, and what changed

| # | Loss | Cause | Change (decision) | Effect |
|---|---|---|---|---|
| 1 | Up to 59 idle minutes per task | The routine fires hourly; a merge at :40 waits for the next tick | The Actions runner also starts on every merged pull request into `main` (`backup-runner.yml`, `pull_request: closed`); the hourly tick becomes the safety net (D-486) | Cycle time falls from "one per hour" to "as fast as work plus checks": roughly double throughput |
| 2 | 12 to 15 minutes of compile on changes that have no code | Every push compiled the Rust workspace and ran npm, including the 41 non-Rust tasks and every plan change | `Ci` compiles and audits only when the diff touches `app/`, `migrations/`, the Rust test crates or the Cargo/npm manifests; the skip guard stays unconditional, so a deleted input still fails (D-489) | About a quarter of runs drop to the 3-minute document checks; the Actions-minutes estimate falls from about 7,000 to about 5,500 |
| 3 | Sessions that compact mid-task and restart | Budgets counted every read as 1%; most prompts were a title; some tasks loaded a 170 KB file to use three lines | Prompts carry each task's own requirement sentences, test rows and decision lines; budgets come from bytes; 44 oversized tasks split at 600 lines, 12 requirements or 10 files; every task has a real goal and its own test command (D-485) | Fewer restarts; the reviewer sees the rule text beside the diff |
| 4 | A session following prose instead of code | The runner's code had no entry point | `python3 -m runner` runs the cycle; `--after` does the hand-in; state commits carry `runner_sha` and the starter; `auto-pr` merges only a reviewed head (D-486) | Rules enforced by code, not by reading |
| 5 | Design mistakes discovered by the first task to hit them | The Rust layout had compiled only as a one-crate trial | A throwaway build of the full 19-crate skeleton with the capability types, in-crate tests, offline SQLx, jiff-sqlx, askama_web, trybuild and the layer test (D-488); findings folded into T009, T012, T168, T169 | T009 starts from a layout that is known to compile; real timings replace estimates |
| 6 | Knowledge that lived outside the repository | 53 references point at `/home/claude/hhq/outputs/…` (legal memos, the simulation, the GTM pack, the operator pack), unreachable from any new session | Eight sourced expertise files under `docs/brains/`, locked and read by the tasks they serve (D-487); the outside references are listed as a risk until the files are committed or re-derived | A session that knows nothing can still build the texting, payments, accessibility and email parts to the standard the rules assume |
| 7 | A blocked task halting every later stage | Next-stage tasks depended on every task of the stage before | Stage gating by `scripts/stage_exit.py` on the previous stage's evidence, not by depending on every task; a blocked mid-stage task holds only its dependents (D-486) | A single stuck task no longer stops the build |
| 8 | Red `main` with no way out in code | `main_red` set and never cleared | FIX-MAIN modelled as a revert-first task; cleared only on a verified green run (D-486) | A bad merge costs one cycle, not the build |

## 3. What was examined and left alone

- **One task at a time.** The validator proves no two concurrent tasks own a file, so two workers could run in parallel, but the shared paths (`Cargo.lock`, `lib.rs`, `api.rs`, `decisions.md`, `architecture.md`) would collide on nearly every pair. The merge trigger (change 1) buys most of the same throughput with none of the conflict risk. Revisit only if the measured cycle time after T174 is still above 90 minutes.
- **The model mix** (114 Opus, 50 Sonnet, 2 Haiku after D-483). Opus writes the money, privacy, security and recovery code and reviews all of it; the rest is Sonnet. The first three measured tasks set the pace; nothing to change before that.
- **The retry ladder** (same model once more, then Opus once, then BLOCKED). Sound; the per-attempt history that T174 adds makes it auditable.
- **The hand-run test before Gate P (D-455).** Casey chose proof before building, with a stated reason; the build's cost is plan usage, not money, but a "change the plan" result would still invalidate finished tasks. Kept. The option of starting stage 1 (the twin and catalog, which no test result changes) during the test is recorded in section 5 for Casey, not taken.
- **Nightly, twin and accessibility runs.** Cheap and load-bearing.

## 4. Failure modes still open

| Mode | Where it shows | Mitigation now | Owner |
|---|---|---|---|
| The Actions runner and the routine both start a cycle | Two sessions, one lease | The loser exits in under a minute; T174 records the starter; the routine is paused once the Actions runner has run three clean cycles | T174 report → Casey pauses the routine |
| A worker's cold `target/` | Every session compiles from scratch (5 to 10 minutes of wall time, no tokens) | Accepted; a shared cache across ephemeral sessions is not worth its complexity | — |
| The first Rust tasks hit a pin that no longer resolves | T009 | The skeleton trial resolves every pin today and records the lock | D-488 |
| Casey's three hands-on steps not done when the build starts | The Actions runner has no token; checks run from the branch | `docs/build-readiness.md` section 4 blocks READY on them | Casey |

## 5. Options recorded for Casey, not taken

- Start stage 1 (twin, catalog, stand-ins over HTTP: tasks whose result no hand-run finding changes) during the hand-run test, so the first build week overlaps the test. Saves about a week; costs nothing but plan usage. D-455 chose proof first, so this stays an option.
- Two workers in parallel once the shared-path collisions are measured on real tasks.

## 6. Evidence

Tool runs in this repository on October 29, 2026, on branch `claude/build-optimization`:

- **Plan checks:** `python3 scripts/validate_tasks.py` 0 errors over 216 tasks (173 build, 18 harness, 8 docs, 4 audit, 6 cleanup, 7 gates; 151 Opus, 57 Sonnet, 1 Haiku; Opus reviewer on 190); `python3 scripts/gen_prompts.py --check` 209 prompts, 0 problems, highest build-task context 19%; `python3 scripts/trace_spec.py` 0 errors; `python3 -m pytest tests -q` 518 passed with the plan-lock test deselected until the lock rewrite in this PR.
- **Runner (D-486):** `tests/test_runner_live.py`, `tests/test_stage_exit.py`, `tests/test_check_state.py`, `tests/test_failure_drills.py`, `tests/test_runner_logic.py` 121 passed; `tests/test_pacing.py` and `tests/test_quota_probe.py` green; `python3 -m runner --dry-run` against the live repository: `runner: idle (dry run)`.
- **Skeleton trial (D-488):** `docs/audit/rust-skeleton-trial-2026-10-29.md` section 3: cold build 106 s (19 crates) and 127 s (full set), cold clippy 109 s, cold `cargo test` 515 s (trybuild 406 s), warm one-line change 2.8 s build and 8.6 s test, release server 200 to 301 s, `target/` 3.6 GB, 421 external packages, audit and deny clean.
- **Expertise files (D-487):** `rust-production.md` 337 lines, 36 source links, 4 inferred; `postgres-operations.md` 132 / 37 / 9; `sms-compliance.md` 202 / 22 / 11; `payments-stripe.md` 206 / 26 / 9; `accessibility-forms.md` 217 / 30 / 7; `email-deliverability.md` 166 / 23 / 11; `contractor-lead-experience.md` 193 / 21 / 8; `operator.md` 155 / 14 / 7. Their proposals became 39 requirements and 12 test-design rows (D-490).
- **CI path filtering (D-489):** `.github/workflows/ci.yml` parses (`yaml.safe_load`) and its `run:` steps pass `bash -n`; the first run on this PR's branch is the first real measurement of the document-only path.

Numbers that are still estimates: the Actions-minutes total (about 5,500), the cycle time under the merge trigger, and the plan's real usage limits in `kit/pacing.json`; the first three measured tasks replace them (`docs/build-readiness.md` section 2).
