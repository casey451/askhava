"""The D-503 safety hand-in step of backup-runner.yml reports `released=no` whenever `--after` freed no lease
(D-3419): a session that no longer holds it (`not_lease_holder`), a usage-limit hand-in, a lost race
(`runner: lost_race`) or an invalid state (`runner: state_invalid`, checked before `--after` runs at all), so
the chain step starts no run for a lease that is still held. Reads the workflow and runner source only.
"""

from __future__ import annotations

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "backup-runner.yml"
NOT_RELEASED = ("usage_limit:*", "*not_lease_holder*", '*"runner: lost_race"*', '*"runner: state_invalid"*')


def _case_line() -> str:
    doc = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    step = next(s for s in doc["jobs"]["backup"]["steps"] if s.get("id") == "safety")
    return next(line for line in step["run"].splitlines() if line.strip().startswith('case "$outcome:$out"'))


def test_safety_hand_in_reports_unreleased_leases_as_released_no() -> None:
    released_no = _case_line().split('echo "released=no"')[0]
    for arm in NOT_RELEASED:
        assert arm in released_no, arm


def test_the_runner_prints_the_results_the_arms_match() -> None:
    main = (ROOT / "runner" / "__main__.py").read_text(encoding="utf-8")
    assert 'print(f"runner: {result}"' in main
    assert 'return "state_invalid"' in main and 'return "lost_race"' in main
