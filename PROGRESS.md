# Progress log

## 2026-10-27 · Runner cycle 1 · K0
- `claude/kit-state` did not exist, so this cycle ran K0.
- **Worker:** Sonnet subagent (the transcript shows only `claude-sonnet-5-5`). It wrote the deny rules (`.claude/settings.json`), fingerprinted `sources/` (`kit/sources.sha256`, 104 files, combined digest in `docs/sources.md` with the order of authority and the superseded rules) and added the `ci`, `twin`, `a11y`, `auto-pr`, `nightly-main`, `backup-runner`, `notify` and `inbox` workflows. It removed `checks-placeholder.yml`.
- **Local checks the worker ran here:** settings JSON parses; YAML parses; actionlint 1.7.7 is clean; `sha256sum -c` passes; gitleaks v8.28.0 `dir .` found nothing; twin `normal_fixed 1` exited 0 in 0.7s; the notify and inbox scripts were tested with a fake `gh`. None of the workflows have run on GitHub yet.
- **Opus fresh-eyes review:** 2 blocking findings (gaps in the deny rules for deleting kit-state and for Railway) plus 11 non-blocking ones. The worker fixed them in one round (commit 291cf8d). Follow-ups for K8 and Phase 2 are recorded in `kit/state.json` notes.
- **Pushed:** `claude/task/K0-setup` @ 291cf8d. K0 is MERGE_PENDING.
- **Next cycle:** confirm the PR merged with `Ci`, `Twin` and `Ally` green and that the tree matches. Check the ruleset with `GET /rulesets`. Then mark K0 DONE and claim K1.

## 2026-10-27 · Runner cycle 2 · K0 DONE, K1 claimed
- K0 confirmed DONE: PR #3 merged; `Ci`, `Twin`, `Ally` all success on head 291cf8d; head tree d4ee3cc9 equals squash commit 4bab4c0's tree. Ruleset `protect-main` lists Ci, Ally, Twin as required (checked via API).
- §16 migration applied: waits on K2b/K3 cleared, GATE-A marked SKIPPED. Inbox was empty.
- Smoke test on main: no app code yet; main's tree equals the tested K0 head plus the docs-only change in #4. Not red.
- K1 claimed (lease 3h), worker on claude-opus-5-5.

