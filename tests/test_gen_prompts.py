"""Tests for scripts/gen_prompts.py (fake tasks only)."""

from __future__ import annotations

import importlib.util
import json
import re
import sys
from pathlib import Path
from string import Template
from types import ModuleType
from typing import Any

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT = REPO_ROOT / "scripts" / "gen_prompts.py"


def load_script() -> ModuleType:
    spec = importlib.util.spec_from_file_location("gen_prompts", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["gen_prompts"] = module
    spec.loader.exec_module(module)
    return module


gp = load_script()
TEMPLATE_TEXT = (REPO_ROOT / "kit/prompts/TEMPLATE.md").read_text(encoding="utf-8")


def fake_task(**over: Any) -> dict[str, Any]:
    task: dict[str, Any] = {
        "id": "T900",
        "slug": "fake-thing",
        "title": "Fake thing",
        "goal": "Do a fake thing.",
        "type": "build",
        "phase": 1,
        "stage": "2",
        "model": "claude-sonnet-5-5",
        "effort": "medium",
        "reviewer_model": "claude-opus-5-5",
        "review_areas": ["money"],
        "owns": ["app/fake.py"],
        "reads": ["docs/spec/fees.md"],
        "requirements": ["REQ-FAKE-001"],
        "tests": ["TD-FAKE-01", "J1"],
        "decisions": ["D-001"],
        "cites": ["BUILD_BRIEF.md:61"],
        "budget": {"files": 1, "lines": 50, "context_pct": 8},
        "checks": ["Ci", "Twin", "Ally"],
        "branch": "claude/task/T900-fake-thing",
        "prompt": "kit/prompts/T900.md",
    }
    task.update(over)
    return task


def gate_task() -> dict[str, Any]:
    return fake_task(id="T901", prompt=None, model=None, type="gate")


def make_repo(tmp_path: Path, tasks: list[dict[str, Any]]) -> Path:
    (tmp_path / "kit/prompts").mkdir(parents=True)
    (tmp_path / "kit/tasks.json").write_text(
        json.dumps({"tasks": tasks}), encoding="utf-8"
    )
    (tmp_path / "kit/prompts/TEMPLATE.md").write_text(TEMPLATE_TEXT, encoding="utf-8")
    return tmp_path


def test_render_names_everything() -> None:
    task = fake_task()
    text = gp.render(Template(TEMPLATE_TEXT), task)
    for item in (
        "claude-sonnet-5-5",
        "app/fake.py",
        "TD-FAKE-01",
        "J1",
        "REQ-FAKE-001",
        "claude/task/T900-fake-thing",
        "BUILD_BRIEF.md:61",
        "D-001",
    ):
        assert item in text


def test_render_empty_lists_use_note() -> None:
    text = gp.render(
        Template(TEMPLATE_TEXT), fake_task(reads=[], tests=[], review_areas=[])
    )
    assert gp.NONE_NOTE in text
    assert "areas: none" in text


def test_template_wording_rules() -> None:
    lowered = TEMPLATE_TEXT.lower()
    assert "introduction fee by job size" in lowered
    for banned in ("commission", "vetted", "best"):
        assert re.search(rf"\b{banned}\b", lowered) is None


def test_generate_skips_gates_and_check_passes(tmp_path: Path) -> None:
    root = make_repo(tmp_path, [fake_task(), gate_task()])
    assert gp.main(["--root", str(root)]) == 0
    assert (root / "kit/prompts/T900.md").is_file()
    assert not (root / "kit/prompts/T901.md").exists()
    assert gp.main(["--root", str(root), "--check"]) == 0


def test_check_fails_when_missing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = make_repo(tmp_path, [fake_task()])
    assert gp.main(["--root", str(root), "--check"]) == 1
    assert "missing" in capsys.readouterr().out


def test_check_fails_when_stale(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = make_repo(tmp_path, [fake_task()])
    gp.main(["--root", str(root)])
    path = root / "kit/prompts/T900.md"
    path.write_text(path.read_text(encoding="utf-8") + "\nextra\n", encoding="utf-8")
    assert gp.main(["--root", str(root), "--check"]) == 1
    assert "stale" in capsys.readouterr().out


def test_check_fails_when_tasks_change(tmp_path: Path) -> None:
    root = make_repo(tmp_path, [fake_task()])
    gp.main(["--root", str(root)])
    (root / "kit/tasks.json").write_text(
        json.dumps({"tasks": [fake_task(model="claude-opus-5-5")]}), encoding="utf-8"
    )
    assert gp.main(["--root", str(root), "--check"]) == 1


def test_check_fails_when_test_id_dropped(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = make_repo(tmp_path, [fake_task()])
    gp.main(["--root", str(root)])
    path = root / "kit/prompts/T900.md"
    text = path.read_text(encoding="utf-8").replace("TD-FAKE-01", "TD-REDACTED")
    path.write_text(text, encoding="utf-8")
    assert gp.main(["--root", str(root), "--check"]) == 1
    assert "stale" in capsys.readouterr().out


def test_real_prompts_are_current() -> None:
    assert gp.main(["--root", str(REPO_ROOT), "--check"]) == 0


def test_check_fails_on_orphan_prompt(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = make_repo(tmp_path, [fake_task()])
    gp.main(["--root", str(root)])
    (root / "kit/prompts/T777.md").write_text("old", encoding="utf-8")
    assert gp.main(["--root", str(root), "--check"]) == 1
    assert "orphan prompt with no task: T777.md" in capsys.readouterr().out


def test_missing_key_names_the_task(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    broken = fake_task()
    del broken["tests"]
    del broken["budget"]["lines"]
    root = make_repo(tmp_path, [broken])
    with pytest.raises(
        ValueError, match=r"task T900: missing required key\(s\): tests, budget.lines"
    ):
        gp.render(Template(TEMPLATE_TEXT), broken)
    assert gp.main(["--root", str(root)]) == 1
    assert "T900" in capsys.readouterr().out


def test_template_keeps_safety_rules() -> None:
    for phrase in (
        "Do not mark this task done",
        "Do not touch `claude/kit-state`",
        "do not force-push",
        "Fake data only",
        "Protected paths",
        "## Start routine",
        "## End routine",
        "matches the next task in `kit/state.json`",
        "Never weaken a test",
    ):
        assert phrase in TEMPLATE_TEXT


def test_fees_render_and_no_placeholder_left() -> None:
    text = gp.render(Template(TEMPLATE_TEXT), fake_task())
    assert "$25 / $75 / $199" in text
    assert "$$" not in text
    assert "${" not in text


def test_goal_section_omitted_when_equal_to_title() -> None:
    same = gp.render(Template(TEMPLATE_TEXT), fake_task(goal="Fake thing"))
    assert "## Goal" not in same
    assert "## Goal" in gp.render(Template(TEMPLATE_TEXT), fake_task())


def test_no_tests_changes_definition_of_done() -> None:
    text = gp.render(Template(TEMPLATE_TEXT), fake_task(tests=[]))
    assert gp.NO_TESTS_DONE in text
    assert gp.TESTS_DONE not in text
    assert gp.TESTS_DONE in gp.render(Template(TEMPLATE_TEXT), fake_task())
