"""Four lanes (D-2986): the three-lane drills of tests/test_runner_three_lanes.py over `"lanes": 4`, so
the lane count kit/runner.json may hold is a count the drills cover (tests/test_runner_lanes.py). Four
claims on non-overlapping tasks, the lane check against three tasks in flight, refill, a strike on lane
4 falling back to one lane, MERGE_PENDING in three lanes, pacing over three other lanes and the gate's
pick.

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


def four_lane(graph: list[dict[str, Any]], tmp_path: Path, session: str, hint: str | None, **kw: Any) -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path, session, **kw), lanes=4, lane_hint=hint)


def test_drill_four_lanes_claim_four_non_overlapping_tasks_and_a_fifth_run_claims_nothing(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/a/sub/"]), staged("T003", owns=["app/b/"]),
             staged("T004", "3", owns=["app/e/"]), staged("T005", owns=["app/c/"]), staged("T006", owns=["app/d/"]),
             staged("T007", owns=["app/f/"])]
    env = FakeEnv(handed_off)
    for n, lane in enumerate(("1", "2", "3", "4")):
        assert cycle.run_cycle(four_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=n)) == "handed_off"
    at = NOW + timedelta(minutes=4)
    held = {lane: lease["task"] for lane, lease in st.live_leases(env.store, at).items()}
    assert held == {"1": "T001", "2": "T003", "3": "T005", "4": "T006"} and st.entry(env.store, "T006")["lane"] == "4"
    assert lanes.free_lanes(4, env.store, at) == []
    assert cs.check(env.store, lanes=4, now=at) == []
    assert any("4 live leases (1, 2, 3, 4) but lanes is 3" in p for p in cs.check(env.store, lanes=3, now=at))
    assert cycle.run_cycle(four_lane(graph, tmp_path, "s5", None), env, NOW + timedelta(minutes=5)) == "lease_live"


def test_the_fourth_lane_checks_a_candidate_against_all_three_tasks_in_flight(tmp_path: Path) -> None:
    a, b, c = staged("T001", owns=["app/a/"]), staged("T003", owns=["app/b/"]), staged("T005", owns=["app/c/"])
    assert lanes.refusal(staged("T006", owns=["app/c/x.rs"]), [a, b, c], set(), None) == "owns what T005 owns (app/c/x.rs ~ app/c/)"
    assert lanes.refusal(staged("T007", "3", owns=["app/g/"]), [a, b, c], set(), None) == "T001 is in stage 2, not stage 3"
    assert lanes.refusal(staged("T008", owns=["app/h/"]), [a, b, c], set(), None) == ""
    graph = [a, b, c, staged("T002", owns=["app/a/x.rs"]), staged("T006", owns=["app/c/y.rs"]), staged("T008", owns=["app/h/"])]
    env = FakeEnv(handed_off)
    for n, lane in enumerate(("1", "2", "3", "4")):
        assert cycle.run_cycle(four_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=n)) == "handed_off"
    assert st.lane_of(env.store, "T008") == "4" and [r[0] for r in env.runs] == ["T001", "T003", "T005", "T008"]


def test_refill_lanes_names_up_to_four_lanes() -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"]),
             staged("T004", owns=["app/d/"]), staged("T005", owns=["app/a/x.rs"])]
    cfg = four_lane(graph, Path("."), "s1", None)
    s = st.sync_graph(st.new_state(), graph)
    assert cycle.refill_lanes(cfg, s, NOW) == ["1", "2", "3", "4"]
    s = st.claim(s, "T001", "a", NOW)
    assert cycle.refill_lanes(cfg, s, NOW) == ["2", "3", "4"]
    done = st.with_task(st.with_task(s, "T003", status=st.DONE), "T004", status=st.DONE)
    assert cycle.refill_lanes(cfg, done, NOW) == ["2"]  # T005 overlaps T001
    s = st.claim(st.claim(s, "T002", "b", NOW, lane="3"), "T003", "c", NOW, lane="4")
    assert cycle.refill_lanes(cfg, s, NOW) == ["2"]
    assert cycle.refill_lanes(dataclasses.replace(cfg, lanes=3), s, NOW) == []


def test_drill_two_harness_failures_on_lane_four_fall_back_to_one_lane(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    _rejected(env, "! [rejected] head0001 -> claude/task/T001-x (non-fast-forward)")
    cfg = four_lane(graph, tmp_path, "s1", "4")
    assert cycle.run_cycle(cfg, env, NOW) == "push_failed"
    assert env.store["lane_strikes"]["4"]["count"] == 1 and not env.store.get("lanes_fallback")
    assert cycle.run_cycle(dataclasses.replace(cfg, session="s2"), env, NOW + timedelta(minutes=5)) == "push_failed"
    fell = env.store["lanes_fallback"]
    assert fell["lane"] == "4" and len(fell["why"]) == 2
    assert "kind: info" in (tmp_path / "lanes-fallback.md").read_text(encoding="utf-8")
    assert lanes.effective_lanes(4, env.store) == 1 and lanes.free_lanes(4, env.store, NOW) == ["1"]
    assert st.run_lane(env.store, NOW, 4) == "1"
    assert cycle.run_cycle(four_lane(graph, tmp_path, "s3", "3"), env, NOW + timedelta(minutes=6)) == cycle.NO_FREE_LANE
    restored = lanes.restore(env.store)
    assert restored is not None and lanes.free_lanes(4, restored, NOW) == ["1", "2", "3", "4"]


def test_strikes_count_per_lane_so_one_each_on_lanes_two_three_and_four_do_not_fall_back() -> None:
    s = lanes.strike(lanes.strike(lanes.strike(st.new_state(), "2", "one", NOW), "3", "two", NOW), "4", "three", NOW)
    assert not s.get("lanes_fallback") and all(s["lane_strikes"][n]["count"] == 1 for n in ("2", "3", "4"))
    assert lanes.strike(s, "4", "four", NOW)["lanes_fallback"]["lane"] == "4"


def test_drill_merge_pending_in_three_lanes_leaves_only_the_fourth_to_claim(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"]),
             staged("T004", owns=["app/d/"]), staged("T005", owns=["app/e/"])]
    env = FakeEnv(by_head)
    for n, lane in enumerate(("1", "2", "3")):
        assert cycle.run_cycle(four_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=5 * n)) == st.MERGE_PENDING
    later = NOW + timedelta(minutes=11)
    assert st.in_flight(env.store, later) == {"T001": "1", "T002": "2", "T003": "3"} and st.live_leases(env.store, later) == {}
    assert lanes.free_lanes(4, env.store, later) == ["4"] and st.run_lane(env.store, later, 4) == "4"
    assert lanes.claim_lane(4, env.store, later, "3") is None and lanes.claim_lane(4, env.store, later, None) == "4"
    env.worker = handed_off
    assert cycle.run_cycle(four_lane(graph, tmp_path, "s4", "4"), env, later) == "handed_off"
    assert st.in_flight(env.store, later) == {"T001": "1", "T002": "2", "T003": "3", "T004": "4"}
    assert lanes.free_lanes(4, env.store, later) == [] and cs.check(env.store, lanes=4, now=later) == []
    assert st.run_lane(env.store, later, 4) == "1"  # no lane without a live lease but the MERGE_PENDING ones: lane 1's first


def test_pacing_counts_the_estimates_of_attempts_running_in_three_other_lanes() -> None:
    s = st.new_state()
    for tid in ("T001", "T002", "T003"):
        s = st.with_task(s, tid, status=st.IN_PROGRESS)
    s = st.with_task(s, "T004")
    s["usage"] = {"by_task": {"T001": {"estimated_tokens": 2000}, "T002": {"estimated_tokens": 3000},
                              "T003": {"estimated_tokens": 4000}}}
    assert pacing.in_flight_tokens(s, "T004") == 9000 and pacing.in_flight_tokens(s, "T001") == 7000
    task = {"id": "T004", "owns": [], "model": MODELS["sonnet"]}
    found = pacing.assess(s, task, config=_pacing_config(), now=NOW)
    assert found["in_flight"] == 9000 and found["standing"]["five_hour"]["used"] == 9000
    assert "after 9k still running in another lane" in pacing.line("T004", found)


def test_the_gate_picks_the_fourth_lane_when_three_hold_tasks(tmp_path: Path) -> None:
    later = NOW.replace(year=2030)  # the snippet uses the real clock: leases far in the future are live
    s = st.new_state()
    for tid in ("T001", "T002", "T003", "T004"):
        s = st.with_task(s, tid)
    assert _pick(tmp_path, s, 4) == ["1", "4", "1", "2", "3", "4"]
    three = st.claim(st.claim(st.claim(s, "T001", "a", later), "T002", "b", later, lane="2"), "T003", "c", later, lane="3")
    assert _pick(tmp_path, three, 4) == ["4", "4", "4"]
    assert _pick(tmp_path, three, 4, "4") == ["4", "4", "4"] and _pick(tmp_path, three, 4, "5") == ["4", "4", "4"]
    full = st.claim(three, "T004", "d", later, lane="4")
    assert _pick(tmp_path, full, 4) == ["1", "4", "-"]
    fallen = dict(three, lanes_fallback={"at": "x", "lane": "4", "why": []})
    assert _pick(tmp_path, fallen, 4, "4") == ["1", "1", "-"]
