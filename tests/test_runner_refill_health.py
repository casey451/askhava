"""Refill gives no lane to a task a claim has already refused on plan health and the check still refuses (D-3812, D-1350).

From 22:07Z on November 14 to 02:42Z on November 15, 2026 the state on `claude/kit-state` held T365 QUEUED with its
dependencies done and a `plan_gap` mark "T365: not in kit/tasks.json" (D-3106 had removed it from kit/tasks.json): the
claim's plan-health pass refused it every time while `refill_lanes` still gave it a lane, so about 33 backup runs
started, each ending `idle` and starting the next. Refill now asks the same check read-only (`cycle._plan_keep`), but
only for a task already marked: its checkout may be a task branch cut before later merges, so an unmarked task is
never refused on its disk. It fails open when the check itself fails and writes no mark. Drills run over
test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
from datetime import timedelta
from pathlib import Path
from typing import Any

from test_failure_drills import NOW, FakeEnv, config, make_task
from test_runner_two_lanes import handed_off

from runner import cycle
from runner import state as st

GONE = "T365"  # QUEUED in the state, absent from the graph (kit/tasks.json)
MARK = {"at": "2026-11-12T04:23:07Z", "problems": [f"{GONE}: not in kit/tasks.json"]}


def graph() -> list[dict[str, Any]]:
    """T001 and T002 write `pay`, T020 `web`, T004 `site`; T365 is only in the state and writes `pay` and `web`."""
    return [make_task("T001", owns=["app/pay/src/a.rs"]), make_task("T002", owns=["app/pay/src/b.rs"]),
            make_task("T020", owns=["app/web/src/e.rs"]), make_task("T004", owns=["app/site/src/d.rs"])]


def in_graph(tasks: list[dict[str, Any]]) -> Any:
    """The fake plan-health check: a task the graph does not hold has a problem, as scripts/plan_health.py reports."""
    known = {t["id"] for t in tasks}
    return lambda state, tid: [] if tid in known else [f"{tid}: not in kit/tasks.json"]


def stranded(tasks: list[dict[str, Any]], *queued: str, marked: bool = True) -> st.State:
    """The live shape: the graph's tasks DONE but `queued`, and GONE QUEUED with no deps (added by an older graph),
    carrying the claim's `plan_gap` mark when `marked`."""
    gone = make_task(GONE, owns=["app/pay/src/g.rs", "app/web/src/g.rs"])
    s = st.sync_graph(st.new_state(), [*tasks, gone])
    for tid in [t["id"] for t in tasks if t["id"] not in queued]:
        s = st.with_task(s, tid, status=st.DONE)
    return st.with_task(s, GONE, plan_gap=MARK) if marked else s


def lanes_cfg(tasks: list[dict[str, Any]], tmp_path: Path, **kw: Any) -> cycle.Config:
    """Three lanes, the run on lane 1, the fake check set; `kw` overrides (exclusive_crates, plan_health)."""
    cfg = dataclasses.replace(config(tasks, tmp_path, "s1"), lanes=3, lane_hint="1", plan_health=in_graph(tasks))
    return dataclasses.replace(cfg, **kw)


def test_a_marked_task_gone_from_the_graph_gets_no_lane_and_the_claim_idles(tmp_path: Path) -> None:
    tasks = graph()
    s = stranded(tasks)
    assert cycle.refill_lanes(lanes_cfg(tasks, tmp_path, plan_health=None), s, NOW) == ["1"]  # no check: a lane, as before
    for exclusive in (False, True):
        cfg = lanes_cfg(tasks, tmp_path, exclusive_crates=exclusive)
        assert cycle.refill_lanes(cfg, s, NOW) == []
        env = FakeEnv(handed_off)
        env.store = s
        assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []
        assert st.entry(env.store, GONE)["status"] == st.QUEUED and st.entry(env.store, GONE)["plan_gap"] == MARK
        assert cycle.refill_lanes(cfg, env.store, NOW) == []  # and the run that ended starts no other


def test_an_unmarked_task_is_judged_only_by_a_claim_which_then_marks_it(tmp_path: Path) -> None:
    tasks = graph()
    cfg = lanes_cfg(tasks, tmp_path)
    s = stranded(tasks, marked=False)
    assert cycle.refill_lanes(cfg, s, NOW) == ["1"]  # no claim has refused it yet: refill does not judge it
    assert "plan_gap" not in st.entry(s, GONE)  # refill reads only: no mark
    env = FakeEnv(handed_off)
    env.store = s
    assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []
    assert st.entry(env.store, GONE)["plan_gap"]["problems"] == MARK["problems"]  # the claim marks it
    assert cycle.refill_lanes(cfg, env.store, NOW + timedelta(minutes=1)) == []


