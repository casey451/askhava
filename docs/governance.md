# Ask Hava engineering governance

**Status:** adopted October 28, 2026 (D-479). **Owner:** Casey. **Applies to:** every session, human-started or scheduled, that changes this repository, and to the runner.

This file is the rulebook that `BUILD_BRIEF.md` (how), `docs/constitution.md` (guardrails) and `docs/spec/` (what) sit under. It does not add product rules. It says who may change which part of the blueprint, what a machine checks, how progress is measured, and how the build pauses and resumes without losing anything. Where this file and `BUILD_BRIEF.md` disagree, `BUILD_BRIEF.md` wins and this file is the bug.

---

## 1. The three layers of the blueprint, and who may change each

| Layer | Files | Who may change it | How a change becomes valid |
|---|---|---|---|
| **Inputs** (frozen) | `sources/` | Nobody. Fingerprinted in `kit/sources.sha256`; `Ci` fails on any change | Never. Corrections go into `docs/decisions.md` and the spec, with the superseded line listed in `docs/sources.md` |
| **Blueprint** (locked) | `BUILD_BRIEF.md`, `CLAUDE.md`, `docs/constitution.md`, `docs/sources.md`, `docs/spec/*.md`, `docs/test-design.md`, `docs/threat-model.md`, `docs/code-docs.md`, this file, `docs/catalog/*.yaml`, `kit/tasks.json`, `kit/prompts/TEMPLATE.md`, `kit/layers.json`, `kit/allowed_crates.txt` and `kit/superseded_decisions.json` (protected paths today; they join `kit/plan.lock` in the plan change after D-491, because the Governance check runs `main`'s lock script, which must first learn a new locked file), every script in `scripts/`, and sections 3 to 6 of `docs/architecture.md` (module ownership, dependency rules, the shared kernel, the job state machine) | A **plan-change session**: a session Casey starts for that purpose, or a `harness` task with an Opus review. Never a `build`, `docs`, `cleanup` or `audit` task through the runner, except the single locked file such a task explicitly owns in `kit/tasks.json` | The pull request rewrites `kit/plan.lock` and appends a `D-###` entry naming what changed and why; for a changed requirement, the entry names its ID. `Ci` fails otherwise (`scripts/check_plan_lock.py`, `scripts/check_pr_change.py`) |
| **Work product** | `app/`, `tests/`, `migrations/`, the rest of `docs/` | The task that owns the path, plus the shared paths (`kit/tasks.json` → `shared_paths`) | The task's pull request passes `Ci`, `Twin`, `Ally`; the runner marks it DONE only after the squash commit's tree equals the tested head |

**Harness** (`.github/`, `.claude/`, `runner/`, `stand_ins/`, `twin/`, the protected tests and scripts) is work product that only `harness` tasks change, each with an Opus review (`BUILD_BRIEF.md:70`).

## 2. What a machine enforces, and what only a reader enforces

A rule enforced only by a document is a rule a tired session can skip. The matrix below is kept current; every row marked "reader" is a known gap with a named owner.

| Rule | Enforced by |
|---|---|
| `sources/` never changes | `Ci` fingerprint step |
| No secrets in the tree | `Ci` gitleaks step; deny rules on `.env*` (SEC-045 adds a commit-range scan at T169) |
| No real personal data in the tree | `Ci` PII scan |
| `main` takes only pull requests with the required checks green; no force-push, no deletion | GitHub ruleset `protect-main` (verified by the runner at K0, `ruleset_verified` in `kit/state.json`). **Open (E-2):** `Governance` added as required, the Actions source on every check, up-to-date branches, a `claude/kit-state` ruleset |
| The rules a pull request is checked against come from `main`, not from the branch | `Governance` check (`.github/workflows/governance.yml`, `workflow_run`) runs `main`'s `check_plan_lock.py` and `check_pr_change.py` against the head |
| A task branch changes only what its task owns; protected paths only in a harness task | `Governance` and `Ci` (`scripts/check_pr_change.py`, rule 1, `--no-renames`); the runner (`runner/verify.py`) once it runs live (D-486) |
| The blueprint changes only with a decision entry and a lock rewrite; on a task branch only a harness task or the owner of a changed locked file may rewrite the lock | `Governance` and `Ci` (`scripts/check_plan_lock.py`, `scripts/check_pr_change.py`, rule 3) |
| A plan change from a non-task branch names its decision | `Governance` and `Ci` (`Plan-Change: D-###` trailer, rule 5). **Reader** for authorship until Claude has its own GitHub identity (E-1) |
| A requirement line changes only with a decision naming its ID; an ID is never removed | `Governance` and `Ci` (rules 3 and 4) |
| `docs/decisions.md` is append-only, entries byte-identical and in order; no D-number on two entries | `Governance` and `Ci` (rule 2) |
| Every requirement traces to a source line; every `D-###` it names exists; no superseded value | `Ci` (`scripts/trace_spec.py`) |
| Every live requirement and test-design entry is owned by a task; no withdrawn ID is named; no two concurrent tasks own one file; protected and shared paths are as listed | `Ci` (`scripts/validate_tasks.py`) |
| Every task prompt matches `kit/tasks.json` and the template | `Ci` (`scripts/gen_prompts.py --check`) |
| Fee rules and worked examples agree; legal drafts carry their phrases; copy passes the AI-smell and copy checks | `Ci` (`check_catalog.py`, `check_legal_docs.py`, `check_ai_smell.py`, `check_copy.py`) |
| A check that exists on `main` cannot be skipped by deleting its inputs | `Ci` (the skip guard on `Cargo.toml`, lock files, `rust-toolchain.toml`, `deny.toml`, `tests/coverage_floor.json`) |
| Rust: formatted, clippy clean with warnings denied and no `#[allow]` without a reason, `unsafe` forbidden, no wildcard arm in a status module, no TODO or `dbg!`, tests green, dependencies audited and licensed, lock files committed | `Ci` (`cargo fmt`, `clippy -D warnings -F clippy::allow_attributes_without_reason`, `scripts/check_rust_rules.py`, `cargo test`, `cargo audit`, `cargo deny`) |
| Only approved crates | `Ci` (`scripts/check_crates.py` against the protected `kit/allowed_crates.txt`) |
| Only `api` is public across crates; the layer table is `kit/layers.json`; no cross-schema query or foreign key; `ReleaseProof` cannot be built outside `payments`; status enums are exhaustively matched | Compiler plus the architecture test and trybuild cases (T009, SEC-029 to SEC-031) |
| Coverage never drops; 95% branch floor on money, privacy, licensing | `Ci` from T169, which now precedes the first vault and licensing code |
| The end-to-end journeys and the accessibility suite are required checks | `Ci` e2e job and `Ally` from T173 |
| The twin is a hard-failing oracle | `Twin` from T172 (stand-alone twin) and T074 (against the app's rules) |
| Every Rust file and public item is documented; rule code names its requirement IDs; `CLAUDE.md` stays under 200 lines | `Ci` (`scripts/check_code_docs.py`) |
| The model named in the task is the model that ran; a task is DONE only when its PR merged green and `main`'s tree equals the tested head | Runner code (`runner/verify.py`, `runner/cycle.py`; `python3 -m runner`, D-486) |
| A push to a task branch gets auto-merge only after the runner's review | `auto-pr.yml` reads `reviewed_sha` and the review lease from `claude/kit-state` and `claude/lease` with `runner/verify.py` from `main`; the runner alone turns auto-merge on (D-486) |
| The full suite runs on `main` nightly and its result is on record | `nightly-main` writes `kit/nightly.json` to `claude/kit-state` |
| An owner gate clears only on a short, unnegated reply carrying its phrase; a comment performed through a GitHub App is never an owner answer | `runner/outbox.py`; `inbox.yml`. **Reader** for a session posting as the owner through other means until E-1 |
| Live keys, live hosts, `main`, force-push, `claude/kit-state` deletion, issue comments, statuses, check runs and rulesets are unreachable from a session | Deny rules in `.claude/settings.json` (pattern rules; a session that edits a protected file could widen them, which the path rule catches) |
| "Fix main": revert, never patch over; `main_red` clears only on a verified green run | Runner code (`runner/cycle.py`: FIX-MAIN task, `smoke_runs`, `main_red_history`; drills in `tests/test_failure_drills.py`, D-486) |
| At most `lanes` tasks in flight (`kit/runner.json`, 1 unless Casey sets 2), one per lane; nothing claimed on a lane while its task is MERGE_PENDING; tasks side by side never own the same file, sit in different stages or include a cleanup task; a lease is honoured; a branch brought up to date with `main` carries exactly the reviewed work plus `main` | Runner code: the lease ref `claude/lease` pushed atomically with the state (D-486); `runner/lanes.py`, `runner/update_merge.py` and the per-lane concurrency groups of `backup-runner.yml` (D-620) |
| The conflict rule (changes file wins on what, brief on how) | **Reader**, tightened in section 4 |
| Progress is counted in verified requirements | `scripts/progress.py --markdown`, written into STATUS.md on every state commit (D-486) |

## 3. Decisions: how a choice becomes binding

1. Every choice that is not a plain restatement of a source line is one appended line in `docs/decisions.md`: `- **D-###** Question: … Choice: … Reason: … Source: path:LINE`. Numbers never repeat; entries are never edited. A later entry supersedes an earlier one by saying so.
2. A decision that changes a requirement also changes the requirement line (same PR) and names the ID; one that changes a boundary also changes `docs/architecture.md` sections 3 to 6 (same PR) and rewrites `kit/plan.lock`.
3. **Who decides.** Inside the stops in `BUILD_BRIEF.md` section 16, Casey. Everywhere else, the session decides by the order of authority and logs it, and the item goes to `docs/for-casey-review.md` with the ones most worth a second look first. Casey may reverse any logged decision by a plan-change session; the reversal is a new entry.
4. **Architecture change.** A change to a module's ownership, a layer (`kit/layers.json`), an invariant, a status, a timeout, the vault readers, an adapter or a pinned version is an architecture change. It requires: a `D-###` entry, the architecture file and `kit/layers.json` updated in the same PR, the plan lock rewritten, and an Opus fresh-eyes review. On a task branch only a harness task may make one (`scripts/check_pr_change.py`); any other task that finds the architecture wrong reports it and the plan-change session decides.
5. **New dependency.** A crate or npm package not in `docs/architecture.md` section 11 needs a `D-###` entry with purpose, license and the audit result, and must pass `cargo deny` (`BUILD_BRIEF.md:122`).
6. **Numbering.** The runner reserves numbers: each claim gives the task a block of five in `decision_ids` (in the hand-off) and advances `next_decision` in `kit/state.json`, and the worker numbers its entries only from that block, in order (D-530). A change made outside the runner (a plan-change session, a human fix) takes the next number above both `main`'s highest entry and the state's `next_decision`, and bumps nothing; gaps in the numbering are allowed. If two changes still take one number, the duplicate check fails whichever lands second (rule 2), and that change renumbers its entry.

## 4. The conflict rule, tightened

`BUILD_BRIEF.md:539` lets a session decide a conflict between the changes file and the brief by "what versus how". To stop a session from reclassifying any disagreement to get the answer it prefers:

- **What** is defined mechanically: anything that changes a requirement line, a test-design expected value, a fee, a timing, a consent or privacy rule, or customer wording. It is never a judgement call, and the changes file always wins on it; the brief can never override a what-line. **How** is only: language, library, layout, process, tooling, model, review and test mechanics, and the brief wins on it.
- Decide-and-log covers only questions the sources leave open. It never permits departing from an explicit rule in `BUILD_BRIEF.md` sections 2, 7, 12 or the section 4 stack: such a departure makes the task `BLOCKED` with an end-of-build item, never a logged decision.
- A conflict the session cannot place in one bucket with a one-sentence reason is **not decided by the session**: the task takes the option that changes nothing observable, logs the conflict as `open` in the decision entry (`Kind: conflict`, both lines quoted verbatim with `path:LINE`), and the item goes to the top of `docs/for-casey-review.md`.

## 5. Red `main`

`main` is red when `Ci`, `Twin` or `Ally` fails on `main`'s head, the nightly run fails, the runner finds the squash tree differs from the tested head, or a task's merged head is not the head the runner reviewed (nor exactly that work combined with `main`, as GitHub's "Update branch" makes it), so work nobody reviewed is reverted (D-584).

1. The runner sets `main_red` in `kit/state.json` with the failing check and SHA. No task is eligible but `FIX-MAIN`.
2. `FIX-MAIN` is a harness-class task: Sonnet worker, Opus reviewer, allowed to change only the files of the offending merge, and its first option is always `git revert` of that squash commit. Patching over is allowed only when the revert itself would break `main` (the entry says why).
3. `main_red` clears only when the runner verifies, with its own tool run, that the smoke test is green on the new `main` head and that `kit/nightly.json`, if its SHA is the current head, says success.
4. The reverted task returns to the queue with its attempt count raised; the original branch is kept for evidence.

## 6. Progress and milestones

**Progress is counted in verified requirements, never in tasks or estimates.** `scripts/progress.py` computes it from `docs/spec/`, `kit/tasks.json`, `kit/state.json` and `kit/nightly.json`:

- a requirement is **verified** when every task that names it is DONE, every test-design entry covering it is owned by a DONE task, and the nightly full run on `main` was green at or after the last of those merges;
- **partial** when some but not all of its tasks are DONE;
- **unstarted** otherwise; withdrawn requirements are not counted.

The runner pastes `scripts/progress.py --markdown` at the top of STATUS.md every cycle. Task counts stay as a throughput line.

**Milestones are stages, and each has an exit test** (`docs/build-readiness.md` section 3 holds the table). A stage is complete when its cleanup task is DONE, every requirement whose tasks all sit in that stage or earlier is verified, and the stage's named proof (for example, the twin gate linked to the app's rules in stage 3; the ledger matching the Stripe mock over 1,000 simulated jobs) has run in `Ci` or nightly on `main`. The stage's cleanup task prompt names that proof and the runner refuses to mark it DONE without it. Where every clause of a stage's row is a test in the nightly suite (stage 1 today, `runner/proof.py`), the runner records the proof itself, and only once a green nightly run on `main` contains every merge of the stage and those tests; until then that stage's cleanup task, merged and waiting only for the proof, does not hold the later tasks of its own stage, whose work the proof needs (D-551). A cleanup task that finds nothing to change hands in nothing; any other task's empty hand-in is a failed attempt (D-551).

## 7. Reviews, audits and regression

- **Every plan change, including an audit's:** an independent Opus review of its own diff before merge (D-484 found 16 defects in the audit that wrote this file).
- **Every task:** a fresh-eyes reviewer that sees only the diff and the task's spec files (Opus for money, privacy, security, recovery, harness and any diff touching a locked architecture section; Sonnet otherwise). Findings go back once; a second round of findings blocks the hand-in.
- **Every stage:** the cleanup task (dead code, duplicates, docs current) and, from stage 3 on, the twin chaos run.
- **Every night:** the full suite, randomized property tests, cargo-mutants on the rule modules; a red run makes `main` red.
- **Before the account checkpoint and before "Finished":** a fresh Opus audit of the whole codebase against the spec, the threat model and ASVS L2 (T125, T140); no open Critical or High finding. The threat model is re-reviewed at each audit.
- **Before "Finished":** the cold-start drill (T171): a session that has never seen the code makes three real changes from the documentation alone.
- **Regression:** no test is ever weakened or deleted to pass; a wrong test changes only with a spec change and its decision (`BUILD_BRIEF.md:69`). A flaky test is quarantined for at most 48 hours and never deleted.

## 8. Persistent state: resuming without loss

A fresh session needs these, and nothing in chat:

| What | Where | Written by |
|---|---|---|
| Task statuses, lease, attempts, gates, owner answers, `main_red`, usage readings | `kit/state.json` on `claude/kit-state` | the runner only |
| What happened, in plain words, cycle by cycle | `PROGRESS.md` on `claude/kit-state` | the runner only |
| Casey's view: progress, blocks, next owner action | `STATUS.md` on `claude/kit-state` | the runner only |
| Nightly result on `main` | `kit/nightly.json` on `claude/kit-state` | the `nightly-main` workflow |
| Every choice and why | `docs/decisions.md` on `main` | the task or plan-change PR that made it |
| The approved blueprint's fingerprint | `kit/plan.lock` on `main` | the plan-change PR; when `main` is merged into a task branch and both sides rewrote it, the runner's update merge recomputes it, only from two locks that each matched their own tree (D-620) |
| Open risks, owners and resolution criteria | `docs/risk-register.md` on `main` | plan-change PRs and audits |
| Readiness verdict and stage exit tests | `docs/build-readiness.md` on `main` | plan-change PRs and audits |
| How to operate as a session | `docs/ai-operating-manual.md` on `main` | plan-change PRs |

Start-of-session order is in `docs/ai-operating-manual.md`. If any of these files is missing or unreadable, the session's only task is to report that (outbox) and stop.

## 9. Risks

Every open planning or build risk lives in `docs/risk-register.md` with an ID, owner (Claude, Casey, automatic, or outside), the mitigation already in the plan, and a resolution criterion that a tool run or a written answer can satisfy. A risk is closed only by a dated note citing that evidence. New risks found by an audit or a failed task are added in the same PR that reports them.

## 10. Readiness

No build task (T002 onward; T001 was absorbed by D-479) is claimed until `docs/build-readiness.md` records READY with every Critical and High risk closed or accepted by Casey in writing, Gate P is answered `approved to build`, and this file's section 2 shows no "reader"-enforced rule that a build task could break in its first stage. The readiness file is re-issued at each stage boundary and before Gate C.
