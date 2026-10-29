"""Stage A of the two-lane runner (D-514): the lane map, its mirror of `lease`, the lane-aware lease
primitives, scripts/check_state.py's lane checks and auto_merge_reason over both lease formats.
Only lane "1" is ever claimed by the cycle in this stage; lanes > 1 appear here at primitive level.
Fake data only.
"""

from __future__ import annotations

import json
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest
import yaml

import check_state as cs
from runner import __main__ as entry, state as st, verify

NOW = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "tests" / "fixtures" / "kit_state_real_shape.json"


def two_tasks() -> st.State:
    return st.with_task(st.with_task(st.new_state(), "T001"), "T002")


def lane_map() -> st.State:
    """A state in the D-514 shape: `leases` present, every lane free."""
    return st.with_leases_map(two_tasks())


def test_old_format_state_loads_as_lane_one() -> None:
    old = json.loads(FIXTURE.read_text(encoding="utf-8"))
    assert "leases" not in old
    lanes = st.leases_of(old)
    assert list(lanes) == ["1"] and lanes["1"] == dict(old["lease"], lane="1")
    assert st.lease_in(old) is old["lease"]
    assert cs.check(st.with_leases_map(old)) == []
    assert st.leases_of(st.new_state()) == {"1": None}


def test_lane_one_writes_keep_an_old_state_in_its_old_shape() -> None:
    s = st.claim(two_tasks(), "T001", "a", NOW)
    assert "leases" not in s and s["lease"]["task"] == "T001"


def test_leases_mirror_lease_through_claim_renew_and_release() -> None:
    s = st.claim(lane_map(), "T001", "a", NOW)
    assert s["lease"] == {"task": "T001", "session": "a", "expires": "2026-11-01T15:00:00Z"}
    assert s["leases"]["1"] == dict(s["lease"], lane="1")
    later = NOW + timedelta(minutes=30)
    s = st.renew(s, "T001", "a", later)
    assert s["leases"]["1"] == dict(s["lease"], lane="1") and st.parse(s["lease"]["expires"]) == later + timedelta(hours=3)
    assert cs.check(s, now=later) == []
    freed = st.release_expired(s, later + timedelta(hours=3, minutes=1))
    assert freed["lease"] is None and freed["leases"]["1"] is None
    assert st.status(freed, "T001") == st.QUEUED


def test_release_expired_with_two_lanes_frees_only_the_expired_one() -> None:
    s = st.claim(lane_map(), "T001", "a", NOW)
    s = st.claim(s, "T002", "b", NOW + timedelta(hours=1), lane="2")
    out = st.release_expired(s, NOW + timedelta(hours=3, minutes=1))
    assert out["lease"] is None and out["leases"]["1"] is None
    assert out["leases"]["2"]["task"] == "T002" and out["leases"]["2"]["lane"] == "2"
    assert st.status(out, "T001") == st.QUEUED and st.status(out, "T002") == st.IN_PROGRESS
    assert st.entry(out, "T001")["stale_leases"][0]["session"] == "a"
    assert "stale_leases" not in st.entry(out, "T002")


def test_claim_is_refused_on_a_busy_lane_and_allowed_on_a_free_one() -> None:
    s = st.claim(lane_map(), "T001", "a", NOW)
    with pytest.raises(st.LeaseError, match="lease on T001 is live"):
        st.claim(s, "T002", "b", NOW + timedelta(minutes=5))
    assert st.free_lanes(s, NOW, 2) == ["2"] and st.free_lanes(s, NOW, 1) == []
    both = st.claim(s, "T002", "b", NOW + timedelta(minutes=5), lane="2")
    assert st.lane_of(both, "T002") == "2" and st.lane_of(both, "T001") == "1"
    assert set(st.live_leases(both, NOW + timedelta(minutes=6))) == {"1", "2"}
    assert st.free_lanes(both, NOW + timedelta(minutes=6), 2) == []
    assert both["lease"]["task"] == "T001"
    assert cs.check(both, lanes=2, now=NOW + timedelta(minutes=6)) == []


def test_a_task_never_holds_two_lanes_and_a_lane_name_is_checked() -> None:
    s = st.claim(lane_map(), "T001", "a", NOW)
    requeued = st.with_task(s, "T001", status=st.QUEUED)
    with pytest.raises(st.LeaseError, match="already holds lane 1"):
        st.claim(requeued, "T001", "b", NOW, lane="2")
    with pytest.raises(st.LeaseError, match="not a lane"):
        st.claim(lane_map(), "T001", "a", NOW, lane="0")
    assert st.lane_ok("fix") and st.lane_ok("3") and not st.lane_ok("x")


def test_renewal_is_per_lane() -> None:
    s = st.claim(lane_map(), "T002", "b", NOW, lane="2")
    assert not st.renewal_due(s, NOW + timedelta(minutes=30)) and st.renewal_due(s, NOW + timedelta(minutes=30), "2")
    with pytest.raises(st.LeaseError):
        st.renew(s, "T002", "b", NOW + timedelta(minutes=30))
    renewed = st.renew(s, "T002", "b", NOW + timedelta(minutes=30), "2")
    assert renewed["leases"]["2"]["expires"] == "2026-11-01T15:30:00Z" and renewed["lease"] is None


def test_the_fix_lane_is_never_a_free_numbered_lane() -> None:
    s = st.claim(st.with_task(lane_map(), st.FIX_MAIN), st.FIX_MAIN, "f", NOW, lane="fix")
    assert st.free_lanes(s, NOW, 1) == ["1"]
    assert cs.check(s, lanes=1, now=NOW) == []


# ---- scripts/check_state.py


