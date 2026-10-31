# Session hand-off, October 31, 2026 (afternoon)

For the next Claude session that looks after the build. Read CLAUDE.md, then docs/handoff/2026-10-29-evening.md, docs/handoff/2026-10-30-day.md, docs/handoff/2026-10-30-afternoon.md, docs/handoff/2026-10-30-evening.md, docs/handoff/2026-10-31-midday.md and this note. This one wins where they differ.

## Where the build is (about 21:40Z, 2:40 pm Phoenix)
- 72 of 228 tasks are done (count only `T###` ids in `kit/state.json`). T185 merged; T156 merged and was then **reverted** (see the incident below) and returns to the queue on attempt 2 when `main` clears; T046 (quotes: window, fields, bounces, all-quotes-in) is MERGE_PENDING on PR #200 and merges once `main` is green.
- `main` went red for the first time at 19:43Z. The revert (FIX-MAIN, PR #204) merged at about 20:52Z; the runner's verify cycle was still running when this note was written (run 41243009732). If `main_red` is still set when you start, read the incident section and the latest `runner_log` row before doing anything.
- Casey's instructions today, in force: **fix problems without asking** and tell him afterwards in the regular report (13:02 pm); keep the 15-minute checks with a report every 30; run until the plan's weekly cap and then wait for Thursday's reset (14:14 pm) — no third lane while the cap is the constraint.
- Budget: Casey read about 50% of the weekly plan used at 14:10 pm while the runner's ledger stood at 102M weighted tokens, so `kit/pacing.json` `budget_tokens_week` is now 200M (D-1110). Expect the pacer to stop claiming around 30 tasks from now and wait until 2026-11-05T18:38Z; that is intended. The quota probe (`scripts/quota_probe.py`) still falls back to the ledger (`state.usage.source` = "ledger") — finding out why is an open item.
- Finish estimate: the pacer said Wednesday morning before the budget calibration; with the Thursday pause, realistically the weekend of Nov 7–8. Casey knows.

## The red-main incident (19:43Z–21:xxZ) and what it exposed
1. **Cause.** T156's head had two complete CI run sets because `ci.yml`, `twin.yml` and `a11y.yml` fired on both `push` and `pull_request`. One set was green and auto-merge merged on it; the other set's Coverage job failed on a flaky adapters test (`voice_client_dials_from_the_pool_over_http` → `NotSent`). `runner/verify.py check_done` requires every run on the head to be green, so main went red (check "tree") and FIX-MAIN was queued.
2. **FIX-MAIN attempt 1 (PR #199) failed** because `git revert` of the squash removed T156's five decision entries and `check_pr_change.py` enforces append-only. Attempt 2 (PR #204) restored `docs/decisions.md` on its own and merged.
3. **Fixes merged today (all reviewed by an independent Opus reviewer):**
   - D-1087 (#203): one CI run per head — `pull_request` trigger dropped from the three check workflows (push on `claude/**` and `dependabot/**` only; a PR from any other branch prefix gets no checks and cannot merge); `governance.yml` skips a head that already belongs to a merged PR (it used to post a false failure after every squash merge).
   - D-1086 (#205): the flaky test's cause — the test helper `stand_in()` picked a port, closed it, started the Python stand-in server and called it ready on any TCP connect; a parallel test could take the port. It now waits for the server's own "listening" line, with a 10 s limit and up to five starts. The same old helper still exists in `app/ai/src/tests/adapter_calls.rs` (open item).
   - D-1088 (#202): the FIX-MAIN goal tells the worker to revert, then `git checkout origin/main -- docs/decisions.md`, then append its own entry (and a `Plan-Change` trailer if a protected or locked path is touched).
   - D-1099 (#206): a FIX-MAIN BLOCKED for any reason while main is red is re-armed by the runner after a 30-minute cool-off (attempts 0, same files, every earlier failure in its goal), one outbox item per retry, escalation after 3 retries; when the revert already landed and smoke is still red it escalates at once instead of re-running the revert; `--requeue FIX-MAIN --reason` is allowed while main is red; the D-498 false-red withdrawal ignores a re-armed/requeued FIX-MAIN (a reviewer-found hole). Governance §5 point 5.
   - D-1064 (#198): PII scanner text limit 1 MB → 4 MB; `docs/decisions.md` was 1.2 KB under the old limit and the scanner fails a file it cannot scan.
   - D-1075 (#201): context budgets keep a 3-point band so a growing `docs/architecture.md` (read whole by 207 prompts) no longer re-renders ~90 prompts on every plan change; strict mode fails only over the limit or outside the band; `pii_scan.size_warnings`/`size_stops` warn at 75% and fail the plan-change check at 90% of `MAX_BYTES`.
   - D-1110 (#207): pacer weekly budget 200M (above).
4. **A trap I set off twice.** Six quick PR merges (#202–#207) each fired a `pull_request: closed` runner run whose pending backup job was replaced by the next one (`backup-runner.yml` concurrency group; GitHub replaces a pending job in a group even with `cancel-in-progress: false`), so no runner cycle completed between 20:45Z and 21:19Z and `main_red` stayed set with FIX-MAIN already DONE. Space your merges (one every ~10 minutes while the runner is mid-cycle), or treat a fix to that pattern as a harness item for when budget allows.
5. **Merge hygiene.** Twice I pushed a branch with conflict markers in `docs/for-casey-review.md` and the new decision entry not last, because a chained shell command committed and pushed before I had looked at the conflict. Resolve, run `check_pr_change.py --base origin/main`, read the output, then push — as separate steps.

## Numbering (read before any plan change)
- `main`'s highest entry is D-1110; the runner's `next_decision` is 1058 (it rises when the next task is claimed: a claim starts at max(`next_decision`, main's highest + 1, highest reserved + 1) in blocks of 5, so the next block is D-1111–1115).
- The approved rule (D-1058) is `python3 -m runner --reserve-decision --reason "..."` from an origin/main checkout. In this session's environment the permission layer refuses the write to `claude/kit-state` ("Modify Shared Resources"); I did not route around it, and `docs/ai-operating-manual.md` step 4 names the fallback: the interim rule (above the next TWO claim blocks, counting every open plan PR's number as taken), stated in the entry. Try the command first in your session; if it works, say so in the entry and the fallback is over.
- Keep the new entry LAST in `docs/decisions.md` after every merge of main (a plain merge often puts it before main's newest entries, and Governance fails the append-only rule). Run the full `gen_prompts.py` only if `--check --strict` complains; since D-1075 it usually does not.

## Watch for
- T156 measures 45% against its build limit of 45 (D-1036). Once it merges again, the next task merge pushes it to 46 and every plan change fails `--check --strict` until T156 gets the `CONTEXT_LIMIT_HISTORY` allowance D-1036 made for T005–T007 (`scripts/validate_tasks.py`); do that in the next plan change, not a higher limit for live tasks.
- "not owned by T###" in a `last_failure` → plan gap → plan change in the #164/#186 pattern; under D-1058 the first such failure costs no attempt.
- The ruleset requires branches up to date: a green PR that shows `mergeable_state: behind` needs `git merge origin/main` pushed (and the entry kept last). The runner does this for task PRs; you do it for yours.
- Smoke false reds: the flaky adapters test is fixed; if a smoke red appears, read the failing step before anything else — the runner log keeps only the step's last line (open item: keep the full log).
- A new red main: D-1099 means the runner retries and escalates by itself; your job is to read the outbox item and the failing step, and to tell Casey plainly.

## Open items, in rough priority
1. Why `scripts/quota_probe.py` falls back in the runner (then the pacer reads the real plan usage).
2. The old `stand_in` helper in `app/ai/src/tests/adapter_calls.rs:312` (same race as D-1086).
3. Runner runs replacing each other's pending jobs after rapid merges (above).
4. Fresh FIX-MAIN branch names run only `-a2`..`-a11` (`runner/github_env.py:1045`); with D-1099 a FIX-MAIN can reach 12 attempts.
5. Smoke step full-log retention.
6. Three lanes (`claude/runner-lanes-3`, worktree `/home/claude/askhava-wt-lanes3`, stale) — only once the plan cap is no longer the constraint and stage 3 is open.
7. Which task wires `ai::api` into intake is still OPEN for Casey (noted in T048's prompt).

## Taking over
- The old session's next check is the scheduled task named "Ask Hava 15-minute build check (report every 30)" (`list_triggers`); delete it, then start your own chain with `send_later` (one chain only, alternate silent and report).
- Each check: `git fetch --depth 1 origin claude/kit-state`; read `kit/state.json` from `FETCH_HEAD` (tasks is a list; DONE count of `T###` ids; IN_PROGRESS/MERGE_PENDING/BLOCKED with lane/attempts/reason/last_failure/decision_ids; `next_decision`; `main_red`; `lanes_fallback`; `pacing`), the `kit/runner_log.jsonl` tail, open `claude/*` PRs through REST (`gh api 'repos/casey451/askhava/pulls?state=open'`; GraphQL is blocked), recent `backup-runner.yml` runs. Never cancel a run without reading `lease.json` on `claude/lease`; never clear a lease without Casey's exact words.
- Task titles: `kit/tasks.json`. Reports: 2–3 plain lines with the done count of 228 and what each running task does for the product.
- Changes go builder agent → independent Opus reviewer that did not write it → merge main (entry last) → checks → push → REST PR → `gh api -X PUT repos/casey451/askhava/pulls/N/ccr/auto_merge -f merge_method=squash`. Budget is tight this week: one reviewer per change, builders only for real problems.
