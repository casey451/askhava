"""kit/runner.json "order": "critical_path" (D-2591): the longest remaining dependency chain is tried first.

Casey, November 10, 2026: "We have 50% weekly tokens to get through by Thursday morning. Optimize the build
as well as we can ... Dont negatively impact the build". The longest open chain (64 tasks, through T157)
nearly sets the end date with two lanes, so after a resync and `claim_first` the cycle tries the claimable
ids by the length of their chain of not-yet-DONE/SKIPPED dependents, kit-state order breaking ties. Each
test names a wrong implementation it catches.
"""

from __future__ import annotations

import dataclasses
import json
import subprocess
from pathlib import Path
from typing import Any

from runner import __main__ as entry, cycle, state as st
from test_failure_drills import NOW, FakeEnv, config, make_task

ROOT = Path(__file__).resolve().parent.parent


def graph() -> list[dict[str, Any]]:
    """State order T001..T003 all claimable; T003 heads a chain T003 -> T004 -> T005, T002 one of two."""
    return [make_task("T001"), make_task("T002"), make_task("T003"),
            make_task("T004", deps=["T003"]), make_task("T005", deps=["T004"]), make_task("T006", deps=["T002"])]


def first_claim(tmp_path: Path, order: str, done: tuple[str, ...] = (), first: tuple[str, ...] = ()) -> str:
    """The id one full cycle claims with the given `order` and `claim_first`."""
    tasks = graph()
    cfg = dataclasses.replace(config(tasks, tmp_path), order=order, claim_first=first)
    env = FakeEnv()
    env.store = st.sync_graph(env.store, tasks)
    for tid in done:
        env.store = st.with_task(env.store, tid, status=st.DONE)
    cycle.run_cycle(cfg, env, NOW)
    return env.runs[0][0]


def test_the_longest_chain_is_claimed_first(tmp_path: Path) -> None:
    """Catches: the switch read but never applied in `_next_step`; shortest-first (a sign error)."""
    assert first_claim(tmp_path, cycle.CRITICAL_PATH) == "T003"
    assert cycle.chain_lengths(graph(), st.sync_graph(st.new_state(), graph()))["T003"] == 3  # the task counts


def test_finished_dependents_do_not_count(tmp_path: Path) -> None:
    """Catches: counting every descendant whatever its status. T005 SKIPPED and T004 DONE leave T003 alone,
    so T002 (with open T006) leads; T001 and T003 then tie behind it."""
    s = st.sync_graph(st.new_state(), graph())
    s = st.with_task(st.with_task(s, "T004", status=st.DONE), "T005", status=st.SKIPPED)
    chains = cycle.chain_lengths(graph(), s)
    assert chains["T003"] == 1 and chains["T002"] == 2
    assert cycle.claim_order(["T001", "T002", "T003"], s, (), chains) == ["T002", "T001", "T003"]


def test_a_tie_keeps_the_state_order() -> None:
    """Catches: an unstable sort or a tiebreak by id or by reverse order."""
    tasks = [make_task("T009"), make_task("T001"), make_task("T005")]
    s = st.sync_graph(st.new_state(), tasks)
    chains = cycle.chain_lengths(tasks, s)
    assert cycle.claim_order(["T009", "T001", "T005"], s, (), chains) == ["T009", "T001", "T005"]


def test_resync_and_claim_first_still_go_first(tmp_path: Path) -> None:
    """Catches: the chain key placed before the resync or `claim_first` keys."""
    s = st.sync_graph(st.new_state(), graph())
    chains = cycle.chain_lengths(graph(), s)
    assert cycle.claim_order(["T001", "T002", "T003"], s, ("T001",), chains) == ["T001", "T003", "T002"]
    s = st.with_task(s, "T001", resync={"attempt": 1, "from": "h", "paths": [], "at": "x"})
    assert cycle.claim_order(["T001", "T002", "T003"], s, ("T002",), chains) == ["T001", "T002", "T003"]
    assert first_claim(tmp_path, cycle.CRITICAL_PATH, first=("T001",)) == "T001"


def test_without_the_switch_the_order_is_the_state_order(tmp_path: Path) -> None:
    """Catches: critical-path order made the default, so removing the key would not revert it."""
    assert config(graph(), tmp_path).order == "state"
    assert first_claim(tmp_path, "state") == "T001"
    s = st.sync_graph(st.new_state(), graph())
    assert cycle.claim_order(["T001", "T002", "T003"], s, (), None) == ["T001", "T002", "T003"]


