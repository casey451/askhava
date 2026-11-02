"""Tests for scripts/plan_health.py and the runner's plan-health pass before a claim (D-1350). Fake tasks only."""

from __future__ import annotations

import dataclasses
import json
from datetime import timedelta
from pathlib import Path
from typing import Any

import plan_health as ph
from test_failure_drills import NOW, FakeEnv, config, make_task

from runner import __main__ as entry
from runner import cycle
from runner import state as st
from runner.github_env import render_status

ROW = "| TD-FAKE-01 | B | In | REQ-FAKE-001 | The case lives in `tests/integration/test_fake.py` (D-001) |\n"


def plan_task(tid: str, deps: list[str] | None = None, **over: Any) -> dict[str, Any]:
    task: dict[str, Any] = {
        "id": tid, "type": "build", "deps": deps or [], "owns": [f"app/{tid.lower()}.rs"], "reads": [],
        "requirements": [], "tests": [], "decisions": [], "prompt": f"kit/prompts/{tid}.md",
    }
    task.update(over)
    return task


def make_repo(tmp_path: Path, tasks: list[dict[str, Any]], decisions: str = "") -> Path:
    graph = {"tasks": tasks, "shared_paths": ["docs/decisions.md"], "protected_paths": [".github/"]}
    (tmp_path / "kit/prompts").mkdir(parents=True)
    (tmp_path / "kit/tasks.json").write_text(json.dumps(graph), encoding="utf-8")
    for task in tasks:
        (tmp_path / task["prompt"]).write_text("prompt " * 10, encoding="utf-8")
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs/test-design.md").write_text(ROW, encoding="utf-8")
    (tmp_path / "docs/decisions.md").write_text(decisions, encoding="utf-8")
    return tmp_path


def problems(root: Path, state: st.State, tid: str, pending_ok: bool = False) -> list[str]:
    doc = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    return ph.checker(root, doc, pending_ok)(state, tid)


def done(*ids: str) -> st.State:
    state = st.new_state()
    for tid in ids:
        state = st.with_task(state, tid, status=st.DONE)
    return state


def test_a_sound_task_has_no_problems(tmp_path: Path) -> None:
    """The cited test file is owned by a dependency; the read exists; the DONE dependency's file is on disk."""
    tasks = [plan_task("T001", owns=["tests/integration/test_fake.py"]), plan_task("T002", ["T001"], tests=["TD-FAKE-01"], reads=["docs/test-design.md"])]
    root = make_repo(tmp_path, tasks)
    (root / "tests/integration").mkdir(parents=True)
    (root / "tests/integration/test_fake.py").write_text("x", encoding="utf-8")
    assert problems(root, done("T001"), "T002") == []
    assert ph.main(["--root", str(root), "--state", str(write_state(root, done("T001"))), "T002"]) == 0


def test_a_cited_path_no_one_owns_is_a_gap(tmp_path: Path) -> None:
    root = make_repo(tmp_path, [plan_task("T001", tests=["TD-FAKE-01"]), plan_task("T009", owns=["tests/integration/test_fake.py"])])
    found = problems(root, st.new_state(), "T001")
    assert found == [("T001: tests/integration/test_fake.py is named by its requirements, tests or reads, but neither "
                      "it nor a dependency owns it and it does not exist yet")]
    (root / "tests/integration").mkdir(parents=True)
    (root / "tests/integration/test_fake.py").write_text("x", encoding="utf-8")
    assert problems(root, st.new_state(), "T001") == []  # it exists now: an earlier task wrote it


def test_a_path_a_decision_names_is_a_gap_only_when_no_task_owns_it(tmp_path: Path) -> None:
    log = "- **D-001** Question: x? Choice: `tests/integration/test_orphan.py` and `docs/later.md`.\n"
    tasks = [plan_task("T001", decisions=["D-001"]), plan_task("T005", owns=["docs/later.md"])]
    root = make_repo(tmp_path, tasks, log)
    assert problems(root, st.new_state(), "T001") == [
        "T001: tests/integration/test_orphan.py is named by D-001, but no task owns it and it does not exist"
    ]


