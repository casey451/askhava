"""Three lanes (D-2810): the two-lane drills of tests/test_runner_two_lanes.py over `"lanes": 3`, so the
lane count kit/runner.json may hold is a count the drills cover (tests/test_runner_lanes.py). Three
claims on non-overlapping tasks, the lane check against two tasks in flight, refill, a strike on lane 3
falling back to one lane, MERGE_PENDING in two lanes, pacing over two other lanes and the gate's pick.

Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
from datetime import timedelta
from pathlib import Path
from typing import Any

import check_state as cs
import pacing
from runner import cycle, lanes, state as st
from test_failure_drills import MODELS, NOW, FakeEnv, config
from test_runner_two_lanes import _pacing_config, _pick, _rejected, by_head, handed_off, staged


def three_lane(graph: list[dict[str, Any]], tmp_path: Path, session: str, hint: str | None, **kw: Any) -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path, session, **kw), lanes=3, lane_hint=hint)


def test_drill_three_lanes_claim_three_non_overlapping_tasks_and_a_fourth_run_claims_nothing(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/a/sub/"]), staged("T003", owns=["app/b/"]),
             staged("T004", "3", owns=["app/e/"]), staged("T005", owns=["app/c/"])]
    env = FakeEnv(handed_off)
    for n, lane in enumerate(("1", "2", "3")):
        assert cycle.run_cycle(three_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=n)) == "handed_off"
    held = {lane: lease["task"] for lane, lease in st.live_leases(env.store, NOW + timedelta(minutes=3)).items()}
    assert held == {"1": "T001", "2": "T003", "3": "T005"} and st.entry(env.store, "T005")["lane"] == "3"
    assert lanes.free_lanes(3, env.store, NOW + timedelta(minutes=3)) == []
    assert cs.check(env.store, lanes=3, now=NOW + timedelta(minutes=3)) == []
    assert any("3 live leases (1, 2, 3) but lanes is 2" in p for p in cs.check(env.store, lanes=2, now=NOW + timedelta(minutes=3)))
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s4", None), env, NOW + timedelta(minutes=4)) == "lease_live"


def test_the_third_lane_checks_a_candidate_against_both_tasks_in_flight(tmp_path: Path) -> None:
    a, b = staged("T001", owns=["app/a/"]), staged("T003", owns=["app/b/"])
    assert lanes.refusal(staged("T006", owns=["app/b/x.rs"]), [a, b], set(), None) == "owns what T003 owns (app/b/x.rs ~ app/b/)"
    assert lanes.refusal(staged("T007", "3", owns=["app/g/"]), [a, b], set(), None) == "T001 is in stage 2, not stage 3"
    assert lanes.refusal(staged("T008", owns=["app/h/"]), [a, b], set(), None) == ""
    graph = [a, b, staged("T002", owns=["app/a/x.rs"]), staged("T006", owns=["app/b/y.rs"]), staged("T008", owns=["app/h/"])]
    env = FakeEnv(handed_off)
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s1", "1"), env, NOW) == "handed_off"
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=1)) == "handed_off"
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s3", "3"), env, NOW + timedelta(minutes=2)) == "handed_off"
    assert st.lane_of(env.store, "T008") == "3" and [r[0] for r in env.runs] == ["T001", "T003", "T008"]


def test_refill_lanes_names_up_to_three_lanes() -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"]),
             staged("T004", owns=["app/a/x.rs"])]
    cfg = three_lane(graph, Path("."), "s1", None)
    s = st.sync_graph(st.new_state(), graph)
    assert cycle.refill_lanes(cfg, s, NOW) == ["1", "2", "3"]
    s = st.claim(s, "T001", "a", NOW)
    assert cycle.refill_lanes(cfg, s, NOW) == ["2", "3"]
    assert cycle.refill_lanes(cfg, st.with_task(s, "T003", status=st.DONE), NOW) == ["2"]  # T004 overlaps T001
    s = st.claim(s, "T002", "b", NOW, lane="3")
    assert cycle.refill_lanes(cfg, s, NOW) == ["2"]
    assert cycle.refill_lanes(dataclasses.replace(cfg, lanes=2), s, NOW) == []


def test_drill_two_harness_failures_on_lane_three_fall_back_to_one_lane(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    _rejected(env, "! [rejected] head0001 -> claude/task/T001-x (non-fast-forward)")
    cfg = three_lane(graph, tmp_path, "s1", "3")
    assert cycle.run_cycle(cfg, env, NOW) == "push_failed"
    assert env.store["lane_strikes"]["3"]["count"] == 1 and not env.store.get("lanes_fallback")
    assert cycle.run_cycle(dataclasses.replace(cfg, session="s2"), env, NOW + timedelta(minutes=5)) == "push_failed"
    fell = env.store["lanes_fallback"]
    assert fell["lane"] == "3" and len(fell["why"]) == 2
    assert "kind: info" in (tmp_path / "lanes-fallback.md").read_text(encoding="utf-8")
    assert lanes.effective_lanes(3, env.store) == 1 and lanes.free_lanes(3, env.store, NOW) == ["1"]
    assert st.run_lane(env.store, NOW, 3) == "1"
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s3", "2"), env, NOW + timedelta(minutes=6)) == cycle.NO_FREE_LANE
    restored = lanes.restore(env.store)
    assert restored is not None and lanes.free_lanes(3, restored, NOW) == ["1", "2", "3"]


def test_strikes_count_per_lane_so_one_on_lane_two_and_one_on_lane_three_do_not_fall_back() -> None:
    s = lanes.strike(lanes.strike(st.new_state(), "2", "one", NOW), "3", "two", NOW)
    assert not s.get("lanes_fallback") and s["lane_strikes"]["2"]["count"] == 1 and s["lane_strikes"]["3"]["count"] == 1
    assert lanes.strike(s, "3", "three", NOW)["lanes_fallback"]["lane"] == "3"


def test_drill_merge_pending_in_two_lanes_leaves_only_the_third_to_claim(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"]),
             staged("T004", owns=["app/d/"])]
    env = FakeEnv(by_head)
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s1", "1"), env, NOW) == st.MERGE_PENDING
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s2", "2"), env, NOW + timedelta(minutes=5)) == st.MERGE_PENDING
    later = NOW + timedelta(minutes=6)
    assert st.in_flight(env.store, later) == {"T001": "1", "T002": "2"} and st.live_leases(env.store, later) == {}
    assert lanes.free_lanes(3, env.store, later) == ["3"] and st.run_lane(env.store, later, 3) == "3"
    assert lanes.claim_lane(3, env.store, later, "2") is None and lanes.claim_lane(3, env.store, later, None) == "3"
    env.worker = handed_off
    assert cycle.run_cycle(three_lane(graph, tmp_path, "s3", "3"), env, later) == "handed_off"
    assert st.in_flight(env.store, later) == {"T001": "1", "T002": "2", "T003": "3"}
    assert lanes.free_lanes(3, env.store, later) == [] and cs.check(env.store, lanes=3, now=later) == []
    assert st.run_lane(env.store, later, 3) == "1"  # no lane without a live lease but the MERGE_PENDING ones: lane 1's first


def test_pacing_counts_the_estimates_of_attempts_running_in_two_other_lanes() -> None:
    s = st.with_task(st.with_task(st.with_task(st.new_state(), "T001", status=st.IN_PROGRESS), "T002", status=st.IN_PROGRESS), "T003")
    s["usage"] = {"by_task": {"T001": {"estimated_tokens": 3000}, "T002": {"estimated_tokens": 4000}}}
    assert pacing.in_flight_tokens(s, "T003") == 7000 and pacing.in_flight_tokens(s, "T001") == 4000
    task = {"id": "T003", "owns": [], "model": MODELS["sonnet"]}
    found = pacing.assess(s, task, config=_pacing_config(), now=NOW)
    assert found["in_flight"] == 7000 and found["standing"]["five_hour"]["used"] == 7000
    assert "after 7k still running in another lane" in pacing.line("T003", found)


def test_the_gate_picks_the_third_lane_when_two_hold_tasks(tmp_path: Path) -> None:
    later = NOW.replace(year=2030)  # the snippet uses the real clock: leases far in the future are live
    s = st.with_task(st.with_task(st.with_task(st.new_state(), "T001"), "T002"), "T003")
    assert _pick(tmp_path, s, 3) == ["1", "3", "1", "2", "3"]
    two = st.claim(st.claim(s, "T001", "a", later), "T002", "b", later, lane="2")
    assert _pick(tmp_path, two, 3) == ["3", "3", "3"]
    assert _pick(tmp_path, two, 3, "3") == ["3", "3", "3"] and _pick(tmp_path, two, 3, "4") == ["3", "3", "3"]
    full = st.claim(two, "T003", "c", later, lane="3")
    assert _pick(tmp_path, full, 3) == ["1", "3", "-"]
    fallen = dict(two, lanes_fallback={"at": "x", "lane": "3", "why": []})
    assert _pick(tmp_path, fallen, 3, "3") == ["1", "1", "-"]
