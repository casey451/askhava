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


def test_goal_equal_to_title_is_an_error() -> None:
    with pytest.raises(ValueError, match="goal repeats the title"):
        gp.render(Template(TEMPLATE_TEXT), fake_task(goal="Fake thing"))
    assert "## Goal\n\nDo a fake thing." in gp.render(Template(TEMPLATE_TEXT), fake_task())


def test_no_tests_changes_definition_of_done() -> None:
    text = gp.render(Template(TEMPLATE_TEXT), fake_task(tests=[], acceptance="The fake thing works."))
    assert gp.NO_TESTS_DONE in text
    assert gp.TESTS_DONE not in text
    assert "Acceptance: The fake thing works." in text
    assert gp.TESTS_DONE in gp.render(Template(TEMPLATE_TEXT), fake_task())


# --- P-1 (D-485): pasted sources, test commands, copy steps, derived context ---------------

SPEC_TEXT = (
    "# Spec: fake\n\nPrefix: `REQ`\n\n"
    "- **REQ-001** The fake rule holds at 9:00 sharp. Source: `src.md:1`.\n"
)
DESIGN_TEXT = (
    "# Test design\n\n## 5. The four end-to-end journeys\n\nEvery journey ends with the fake closing checks.\n\n"
    "**J1 Homeowner** (INT).\n1. A fake step.\n\n"
    "## 7. Catalog\n\n| ID | Kind | Layer | Covers | Cases |\n|---|---|---|---|---|\n"
    "| TD-REQ-01 | E | U | REQ-001 | 8:59:59 refused, 9:00:00 accepted |\n"
)
DECISION_TEXT = "# Decisions\n\n- **D-001** Question: fake? Choice: " + "x" * 600 + "\n"


def sourced_repo(tmp_path: Path, task: dict[str, Any]) -> Path:
    root = make_repo(tmp_path, [task])
    (root / "docs" / "spec").mkdir(parents=True)
    (root / "docs" / "spec" / "fake.md").write_text(SPEC_TEXT, encoding="utf-8")
    (root / "docs" / "test-design.md").write_text(DESIGN_TEXT, encoding="utf-8")
    (root / "docs" / "decisions.md").write_text(DECISION_TEXT, encoding="utf-8")
    (root / "src.md").write_text("The cited source line.\n", encoding="utf-8")
    return root


def pasted_task() -> dict[str, Any]:
    return fake_task(
        requirements=["REQ-001"], tests=["TD-REQ-01", "J1"], decisions=["D-001"], cites=["src.md:1"],
        reads=["docs/spec/fake.md"],
    )


def test_prompt_pastes_requirements_tests_journeys_decisions_and_cites(tmp_path: Path) -> None:
    root = sourced_repo(tmp_path, pasted_task())
    text = gp.render(Template(TEMPLATE_TEXT), pasted_task(), gp.load_sources(root))
    assert "**REQ-001** The fake rule holds at 9:00 sharp. (`docs/spec/fake.md:5`)" in text
    assert "**TD-REQ-01** (kind E; layers U; covers REQ-001): 8:59:59 refused, 9:00:00 accepted" in text
    assert "1. A fake step." in text and "Every journey ends with the fake closing checks." in text
    assert "- **D-001** Question: fake? Choice: " + "x" * (gp.DECISION_CHARS - 25) in text
    assert "x" * 500 not in text
    assert "- `src.md:1`: The cited source line." in text


def test_run_tests_block_maps_each_test_layer() -> None:
    owns = [
        "app/fake/src/rules.rs",
        "app/fake/src/tests/fee_tiers.rs",
        "tests/integration/tests/outbox.rs",
        "tests/security/ui/forge.rs",
        "tests/e2e/pick_flow.spec.ts",
        "tests/a11y/pick_forms.spec.ts",
        "tests/docs/test_terms.py",
    ]
    block = gp.run_tests_block(fake_task(owns=owns))
    assert "cargo test -p askhava-fake tests::fee_tiers" in block
    assert "cargo test -p askhava-integration --test outbox" in block
    assert "npx playwright test tests/e2e/pick_flow.spec.ts" in block
    assert "npx playwright test --config tests/a11y/playwright.config.ts tests/a11y/pick_forms.spec.ts" in block
    assert "python3 -m pytest tests/docs/test_terms.py" in block
    assert "forge" not in block and "rules.rs" not in block
    assert gp.run_tests_block(fake_task(owns=["app/fake/src/rules.rs"])) == gp.NO_TEST_FILES


def test_copy_steps_only_for_tasks_that_own_copy() -> None:
    plain = gp.render(Template(TEMPLATE_TEXT), fake_task(owns=["app/fake/src/rules.rs"]))
    assert gp.COPY_SKIP in plain and gp.COPY_STEPS not in plain
    for path in ("app/site/templates/home.html", "app/site/static/site.css", "docs/legal/terms.md", "docs/copy/site.md"):
        text = gp.render(Template(TEMPLATE_TEXT), fake_task(owns=[path]))
        assert gp.COPY_STEPS in text, path
    assert "cold read (WORD-028)" in plain


def test_definition_of_done_has_no_unverifiable_items() -> None:
    text = gp.render(Template(TEMPLATE_TEXT), fake_task())
    assert "`bash scripts/smoke.sh` is green on your branch" in text
    for gone in ("Required checks pass on your commit", "Coverage does not drop", "A fresh-eyes review of the diff"):
        assert gone not in text
    assert "docs/architecture.md` section 2" in text and "docs/code-docs.md" in text


def test_context_is_derived_from_bytes_and_written_back(tmp_path: Path) -> None:
    task = pasted_task()
    task["owns"] = ["app/fake.rs"]
    task["reads"] = ["docs/spec/fake.md", "big.md"]
    root = sourced_repo(tmp_path, task)
    (root / "big.md").write_text("y" * 400_000, encoding="utf-8")
    (root / "app").mkdir()
    (root / "app" / "fake.rs").write_text("z" * 80_000, encoding="utf-8")
    assert gp.main(["--root", str(root)]) == 0
    stored = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))["tasks"][0]["budget"]["context_pct"]
    prompt = (root / "kit/prompts/T900.md").read_text(encoding="utf-8")
    expected = -(-(len(prompt.encode()) + 480_000) * 100 // (4 * 200_000))
    assert stored == expected and f"{stored}% of the context window" in prompt
    assert gp.main(["--root", str(root), "--check"]) == 0


def test_check_prints_token_estimates_and_notes_drift(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = make_repo(tmp_path, [fake_task()])
    assert gp.main(["--root", str(root)]) == 0
    capsys.readouterr()
    (root / "app").mkdir()
    (root / "app" / "fake.py").write_text("q" * 200_000, encoding="utf-8")
    assert gp.main(["--root", str(root), "--check"]) == 0
    out = capsys.readouterr().out
    assert "T900: about" in out and "tokens" in out and "regenerate to refresh" in out