def test_check_state_flags_a_task_in_two_leases() -> None:
    s = st.claim(lane_map(), "T001", "a", NOW)
    s["leases"]["2"] = dict(s["leases"]["1"], lane="2")
    problems = cs.check(s, lanes=2, now=NOW)
    assert any("T001 holds more than one lane" in p for p in problems)


def test_check_state_flags_too_many_live_leases() -> None:
    s = st.claim(st.claim(lane_map(), "T001", "a", NOW), "T002", "b", NOW, lane="2")
    problems = cs.check(s, now=NOW)
    assert any("2 live leases (1, 2) but lanes is 1" in p for p in problems)
    assert cs.check(dict(s, lanes=2), now=NOW) == []
    assert cs.check(s, now=NOW + timedelta(hours=4)) == []  # expired leases are not live


def test_check_state_flags_a_lease_that_does_not_mirror_lane_one_and_an_unknown_task() -> None:
    s = st.claim(lane_map(), "T001", "a", NOW)
    drifted = dict(s, lease=None)
    assert any("differs from state.leases.1" in p for p in cs.check(drifted, now=NOW))
    ghost = st.with_lease(lane_map(), "2", {"task": "T999", "session": "x", "expires": "2026-11-01T15:00:00Z"})
    assert any("state.leases.2: names T999" in p for p in cs.check(ghost, lanes=2, now=NOW))
    bad = dict(lane_map(), leases={"2": {"task": "T002", "session": "", "expires": "soon"}})
    assert any("state.leases.2.expires" in p for p in cs.check(bad, now=NOW))


# ---- runner/verify.py: auto_merge_reason over both lease formats


def reviewed(lane: str = "1") -> tuple[st.State, dict[str, object]]:
    s = st.claim(lane_map(), "T002", "b", NOW, lane=lane)
    review = dict(st.lease_in(s, lane) or {})
    s = st.with_task(s, "T002", reviewed_sha="good0001", review_lease=review, branch="claude/task/T002-x")
    return s, review


def test_auto_merge_reason_accepts_a_single_lease_or_the_lane_map() -> None:
    s, _ = reviewed()
    branch = "claude/task/T002-x"
    assert verify.auto_merge_reason(s, branch, "good0001", s["lease"]) == ""
    assert verify.auto_merge_reason(s, branch, "good0001", s["leases"]) == ""
    assert verify.auto_merge_reason(s, branch, "good0001", None) == ""
    other = {"task": "T001", "session": "a", "expires": "2026-11-01T15:00:00Z"}
    assert verify.auto_merge_reason(s, branch, "good0001", other) == "the lease is held by T001"
    assert verify.auto_merge_reason(s, branch, "good0001", {"1": dict(other, lane="1")}) == "the lease is held by T001"


def test_auto_merge_reason_reads_the_tasks_own_lane_from_the_map() -> None:
    s, review = reviewed("2")
    assert review["lane"] == "2"
    s = st.claim(s, "T001", "a", NOW)  # lane 1 now held by another task
    branch = "claude/task/T002-x"
    assert verify.auto_merge_reason(s, branch, "good0001", st.leases_of(s)) == ""
    assert verify.auto_merge_reason(s, branch, "good0001", s["lease"]) == "the lease is held by T001"  # old shape: lane 1
    taken = st.with_lease(s, "2", {"task": "T001", "session": "a", "expires": "2026-11-01T15:00:00Z"})
    assert verify.auto_merge_reason(taken, branch, "good0001", st.leases_of(taken)) == "the lease is held by T001"
    assert verify.lane_lease({}, "1") is None and verify.lane_lease({"1": None}, "1") is None


def auto_pr_decision(tmp_path: Path, state: st.State, lease_doc: dict[str, object]) -> str:
    """Run auto-pr.yml's own decision snippet over runner/verify.py, as the workflow does."""
    steps = yaml.safe_load((ROOT / ".github" / "workflows" / "auto-pr.yml").read_text(encoding="utf-8"))["jobs"]["auto-pr"]["steps"]
    script = next(s["run"] for s in steps if s.get("id") == "decide")
    code = script.split("python3 -I -c '", 1)[1].split("\n' ", 1)[0]
    (tmp_path / "state.json").write_text(json.dumps(state))
    (tmp_path / "lease.json").write_text(json.dumps(lease_doc))
    args = [sys.executable, "-I", "-c", code, str(ROOT / "runner" / "verify.py"), str(tmp_path / "state.json"),
            str(tmp_path / "lease.json"), "claude/task/T002-x", "good0001"]
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout.strip()


def test_auto_pr_passes_the_lane_map_or_the_old_single_lease(tmp_path: Path) -> None:
    s, _ = reviewed("2")
    s = st.claim(s, "T001", "a", NOW)
    assert auto_pr_decision(tmp_path, s, {"lease": s["lease"], "leases": st.leases_of(s), "seq": 3}) == ""
    assert auto_pr_decision(tmp_path, s, {"lease": s["lease"], "seq": 3}) == "the lease is held by T001"
    one, _ = reviewed()
    assert auto_pr_decision(tmp_path, one, {"lease": one["lease"], "seq": 3}) == ""
    assert auto_pr_decision(tmp_path, one, {"lease": None}) == ""


# ---- kit/runner.json


def test_lanes_setting_defaults_to_one(tmp_path: Path) -> None:
    f = tmp_path / "runner.json"
    assert entry.lanes(f) == 1
    for raw, want in (('{"lanes": 2}', 2), ('{"owner_review": false}', 1), ('{"lanes": 0}', 1), ('{"lanes": true}', 1), ('{"lanes": "2"}', 1)):
        f.write_text(raw)
        assert entry.lanes(f) == want
    repo_settings = Path(__file__).resolve().parent.parent / "kit" / "runner.json"
    assert entry.lanes(repo_settings) == 1