def test_a_task_refused_only_on_this_checkouts_disk_still_gets_a_lane(tmp_path: Path) -> None:
    """A hand-in run's checkout is the task branch cut at its claim: a DONE dependency's owned file merged since is
    missing there, so the check refuses T002; with no mark from a claim on main, refill still gives it a lane."""
    tasks = [make_task("T001", owns=["app/pay/src/a.rs"]), make_task("T002", ["T001"], owns=["app/pay/src/b.rs"])]
    asked: list[str] = []

    def on_disk(state: st.State, tid: str) -> list[str]:
        asked.append(tid)
        on = (tmp_path / "app/pay/src/a.rs").exists()
        return [] if on or tid != "T002" else ["T002: dependency T001's owned file app/pay/src/a.rs is not on main"]

    s = st.with_task(st.sync_graph(st.new_state(), tasks), "T001", status=st.DONE)
    assert on_disk(s, "T002") and asked == ["T002"]  # the check would refuse it on this disk
    asked.clear()
    for exclusive in (False, True):
        cfg = lanes_cfg(tasks, tmp_path, exclusive_crates=exclusive, plan_health=on_disk)
        assert cycle.refill_lanes(cfg, s, NOW) == ["1"] and asked == []  # unmarked: the check is not even asked


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
    assert st.entry(s, GONE)["plan_gap"] == MARK  # and refill writes nothing
    assert cycle.refill_lanes(dataclasses.replace(cfg, exclusive_crates=True), s, NOW) == ["1"]
    asked.clear()
    keep = cycle._plan_keep(cfg, st.with_task(s, st.FIX_MAIN, plan_gap=MARK))
    assert keep(st.FIX_MAIN) and keep(GONE) and keep(GONE) and asked == [GONE]  # FIX-MAIN unchecked; one ask per id


def claims_on_refill(cap: int, tmp_path: Path) -> tuple[list[str], list[str]]:
    """Beside T001 in flight, with T010 (wide: `pay` and `web`; marked and still refused) ranked first: the lanes
    refill names, then the tasks one run per named lane claims; afterwards refill names none and a further run
    idles, so refill and claim agree. (T365 itself owns nothing in the graph, so it holds no crate.)"""
    tasks = [*graph(), make_task("T010", owns=["app/pay/src/w.rs", "app/web/src/w.rs"])]
    refused = {"T010": ["T010: tests/w.rs is named by its tests, but no one owns it"]}
    check = in_graph(tasks)
    base = dataclasses.replace(config(tasks, tmp_path, "s1"), lanes=7, exclusive_crates=True, crate_cap=cap,
                               crate_full=lambda crate, held: False, claim_first=("T010",),
                               plan_health=lambda state, tid: refused.get(tid) or check(state, tid))
    s = stranded(tasks, "T001", "T002", "T020", "T004", "T010")
    s = st.with_task(s, "T010", plan_gap={"at": MARK["at"], "problems": refused["T010"]})
    env = FakeEnv(handed_off)
    env.store = st.claim(s, "T001", "sT001", NOW, lane="1")
    named = cycle.refill_lanes(dataclasses.replace(base, lane_hint="1"), env.store, NOW)
    for n, lane in enumerate(named, start=1):
        assert cycle.run_cycle(dataclasses.replace(base, lane_hint=lane), env, NOW + timedelta(minutes=n)) == "handed_off"
    after = NOW + timedelta(minutes=len(named) + 1)
    assert cycle.refill_lanes(dataclasses.replace(base, lane_hint="1"), env.store, after) == []
    assert cycle.run_cycle(dataclasses.replace(base, lane_hint="7"), env, after) == "idle"
    return named, [run[0] for run in env.runs]


def test_refill_and_claim_agree_when_a_refused_first_id_reserves_crates(tmp_path: Path) -> None:
    named, claimed = claims_on_refill(1, tmp_path)  # T010 is over `pay`'s limit, so it reserves `pay` and `web`
    assert named == ["2"] and claimed == ["T004"]  # T002 (`pay` full) and T020 (`web` reserved) wait with it
    named, claimed = claims_on_refill(2, tmp_path)  # T010 fits but is refused; once T002 fills `pay` it reserves
    assert named == ["2", "3", "4"] and claimed == ["T002", "T020", "T004"]  # `web` keeps a slot for T010
