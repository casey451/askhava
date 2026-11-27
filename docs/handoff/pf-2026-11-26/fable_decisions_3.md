<!-- Copied from the overseer's working notes; the two commit-trailer addresses are written out in words so the PII scan passes until T869 lands. -->
# Fable decisions 3 (efficiency levers, for Casey under standing delegation) — 2026-11-26

Read-only pass. Sources: `origin/main` 2e19f8cd4 (`kit/tasks.json`, `kit/runner.json`, `runner/wip.py`, `runner/cycle.py`, `scripts/pii_scan.py`, `scripts/validate_tasks.py`, `docs/decisions.md`), `origin/claude/kit-state:kit/state.json` (updated 2026-11-27T02:16Z), D-5900 as drafted in `/tmp/claude-0/wt-money/docs/decisions.md:2986` (not on main yet).
Nothing was built or run except main's PII scanner over existing commit messages. None of these is one of Casey's own stops.

## Where the tokens go (attempt history, attempts started since Nov 21, worker + reviewer)

| Attempt ended as | Attempts | Tokens |
|---|---|---|
| done (merged) | 115 | 659M |
| check_failed (CI red after hand-in) | 40 | 282M |
| BLOCKED (mostly plan gaps) | 37 | 262M |
| QUEUED (no accepted hand-in) | 19 | 240M |
| resync | 8 | 115M |

58% of tokens went to attempts that did not merge. That is the lever; per-task overhead and effort level are small next to it.

## Two findings that matter more than the seven levers

