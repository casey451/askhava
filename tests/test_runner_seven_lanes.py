"""Seven lanes (D-3269): the five-lane drills of tests/test_runner_five_lanes.py over `"lanes": 7`, so the lane
count kit/runner.json holds is a count the drills cover (tests/test_runner_lanes.py). Seven claims on
non-overlapping tasks and an eighth run claiming nothing, the lane check against six tasks in flight, refill
naming up to seven lanes, a strike on lane 7 falling back to one lane and `restore` giving all seven back,
MERGE_PENDING in six lanes, the gate's pick, the same seven claims with exclusive crates on, and a lost-race
storm at seven lanes retried once per other lane with no strike.

Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest

import check_state as cs
from runner import __main__ as entry, cycle, lanes, state as st
from test_failure_drills import NOW, FakeEnv, config
from test_runner_two_lanes import _pick, _rejected, by_head, handed_off, staged

SEVEN = ("1", "2", "3", "4", "5", "6", "7")


def seven_lane(graph: list[dict[str, Any]], tmp_path: Path, session: str, hint: str | None, **kw: Any) -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path, session, **kw), lanes=7, lane_hint=hint)


def eight_crates() -> list[dict[str, Any]]:
    """T001 and T002 overlap (app/a/), then one task per crate b to h."""
    return [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/a/sub/"]),
            *[staged(f"T00{n}", owns=[f"app/{c}/"]) for n, c in zip(range(3, 10), "bcdefgh")]]


@pytest.mark.parametrize("exclusive", [False, True])
def test_drill_seven_lanes_claim_seven_non_overlapping_tasks_and_an_eighth_run_claims_nothing(tmp_path: Path, exclusive: bool) -> None:
    graph = eight_crates()
    env = FakeEnv(handed_off)
    for n, lane in enumerate(SEVEN):
        cfg = dataclasses.replace(seven_lane(graph, tmp_path, f"s{lane}", lane), exclusive_crates=exclusive)
        assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=n)) == "handed_off"
    at = NOW + timedelta(minutes=7)
    held = {lane: lease["task"] for lane, lease in st.live_leases(env.store, at).items()}
    assert held == {"1": "T001", "2": "T003", "3": "T004", "4": "T005", "5": "T006", "6": "T007", "7": "T008"}
    assert sorted(env.store["leases"]) == [*SEVEN] and lanes.free_lanes(7, env.store, at) == []
    assert cs.check(env.store, lanes=7, now=at) == []
    assert any("7 live leases (1, 2, 3, 4, 5, 6, 7) but lanes is 6" in p for p in cs.check(env.store, lanes=6, now=at))
    assert cycle.run_cycle(seven_lane(graph, tmp_path, "s8", None), env, NOW + timedelta(minutes=8)) == "lease_live"


def test_the_seventh_lane_checks_a_candidate_against_all_six_tasks_in_flight() -> None:
    flying = [staged(f"T00{n}", owns=[f"app/{c}/"]) for n, c in zip(range(1, 7), "abcdef")]
    assert lanes.refusal(staged("T007", owns=["app/f/x.rs"]), flying, set(), None) == "owns what T006 owns (app/f/x.rs ~ app/f/)"
    assert lanes.refusal(staged("T008", "3", owns=["app/g/"]), flying, set(), None) == "T001 is in stage 2, not stage 3"
    assert lanes.refusal(staged("T009", owns=["app/h/"]), flying, set(), None) == ""
    assert lanes.crate_clash(staged("T010", owns=["app/f/CLAUDE.md"]), flying) != ""  # a shared path: the lane check allows it
    assert lanes.crate_clash(staged("T009", owns=["app/h/"]), flying) == ""


def test_refill_lanes_names_up_to_seven_lanes() -> None:
    graph = [*[staged(f"T00{n}", owns=[f"app/{c}/"]) for n, c in zip(range(1, 8), "abcdefg")], staged("T008", owns=["app/a/x.rs"])]
    cfg = seven_lane(graph, Path("."), "s1", None)
    s = st.sync_graph(st.new_state(), graph)
    assert cycle.refill_lanes(cfg, s, NOW) == [*SEVEN]
    s = st.claim(s, "T001", "a", NOW)
    assert cycle.refill_lanes(cfg, s, NOW) == [*SEVEN[1:]]
    for tid, lane in (("T002", "3"), ("T003", "4"), ("T004", "5"), ("T005", "6"), ("T006", "7")):
        s = st.claim(s, tid, tid.lower(), NOW, lane=lane)
    assert cycle.refill_lanes(cfg, s, NOW) == ["2"]
    assert cycle.refill_lanes(dataclasses.replace(cfg, lanes=6), s, NOW) == []


def test_drill_two_harness_failures_on_lane_seven_fall_back_to_one_lane_and_restore_gives_seven(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    _rejected(env, "! [rejected] head0001 -> claude/task/T001-x (non-fast-forward)")
    cfg = seven_lane(graph, tmp_path, "s1", "7")
    assert cycle.run_cycle(cfg, env, NOW) == "push_failed"
    assert env.store["lane_strikes"]["7"]["count"] == 1 and not env.store.get("lanes_fallback")
    assert cycle.run_cycle(dataclasses.replace(cfg, session="s2"), env, NOW + timedelta(minutes=5)) == "push_failed"
    assert env.store["lanes_fallback"]["lane"] == "7"
    assert lanes.effective_lanes(7, env.store) == 1 and lanes.free_lanes(7, env.store, NOW) == ["1"]
    for lane in SEVEN[1:]:
        assert cycle.run_cycle(seven_lane(graph, tmp_path, f"t{lane}", lane), env, NOW + timedelta(minutes=6)) == cycle.NO_FREE_LANE
    restored = lanes.restore(env.store)
    assert restored is not None and lanes.effective_lanes(7, restored) == 7 and lanes.free_lanes(7, restored, NOW) == [*SEVEN]


def test_drill_merge_pending_in_six_lanes_leaves_only_the_seventh_to_claim(tmp_path: Path) -> None:
    graph = [staged(f"T00{n}", owns=[f"app/{c}/"]) for n, c in zip(range(1, 9), "abcdefgh")]
    env = FakeEnv(by_head)
    for n, lane in enumerate(SEVEN[:6]):
        assert cycle.run_cycle(seven_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=5 * n)) == st.MERGE_PENDING
    later = NOW + timedelta(minutes=26)
    six = {f"T00{n}": str(n) for n in range(1, 7)}
    assert st.in_flight(env.store, later) == six and st.live_leases(env.store, later) == {}
    assert lanes.free_lanes(7, env.store, later) == ["7"] and st.run_lane(env.store, later, 7) == "7"
    env.worker = handed_off
    assert cycle.run_cycle(seven_lane(graph, tmp_path, "s7", "7"), env, later) == "handed_off"
    assert st.in_flight(env.store, later) == {**six, "T007": "7"} and cs.check(env.store, lanes=7, now=later) == []


def test_the_gate_picks_the_seventh_lane_when_six_hold_tasks(tmp_path: Path) -> None:
    later = NOW.replace(year=2030)  # the snippet uses the real clock: leases far in the future are live
    s = st.new_state()
    for n in range(1, 8):
        s = st.with_task(s, f"T00{n}")
    assert _pick(tmp_path, s, 7) == ["1", "7", *SEVEN]
    six = st.claim(s, "T001", "a", later)
    for n in range(2, 7):
        six = st.claim(six, f"T00{n}", f"x{n}", later, lane=str(n))
    assert _pick(tmp_path, six, 7) == ["7", "7", "7"]
    assert _pick(tmp_path, six, 7, "7") == ["7", "7", "7"] and _pick(tmp_path, six, 7, "8") == ["7", "7", "7"]
    assert _pick(tmp_path, six, 6, "7") == ["1", "6", "-"]  # lane 7 is not served while the setting is 6


def test_a_lost_race_storm_at_seven_lanes_is_retried_once_per_other_lane_and_never_strikes(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("RUNNER_WORKER_CMD", raising=False)
    env = FakeEnv()
    waits: list[float] = []
    env.sleep = waits.append  # type: ignore[attr-defined]  # reason: record the waits instead of sleeping
    results: list[str] = []
    monkeypatch.setattr(cycle, "run_cycle", lambda cfg, e, now: results.append("lost_race") or "lost_race")
    assert entry.lost_race_retries(2) == entry.LOST_RACE_RETRIES == 2 and entry.lost_race_retries(7) == 6
    assert entry.cycle_with_retries(seven_lane([staged("T001")], tmp_path, "s7", "7"), env) == "lost_race"  # type: ignore[arg-type]  # reason: FakeEnv stands in for GitHubEnv
    assert len(results) == 7 and len(waits) == 6 and all(1.0 <= w <= 15.0 for w in waits)
    assert not env.store.get("lane_strikes") and not env.store.get("lanes_fallback")
