"""Five lanes (D-3043): the four-lane drills of tests/test_runner_four_lanes.py over `"lanes": 5`, so
the lane count kit/runner.json may hold is a count the drills cover (tests/test_runner_lanes.py). Five
claims on non-overlapping tasks, the lane check against four tasks in flight, refill, a strike on lane
5 falling back to one lane and `restore` giving all five back, MERGE_PENDING in four lanes, pacing over
four other lanes, the gate's pick, and the workflows a hand-in starts queueing rather than cancelling
another lane's checks when five hand in together.

Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
from datetime import timedelta
from pathlib import Path
from typing import Any

import yaml

import check_state as cs
import pacing
from runner import cycle, lanes, state as st
from test_failure_drills import MODELS, NOW, FakeEnv, config
from test_runner_two_lanes import _pacing_config, _pick, _rejected, _workflow, by_head, handed_off, staged

ROOT = Path(__file__).resolve().parent.parent
FIVE = ("1", "2", "3", "4", "5")


def five_lane(graph: list[dict[str, Any]], tmp_path: Path, session: str, hint: str | None, **kw: Any) -> cycle.Config:
    return dataclasses.replace(config(graph, tmp_path, session, **kw), lanes=5, lane_hint=hint)


def test_drill_five_lanes_claim_five_non_overlapping_tasks_and_a_sixth_run_claims_nothing(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/a/sub/"]), staged("T003", owns=["app/b/"]),
             staged("T004", "3", owns=["app/e/"]), staged("T005", owns=["app/c/"]), staged("T006", owns=["app/d/"]),
             staged("T007", owns=["app/f/"]), staged("T008", owns=["app/g/"])]
    env = FakeEnv(handed_off)
    for n, lane in enumerate(FIVE):
        assert cycle.run_cycle(five_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=n)) == "handed_off"
    at = NOW + timedelta(minutes=5)
    held = {lane: lease["task"] for lane, lease in st.live_leases(env.store, at).items()}
    assert held == {"1": "T001", "2": "T003", "3": "T005", "4": "T006", "5": "T007"}
    assert st.entry(env.store, "T007")["lane"] == "5" and sorted(env.store["leases"]) == [*FIVE]
    assert lanes.free_lanes(5, env.store, at) == []
    assert cs.check(env.store, lanes=5, now=at) == []
    assert any("5 live leases (1, 2, 3, 4, 5) but lanes is 4" in p for p in cs.check(env.store, lanes=4, now=at))
    assert cycle.run_cycle(five_lane(graph, tmp_path, "s6", None), env, NOW + timedelta(minutes=6)) == "lease_live"


def test_the_fifth_lane_checks_a_candidate_against_all_four_tasks_in_flight(tmp_path: Path) -> None:
    flying = [staged("T001", owns=["app/a/"]), staged("T003", owns=["app/b/"]), staged("T005", owns=["app/c/"]),
              staged("T006", owns=["app/d/"])]
    assert lanes.refusal(staged("T007", owns=["app/d/x.rs"]), flying, set(), None) == "owns what T006 owns (app/d/x.rs ~ app/d/)"
    assert lanes.refusal(staged("T008", "3", owns=["app/g/"]), flying, set(), None) == "T001 is in stage 2, not stage 3"
    assert lanes.refusal(staged("T009", owns=["app/h/"]), flying, set(), None) == ""
    graph = [*flying, staged("T002", owns=["app/a/x.rs"]), staged("T007", owns=["app/d/y.rs"]), staged("T009", owns=["app/h/"])]
    env = FakeEnv(handed_off)
    for n, lane in enumerate(FIVE):
        assert cycle.run_cycle(five_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=n)) == "handed_off"
    assert st.lane_of(env.store, "T009") == "5" and [r[0] for r in env.runs] == ["T001", "T003", "T005", "T006", "T009"]


def test_refill_lanes_names_up_to_five_lanes() -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"]), staged("T003", owns=["app/c/"]),
             staged("T004", owns=["app/d/"]), staged("T005", owns=["app/e/"]), staged("T006", owns=["app/a/x.rs"])]
    cfg = five_lane(graph, Path("."), "s1", None)
    s = st.sync_graph(st.new_state(), graph)
    assert cycle.refill_lanes(cfg, s, NOW) == [*FIVE]
    s = st.claim(s, "T001", "a", NOW)
    assert cycle.refill_lanes(cfg, s, NOW) == ["2", "3", "4", "5"]
    done = st.with_task(st.with_task(st.with_task(s, "T003", status=st.DONE), "T004", status=st.DONE), "T005", status=st.DONE)
    assert cycle.refill_lanes(cfg, done, NOW) == ["2"]  # T006 overlaps T001
    s = st.claim(st.claim(st.claim(s, "T002", "b", NOW, lane="3"), "T003", "c", NOW, lane="4"), "T004", "d", NOW, lane="5")
    assert cycle.refill_lanes(cfg, s, NOW) == ["2"]
    assert cycle.refill_lanes(dataclasses.replace(cfg, lanes=4), s, NOW) == []


def test_drill_two_harness_failures_on_lane_five_fall_back_to_one_lane_and_restore_gives_five(tmp_path: Path) -> None:
    graph = [staged("T001", owns=["app/a/"]), staged("T002", owns=["app/b/"])]
    env = FakeEnv(by_head)
    _rejected(env, "! [rejected] head0001 -> claude/task/T001-x (non-fast-forward)")
    cfg = five_lane(graph, tmp_path, "s1", "5")
    assert cycle.run_cycle(cfg, env, NOW) == "push_failed"
    assert env.store["lane_strikes"]["5"]["count"] == 1 and not env.store.get("lanes_fallback")
    assert cycle.run_cycle(dataclasses.replace(cfg, session="s2"), env, NOW + timedelta(minutes=5)) == "push_failed"
    fell = env.store["lanes_fallback"]
    assert fell["lane"] == "5" and len(fell["why"]) == 2
    assert "kind: info" in (tmp_path / "lanes-fallback.md").read_text(encoding="utf-8")
    assert lanes.effective_lanes(5, env.store) == 1 and lanes.free_lanes(5, env.store, NOW) == ["1"]
    assert st.run_lane(env.store, NOW, 5) == "1"
    for lane in ("2", "3", "4", "5"):
        assert cycle.run_cycle(five_lane(graph, tmp_path, f"t{lane}", lane), env, NOW + timedelta(minutes=6)) == cycle.NO_FREE_LANE
    restored = lanes.restore(env.store)
    assert restored is not None and not restored.get("lanes_fallback")
    assert lanes.effective_lanes(5, restored) == 5 and lanes.free_lanes(5, restored, NOW) == [*FIVE]


def test_strikes_count_per_lane_so_one_each_on_lanes_two_to_five_do_not_fall_back() -> None:
    s = st.new_state()
    for lane in ("2", "3", "4", "5"):
        s = lanes.strike(s, lane, f"one on {lane}", NOW)
    assert not s.get("lanes_fallback") and all(s["lane_strikes"][n]["count"] == 1 for n in ("2", "3", "4", "5"))
    assert lanes.strike(s, "5", "second on 5", NOW)["lanes_fallback"]["lane"] == "5"


def test_drill_merge_pending_in_four_lanes_leaves_only_the_fifth_to_claim(tmp_path: Path) -> None:
    graph = [staged(f"T00{n}", owns=[f"app/{c}/"]) for n, c in zip(range(1, 7), "abcdef")]
    env = FakeEnv(by_head)
    for n, lane in enumerate(("1", "2", "3", "4")):
        assert cycle.run_cycle(five_lane(graph, tmp_path, f"s{lane}", lane), env, NOW + timedelta(minutes=5 * n)) == st.MERGE_PENDING
    later = NOW + timedelta(minutes=16)
    four = {"T001": "1", "T002": "2", "T003": "3", "T004": "4"}
    assert st.in_flight(env.store, later) == four and st.live_leases(env.store, later) == {}
    assert lanes.free_lanes(5, env.store, later) == ["5"] and st.run_lane(env.store, later, 5) == "5"
    assert lanes.claim_lane(5, env.store, later, "4") is None and lanes.claim_lane(5, env.store, later, None) == "5"
    env.worker = handed_off
    assert cycle.run_cycle(five_lane(graph, tmp_path, "s5", "5"), env, later) == "handed_off"
    assert st.in_flight(env.store, later) == {**four, "T005": "5"}
    assert lanes.free_lanes(5, env.store, later) == [] and cs.check(env.store, lanes=5, now=later) == []
    assert st.run_lane(env.store, later, 5) == "1"  # no lane without a live lease but the MERGE_PENDING ones: lane 1's first


def test_pacing_counts_the_estimates_of_attempts_running_in_four_other_lanes() -> None:
    s = st.new_state()
    for tid in ("T001", "T002", "T003", "T004"):
        s = st.with_task(s, tid, status=st.IN_PROGRESS)
    s = st.with_task(s, "T005")
    s["usage"] = {"by_task": {"T001": {"estimated_tokens": 2000}, "T002": {"estimated_tokens": 3000},
                              "T003": {"estimated_tokens": 4000}, "T004": {"estimated_tokens": 5000}}}
    assert pacing.in_flight_tokens(s, "T005") == 14000 and pacing.in_flight_tokens(s, "T001") == 12000
    task = {"id": "T005", "owns": [], "model": MODELS["sonnet"]}
    found = pacing.assess(s, task, config=_pacing_config(), now=NOW)
    assert found["in_flight"] == 14000 and found["standing"]["five_hour"]["used"] == 14000
    assert "after 14k still running in another lane" in pacing.line("T005", found)


def test_the_gate_picks_the_fifth_lane_when_four_hold_tasks(tmp_path: Path) -> None:
    later = NOW.replace(year=2030)  # the snippet uses the real clock: leases far in the future are live
    s = st.new_state()
    for tid in ("T001", "T002", "T003", "T004", "T005"):
        s = st.with_task(s, tid)
    assert _pick(tmp_path, s, 5) == ["1", "5", *FIVE]
    four = st.claim(s, "T001", "a", later)
    for tid, lane in (("T002", "2"), ("T003", "3"), ("T004", "4")):
        four = st.claim(four, tid, tid.lower(), later, lane=lane)
    assert _pick(tmp_path, four, 5) == ["5", "5", "5"]
    assert _pick(tmp_path, four, 5, "5") == ["5", "5", "5"] and _pick(tmp_path, four, 5, "6") == ["5", "5", "5"]
    assert _pick(tmp_path, four, 4, "5") == ["1", "4", "-"]  # lane 5 is not served while the setting is 4
    full = st.claim(four, "T005", "e", later, lane="5")
    assert _pick(tmp_path, full, 5) == ["1", "5", "-"]
    fallen = dict(four, lanes_fallback={"at": "x", "lane": "5", "why": []})
    assert _pick(tmp_path, fallen, 5, "5") == ["1", "1", "-"]


def test_lane_five_has_its_own_backup_concurrency_group_that_never_cancels() -> None:
    backup = _workflow()["jobs"]["backup"]
    group = backup["concurrency"]["group"]
    assert backup["concurrency"]["cancel-in-progress"] is False
    assert "format('backup-runner-lane-{0}', needs.gate.outputs.lane)" in group and "'backup-runner'" in group


def _jobs_with_concurrency(doc: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    found = [("(workflow)", doc["concurrency"])] if isinstance(doc.get("concurrency"), dict) else []
    return found + [(name, job["concurrency"]) for name, job in doc["jobs"].items() if isinstance(job.get("concurrency"), dict)]


def test_no_workflow_a_hand_in_starts_cancels_another_lanes_run_when_five_hand_in_together() -> None:
    """A hand-in push to claude/task/** starts ci.yml, a11y.yml, twin.yml and auto-pr.yml; ci's end starts
    governance.yml and backup-runner.yml. GitHub's concurrent-job cap queues the excess; nothing a lane
    starts may cancel another lane's checks (runner/verify.py needs every run of a required check green)."""
    names = ("ci", "a11y", "twin", "governance", "auto-pr")
    for name in names:
        doc = yaml.safe_load((ROOT / ".github" / "workflows" / f"{name}.yml").read_text(encoding="utf-8"))
        for where, conc in _jobs_with_concurrency(doc):
            assert conc.get("cancel-in-progress") is False, f"{name}.yml {where} cancels in progress"
    cancelling = [where for where, conc in _jobs_with_concurrency(_workflow()) if conc.get("cancel-in-progress") is not False]
    assert cancelling == ["keeper"]  # the one sleeper (D-1451), never a lane's run or a check