def test_a_cycle_in_a_malformed_graph_ends() -> None:
    """Catches: plain recursion or a walk with no on-stack guard, which never returns (or overflows) here."""
    tasks = [make_task("T001", deps=["T003"]), make_task("T002", deps=["T001"]), make_task("T003", deps=["T002"]),
             make_task("T004", deps=["T003"])]
    chains = cycle.chain_lengths(tasks, st.sync_graph(st.new_state(), tasks))
    assert set(chains) == {"T001", "T002", "T003", "T004"}
    assert all(1 <= n <= 4 for n in chains.values())
    deep = [make_task("T0000")] + [make_task(f"T{i:04d}", deps=[f"T{i - 1:04d}"]) for i in range(1, 3000)]
    assert cycle.chain_lengths(deep, st.new_state())["T0000"] == 3000  # no recursion limit


def test_the_setting_is_read_from_kit_runner_json_and_fails_safe(tmp_path: Path) -> None:
    """Catches: a loader that crashes the runner on a bad file, or treats any truthy value as on."""
    f = tmp_path / "runner.json"
    assert entry.claim_order_setting(f) == "state"
    for raw, want in (("{not json", "state"), ("[]", "state"), ('{"lanes": 2}', "state"), ('{"order": true}', "state"),
                      ('{"order": "state"}', "state"), ('{"order": "critical_path"}', cycle.CRITICAL_PATH)):
        f.write_text(raw, encoding="utf-8")
        assert entry.claim_order_setting(f) == want, raw


def test_the_entry_point_passes_the_setting_into_the_config() -> None:
    """Catches: the loader written but never wired into `build` (read from source; `build` needs GitHub)."""
    import inspect

    assert 'order=claim_order_setting(ROOT / "kit" / "runner.json")' in inspect.getsource(entry.build)


def test_on_the_real_plan_the_top_claim_heads_the_longest_open_chain() -> None:
    """On kit/tasks.json and the kit-state (when readable), properties only, no pinned ids: the chain of the
    top id in critical-path order is the largest among the open, dependency-ready tasks, and every chain is
    at least 1. Catches: dependents read the wrong way round (deps instead of dependents)."""
    tasks = json.loads((ROOT / "kit" / "tasks.json").read_text(encoding="utf-8"))["tasks"]
    try:
        raw = subprocess.run(["git", "show", "origin/claude/kit-state:kit/state.json"], cwd=ROOT,
                             capture_output=True, text=True, check=True).stdout
        state = st.sync_graph(json.loads(raw), tasks)
    except (OSError, subprocess.CalledProcessError, ValueError):
        state = st.sync_graph(st.new_state(), tasks)
    chains = cycle.chain_lengths(tasks, state)
    finished = (st.DONE, st.SKIPPED)
    ready = [t["id"] for t in tasks if st.status(state, t["id"]) not in finished
             and all(st.status(state, d) in finished for d in t.get("deps", []))]
    if not ready:
        return
    ordered = cycle.claim_order(ready, state, (), chains)
    assert chains[ordered[0]] == max(chains[i] for i in ready)
    open_ids = [t["id"] for t in tasks if st.status(state, t["id"]) not in finished]
    assert chains[ordered[0]] == max(chains[i] for i in open_ids)  # the longest open chain starts at a ready task
    assert [chains[i] for i in ordered] == sorted((chains[i] for i in ordered), reverse=True)


def test_a_graph_the_chain_count_cannot_read_falls_back_to_state_order(tmp_path: Path, capsys: Any) -> None:
    """Catches: an unguarded `chain_lengths` call, which would stop every cycle on a malformed graph."""
    tasks = [make_task("T001"), make_task("T002"), make_task("T003", deps=None)]
    tasks[2]["deps"] = None
    cfg = dataclasses.replace(config(tasks, tmp_path), order=cycle.CRITICAL_PATH)
    s = st.sync_graph(st.new_state(), [make_task("T001"), make_task("T002"), make_task("T003")])
    assert cycle._chains_or_none(cfg, s) is None
    assert "critical-path order unavailable" in capsys.readouterr().out
    assert first_claim(tmp_path, cycle.CRITICAL_PATH) == "T003"  # a sound graph still orders by chain
