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