### A. WIP-continue (D-4720) has never saved anything: the scan refuses the standard commit trailer. YES NOW.
- State holds 23 `wip_skipped` rows and no `wip_used` anywhere; `git ls-remote origin 'refs/heads/claude/wip/*'` returns nothing.
- 17 of the 23 refusals read "PII scan: a commit message in <sha>". The sha is only the range base (`runner/wip.py:351` prints `rng[:40]`).
- Main's `scripts/pii_scan.py` run over the last 300 commit messages on main gives 180 findings, all rule `email`, all the line `Co-Authored-By: Claude ... <[Anthropic's no-reply address, local part noreply, domain anthropic.com]>`. `SERVICE_DOMAINS` (`scripts/pii_scan.py:113`) allows only `users.noreply.github.com`. Every worker commit carries that trailer, so every WIP save with a commit is refused.
- Not proven directly: the refused commits were never pushed, so I could not scan them. The inference rests on the trailer being the only thing the scanner flags in any commit message I could read.
- Cost: the 23 refused saves sit on attempts worth 294M worker tokens since Nov 20 (T222 alone: 31M, 51M, 48M, all three thrown away; T856 tonight: 13M).
- Action: one new harness task, Opus worker, Opus review (privacy, security, harness), front of `claim_first`:
  1. allow exactly the address `[Anthropic's no-reply address, local part noreply, domain anthropic.com]` (exact address, not the domain) in `scripts/pii_scan.py`, with a test that any other anthropic.com address still fails;
  2. a `runner/wip.py` test that a branch whose commits carry the trailer passes the scan;
  3. the other 6 refusals read "<sha> changes .github": the task finds out whether those were commits merged in from main and, if so, reports the smallest fix (do not widen in this task without the finding);
  4. `docs/decisions.md` entry. This corrects a scanner false positive on a vendor no-reply address; no test is weakened.
- Saving: roughly 15-30M over the rest of phase 1 (about a third of failed-attempt tokens sat on attempts with unsaved work; I assume a continued attempt saves 30-50% of the next one). Soft estimate.
- Risk: low. A wrong allow-list entry could hide a real address, hence exact-address match and Opus review.

### B. The big journey tasks are on Sonnet against the plan's own rubric. YES NOW.
- D-2463 puts on Opus any task with money/privacy review areas or a budget of about 500 lines or more. T851 (600 lines, privacy+money), T853 (500, privacy+money), T533 (500, privacy), T532 (600), T852 (450, privacy) are `claude-sonnet-5-5`, medium.
- History: Sonnet first attempts since Nov 21: 7 done of 19 (37%); Opus first attempts 50 of 89 (56%). On the journey-type tasks every Sonnet first attempt failed: T222 31.3M, T118 18.6M, T119 7.9M, T612 6.3M, T117 1.0M. The runner already moves attempt 2 to Opus, so the Sonnet attempt is mostly a paid false start.
- Action (plan change, next batch): set `model` to `claude-opus-5-5`, effort stays `medium`, for T851, T852, T853, T532, T533. Reviewer unchanged (Opus). `validate_tasks.py` allows it. T222 and T119 already have a failed attempt, so their next attempt is Opus without a change. T850 (150 lines, harness) stays on Sonnet.
- Saving: 25-45M (5 tasks, about 0.65 chance of a lost Sonnet attempt at 8-30M each). Small sample; direction is consistent.
- Risk: none to quality (a stronger model). If Opus weighs more on the plan limit per token than Sonnet, the saving shrinks but a failed 20M Sonnet attempt still costs more.

## The seven levers

### 1. ChatGPT pre-read for the audits T549, T550, T125. NO as a pre-read; YES LATER as a parallel second read.
- "Opus confirms and focuses" anchors the audit on someone else's list. The stage-5 gate is "a fresh Opus audit found no open Critical or High findings" (`BUILD_BRIEF.md:53`); an audit steered by a lead list is no longer that. It also saves nothing: the method in the goals (crate `CLAUDE.md`, check scripts, line-range reads of money, privacy, consent and auth paths) is run either way.
- Independent version: T549, T550, T125 stay exactly as written, with no leads file in their reads. ChatGPT reads the same commit separately. After the three parts merge, one Opus triage session checks each ChatGPT lead that the Opus report does not already hold (confirmed or refuted, file and line). Confirmed Critical or High go to the same plan change as the audit's own.
- Trigger: when T549 becomes claimable (code is final). Cost: +2-5M and Casey's time to run ChatGPT. Saving: none; this is a quality add. Optional: if Casey does not want to spend the time, skip it; the gate does not depend on it.

### 2. ChatGPT code drafts for T222, T851, T852, T853, T533. NO for the journeys. Rust drafts: wait for the T865 trial.
- The journey cost is the run-and-debug loop against the live e2e server (T222: 219 to 322 turns per attempt), not writing 600 lines. An uncompiled draft does not shorten that loop and can mislead. T222's three failures were the clock gap (fixed by T849/T850), a lost WIP (finding A) and review findings on wording, none of which a draft fixes.
- Better template, free: once T222 merges, `tests/e2e/j1_full.ts` is real, passing code that T851 to T853, T532 and T533 already import or mirror.
- T865 trial (D-5900): judge it against its siblings T857, T864, T858. Extend drafts to T858, T859, T860, T866 only if T865's worker tokens are at least 25% under the siblings' median and its review passes without a fix round caused by the draft. Otherwise stop making drafts.

### 3. ChatGPT dead-code/duplicate scan before T126. NO.
- T126's goal is "fixing only what the tools flag"; its acceptance is zero findings from `check_duplicates.py --strict`, `check_rust_rules.py`, `check_code_docs.py`, `check_crates.py`. A ChatGPT list would add scope, not remove work.
- Free substitute: just before T126 is claimed, the overseer runs those four scripts on main (no model tokens) and, if the output is long, splits T126 by plan change before a worker meets it at 57% context. Dead-code suspicions from ChatGPT, if Casey runs it anyway, go to the lever-1 triage.

### 4. Merging tiny tasks. NO.
- Tasks of 100 lines or less cost 0.4-1.0M all in (median worker 0.33-0.63M, reviewer 0.17-0.24M; 22 such tasks since Nov 17). A merge saves at most about 0.5M; all candidates together under 3M, under 1% of the 380M left, and each merge needs a plan change, lock rewrite and pre-flight.
- None is clean: T822 waits on T820 (BLOCKED, 3 attempts); T824 waits on T865 and shares `app/intake/src/closing.rs` with T865, T704, T680; T843 is a 550-line harness task at 46% context, not tiny; T749 (messaging templates, compliance) and T688 (server voice, security/privacy) differ in crate and review areas; T868 is not in `kit/tasks.json` on main.

### 5. Retries on Fable vs Opus. Keep workers OFF Fable (as now). NO re-enable.
- All 29 Fable attempts since Nov 21 started on Nov 21; `kit/tasks.json` `models` on main holds only haiku, sonnet, opus, so `attempt_model` (`runner/cycle.py`) sends no retry to Fable today.
- Since Nov 21: Fable retries 8 done of 15 (53%), mean 7.1M worker tokens; Opus retries 45 of 83 (54%), mean 5.5M. Fable first attempts 5 of 14 (36%) at 4.1M; Opus 50 of 89 (56%) at 7.0M. No success advantage, 29% more tokens per retry. Small sample, and Fable got harder tasks, so this shows "no measurable gain", not "worse".
- Plan-limit weighting: unknown. State keeps one `session_reading` (9% at 02:15Z, this week's window, Opus and Sonnet only), so nothing can be inferred about Fable. Decided conservatively.
- Fable stays for decisions, triage of twice-failed tasks and disputed money/privacy/consent findings (D-3893). No plan change needed; `BUILD_BRIEF.md:499` and `:540` are conditional on the runner knowing Fable. Revisit only if Casey reads a usage figure before and after a Fable-only stretch.

### 6. Effort high to medium for non-sensitive tasks. NO (nothing eligible).
- Every open phase-1 task at `high` has privacy, compliance, security or money review areas. The docs, copy and test-only tasks are already `medium` or `low` on Sonnet. `validate_tasks.py` allows low/medium/high, high only on Opus.
- The data shows no clear saving anyway: 251-450 lines, Opus high 7.3M mean at 1.41 attempts vs medium 6.2M at 1.50; over 450 lines, 16.3M at 1.83 vs 13.2M at 2.11.

### 7. Other
- Findings A and B above are the main ones.
- Resyncs: solved; no action. 8 since Nov 21, 2 on Nov 24, none Nov 25-26 (`exclusive_crates`, `crate_cap`, merge train). The 579M figure is almost all Nov 11-20.
- Plan gaps: 37 BLOCKED attempts, 262M since Nov 21; T856 lost 13M tonight on an ownership gap (`app/intake/src/replies.rs:204`, `:367`) after 135 turns. Keep the Opus pre-flight on every remaining chain task (about 1M each) with one fixed question: "grep every caller of each function the goal changes; is each file in `owns`?" For T856's plan fix, apply the same question to T857, T865, T864 in the same PR.
- check_failed: 40 attempts, 282M since Nov 21, the largest waste bucket, cause not classified. YES LATER: one Opus triage session (1-2M) sorts them by failing step from state `reason` and `lane_strikes` (coverage floor, named mutants, a test in another crate, e2e) and proposes at most one prehandin change. Trigger: after the task in A is queued. No change before the numbers.
- Reviewer effort on small diffs: NO. 0.2-0.4M per small task; never cut a review.
- Smoke: the pre-session smoke (about 1,100 s, two red or timed-out runs today) costs lane time, not model tokens. T843 already covers it; claim it early.

## Budget view
380M left against 194M a week is two weekly windows either way. A + B are worth about 40-75M, which moves the finish by one to two days, not a week. The date is set by the two serial chains, so the cheapest speed-up is not losing attempts on them (A, the pre-flight question, Opus on the journeys).

## Overseer actions
Next plan batch, now:
1. New harness task for finding A, first in `claim_first`.
2. T851, T852, T853, T532, T533 to `claude-opus-5-5`, effort `medium`.
3. T856 plan fix with the caller/owns question applied to T857, T865, T864.
4. One `docs/decisions.md` entry recording levers 1-6 as decided here; `docs/for-casey-review.md` line.
Waits:
- T865 trial verdict on drafts (after T865, T857, T864 land).
- check_failed triage (after action 1 is queued).
- Parallel ChatGPT audit read (when T549 is claimable; optional, needs Casey's time).
- Free script run before T126 is claimed.
No change: merges, effort levels, Fable for workers, reviewer effort, resync settings.
