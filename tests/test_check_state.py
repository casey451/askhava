"""Tests for scripts/check_state.py and kit/state_schema.json (D-486). Fake data only."""

from __future__ import annotations

import json
from pathlib import Path

import check_state as cs

FIXTURE = Path(__file__).resolve().parent / "fixtures" / "kit_state_real_shape.json"


def base() -> dict:
    return {"schema": 1, "lease": None, "tasks": [{"id": "T001", "deps": [], "status": "QUEUED", "attempts": 0}]}


def test_real_shape_fixture_is_valid() -> None:
    assert cs.check(json.loads(FIXTURE.read_text(encoding="utf-8"))) == []


def test_minimal_state_and_unknown_keys_are_valid() -> None:
    assert cs.check(dict(base(), anything_else={"kept": True})) == []


def test_wrong_types_missing_keys_and_bad_status_are_reported() -> None:
    state = base()
    state["tasks"][0]["status"] = "FINISHED"
    state["lease"] = {"task": "T001", "session": "", "expires": "tomorrow"}
    problems = cs.check(state)
    assert any("state.tasks[0].status" in p for p in problems)
    assert any("state.lease.session: empty" in p for p in problems)
    assert any("state.lease.expires" in p for p in problems)
    assert cs.check({"tasks": []}) == ["state: missing schema"]
    assert cs.check([]) == ["state: expected object, got list"]


def test_history_entries_need_every_field() -> None:
    state = base()
    state["tasks"][0]["history"] = [{"attempt": 1, "model": "claude-sonnet-5-5"}]
    problems = cs.check(state)
    assert "state.tasks[0].history[0]: missing head_sha" in problems and "state.tasks[0].history[0]: missing result" in problems


def test_deviations_register_shape() -> None:
    good = {"id": "DV-1", "rule": "BUILD_BRIEF.md:69", "decision": "D-999", "expiry": "2026-12-01", "owner": "Claude"}
    assert cs.check(dict(base(), deviations=[good])) == []
    bad = dict(good)
    del bad["owner"]
    assert cs.check(dict(base(), deviations=[bad])) == ["state.deviations[0]: missing owner"]


def test_stage_proofs_main_red_and_usage() -> None:
    assert cs.check(dict(base(), stage_proofs={"2": {"evidence": ""}})) == ["state.stage_proofs.2.evidence: empty"]
    assert cs.check(dict(base(), main_red={"check": "nightly", "sha": "abc"})) == []
    assert cs.check(dict(base(), main_red=True)) == []
    assert cs.check(dict(base(), main_red={"check": "vibes", "sha": "abc"}))
    assert cs.check(dict(base(), usage={"by_task": {"T001": 5}})) == ["state.usage.by_task.T001: expected object, got int"]


def test_booleans_are_not_integers() -> None:
    state = base()
    state["tasks"][0]["attempts"] = True
    assert cs.check(state) == ["state.tasks[0].attempts: expected integer, got bool"]


def test_cross_checks_duplicate_ids_and_lease_task() -> None:
    state = base()
    state["tasks"].append({"id": "T001"})
    state["lease"] = {"task": "T999", "session": "s", "expires": "2026-11-01T12:00:00Z"}
    assert cs.check(state) == ["state.tasks: T001 appears more than once", "state.lease: names T999, which is not in tasks"]


def test_cli(tmp_path: Path) -> None:
    path = tmp_path / "state.json"
    path.write_text(json.dumps(base()))
    assert cs.main(["--state", str(path)]) == 0
    path.write_text("{not json")
    assert cs.main(["--state", str(path)]) == 1


def test_usage_source_carries_a_note_and_a_session_reading_is_allowed() -> None:
    """D-1156: the pacing source names its reason ("ledger; quota_probe: ...") or the session reading;
    the runner's first cycle after D-1128 refused the live state over the enum this replaces."""
    base = json.loads(FIXTURE.read_text(encoding="utf-8"))
    base.setdefault("usage", {})
    for ok in ("ledger", "quota_cmd", "ledger; quota_probe: no token: CLAUDE_CODE_OAUTH_TOKEN unset", "session_reading 2026-11-01T00:30:00Z (week)"):
        base["usage"]["source"] = ok
        assert not [p for p in cs.check(base) if "usage.source" in p], ok
    base["usage"]["source"] = "bogus"
    assert any("usage.source" in p for p in cs.check(base))
    base["usage"]["source"] = "ledger"
    base["usage"]["session_reading"] = {"at": "2026-11-01T00:30:00Z", "week": {"utilization": 0.55, "resets_at": "2026-11-05T17:00:00Z"}}
    assert not [p for p in cs.check(base) if "session_reading" in p]
    base["usage"]["session_reading"] = {"week": {}}
    assert any("session_reading" in p for p in cs.check(base))
