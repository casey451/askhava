# Build readiness assessment

**Issued:** October 28, 2026, pre-build engineering audit (D-479). **Re-issued at:** each stage boundary, the account checkpoint, and before Gate C (`docs/governance.md` section 10).

## 1. Verdict

**NOT READY TO BUILD.** Two things stand between the plan and the first task, and both are deliberate:

1. **Gate P is closed by design** until the hand-run test (`docs/pilot/hand-run-test.md`) has run and the re-plan pass has merged (`docs/PREBUILD.md` section 3, items 11 to 17). Nothing in this audit changes that order.
2. **Open Critical and High risks** in `docs/risk-register.md`: R-04 (the runner's code never runs live; the scheduled session follows prose), R-14 (Claude acts as Casey on GitHub, so no owner answer or review can be told from a session's), R-01 (the 19-crate workspace is unproven as a whole), R-15 (28 security requirements to build), R-16 (prompt budgets and oversized tasks), R-17 (stage exits not mechanised). The audit report lists each with its fix and resolution criterion.

The technical blueprint is otherwise complete, consistent to the limit the checks can measure, and now machine-guarded (section 2). When the items in section 4 are done, this file is re-issued as READY by a plan-change session, with each item's evidence linked.

## 2. Evidence, as of this audit

| Area | Evidence | Result |
|---|---|---|
| Inputs frozen | `sha256sum -c kit/sources.sha256`, 104 files | pass |
| Requirements traceable | `scripts/trace_spec.py`: 674 requirements (664 live, 10 withdrawn), 0 errors | pass |
| Requirements → tasks → tests | `scripts/validate_tasks.py`: every live requirement, every TD entry (332) and every journey (J1 to J4) owned by a task; withdrawn IDs rejected; 0 errors | pass |
| Requirements → test design | every live requirement appears in a test-design row or the accessibility table (WORD-029 through TD-OPS-20's docs test is the one process-class item) | pass |
| Task prompts current | `scripts/gen_prompts.py --check`: 166 prompts, 0 problems | pass |
| Blueprint locked | `scripts/check_plan_lock.py` | pass |
| Governance rules from `main` | `.github/workflows/governance.yml` (first run on this PR) | pending E-2 to become required |
| Per-change rules | `scripts/check_pr_change.py` on this branch | pass |
| Tooling tests | `pytest tests`: 380 before this audit, plus the 23 added here | pass |
| Rust path in `Ci` | D-476 spike: fmt, clippy, build, test against Postgres, audit, deny, 12 minutes | pass (1 crate only; see R-01) |
| Rust design reviewed | Independent review of every pin and the layout; eight design defects found and decided (D-481) | decided on paper, unproven until T009 |
| Runner rehearsed | K9 (D-171): one task passed end to end, one failed and never merged, one double-triggered and resumed after a simulated usage limit; paper rehearsal D-472 reached "Finished" | pass |
| Independent review of this audit's own PR | 16 defects found in the audit's changes and fixed (D-484) | pass |
| Fresh contradiction sweeps | D-477, D-478: 27 conflicts found and fixed by an independent Opus sweep; this audit's five reviewers' findings in `docs/audit/` | see the audit |

## 3. Stage exit tests (milestone completion criteria)

A stage is complete only when every line in its row is true, verified by a tool run recorded in `PROGRESS.md`. The stage's cleanup task prompt names this row, and the runner will not mark the cleanup task DONE without the proof (`docs/governance.md` section 6).

The task list of each stage is the set of tasks whose `stage` value in `kit/tasks.json` equals the row (never a range); `python3 -c "import json;..."` or `scripts/progress.py --json` lists them (D-482).

| Stage (tasks by `stage` value) | Exit test (all must hold) |
|---|---|
| **1. Twin and catalog** (11 tasks: T002 to T008, T154, T167, T170, T172) | Twin gate exits non-zero on any non-waived invariant failure (planted failure in T172's report); every worked example in `docs/catalog/worked_examples.yaml` and `matching_examples.yaml` passes `check_catalog.py`; stand-ins answer over HTTP at the vendors' URL shapes with every contract test green; `scripts/progress.py` shows every requirement owned only by stage-1 tasks verified |
| **2. Core, cards, quote page, pick** (73 tasks: T009 to T057, T152, T153, T156, T163, T168, T169, T173, T175 to T187, T208, T213, T217, T219) | `cargo build -p askhava-server` refuses a planted cross-layer import and a planted `ReleaseProof` outside `payments` (trybuild, no feature on); migrations apply from empty in number order; a reduced J1 (steps 1 to 6: text to pick code, no payment or call) runs in Playwright against stand-ins with axe clean at 320/768/1280; coverage floors active in `Ci`; named mutants for license filter, consent, quiet hours all killed; a `timestamptz` round trip through jiff-sqlx; progress: every requirement owned only by stage ≤2 tasks verified |
| **3. Money** (23 tasks: T058 to T075, T188 to T192) | Ledger reconciles to the Stripe mock over 1,000 simulated jobs with zero drift (T071); every fee worked example passes as a unit test; cargo-mutants on `payments` and `catalog` rules reports no surviving unequivalent mutant; `Twin` runs against the app's rules through `rules_bridge` and a planted 2-month rule turns it red (T074); progress: stage ≤3 requirements verified |
| **4. Calls, checks, portals** (46 tasks: T076 to T100, T147 to T150, T155, T157, T162, T164, T165, T193 to T199, T207, T209, T212, T214, T215) | Full J1 (through the paid release, the masked call and the day-3 feedback) and J2 (business) green; authorization matrix test covers every route; masked call bridged through the fake Twilio with the PIN and the CALL-025 limits; license import and the live-lookup fixture both pass; the stage-2 sentinel scan still clean; progress: stage ≤4 requirements verified |
| **5. Website, owner tools, documents** (41 tasks: T101 to T123, T125, T126, T151, T158, T160, T161, T166, T200 to T206, T210, T211, T216, T218) | J3 (operator) and J4 (vendor outage) green; every page passes `Ally` at three widths; `check_legal_docs.py`, `check_copy.py`, `check_ai_smell.py` clean on the built pages; twin chaos run (inside T126, D-485) zero new problems against the committed baseline; fresh Opus audit (T125) no Critical or High; handover drafts exist (T111, T112, T113, T117, T122, T123); progress: every Phase 1 requirement verified |
| **Account checkpoint** (T127) | Issue "Your turn: set up accounts" open; `WAITING_FOR_OWNER`; nothing else runs |
| **Stage 0.5** (T128) | Casey's `go` recorded; the static page set (T116) published at askhava.com with `Ally` green on it; nothing depends on it |
| **6. Real sandboxes and the test site** (18 tasks: T129 to T144, T159, T171) | Every contract test green against the real sandboxes; J1 to J4 green on the test site (T136); ZAP baseline no High; restore drill with switchover passed (T139); fresh Opus audit (T140) no Critical or High; cold-start drill (T171) three changes merged; "Finished" sent |
| **Gate B** (T144) | Casey's click-through completes every J1 step and the business card-entry step with VoiceOver, with no step he could not finish and no violation he reports (the pass bar; D-482) |
| **7. Go live** (T145, T146) | Every item of OPS-042 and `docs/account-checklist.md` Part 2 ticked; live keys entered by Casey; the real-phone rehearsal completes J1 on his phone end to end (the script is T141's); the reviewed commit named (OPS-043); first live charge approved and reconciled to Stripe |

## 4. Before the first build task (ordered)

| # | Item | Owner | Done when |
|---|---|---|---|
| 1 | Merge this audit's PR (D-479, D-480) | Claude | `Ci`, `Twin`, `Ally` green; merged |
| 2 | Ruleset settings (D-483): `Governance` required, GitHub Actions as the source of all four checks, "Require branches to be up to date", "Require review from Code Owners", a `claude/kit-state` ruleset (no deletion, no force-push) | Casey (hands-on) | Ruleset JSON shows the four checks with the Actions integration, `strict: true` and code-owner review; the kit-state ruleset exists |
| 3 | The build's own identity (D-483): `claude setup-token` into the `CLAUDE_CODE_OAUTH_TOKEN` secret; D-486 then flips `backup_runner` on after a rehearsal under it | Casey (hands-on), then D-486 | A session-posted comment on an owner issue produces no inbox file; a cycle runs under the Actions runner |
| 4 | Model allocation (D-483): done in this PR | — | `validate_tasks.py` green |
| 5 | The Actions minutes quota read and logged (D-483) | Casey (one number) | Decision entry with the figure and the pacing |
| 6 | **D-486** runner live: done October 29, 2026 on the build-optimization PR (`python3 -m runner`, the GitHub `Env`, `runner_sha` on state commits, `reviewed_sha` gating of `auto-pr`, auto-merge turned on by the runner, the merge trigger on `backup-runner.yml`, the red-`main` protocol with its drill, `scripts/stage_exit.py`, per-attempt history, the deviations register, `scripts/check_state.py`, the lease on `claude/lease`, pacing with `scripts/pacing.py` and `scripts/quota_probe.py`) | Claude (done) | Drills, runner, pacing and probe tests green; `--dry-run` idle against the live repository. **Open:** one rehearsal cycle under Casey's token (item 3), the real plan limits written into `kit/pacing.json`, and the usage endpoint's shape confirmed by that rehearsal |
| 7 | **P-1** plan change: done October 29, 2026 on the same PR (D-485): prompts carry requirement, test, decision and cited text with byte-based budgets; 41 tasks split into 83 parts (T175 to T216), T217 added; the validator's new checks; the 26 testability rewrites; the 10 brief lines; photos in object storage; the PII scanner's email and image rules; the threat model's inbound-email boundary | Claude (done) | `validate_tasks.py` 0 errors, `gen_prompts.py --check` 0 problems, `trace_spec.py` clean |
| 7a | Expertise files `docs/brains/` (D-487) and the 39 requirements adopted from them (D-490, CON-036, TXT-043 to TXT-046, TERMS-029 to TERMS-032, FEE-047 to FEE-053, SEC-049 to SEC-051, A11Y-025 to A11Y-035, BIZ-055 to BIZ-057, OPS-044 to OPS-047, WORD-030, PICK-057; T218 added) | Claude (done) | Every brain principle carries a source URL or `[inferred]`; `trace_spec.py` clean on the new requirements; R-19 open for the packs outside the repository |
| 7b | Build-loop optimizations (D-489): the merge trigger, compile-only-when-code-changed in `Ci`, the Rust skeleton trial folded into T009, T012, T168 and T169 (D-488) | Claude (done) | `docs/audit/process-review-2026-10-29.md` section 6 holds the measured figures |
| 8 | Hand-run test, re-plan pass, Gate P | Casey, then Claude | PREBUILD items 11 to 17 |
| 9 | Re-issue this file as READY | Claude | Every Critical and High risk closed or accepted in writing; section 2 evidence refreshed |

## 5. What this assessment does not claim

It does not claim the business will work (that is the hand-run test's job), that Twilio or Stripe will say yes, or that the Rust estimates are right. It claims only that, once section 4 is done, every requirement has a task and a test, every rule a machine can check is checked, and a session that drifts from the blueprint cannot merge.
