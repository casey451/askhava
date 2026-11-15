"""Refill counts only the candidates the claim's plan-health check would accept (D-3812, D-1350).

From 22:07Z on November 14 to 02:42Z on November 15, 2026 the state on `claude/kit-state` held T365 QUEUED with its
dependencies done, but D-3106 had removed T365 from kit/tasks.json, so the claim's plan-health pass refused it every
time ("T365: not in kit/tasks.json") while `refill_lanes` still gave it a lane: about 33 backup runs started, each
ending `idle` and starting the next. Refill now judges the same check read-only (`cycle._plan_sound`), fails open when
the check itself fails, and writes no `plan_gap` mark. Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
from pathlib import Path
from typing import Any

from test_failure_drills import NOW, FakeEnv, config, make_task
from test_runner_two_lanes import handed_off

from runner import cycle
from runner import state as st

GONE = "T365"  # QUEUED in the state, absent from the graph (kit/tasks.json)


def graph() -> list[dict[str, Any]]:
    """T001 (done in every drill) and T002 write `pay`; T365 is only in the state."""
    return [make_task("T001", owns=["app/pay/src/a.rs"]), make_task("T002", owns=["app/pay/src/b.rs"])]


def in_graph(tasks: list[dict[str, Any]]) -> Any:
    """The fake plan-health check: a task the graph does not hold has a problem, as scripts/plan_health.py reports."""
    known = {t["id"] for t in tasks}
    return lambda state, tid: [] if tid in known else [f"{tid}: not in kit/tasks.json"]


def stranded(tasks: list[dict[str, Any]], *queued: str) -> st.State:
    """The live shape: T001 DONE, GONE QUEUED with no deps (added by an older graph), `queued` left QUEUED."""
    s = st.sync_graph(st.new_state(), [*tasks, make_task(GONE, owns=["app/pay/src/g.rs"])])
    for tid in [t["id"] for t in tasks if t["id"] not in queued]:
        s = st.with_task(s, tid, status=st.DONE)
    return s


def lanes_cfg(tasks: list[dict[str, Any]], tmp_path: Path, **kw: Any) -> cycle.Config:
    """Three lanes, the run on lane 1, the fake check set; `kw` overrides (exclusive_crates, plan_health)."""
    cfg = dataclasses.replace(config(tasks, tmp_path, "s1"), lanes=3, lane_hint="1", plan_health=in_graph(tasks))
    return dataclasses.replace(cfg, **kw)


def test_a_queued_task_gone_from_the_graph_gets_no_lane_and_the_claim_idles(tmp_path: Path) -> None:
    tasks = graph()
    s = stranded(tasks)
    assert cycle.refill_lanes(lanes_cfg(tasks, tmp_path, plan_health=None), s, NOW) == ["1"]  # no check: a lane, as before
    for exclusive in (False, True):
        cfg = lanes_cfg(tasks, tmp_path, exclusive_crates=exclusive)
        assert cycle.refill_lanes(cfg, s, NOW) == []
        assert "plan_gap" not in st.entry(s, GONE)  # refill reads only: no mark
        env = FakeEnv(handed_off)
        env.store = s
        assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []
        assert st.entry(env.store, GONE)["status"] == st.QUEUED
        assert st.entry(env.store, GONE)["plan_gap"]["problems"] == [f"{GONE}: not in kit/tasks.json"]  # the claim marks it
        assert cycle.refill_lanes(cfg, env.store, NOW) == []  # and the run that ended starts no other


def test_a_sound_candidate_beside_it_still_gets_a_lane_and_is_claimed(tmp_path: Path) -> None:
    tasks = graph()
    s = stranded(tasks, "T002")
    for exclusive in (False, True):
        cfg = lanes_cfg(tasks, tmp_path, exclusive_crates=exclusive)
        assert cycle.refill_lanes(cfg, s, NOW) == ["1"]  # one lane, for T002 only
        env = FakeEnv(handed_off)
        env.store = s
        assert cycle.run_cycle(cfg, env, NOW) == "handed_off" and [r[0] for r in env.runs] == ["T002"]
        assert st.status(env.store, GONE) == st.QUEUED


def test_a_failing_check_fails_open_and_fix_main_is_never_checked(tmp_path: Path) -> None:
    asked: list[str] = []

    def broken(state: st.State, tid: str) -> list[str]:
        asked.append(tid)
        raise RuntimeError("tasks.json unreadable")

    tasks = graph()
    cfg = lanes_cfg(tasks, tmp_path, plan_health=broken)
    s = stranded(tasks)
    assert cycle.refill_lanes(cfg, s, NOW) == ["1"]  # as the claim does (D-1350): a broken check holds nothing
    assert "plan_gap" not in st.entry(s, GONE)
    assert cycle.refill_lanes(dataclasses.replace(cfg, exclusive_crates=True), s, NOW) == ["1"]
    asked.clear()
    assert cycle._plan_sound(cfg, s, [st.FIX_MAIN, GONE]) == [st.FIX_MAIN, GONE] and asked == [GONE]
