"""kit/runner.json "prefer_disjoint_crates" (D-3082): among claimable tasks of equal priority, try first one
whose `app/` crates no task in flight shares.

Efficiency audit F2 (Casey's upload, November 11, 2026): with five lanes, two tasks in one crate both update
that crate's `CLAUDE.md` (docs/code-docs.md section 2, a shared path the lane check allows), and the second
to merge needs a resync (D-891); six resyncs on crate `CLAUDE.md` files were recorded that day. The
preference only reorders: a resync, `claim_first` (D-2540) and the critical-path chain (D-2591) still win,
a crowded task is still claimed when nothing else is, and with the flag off the order is as before.

Drills run over test_failure_drills.FakeEnv. Fake data only.
"""

from __future__ import annotations

import dataclasses
import inspect
import json
from pathlib import Path
from typing import Any

from test_failure_drills import NOW, FakeEnv, config, make_task

from runner import __main__ as entry
from runner import cycle, lanes
from runner import state as st

ROOT = Path(__file__).resolve().parent.parent
RESYNC = {"attempt": 1, "from": "h", "paths": ["app/pay/CLAUDE.md"], "at": "x"}


def graph() -> list[dict[str, Any]]:
    """T001 (in flight in the tests) and two candidates in its crate before one in another crate."""
    return [make_task("T001", owns=["app/pay/src/a.rs"]), make_task("T002", owns=["app/pay/src/b.rs"]),
            make_task("T003", owns=["app/pay/src/c.rs", "tests/integration/tests/c.rs"]),
            make_task("T004", owns=["app/site/src/d.rs"])]


def flying(tasks: list[dict[str, Any]]) -> st.State:
    """The state with T001 claimed on lane 1 (a live lease, so in flight)."""
    return st.claim(st.sync_graph(st.new_state(), tasks), "T001", "a", NOW, lane="1")


def first_claim(tmp_path: Path, tasks: list[dict[str, Any]], prefer: bool | None, **kw: Any) -> str:
    """The id a second-lane cycle claims beside T001 (`prefer` None: the Config default)."""
    cfg = dataclasses.replace(config(tasks, tmp_path, "s2"), lanes=5, lane_hint="2", **kw)
    cfg = cfg if prefer is None else dataclasses.replace(cfg, prefer_disjoint_crates=prefer)
    env = FakeEnv()
    env.store = flying(tasks)
    cycle.run_cycle(cfg, env, NOW)
    return env.runs[0][0]


def by_id(tasks: list[dict[str, Any]]) -> Any:
    table = {t["id"]: t for t in tasks}
    return lambda tid: table[tid]


def test_crates_are_the_app_folders_a_task_owns() -> None:
    assert lanes.crates(["app/pay/src/a.rs", "app/pay/CLAUDE.md", "app/site/", "tests/x.rs", "app/", "docs/a.md"]) == {"pay", "site"}
    assert lanes.crates([]) == frozenset()


def test_crowded_names_the_ids_sharing_a_crate_with_a_task_in_flight() -> None:
    tasks = graph()
    s = flying(tasks)
    assert lanes.crowded(["T002", "T003", "T004"], s, NOW, by_id(tasks)) == {"T002", "T003"}
    assert lanes.crowded(["T002", "T004"], st.sync_graph(st.new_state(), tasks), NOW, by_id(tasks)) == frozenset()
    pending = st.with_task(st.sync_graph(st.new_state(), tasks), "T004", status=st.MERGE_PENDING, lane="3")
    assert lanes.crowded(["T002", "T004"], pending, NOW, by_id(tasks)) == frozenset()  # T004 itself is not its own neighbour
    assert lanes.crowded(["T002"], pending, NOW, by_id([*tasks[:3], make_task("T004", owns=["app/pay/x.rs"])])) == {"T002"}


