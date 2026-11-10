"""kit/runner.json "claim_first" (D-2540): listed task ids are tried before every other claimable task.

Casey, November 10, 2026: run the two flaky-test fixes first and "make these the next claims, by the least
invasive legitimate means". The list changes only the order the cycle tries claimable tasks in: a resync
still goes first (D-891), an id that is not claimable is ignored, and without the key the order is the
state order, as before.
"""

from __future__ import annotations

import dataclasses
import json
from pathlib import Path
from typing import Any

from runner import __main__ as entry, cycle, state as st
from test_failure_drills import NOW, FakeEnv, config, make_task

ROOT = Path(__file__).resolve().parent.parent


def graph() -> list[dict[str, Any]]:
    """Three independent tasks and one waiting for T002, in state order."""
    return [make_task("T001"), make_task("T002"), make_task("T003"), make_task("T004", deps=["T002"])]


def first_claim(tmp_path: Path, first: tuple[str, ...] | None, done: tuple[str, ...] = ()) -> str:
    """The id one cycle claims, with `claim_first` set to `first` (None: the Config default)."""
    tasks = graph()
    cfg = config(tasks, tmp_path)
    cfg = cfg if first is None else dataclasses.replace(cfg, claim_first=first)
    env = FakeEnv()
    env.store = st.sync_graph(env.store, tasks)
    for tid in done:
        env.store = st.with_task(env.store, tid, status=st.DONE)
    cycle.run_cycle(cfg, env, NOW)
    return env.runs[0][0]


def test_a_listed_task_is_claimed_before_an_earlier_eligible_one(tmp_path: Path) -> None:
    assert first_claim(tmp_path, ("T003",)) == "T003"
    assert first_claim(tmp_path, ("T003", "T002")) == "T003"  # the list's own order
    assert first_claim(tmp_path, ("T002", "T003")) == "T002"


def test_a_listed_task_that_is_not_claimable_is_ignored(tmp_path: Path) -> None:
    assert first_claim(tmp_path, ("T004", "T003")) == "T003"  # T004 still waits for T002
    assert first_claim(tmp_path, ("T001", "T003"), done=("T001",)) == "T003"  # a DONE id is harmless
    assert first_claim(tmp_path, ("T999",)) == "T001"  # an id the plan does not have changes nothing
    assert first_claim(tmp_path, ("T004", "T003"), done=("T002",)) == "T004"  # claimed once its dependency is DONE


def test_without_the_key_the_order_is_the_state_order(tmp_path: Path) -> None:
    assert config(graph(), tmp_path).claim_first == ()
    assert first_claim(tmp_path, None) == first_claim(tmp_path, ()) == "T001"
    s = st.sync_graph(st.new_state(), graph())
    ids = ["T001", "T002", "T003"]
    assert cycle.claim_order(ids, s) == cycle.claim_order(ids, s, ()) == ids


def test_a_resync_still_goes_first() -> None:
    s = st.sync_graph(st.new_state(), graph())
    s = st.with_task(s, "T002", resync={"attempt": 1, "from": "h", "paths": [], "at": "x"})
    ids = ["T001", "T002", "T003"]
    assert cycle.claim_order(ids, s, ("T003", "T001")) == ["T002", "T003", "T001"]
    assert cycle.claim_order(ids, s, ("T003", "T003", "T001")) == ["T002", "T003", "T001"]  # a repeat keeps its first place


def test_the_setting_is_read_from_kit_runner_json_and_fails_safe(tmp_path: Path) -> None:
    f = tmp_path / "runner.json"
    assert entry.claim_first(f) == ()  # no file
    for raw, want in (("{not json", ()), ('{"lanes": 2}', ()), ('{"claim_first": "T338"}', ()), ("[]", ()),
                      ('{"claim_first": ["T338", 7, null, "T311"]}', ("T338", "T311"))):
        f.write_text(raw, encoding="utf-8")
        assert entry.claim_first(f) == want, raw


def test_the_repository_list_names_only_plan_tasks() -> None:
    """Whatever kit/runner.json lists (D-2540: T338, T311, T349) is a list of task ids in kit/tasks.json, so
    a typo cannot pass for a priority that silently does nothing."""
    raw = json.loads((ROOT / "kit" / "runner.json").read_text(encoding="utf-8"))
    listed = entry.claim_first(ROOT / "kit" / "runner.json")
    assert list(listed) == raw.get("claim_first", [])
    plan = {t["id"] for t in json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]}
    assert set(listed) <= plan