def test_a_superseded_decision_never_holds_a_task(tmp_path: Path) -> None:
    """D-1350 review: D-001 withdrawn by D-002 is pasted as history only, so what it names is no gap."""
    log = ("- **D-001** Question: x? Choice: `tests/integration/test_orphan.py`.\n"
           "- **D-002** Question: y? Choice: D-001 is superseded by D-002; build `app/t001.rs`.\n")
    root = make_repo(tmp_path, [plan_task("T001", decisions=["D-001"])], log)
    assert problems(root, st.new_state(), "T001") == []


def test_shared_paths_and_gates_are_never_gaps(tmp_path: Path) -> None:
    row = "| TD-FAKE-02 | B | In | REQ-FAKE-001 | Notes go to `docs/decisions.md` and `docs/nowhere.md` |\n"
    root = make_repo(tmp_path, [plan_task("T001", tests=["TD-FAKE-02"]), plan_task("T127", ["T001"], type="gate")])
    (root / "docs/test-design.md").write_text(ROW + row, encoding="utf-8")
    (root / "docs/decisions.md").unlink()  # missing, yet shared: any task may write it
    found = problems(root, st.new_state(), "T001")
    assert len(found) == 1 and found[0].startswith("T001: docs/nowhere.md is named by")
    assert problems(root, st.new_state(), "T127") == []  # a gate has no prompt and no plan to check


def test_a_done_dependency_missing_its_file_and_an_unfinished_one_are_gaps(tmp_path: Path) -> None:
    root = make_repo(tmp_path, [plan_task("T001"), plan_task("T002"), plan_task("T003", ["T001", "T002"])])
    found = problems(root, done("T001"), "T003")
    assert found == ["T003: dependency T001 is DONE but its owned file app/t001.rs is not on main",
                     "T003: dependency T002 is QUEUED, not DONE"]
    assert problems(root, done("T001"), "T003", pending_ok=True) == found[:1]


def test_a_context_over_the_limit_is_a_gap(tmp_path: Path) -> None:
    root = make_repo(tmp_path, [plan_task("T001")])
    (root / "app").mkdir()
    (root / "app/t001.rs").write_text("x" * 400_000, encoding="utf-8")  # 100k tokens: 51% of the window
    assert problems(root, st.new_state(), "T001") == ["T001: measures 51% of the context window, over its 45% limit"]
    (root / "kit/prompts/T001.md").unlink()
    assert "prompt kit/prompts/T001.md is missing" in problems(root, st.new_state(), "T001")[0]


def test_the_command_line_reports_and_exits_one_on_gaps(tmp_path: Path, capsys: Any) -> None:
    root = make_repo(tmp_path, [plan_task("T001"), plan_task("T002", ["T001"])])
    state_file = write_state(root, done("T001"))
    assert ph.main(["--root", str(root), "--state", str(state_file), "--all", "--json"]) == 1
    out = json.loads(capsys.readouterr().out)
    assert out == {"T002": ["T002: dependency T001 is DONE but its owned file app/t001.rs is not on main"],
                   "DONE tasks": ["T001: DONE but its owned file app/t001.rs is not on main"]}
    assert ph.main(["--root", str(root), "--state", str(root / "nowhere.json"), "T001"]) == 2


def write_state(root: Path, state: st.State) -> Path:
    path = root / "state.json"
    path.write_text(json.dumps(state), encoding="utf-8")
    return path


# ---- the runner: a plan gap is passed over before the claim, with no attempt used


def gaps(bad: dict[str, list[str]]) -> Any:
    return lambda state, tid: list(bad.get(tid, []))


def test_a_plan_gap_task_is_passed_over_with_no_attempt_and_the_next_is_claimed(tmp_path: Path) -> None:
    cfg = config([make_task("T001"), make_task("T002")], tmp_path)
    cfg.plan_health = gaps({"T001": ["T001: tests/x.py is named by its tests, but no one owns it"]})
    env = FakeEnv()
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING
    first = st.entry(env.store, "T001")
    assert first["status"] == st.QUEUED and first["attempts"] == 0 and "history" not in first
    assert first["plan_gap"] == {"at": "2026-11-01T12:00:00Z", "problems": ["T001: tests/x.py is named by its tests, but no one owns it"]}
    assert [r[0] for r in env.runs] == ["T002"]
    assert "PLAN GAP T001 (since 2026-11-01T12:00:00Z, no attempt used" in render_status(env.store, "", NOW)


