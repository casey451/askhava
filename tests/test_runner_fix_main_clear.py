"""Clearing a red main blames the offending task only when FIX-MAIN's merged files revert its work (D-3752).

`runner/cycle.py` `clear_main_red` and `fix_main_reverted`, driven through `cycle.run_cycle` with the
fake env of `tests/test_failure_drills.py`. Fake data only.
"""

from __future__ import annotations

from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest
from test_failure_drills import NOW, FakeEnv, config, make_task

from runner import cycle, state as st

SHARED = ["docs/architecture.md", "docs/decisions.md", ".sqlx/", "app/t001/CLAUDE.md"]
OWN = ["app/t001/x.py", "app/t001/y.py"]
REVERTED = "reverted by FIX-MAIN (nightly red on main0001)"


def red_main(tmp_path: Path, attempts: int = 1, red_files: list[str] | None = None) -> tuple[FakeEnv, cycle.Config]:
    """T001 DONE (merged), then a red nightly on main's head blames it and FIX-MAIN is queued."""
    graph = [make_task("T001"), make_task("T002")]
    cfg = config(graph, tmp_path)
    cfg = cycle.Config(graph, cfg.models, cfg.protected, SHARED, tmp_path, "s1", lambda: NOW)
    env = FakeEnv()
    files = [*OWN, "docs/decisions.md", "docs/architecture.md"] if red_files is None else red_files
    state = st.sync_graph(st.new_state(), graph)
    state = st.with_task(state, "T001", status=st.DONE, attempts=attempts, merged_sha="good0001",
                         merged_at="2026-11-01T11:00:00Z", files=files)
    env.store = cycle.set_main_red(state, cfg, "nightly", "main0001", cycle.offending_merge(state), NOW)
    return env, cfg


def clear(env: FakeEnv, cfg: cycle.Config, **fix: Any) -> str:
    """FIX-MAIN went DONE with `fix`'s facts; main moved and a smoke on its head is green."""
    env.store = st.with_task(env.store, st.FIX_MAIN, status=st.DONE, merged_sha="fix00001", **fix)
    env.head, env.smoke_green = "main0002", True
    return cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10))


def last_record(env: FakeEnv) -> dict[str, Any]:
    record: dict[str, Any] = env.store["main_red_history"][-1]
    return record


def test_a_fix_main_that_merged_only_the_decisions_log_leaves_the_task_done(tmp_path: Path) -> None:
    env, cfg = red_main(tmp_path)
    assert env.store["main_red"]["task"] == "T001"
    assert clear(env, cfg, files=["docs/decisions.md"]) == "main_cleared"
    assert not env.store["main_red"]
    t001 = st.entry(env.store, "T001")
    assert t001["status"] == st.DONE and t001["attempts"] == 1 and t001["merged_sha"] == "good0001"
    assert t001["files"] == [*OWN, "docs/decisions.md", "docs/architecture.md"]
    assert last_record(env)["outcome"] == "fixed_otherwise"
    assert last_record(env)["fix_files"] == ["docs/decisions.md"]
    assert last_record(env)["cleared_on"] == "main0002"


def test_a_fix_main_that_reverted_an_own_path_requeues_the_task(tmp_path: Path) -> None:
    env, cfg = red_main(tmp_path)
    assert clear(env, cfg, files=["app/t001/y.py", "docs/decisions.md"]) == "main_cleared"
    t001 = st.entry(env.store, "T001")
    assert t001["status"] == st.QUEUED and t001["attempts"] == 2 and t001["last_failure"] == REVERTED
    assert last_record(env)["outcome"] == "reverted"
    assert last_record(env)["fix_files"] == ["app/t001/y.py", "docs/decisions.md"]


@pytest.mark.parametrize(("fix_files", "status", "attempts"), [
    (["app/t001/x.py"], st.BLOCKED, cycle.MAX_ATTEMPTS),
    (["docs/decisions.md"], st.DONE, cycle.MAX_ATTEMPTS - 1),
])
def test_a_task_one_attempt_short_is_blocked_only_on_a_real_revert(
    tmp_path: Path, fix_files: list[str], status: str, attempts: int
) -> None:
    env, cfg = red_main(tmp_path, attempts=cycle.MAX_ATTEMPTS - 1)
    assert clear(env, cfg, files=fix_files) == "main_cleared"
    t001 = st.entry(env.store, "T001")
    assert t001["status"] == status and t001["attempts"] == attempts
    assert (t001.get("reason") == REVERTED) is (status == st.BLOCKED)


def test_an_overlap_only_in_shared_paths_is_fixed_otherwise(tmp_path: Path) -> None:
    env, cfg = red_main(tmp_path, red_files=[*OWN, "docs/architecture.md", "docs/decisions.md", ".sqlx/query-a.json"])
    fix_files = ["docs/architecture.md", "docs/decisions.md", ".sqlx/query-a.json", "app/t002/z.py"]
    assert clear(env, cfg, files=fix_files) == "main_cleared"
    assert st.status(env.store, "T001") == st.DONE and st.entry(env.store, "T001")["attempts"] == 1
    assert last_record(env)["outcome"] == "fixed_otherwise" and last_record(env)["fix_files"] == fix_files


def test_an_empty_hand_in_fix_main_is_fixed_otherwise(tmp_path: Path) -> None:
    env, cfg = red_main(tmp_path)
    assert clear(env, cfg, files=[], empty_handin=True) == "main_cleared"
    assert st.status(env.store, "T001") == st.DONE and st.entry(env.store, "T001")["attempts"] == 1
    assert last_record(env)["outcome"] == "fixed_otherwise" and last_record(env)["fix_files"] == []


@pytest.mark.parametrize("fix", [{}, {"files": []}])
def test_a_fix_main_with_no_recorded_files_keeps_todays_outcome(tmp_path: Path, fix: dict[str, Any]) -> None:
    env, cfg = red_main(tmp_path)
    assert clear(env, cfg, **fix) == "main_cleared"
    t001 = st.entry(env.store, "T001")
    assert t001["status"] == st.QUEUED and t001["attempts"] == 2 and t001["last_failure"] == REVERTED
    assert last_record(env)["outcome"] == "reverted" and last_record(env)["fix_files"] == []


@pytest.mark.parametrize(("red_files", "fix", "expected"), [
    (OWN, {"files": ["app/t001/x.py"]}, True),
    (OWN, {"files": ["docs/decisions.md", "app/t002/z.py"]}, False),
    (OWN, {"files": [], "empty_handin": True}, False),
    (OWN, {"files": []}, True),
    (OWN, {}, True),
    ([], {"files": ["docs/decisions.md"]}, True),
    ([*OWN, ".sqlx/q.json"], {"files": [".sqlx/q.json"]}, False),
    ([*OWN, "app/t001/NOTES.md"], {"files": ["app/t001/NOTES.md"]}, False),
    (["docs/architecture.md", "docs/decisions.md"], {"files": ["docs/architecture.md"]}, True),
    (["docs/architecture.md", "docs/decisions.md"], {"files": ["docs/decisions.md"]}, False),
    ([".sqlx/q.json", "docs/decisions.md"], {"files": [".sqlx/q.json", "docs/decisions.md"]}, True),
])
def test_fix_main_reverted(red_files: list[str], fix: dict[str, Any], expected: bool) -> None:
    assert cycle.fix_main_reverted({"files": red_files}, fix, SHARED) is expected