def test_a_task_in_a_disjoint_crate_is_claimed_before_an_earlier_crowded_one(tmp_path: Path) -> None:
    assert first_claim(tmp_path, graph(), True) == "T004"
    s = flying(graph())
    assert cycle.claim_order(["T002", "T003", "T004"], s, crowded=frozenset({"T002", "T003"})) == ["T004", "T002", "T003"]


def test_when_every_candidate_shares_a_crate_the_claim_is_made_as_before(tmp_path: Path) -> None:
    tasks = graph()[:3]
    assert first_claim(tmp_path, tasks, True) == "T002"
    crowded = frozenset({"T002", "T003"})
    assert cycle.claim_order(["T002", "T003"], flying(tasks), crowded=crowded) == ["T002", "T003"]


def test_flag_off_keeps_the_old_order(tmp_path: Path) -> None:
    assert config(graph(), tmp_path).prefer_disjoint_crates is False
    assert first_claim(tmp_path, graph(), None) == first_claim(tmp_path, graph(), False) == "T002"
    s = flying(graph())
    ids = ["T002", "T003", "T004"]
    assert cycle.claim_order(ids, s) == cycle.claim_order(ids, s, (), None, frozenset()) == ids


def test_never_reorders_above_a_resync_claim_first_or_the_critical_path(tmp_path: Path) -> None:
    s = st.with_task(flying(graph()), "T003", resync=RESYNC)
    ids, crowded = ["T002", "T003", "T004"], frozenset({"T002", "T003"})
    assert cycle.claim_order(ids, s, crowded=crowded) == ["T003", "T004", "T002"]  # the resync first (D-891)
    assert cycle.claim_order(ids, flying(graph()), ("T002",), crowded=crowded) == ["T002", "T004", "T003"]  # D-2540
    chains = {"T002": 3, "T003": 1, "T004": 1}
    assert cycle.claim_order(ids, flying(graph()), (), chains, crowded) == ["T002", "T004", "T003"]  # D-2591
    chained = [*graph(), make_task("T005", deps=["T002"], owns=["app/web/"])]
    assert first_claim(tmp_path, chained, True, order=cycle.CRITICAL_PATH) == "T002"  # the longer chain still wins
    assert first_claim(tmp_path, chained, True, claim_first=("T003",)) == "T003"
    assert first_claim(tmp_path, chained, True) == "T004"  # equal priority: the disjoint crate


def test_ties_on_the_critical_path_go_to_the_disjoint_crate(tmp_path: Path) -> None:
    chained = [*graph(), make_task("T005", deps=["T002"], owns=["app/web/"]), make_task("T006", deps=["T004"], owns=["app/web2/"])]
    assert first_claim(tmp_path, chained, False, order=cycle.CRITICAL_PATH) == "T002"
    assert first_claim(tmp_path, chained, True, order=cycle.CRITICAL_PATH) == "T004"


def test_the_setting_is_read_from_kit_runner_json_and_fails_safe(tmp_path: Path) -> None:
    f = tmp_path / "runner.json"
    assert entry.prefer_disjoint_crates(f) is False  # no file
    for raw, want in (("{not json", False), ("[]", False), ('{"lanes": 5}', False), ('{"prefer_disjoint_crates": "true"}', False),
                      ('{"prefer_disjoint_crates": 1}', False), ('{"prefer_disjoint_crates": false}', False),
                      ('{"prefer_disjoint_crates": true}', True)):
        f.write_text(raw, encoding="utf-8")
        assert entry.prefer_disjoint_crates(f) is want, raw


def test_the_repository_turns_it_on_and_the_entry_point_passes_it_into_the_config() -> None:
    raw = json.loads((ROOT / "kit" / "runner.json").read_text(encoding="utf-8"))
    assert raw.get("prefer_disjoint_crates") is True
    assert 'prefer_disjoint_crates=prefer_disjoint_crates(ROOT / "kit" / "runner.json")' in inspect.getsource(entry.build)