def test_a_fixed_plan_clears_the_mark_and_the_task_is_claimed(tmp_path: Path) -> None:
    cfg = config([make_task("T001")], tmp_path)
    cfg.plan_health = gaps({"T001": ["T001: gap"]})
    env = FakeEnv()
    assert cycle.run_cycle(cfg, env, NOW) == "idle" and env.runs == []
    pushes = env.pushes
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=5)) == "idle" and env.pushes == pushes  # unchanged: no write
    cfg.plan_health = gaps({})
    assert cycle.run_cycle(cfg, env, NOW + timedelta(minutes=10)) == st.MERGE_PENDING
    assert "plan_gap" not in st.entry(env.store, "T001") and st.entry(env.store, "T001")["attempts"] == 0


def test_a_failing_check_holds_nothing_and_long_lists_are_cut(tmp_path: Path) -> None:
    def broken(state: st.State, tid: str) -> list[str]:
        raise RuntimeError("tasks.json unreadable")

    cfg = config([make_task("T001")], tmp_path)
    cfg.plan_health = broken
    env = FakeEnv()
    env.store = st.with_task(st.sync_graph(env.store, cfg.graph), "T001", plan_gap={"at": "2026-11-01T11:00:00Z", "problems": ["old"]})
    assert cycle.run_cycle(cfg, env, NOW) == st.MERGE_PENDING  # claimed as before D-1350
    assert st.entry(env.store, "T001")["plan_gap"] == {"at": "2026-11-01T11:00:00Z", "problems": ["old"]}  # no evidence of a fix
    many = [f"gap {i}" for i in range(14)]
    sound, marked = cycle.plan_health_pass(dataclasses.replace(cfg, plan_health=gaps({"T001": many})), st.new_state(), ["T001", "T002"], NOW)
    assert sound == ["T002"]
    kept = st.entry(marked, "T001")["plan_gap"]["problems"]
    assert kept[:10] == many[:10] and kept[10] == "and 4 more (python3 scripts/plan_health.py T001)" and len(kept) == 11


def test_fix_main_is_never_checked_and_a_later_gap_is_marked_beside_a_sound_first() -> None:
    cfg = config([make_task("T001"), make_task("T002")], Path("."))
    asked: list[str] = []

    def check(state: st.State, tid: str) -> list[str]:
        asked.append(tid)
        return ["T002: gap"] if tid == "T002" else []

    cfg.plan_health = check
    sound, marked = cycle.plan_health_pass(cfg, st.new_state(), [st.FIX_MAIN, "T001", "T002"], NOW)
    assert sound == [st.FIX_MAIN, "T001"] and asked == ["T001", "T002"]
    assert st.entry(marked, "T002")["plan_gap"]["problems"] == ["T002: gap"]


def test_a_changed_problem_list_keeps_the_time_the_hold_began() -> None:
    cfg = dataclasses.replace(config([make_task("T001")], Path(".")), plan_health=gaps({"T001": ["first"]}))
    _, held = cycle.plan_health_pass(cfg, st.new_state(), ["T001"], NOW)
    later = dataclasses.replace(cfg, plan_health=gaps({"T001": ["first", "second"]}))
    _, held = cycle.plan_health_pass(later, held, ["T001"], NOW + timedelta(hours=2))
    assert st.entry(held, "T001")["plan_gap"] == {"at": "2026-11-01T12:00:00Z", "problems": ["first", "second"]}


def test_an_unbuildable_checker_leaves_the_runner_without_the_check(tmp_path: Path, capsys: Any, monkeypatch: Any) -> None:
    """D-1350 review: a spec or test design that cannot be read must not stop the runner."""
    def boom(root: Path) -> Any:
        raise ValueError("test design unreadable")

    monkeypatch.setattr(ph.gen_prompts, "load_sources", boom)
    assert entry.plan_health_checker(tmp_path, {"tasks": []}) is None
    monkeypatch.undo()
    assert "runner: plan health unavailable: test design unreadable" in capsys.readouterr().out
    assert callable(entry.plan_health_checker(make_repo(tmp_path / "ok", [plan_task("T001")]), {"tasks": [plan_task("T001")]}))
