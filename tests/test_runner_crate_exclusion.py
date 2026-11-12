"""kit/runner.json "exclusive_crates" (D-3269): the runner never claims a task that writes an `app/` crate a
task in flight (IN_PROGRESS under a live lease or MERGE_PENDING, any lane) also writes.

On November 12, 2026 nearly every resync conflict was in a crate `CLAUDE.md` or `src/api.rs`, about one task
in five at five lanes. D-3082's `prefer_disjoint_crates` only reordered, so a crowded task was still claimed
when nothing else was; this setting refuses it. A task is never checked against itself (a resync of the task
holding the crate), FIX-MAIN is exempt, a run whose every candidate collides ends `idle` with no strike, and
refill counts only candidates that could run together. Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
import inspect
import json
from pathlib import Path
from typing import Any

from test_failure_drills import NOW, FakeEnv, config, make_task
from test_runner_two_lanes import handed_off

from runner import __main__ as entry
from runner import cycle, lanes
from runner import state as st

ROOT = Path(__file__).resolve().parent.parent


def graph() -> list[dict[str, Any]]:
    """T001 (in flight in the tests), two candidates in its crate `pay`, one in `site`, one by module only."""
    return [make_task("T001", owns=["app/pay/src/a.rs"]), make_task("T002", owns=["app/pay/src/b.rs"]),
            make_task("T003", owns=["tests/integration/tests/c.rs"], module="pay"),
            make_task("T004", owns=["app/site/src/d.rs"]), make_task("T005", owns=["docs/notes.md"], module=None)]


def flying(tasks: list[dict[str, Any]], status: str = st.IN_PROGRESS) -> st.State:
    """T001 in flight on lane 1: a live lease, or MERGE_PENDING on its own lane."""
    s = st.sync_graph(st.new_state(), tasks)
    if status == st.MERGE_PENDING:
        return st.with_task(s, "T001", status=st.MERGE_PENDING, lane="1")
    return st.claim(s, "T001", "a", NOW, lane="1")


def by_id(tasks: list[dict[str, Any]]) -> Any:
    table = {t["id"]: t for t in tasks}
    return lambda tid: table[tid]


def lane_two(tasks: list[dict[str, Any]], tmp_path: Path, exclusive: bool | None = True, **kw: Any) -> cycle.Config:
    cfg = dataclasses.replace(config(tasks, tmp_path, "s2"), lanes=7, lane_hint="2", **kw)
    return cfg if exclusive is None else dataclasses.replace(cfg, exclusive_crates=exclusive)


def test_task_crates_are_its_owned_app_crates_and_its_module() -> None:
    assert lanes.task_crates(make_task("T1", owns=["app/pay/src/a.rs", "app/site/", "docs/x.md"])) == {"pay", "site"}
    assert lanes.task_crates(make_task("T1", owns=["tests/x.py"], module="quotes")) == {"quotes"}
    assert lanes.task_crates(make_task("T1", owns=["docs/x.md"], module=None)) == frozenset()


def test_crate_clash_names_the_task_in_flight_and_exempts_fix_main() -> None:
    t1, t2, t3, t4, _ = graph()
    assert lanes.crate_clash(t2, [t1]) == "writes app/pay/, which T001 in flight also writes (D-3269)"
    assert lanes.crate_clash(t3, [t1]) != ""  # by module only: it still updates app/pay/CLAUDE.md
    assert lanes.crate_clash(t4, [t1]) == "" and lanes.crate_clash(t2, []) == ""
    assert lanes.crate_clash(make_task(st.FIX_MAIN, owns=["app/pay/src/a.rs"]), [t1]) == ""


def test_compatible_ids_refuses_a_shared_crate_only_when_exclusive() -> None:
    tasks = graph()
    ids = ["T002", "T003", "T004", "T005"]
    for status in (st.IN_PROGRESS, st.MERGE_PENDING):  # in flight on any lane, leased or merge-pending
        s = flying(tasks, status)
        assert lanes.compatible_ids(ids, s, NOW, by_id(tasks), None, exclusive=True) == ["T004", "T005"]
        assert lanes.compatible_ids(ids, s, NOW, by_id(tasks), None) == ids  # off: as before D-3269
    assert lanes.compatible_ids(["T001"], flying(tasks), NOW, by_id(tasks), None, exclusive=True) == ["T001"]  # never its own neighbour


def test_a_non_colliding_task_is_claimed_even_when_a_colliding_one_comes_first(tmp_path: Path) -> None:
    tasks = graph()
    env = FakeEnv(handed_off)
    env.store = flying(tasks)
    assert cycle.run_cycle(lane_two(tasks, tmp_path, claim_first=("T002",)), env, NOW) == "handed_off"
    assert env.runs[0][0] == "T004" and st.lane_of(env.store, "T004") == "2"  # claim_first orders, never overrides the refusal
    off = FakeEnv(handed_off)
    off.store = flying(tasks)
    cycle.run_cycle(lane_two(tasks, tmp_path, None, claim_first=("T002",)), off, NOW)
    assert off.runs[0][0] == "T002"  # the default Config claims as before


def test_when_every_candidate_collides_the_lane_ends_idle_with_no_strike_and_no_refill(tmp_path: Path) -> None:
    tasks = graph()[:3]
    env = FakeEnv()
    env.store = flying(tasks)
    cfg = lane_two(tasks, tmp_path)
    assert cycle.run_cycle(cfg, env, NOW) == "idle"
    assert env.runs == [] and st.in_flight(env.store, NOW) == {"T001": "1"}
    assert not env.store.get("lane_strikes") and not env.store.get("lanes_fallback")
    assert lanes.free_lanes(7, env.store, NOW) == ["2", "3", "4", "5", "6", "7"]  # the lane is not lost
    assert cycle.refill_lanes(cfg, env.store, NOW) == []  # no run started that could only collide
    assert "idle" in entry.REFILL_STOP and entry.refill_starts("idle", ["2"], dict) is None
    assert cycle.refill_lanes(dataclasses.replace(cfg, exclusive_crates=False), env.store, NOW) == ["2", "3"]


def test_refill_counts_two_candidates_of_one_crate_once() -> None:
    tasks = [make_task("T002", owns=["app/pay/src/b.rs"]), make_task("T003", owns=["app/pay/src/c.rs"]),
             make_task("T004", owns=["app/site/src/d.rs"])]
    s = st.sync_graph(st.new_state(), tasks)
    cfg = dataclasses.replace(config(tasks, Path(".")), lanes=7, exclusive_crates=True)
    assert cycle.refill_lanes(cfg, s, NOW) == ["1", "2"]
    assert cycle.refill_lanes(dataclasses.replace(cfg, exclusive_crates=False), s, NOW) == ["1", "2", "3"]
    assert lanes.crate_disjoint(["T002", "T003", "T004"], by_id(tasks)) == ["T002", "T004"]


def test_the_crate_frees_once_the_task_in_flight_is_done(tmp_path: Path) -> None:
    tasks = graph()[:2]
    env = FakeEnv(handed_off)
    env.store = st.with_task(st.sync_graph(st.new_state(), tasks), "T001", status=st.DONE)
    assert cycle.run_cycle(lane_two(tasks, tmp_path), env, NOW) == "handed_off"
    assert env.runs[0][0] == "T002"


def test_the_setting_is_read_from_kit_runner_json_and_fails_safe(tmp_path: Path) -> None:
    f = tmp_path / "runner.json"
    assert entry.exclusive_crates(f) is False  # no file
    for raw, want in (("{not json", False), ("[]", False), ('{"lanes": 7}', False), ('{"exclusive_crates": "true"}', False),
                      ('{"exclusive_crates": 1}', False), ('{"exclusive_crates": false}', False), ('{"exclusive_crates": true}', True)):
        f.write_text(raw, encoding="utf-8")
        assert entry.exclusive_crates(f) is want, raw


def test_the_repository_turns_it_on_beside_the_preference_and_the_entry_point_passes_it() -> None:
    raw = json.loads((ROOT / "kit" / "runner.json").read_text(encoding="utf-8"))
    assert raw.get("exclusive_crates") is True and raw.get("prefer_disjoint_crates") is True  # D-3082's ordering kept
    assert "D-3269" in raw["exclusive_crates_note"]
    assert 'exclusive_crates=exclusive_crates(ROOT / "kit" / "runner.json")' in inspect.getsource(entry.build)
    assert config([], Path(".")).exclusive_crates is False
