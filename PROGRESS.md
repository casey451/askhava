# Progress log

## 2026-10-27 · Runner cycle 1 · K0
- `claude/kit-state` did not exist, so this cycle ran K0.
- **Worker:** Sonnet subagent (the transcript shows only `claude-sonnet-5-5`). It wrote the deny rules (`.claude/settings.json`), fingerprinted `sources/` (`kit/sources.sha256`, 104 files, combined digest in `docs/sources.md` with the order of authority and the superseded rules) and added the `ci`, `twin`, `a11y`, `auto-pr`, `nightly-main`, `backup-runner`, `notify` and `inbox` workflows. It removed `checks-placeholder.yml`.
- **Local checks the worker ran here:** settings JSON parses; YAML parses; actionlint 1.7.7 is clean; `sha256sum -c` passes; gitleaks v8.28.0 `dir .` found nothing; twin `normal_fixed 1` exited 0 in 0.7s; the notify and inbox scripts were tested with a fake `gh`. None of the workflows have run on GitHub yet.
- **Opus fresh-eyes review:** 2 blocking findings (gaps in the deny rules for deleting kit-state and for Railway) plus 11 non-blocking ones. The worker fixed them in one round (commit 291cf8d). Follow-ups for K8 and Phase 2 are recorded in `kit/state.json` notes.
- **Pushed:** `claude/task/K0-setup` @ 291cf8d. K0 is MERGE_PENDING.
- **Next cycle:** confirm the PR merged with `Ci`, `Twin` and `Ally` green and that the tree matches. Check the ruleset with `GET /rulesets`. Then mark K0 DONE and claim K1.