## 2026-10-27 · Runner cycle 2 (cont.) · K1
- Worker: Opus subagent (transcript shows only claude-opus-5-5). Wrote `docs/constitution.md` (249 lines) and `CLAUDE.md` (61 lines); citations script-checked by the worker.
- Opus fresh-eyes review: no blocking findings, 11 non-blocking; all applied in one round.
- Path check: only CLAUDE.md and docs/constitution.md changed. gitleaks is not installed in this session, so I did not run it here; CI runs it.
- Pushed `claude/task/K1-constitution` @ b2ddaa3. K1 is MERGE_PENDING.
- 2026-10-27: K1 DONE. K2 pushed (407 requirements, trace script clean; reviewer blocking finding fixed).
- 2026-10-27: K2 DONE (PR #7 merged, Ci/Twin/Ally green, tree == squash). K2b claimed.
- 2026-10-27: K2b pushed @ 8cb43dd; Opus review had 2 blocking (gas appliance handyman, one-time jobs in recurring categories) fixed in one round; checks run here: catalog check 0 errors, pytest 64 passed. K2b MERGE_PENDING.
- 2026-10-27: K2b DONE (PR #8 merged, Ci/Twin/Ally green on 8cb43dd, tree == squash 9981709). K3 claimed.
- 2026-10-27: K3 pushed @ f1234ad; 52 decisions (D-038..D-089). Opus review: 3 blocking + 8 non-blocking, all fixed in one round. Checks run here: trace_spec 0 errors, check_catalog 0 errors, pytest 64 passed, sources fingerprint ok; path check: docs only. K3 MERGE_PENDING.
- 2026-10-27: K3 DONE (PR #9 merged, Ci/Twin/Ally green on f1234ad, tree f114dc61 == squash 52fa19d). K4 claimed.
- 2026-10-27: K4 pushed @ 75610c4 (architecture, threat model, D-090..D-100). Opus review: 4 blocking + 12 non-blocking, all fixed in one round. Checks run here: trace_spec 0 errors, check_catalog 0 errors, pytest 64 passed, sources fingerprint ok; path check: docs only. K4 MERGE_PENDING.
- 2026-10-27: K4 DONE (PR #10 merged, Ci/Twin/Ally green on 75610c4, tree 3f7f5847 == squash 5b020c7).
- 2026-10-27: K5 pushed @ f4f17e0 (docs/test-design.md, docs/twin-warnings.md, D-101..D-110). Opus review: 2 blocking + 8 non-blocking, all fixed in one round. Checks run here: trace_spec 0 errors, check_catalog 0 errors, sources fingerprint ok (pytest 64 passed on first pass; not re-run after fixes); path check: docs only. K5 MERGE_PENDING.
- 2026-10-27: K5 DONE (PR #11 merged, Ci/Twin/Ally green on f4f17e0, tree d94e5454 == squash c8ffcc8). K6 claimed.
- 2026-10-27: K6 pushed @ 4e18e4c (kit/tasks.json 146 steps, validate_tasks.py, D-111..D-119). Opus review: 4 blocking + 6 non-blocking, all fixed in one round. Checks run here: validate_tasks 0 errors, trace_spec 0, check_catalog 0, fingerprint ok, pytest 115 passed; path check: kit/, scripts/, tests/, docs only. K6 MERGE_PENDING.
- 2026-10-27: K6 DONE (PR #12 merged, Ci/Twin/Ally green on 4e18e4c, tree 6738ea03 == squash aa995a7). Inbox: Casey closed test issue #6; no wait pending.
- 2026-10-27: K7 pushed @ 1175b4b (139 prompts from one template + scripts/gen_prompts.py). Sonnet worker (transcript verified), Haiku check clean, Sonnet review 0 blocking / 13 non-blocking, 5 fixed in one round. Checks run here: gen_prompts --check 0 problems, validate_tasks 0, pytest 130 passed, trace_spec 0, check_catalog 0; path check: kit/prompts, scripts, tests only. K7 MERGE_PENDING.
- 2026-10-27: K7 DONE (PR #13 merged, Ci/Twin/Ally green on 703f71d, tree d4b8274e == squash 5489a99). Inbox empty. Main smoke pending; K8 next.
- 2026-10-27: Smoke test on main: validate_tasks failed (T143 citation on blank line after Gate P shifted BUILD_BRIEF lines). Ran here: gen_prompts --check 0 problems, validate_tasks 0 errors after fix; pytest not installed in this session, so not run. "Fix main" branch claude/task/fix-main-brief-citations pushed (docs/kit only, D-120); no PR visible yet, runner opens via REST if none in 10 min. FIX1 MERGE_PENDING; K8 waits.
- 2026-10-27: FIX1 DONE (PR #15 merged, Ci/Twin/Ally green on 3817a11, tree 2a171924 == squash cf319c3). Smoke on main green: validate_tasks 0, gen_prompts 0 problems, trace_spec 0, check_catalog 0, pytest 130 passed. Inbox empty. K8 claimed (lease 3h).
- 2026-10-27: K8 pushed @ c4023ed (runner/, stand_ins/, pii_scan, failure drills, CI pytest step, D-121..D-126). Sonnet worker (transcript verified). Opus review: 5 blocking + many non-blocking, all fixed in one round. Checks run here: pytest 257 passed, validate_tasks 0, gen_prompts 0 problems, trace_spec 0, check_catalog 0, pii_scan 0; path check: no sources/ or .claude/ changes. Note: kit-task path rules to define at K9. K8 MERGE_PENDING.

- 2026-10-27 18:30Z: K8 DONE (PR #16, tree acbbe2d0 == squash 6d10456; Ci/Twin/Ally green on head c4023ed).
- 2026-10-27 18:40Z: K9 not claimed (no task prompt; needs scheduler/usage evidence). Waiting for a K9 plan.
- 2026-10-27 19:25Z: **K9 claimed and started** by the hourly runner (session 01DyjzW8Ex…). The 18:40 note said there was no K9 prompt; K1–K8 had none either, and §16 says Claude decides rather than waits, so the runner runs the rehearsal itself.
  - P3 double trigger: two claims raced from the same base; the second push was rejected non-fast-forward.
  - Cloud checks: Playwright with Chromium works in the session. There is no Docker daemon, so stripe-mock is CI-only (fallback). There is no merge trigger, so the hourly run carries the chain (fallback). Model evidence from the transcript is confirmed (D-122).
  - P1 (Haiku, model verified): PR #18 green and merged at 19:13Z; tree 365b99d7 equals the head tree.
  - P2 (Haiku, model verified): Ci red by design and not merged; PR #17 closed, branch kept.
  - P3 is set to RETRY_AFTER_RESET (reset at 20:00Z) to prove an hourly resume.
  - Next cycle: run P3, then the K9 report PR (`docs/rehearsal.md`, `kit/schedule.json`, decisions D-127 onward).
- 2026-10-27 21:40Z: **K9 handed in** at 95e21ee: `docs/rehearsal.md`, `kit/schedule.json`, decisions D-166 to D-171 and the K9 section of `for-casey-review.md`.
  - Worker: Opus (model verified, 46 records).
  - Opus fresh-eyes review: 3 blocking findings (the lease incident written backwards, an overclaimed resume proof, the merge-trigger wording) and 6 non-blocking, all fixed in one round.
  - Checks run here: pytest 257 passed; validate_tasks, trace_spec and check_catalog 0 errors; pii_scan 0; gen_prompts 0 problems; sources fingerprint ok.
  - Finding: two schedulers are firing runners. The fresh sessions worked inside this session's live K9 lease at 20:11 and 20:14. Recommendation: keep the UI routine and delete the hourly session trigger.
  - K9 is MERGE_PENDING and the lease is released.

- 2026-10-27 21:50Z runner (webhook, PR #23 closed): K9 marked DONE (Ci/Twin/Ally green on 95e21ee; tree ee0348f9 == squash 632c400). Now at Gate P, waiting for Casey.
