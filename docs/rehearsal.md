# K9 rehearsal: proof that the runner works

Written October 27, 2026, from the runner's evidence. The brief says K8 builds the runner and K9 proves it with three practice tasks (one passes, one fails on purpose, one is triggered twice) and a simulated usage-limit rejection (`BUILD_BRIEF.md:291`). The Kit Plan's done line: all three end correctly, the failing one is blocked and nothing merges (`sources/02_build_plan/Kit_Plan_Before_Coding.md:38`).

The practice tasks touched only harmless files in `kit/rehearsal/`. No app code was written. Every time below is UTC.

Most facts come from `claude/kit-state`, `main` and the PRs. A few were seen by the runner but are not in the state; each of those is marked **(runner observation, not in state)**.

## How it was run

The runner ran the rehearsal itself, without a K9 prompt file. `kit/prompts/` holds only the build tasks' `T###` prompts and their template, and the kit steps K1 to K8 ran from their Appendix B rows (`BUILD_BRIEF.md:595`–`BUILD_BRIEF.md:603`), with no prompt files either. The brief says Claude decides and logs instead of waiting (D-166).

## The three practice tasks

| Task | Meant to prove | What happened | PR |
|---|---|---|---|
| P1 | A task passes end to end | Worker ran on Haiku. The `auto-pr` workflow opened the PR. `Ci`, `Twin` and `Ally` were green on the head commit (8c0c518). It auto-merged at 19:13 as 1cac5d7, and that commit's tree (365b99d7) equals the tested head's tree. Marked `DONE`. | #18 |
| P2 | A task that fails is blocked and never merges | Worker ran on Haiku and added a test that fails on purpose. `Ci` failed; `Twin` and `Ally` were green. The PR never merged. The runner closed it and kept the branch `claude/task/P2-rehearsal-fail` as evidence. Marked `BLOCKED`. | #17 |
| P3 | A task triggered twice, and a usage-limit rejection followed by a resume | See the next two sections. It merged at 20:13 as 65e3f6d with `Ci`, `Twin` and `Ally` green on the head (eafe114), and the trees match (26e08630). Marked `DONE` at 20:14. | #21 |

Result: P1 and P3 are `DONE`, P2 is `BLOCKED`, and nothing from P2 reached `main`. This meets the Kit Plan's done line.

## Triggered twice

At 19:10 two claims were pushed to `claude/kit-state` from the same starting commit (3aab38a). Git accepted the first (bf39373, 19:10:26) and rejected the second as non-fast-forward. The rehearsal's progress was recorded in the state at 19:15 (dc1aa88). Under the brief, the runner whose push is rejected exits, because git's own rule is the lock (`BUILD_BRIEF.md:222`).

## Simulated usage limit and the resume

P3 was given a simulated usage-limit rejection and set to `RETRY_AFTER_RESET` with a reset time of 20:00 (state commit dc1aa88 at 19:15).

After the reset, P3 resumed and merged: a fresh session (...W9x5..., Sonnet) pushed P3's commit (eafe114) at 20:11 on branch `claude/task/P3-rehearsal-resume`, PR #21 merged at 20:13, and another fresh session (...W6ng...) marked it `DONE` at 20:14 (f30d043).

But those sessions **ignored a live lease** held by another session (see "Two schedulers" below). So this shows that work picks up and merges after a reset. It does not show the runner's own resume path running live. That path, through `runner/cycle.py`, is shown only by the offline drills in `tests/test_failure_drills.py`.

## The three cloud checks

The brief asks for three checks in the real cloud session, each with a fallback (`BUILD_BRIEF.md:286`–`BUILD_BRIEF.md:289`).

| Check | Outcome | What the build does |
|---|---|---|
| Browsers in the session | **Works.** Node Playwright launches the pre-installed Chromium 153. The browsers are at `/opt/pw-browsers`. The Python `playwright` package is not installed yet; it has to be installed when the app's browser tests arrive. | Browser tests can run in sessions. `Ally` still runs in GitHub Actions as before. (D-168) |
| Stripe's mock server | **Fallback.** There is no Docker daemon in the session, so `stripe/stripe-mock` can't run there. | stripe-mock runs in CI only; sessions use the twin's fake Stripe. (D-167) |
| Merges starting the next run | **Unverified, so the fallback applies.** The routine kept as the build's scheduler, "Ask Hava build runner", runs hourly at 50 past the hour. The trigger listing does not show GitHub event triggers, so whether it also has the "pull request closed, merged into `main`" trigger is unverified. Fresh sessions started at 20:11 and 20:14, right after merges; that is consistent with a merge trigger but doesn't prove one. The other scheduled task, "Ask Hava build runner (hourly)", was created through the API and has no merge trigger. | The hourly run carries the chain. (D-169) |

## Which model ran: D-122 confirmed

K8 could not test its model check in a real cloud session (D-122). K9 did:

- Each worker's transcript is a file at `~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl`. The runner session can read it, and every assistant record carries `message.model` with the **full** model ID.
- P1's transcript had 6 records and P2's had 10, all `claude-haiku-5-5`, which is the model those tasks name. P3 ran in another session, so its record count is not in this evidence.
- The call that starts a worker accepts the short names `haiku`, `sonnet` and `opus`, **not** full IDs. The brief says the model is passed as a full ID (`BUILD_BRIEF.md:244`). So the runner passes the short name and checks the full ID in the transcript, which is the check that counts (`BUILD_BRIEF.md:247`). (D-170)

## Usage measured

Plan quota itself can't be seen from inside a session. What can be measured is the tokens each worker used, from its transcript. These are the samples taken in the runner session:

| Run | Model | Turns | Output tokens | Cache read | Cache write |
|---|---|---|---|---|---|
| K0 worker | Sonnet | 29 | 29,914 | 2,587,807 | 116,108 |
| K0 review | Opus | 6 | 15,900 | 392,149 | 93,620 |
| P1 worker | Haiku | 3 | 1,183 | 113,521 | 59,239 |
| P2 worker | Haiku | 5 | 2,165 | 233,242 | 60,668 |

- **The Sonnet and Opus samples are from K0, not from K9.** During K9 no Sonnet or Opus worker ran, except for writing this report, which ran on Opus.
- **P3 has no token sample.**
- The practice tasks were tiny on purpose, so the Haiku rows say little about a real build task. The K0 rows are the closest sample of real work.

**Time per kit step** (approximate, claim to hand-in from the `claude/kit-state` log): K2 16 minutes, K2b 13, K3 18, K4 23, K5 30, K6 25, K7 20, K8 20. K0 took about 25 minutes and K1 about 10 **(runner observation, not in state)**. Every step finished inside an hour.

**The schedule** (`kit/schedule.json`): the routine fires hourly at 50 past the hour, and each firing marks the last task done and claims the next. That gives about **one task per hour**, around the clock while limits allow. A merge trigger, if it is confirmed or added, would let the next run start as soon as a merge lands, so cycles could run back to back.

## Two schedulers were firing runners

Two scheduled tasks start runners:

- **(a) "Ask Hava build runner (hourly)":** created through the API at 15:02. It runs at 7 past each hour, and it wakes one long-lived session, ...W8Ex..., which runs on Opus.
- **(b) "Ask Hava build runner":** created at 14:49 from the claude.ai routines page. It runs at 50 past each hour and starts a fresh session each time, on `claude-sonnet-5-5`, with the original Appendix A prompt.

**Who held the lease.** The K9 lease named the long-lived session ...W8Ex.... It held the lease from bf39373 through dfec090. The lease was due to expire at 22:15 and was renewed to 00:15.

**What happened inside that live lease.** Two fresh sessions from (b) did work while it was in force:

- ...W9x5... pushed P3's commit (eafe114) at 20:11, and PR #21 merged.
- ...W6ng... marked P3 `DONE` at 20:14 (f30d043).
- At 20:12 the lease holder, ...W8Ex..., also tried P3. Its push was rejected as non-fast-forward because the branch already existed, so it threw away its copy and stood down **(runner observation, not in state)**.

Git's non-fast-forward rule prevented a double merge. The lease did not stop the fresh sessions:

- The runner's own logic would have returned `lease_live` and exited (`runner/cycle.py:164`), so those sessions did not follow it.
- Appendix A's step 6 says only "claim the next eligible task" (`BUILD_BRIEF.md:582`), and K9 was not eligible to claim.

**Recommendation** (D-171):

- Keep scheduler (b): fresh sessions, with the repository and GitHub tools, on Sonnet, as Appendix A intends (`BUILD_BRIEF.md:563`).
- Delete scheduler (a).
- A live lease naming another session means stop, with no exceptions.

## Where the rehearsal departed from the brief

- **No fresh-eyes reviewer on P1 and P2.** Each changed one 6-line rehearsal file. The runner read them itself instead of starting a reviewer subagent (`BUILD_BRIEF.md:249`).
- **P2's retry was not spent live.** On failure the brief gives the same model one more try (`BUILD_BRIEF.md:271`). P2's test fails by design, so a live retry would fail the same way. The retry and Opus-escalation path is covered offline by `tests/test_failure_drills.py`.
- **P1 and P2 were in flight together**, against "one task in flight at a time" (`BUILD_BRIEF.md:224`). This was safe here, because P2 could never merge and so no branch could fall behind.

## Review

This report had one round of Opus review. Its findings were fixed in a second commit on this branch. That review is the Kit Plan's "Opus reviews" for K9 (`sources/02_build_plan/Kit_Plan_Before_Coding.md:38`).

## Smoke test on `main` (20:12, run in the runner session)

pytest: 257 passed. `validate_tasks`: 0 errors. `gen_prompts --check`: 139 prompts, 0 problems. Sources fingerprint: ok. **(runner observation, not in state)**

## Verdict

**Proven live:**

- passing, failing and blocking
- auto-PR and auto-merge
- the tree check
- git's lock against a double claim
- the model check, from transcripts, with short names in the call

**Shown only offline:** the runner's own resume path after a reset (`tests/test_failure_drills.py`). P3 did resume and merge after the reset, but in sessions that ignored a live lease.

**On the documented fallback:** stripe-mock (CI only) and merge-started runs (merge trigger unverified; the hourly run carries the chain).
