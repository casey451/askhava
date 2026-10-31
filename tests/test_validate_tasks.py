"""Tests for scripts/validate_tasks.py (fake repositories and fake tasks only)."""

from __future__ import annotations

import copy
import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRIPT = REPO_ROOT / "scripts" / "validate_tasks.py"


def load_script() -> ModuleType:
    spec = importlib.util.spec_from_file_location("validate_tasks", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules["validate_tasks"] = module
    spec.loader.exec_module(module)
    return module


vt = load_script()

OPUS = "claude-opus-5-5"
SONNET = "claude-sonnet-5-5"
HAIKU = "claude-haiku-5-5"
SPEC = (
    "# Spec: test\n\nPrefix: `TST`\n\n"
    "- **TST-001** Rule one. Source: `src.md:3`.\n"
    "- **TST-002** Rule two. Source: `src.md:4`.\n"
)
TEST_DESIGN = (
    "# Test design\n\n| ID | Kind | Layer | Covers | Cases |\n|---|---|---|---|---|\n"
    "| TD-TST-01 | B | U | TST-001, TST-002 | fake cases |\n"
)


def make_task(tid: str, slug: str, **over: Any) -> dict[str, Any]:
    task: dict[str, Any] = {
        "id": tid,
        "slug": slug,
        "title": f"Fake task {slug}",
        "goal": "Fake goal.",
        "acceptance": "Fake acceptance.",
        "type": "build",
        "phase": 1,
        "stage": "2",
        "module": "fake",
        "deps": [],
        "model": SONNET,
        "effort": "medium",
        "reviewer_model": SONNET,
        "review_areas": [],
        "owns": [],
        "reads": [],
        "requirements": [],
        "tests": [],
        "decisions": [],
        "cites": [],
        "budget": {"files": 1, "lines": 100, "context_pct": 10},
        "checks": ["Ci", "Twin", "Ally"],
        "branch": f"claude/task/{tid}-{slug}",
        "prompt": f"kit/prompts/{tid}.md",
    }
    task.update(over)
    if "budget" not in over:
        task["budget"]["files"] = len(task["owns"])
    return task


def gate_fields(kind: str) -> dict[str, Any]:
    fields: dict[str, Any] = {"kind": kind, "waits_on_owner": vt.GATE_WAITS[kind]}
    if kind == vt.CONDITIONAL_GATE:
        fields["conditional"] = True
    return fields


def make_gate(
    tid: str, slug: str, kind: str, deps: list[str], stage: str, phase: int
) -> dict[str, Any]:
    return make_task(
        tid,
        slug,
        type="gate",
        stage=stage,
        phase=phase,
        module=None,
        deps=deps,
        model=None,
        effort=None,
        reviewer_model=None,
        checks=[],
        branch=None,
        prompt=None,
        budget={"files": 0, "lines": 0, "context_pct": 0},
        gate=gate_fields(kind),
    )


def valid_tasks() -> list[dict[str, Any]]:
    return [
        make_task(
            "T001",
            "fake-rules",
            owns=["app/fake/rules.py", "tests/unit/fake/test_rules.py"],
            budget={"files": 2, "lines": 200, "context_pct": 10},
            requirements=["TST-001", "TST-002"],
            tests=["TD-TST-01", "J1", "J2", "J3", "J4"],
            reads=["docs/spec/area.md"],
        ),
        make_gate("T002", "checkpoint", "account-checkpoint", ["T001"], "checkpoint", 0),
        make_gate("T003", "page-go", "stage-0.5-go", ["T002"], "0.5", 0),
        make_task(
            "T004",
            "fake-sandbox",
            type="harness",
            stage="6",
            phase=2,
            deps=["T002"],
            model=OPUS,
            reviewer_model=OPUS,
            review_areas=["harness", "security"],
            owns=[
                "stand_ins/fake/",
                "app/adapters/Cargo.toml",
                "app/adapters/src/twilio_sms.rs",
                ".github/workflows/twin.yml",
                ".github/workflows/a11y.yml",
                "tests/e2e/playwright.config.ts",
            ],
            cites=["src.md:3"],
            acceptance="The fake sandbox answers like the vendor.",
        ),
        make_gate("T005", "real-texts", "real-texts-casey-phone", ["T004"], "6", 2),
        make_gate("T006", "finished", "finished", ["T005"], "6", 2),
        make_gate("T007", "gate-b", "gate-b", ["T006"], "6", 2),
        make_gate("T008", "gate-c", "gate-c", ["T007"], "7", 2),
        make_gate("T009", "first-charge", "first-live-charge", ["T008"], "7", 2),
    ]


def write_repo(root: Path, tasks: list[dict[str, Any]], **header: Any) -> Path:
    (root / "docs" / "spec").mkdir(parents=True, exist_ok=True)
    (root / "kit").mkdir(exist_ok=True)
    (root / "src.md").write_text("# Fake source\n\nRule one.\nRule two.\n", encoding="utf-8")
    (root / "docs" / "decisions.md").write_text(
        "# Decisions\n\n- **D-001** Fake decision.\n", encoding="utf-8"
    )
    (root / "docs" / "spec" / "area.md").write_text(SPEC, encoding="utf-8")
    (root / "docs" / "spec" / "index.md").write_text("# Index\n", encoding="utf-8")
    (root / "docs" / "test-design.md").write_text(TEST_DESIGN, encoding="utf-8")
    data: dict[str, Any] = {
        "schema": 1,
        "protected_paths": list(vt.PROTECTED),
        "shared_paths": sorted(vt.ALLOWED_SHARED),
        "kit_outputs": list(vt.KIT_OUTPUTS),
        "tasks": tasks,
    }
    data.update(header)
    (root / "kit" / "tasks.json").write_text(json.dumps(data), encoding="utf-8")
    return root


def errors_for(tmp_path: Path, tasks: list[dict[str, Any]], **header: Any) -> list[str]:
    return list(vt.run(write_repo(tmp_path, tasks, **header)).errors)


def assert_error(errors: list[str], fragment: str) -> None:
    assert any(fragment in e for e in errors), f"no error containing {fragment!r} in {errors}"


def task(tasks: list[dict[str, Any]], tid: str) -> dict[str, Any]:
    return next(t for t in tasks if t["id"] == tid)


def test_valid_graph_passes(tmp_path: Path) -> None:
    report = vt.run(write_repo(tmp_path, valid_tasks()))
    assert report.errors == []
    assert report.requirements == 2
    assert report.td_entries == 1


def test_real_task_graph_passes() -> None:
    report = vt.run(REPO_ROOT)
    assert report.errors == []
    assert len(report.tasks) >= 80


def test_cycle_is_an_error(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["deps"] = ["T004"]
    assert_error(errors_for(tmp_path, tasks), "dependency cycle")


def test_unknown_and_self_dependencies(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["deps"] = ["T999", "T001"]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "depends on unknown task T999")
    assert_error(errors, "depends on itself")


def test_duplicate_id_is_an_error(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.append(make_task("T001", "other-slug", requirements=["TST-001"]))
    assert_error(errors_for(tmp_path, tasks), "duplicate id")


def test_uncovered_requirement_test_entry_and_journey(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(requirements=["TST-001"], tests=["J1"])
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "requirement TST-002 is covered by no task")
    assert_error(errors, "test entry TD-TST-01 is covered by no task")
    assert_error(errors, "journey J4 is covered by no task")


def test_unknown_requirement_and_test_entry(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["requirements"].append("TST-099")
    task(tasks, "T001")["tests"] = ["TD-TST-01", "TD-TST-77", "J1", "J2", "J3", "J4"]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "unknown requirement TST-099")
    assert_error(errors, "unknown test entry TD-TST-77")


def test_protected_path_needs_a_harness_task(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["owns"].append(".github/workflows/ci.yml")
    task(tasks, "T001")["budget"]["files"] = 3
    assert_error(errors_for(tmp_path, tasks), "owns protected path .github/workflows/ci.yml")


def test_sources_are_read_only_even_for_harness(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004")["owns"] = ["sources/01_spec/fake.md", "app/adapters/src/twilio_sms.rs"]
    task(tasks, "T004")["budget"]["files"] = 2
    assert_error(errors_for(tmp_path, tasks), "sources/ is read-only")


def test_unordered_tasks_may_not_share_a_path(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.insert(
        1, make_task("T010", "fake-other", owns=["app/fake/rules.py"], requirements=["TST-001"])
    )
    task(tasks, "T002")["deps"] = ["T001", "T010"]
    assert_error(errors_for(tmp_path, tasks), "can run in either order but both own")


def test_ordered_tasks_may_share_an_app_path(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.insert(
        1,
        make_task(
            "T010",
            "fake-other",
            deps=["T001"],
            owns=["app/fake/rules.py"],
            requirements=["TST-001"],
            reads=["docs/spec/area.md"],
        ),
    )
    task(tasks, "T002")["deps"] = ["T010"]
    assert errors_for(tmp_path, tasks) == []


def test_shared_paths_may_be_owned_by_unordered_tasks(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["owns"].append("docs/decisions.md")
    task(tasks, "T001")["budget"]["files"] = 3
    tasks.insert(
        1,
        make_task(
            "T010", "fake-other", owns=["docs/decisions.md"], requirements=["TST-001"], reads=["docs/spec/area.md"]
        ),
    )
    task(tasks, "T002")["deps"] = ["T001", "T010"]
    assert errors_for(tmp_path, tasks) == []


def test_a_later_task_may_not_take_over_a_test_file(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.insert(
        1,
        make_task(
            "T010",
            "fake-other",
            deps=["T001"],
            owns=["tests/unit/fake/test_rules.py"],
            requirements=["TST-001"],
        ),
    )
    task(tasks, "T002")["deps"] = ["T010"]
    assert_error(errors_for(tmp_path, tasks), "T010: changes test tests/unit/fake/test_rules.py")


def test_models_must_be_full_ids(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(model="sonnet", reviewer_model=HAIKU, effort="max")
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "is not a full model ID")
    assert_error(errors, "is not Sonnet or Opus")
    assert_error(errors, "is not low, medium or high")


@pytest.mark.parametrize("area", ["money", "privacy", "security", "recovery"])
def test_sensitive_areas_need_an_opus_reviewer(tmp_path: Path, area: str) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["review_areas"] = [area]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "need an Opus reviewer")
    if area != "security":  # security alone forces the Opus model only through sensitive owned paths (D-483)
        assert_error(errors, "runs on Opus (BUILD_BRIEF.md:498)")
    task(tasks, "T001").update(reviewer_model=OPUS, model=OPUS)
    assert errors_for(tmp_path, tasks) == []


def test_test_only_work_in_a_sensitive_area_may_run_on_sonnet(tmp_path: Path) -> None:
    """D-483: a task that owns only test files needs the Opus reviewer, not the Opus model."""
    tasks = valid_tasks()
    t = task(tasks, "T001")
    t.update(review_areas=["privacy"], reviewer_model=OPUS, model="claude-sonnet-5-5", effort="medium")
    t["owns"] = [o for o in t["owns"] if not o.startswith(("app/", "migrations/"))] or ["tests/e2e/x.spec.ts"]
    errors = errors_for(tmp_path, tasks)
    assert not [e for e in errors if "runs on Opus" in e]


def test_harness_tasks_need_an_opus_reviewer(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004")["reviewer_model"] = SONNET
    assert_error(errors_for(tmp_path, tasks), "need an Opus reviewer")


def test_high_effort_only_for_opus_and_no_haiku_on_money(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(
        effort="high", model=HAIKU, review_areas=["money"], reviewer_model=OPUS
    )
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "high effort is only for Opus tasks")
    assert_error(errors, "Haiku may not do money")


def test_size_limits(tmp_path: Path) -> None:
    tasks = valid_tasks()
    owns = [f"app/fake/m{i}.py" for i in range(vt.MAX_OWNS + 1)]
    task(tasks, "T001").update(
        owns=owns, budget={"files": 2, "lines": vt.MAX_LINES + 1, "context_pct": vt.MAX_CONTEXT_PCT + 1}
    )
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, f"owned paths {vt.MAX_OWNS + 1} over the limit")
    assert_error(errors, f"budget lines {vt.MAX_LINES + 1} over the limit")
    assert_error(errors, f"budget context_pct {vt.MAX_CONTEXT_PCT + 1} over the limit of {vt.MAX_CONTEXT_PCT}")
    assert_error(errors, f"budget files 2 does not match the {vt.MAX_OWNS + 1} owned files")


def test_every_gate_kind_once(tmp_path: Path) -> None:
    tasks = [t for t in valid_tasks() if t["id"] != "T003"]
    assert_error(errors_for(tmp_path, tasks), "no gate of kind stage-0.5-go")


def test_gate_order(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T008")["deps"] = ["T006"]
    assert_error(errors_for(tmp_path, tasks), "gate gate-c must come after gate gate-b")


def test_gate_owns_nothing(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T006")["owns"] = ["docs/x.md"]
    assert_error(errors_for(tmp_path, tasks), "a gate owns no paths")


def test_phase_two_work_follows_the_checkpoint(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004")["deps"] = ["T001"]
    assert_error(errors_for(tmp_path, tasks), "must come after the account checkpoint")


def test_phase_one_work_precedes_the_checkpoint(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.append(make_task("T010", "late-fake", requirements=["TST-001"]))
    assert_error(errors_for(tmp_path, tasks), "must finish before the account checkpoint")


def test_no_dependency_on_a_later_stage(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.append(
        make_task("T010", "fake-early", stage="1", deps=["T001"], requirements=["TST-001"])
    )
    task(tasks, "T002")["deps"] = ["T001", "T010"]
    assert_error(errors_for(tmp_path, tasks), "from a later stage")


def test_decisions_and_citations_resolve(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004").update(
        decisions=["D-002"], cites=["src.md:2", "src.md:9", "nothing.md:1", "bad"]
    )
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "decision D-002 is not in docs/decisions.md")
    assert_error(errors, "src.md:2 is a blank line")
    assert_error(errors, "src.md:9 is past the end")
    assert_error(errors, "cited file not found in repository: nothing.md")
    assert_error(errors, "'bad' is not path:LINE")


def test_reads_must_exist_or_be_made_earlier(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004")["reads"] = ["app/fake/rules.py", "runner/state.py", "docs/missing.md"]
    errors = errors_for(tmp_path, tasks)
    assert errors == [e for e in errors if "docs/missing.md" in e]
    assert_error(errors, "reads docs/missing.md")


def test_branch_and_prompt_follow_the_task(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(branch="main", prompt="kit/prompts/T999.md", slug="Bad Slug")
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "branch must be")
    assert_error(errors, "prompt must be")
    assert_error(errors, "slug must be")


def test_build_task_must_cover_something(tmp_path: Path) -> None:
    tasks = valid_tasks()
    tasks.insert(1, make_task("T010", "empty-fake"))
    task(tasks, "T002")["deps"] = ["T001", "T010"]
    assert_error(errors_for(tmp_path, tasks), "must cover at least one requirement or test entry")


def test_missing_key_is_reported(tmp_path: Path) -> None:
    tasks = valid_tasks()
    del task(tasks, "T001")["owns"]
    assert_error(errors_for(tmp_path, tasks), "'owns' missing")


def test_file_header_cannot_loosen_the_rules(tmp_path: Path) -> None:
    errors = errors_for(
        tmp_path, valid_tasks(), shared_paths=["app/"], protected_paths=[".github/"]
    )
    assert_error(errors, "shared_paths may only be")
    assert_error(errors, "protected_paths must list every protected path")


def test_missing_and_malformed_files(tmp_path: Path) -> None:
    assert_error(list(vt.run(tmp_path).errors), "tasks file not found")
    bad = tmp_path / "bad.json"
    bad.write_text("{not json", encoding="utf-8")
    assert_error(list(vt.run(tmp_path, bad).errors), "not valid JSON")


def test_main_exit_codes(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    root = write_repo(tmp_path, valid_tasks())
    assert vt.main(["--root", str(root)]) == 0
    assert "Errors: 0" in capsys.readouterr().out
    broken = copy.deepcopy(valid_tasks())
    task(broken, "T001")["deps"] = ["T004"]
    write_repo(tmp_path, broken)
    assert vt.main(["--root", str(root)]) == 1
    assert "ERROR dependency cycle" in capsys.readouterr().err


@pytest.mark.parametrize(
    ("path", "area"),
    [
        ("app/payments/rules.py", "money"),
        ("app/privacy/vault.py", "privacy"),
        ("app/portals/owner_auth.py", "security"),
        ("app/recovery/replay.py", "recovery"),
    ],
)
def test_review_areas_are_derived_from_owned_paths(tmp_path: Path, path: str, area: str) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["owns"] = [path, "tests/unit/fake/test_rules.py"]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, f"review_areas must include ['{area}']")
    assert_error(errors, "need an Opus reviewer")
    assert_error(errors, "runs on Opus")


def test_only_cleanup_or_harness_may_own_a_directory(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["owns"] = ["app/fake/", "tests/unit/fake/test_rules.py"]
    assert_error(errors_for(tmp_path, tasks), "only cleanup and harness tasks may own a directory")


def test_cleanup_owning_app_needs_opus(tmp_path: Path) -> None:
    tasks = valid_tasks()
    cleanup = make_task("T010", "fake-cleanup", type="cleanup", deps=["T001"], owns=["app/"])
    tasks.insert(1, cleanup)
    task(tasks, "T002")["deps"] = ["T010"]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "a task owning a directory needs an Opus reviewer")
    assert_error(errors, "review_areas must include ['money', 'privacy', 'recovery', 'security']")
    areas = ["money", "privacy", "recovery", "security"]
    cleanup.update(model=OPUS, reviewer_model=OPUS, review_areas=areas)
    assert errors_for(tmp_path, tasks) == []


@pytest.mark.parametrize(
    ("tid", "change", "fragment"),
    [
        ("T006", {"waits_on_owner": True}, "gate finished must have waits_on_owner False"),
        ("T007", {"waits_on_owner": False}, "gate gate-b must have waits_on_owner True"),
        ("T007", {"conditional": True}, "only gate real-texts-casey-phone is conditional"),
        ("T005", {"conditional": False}, "only gate real-texts-casey-phone is conditional"),
    ],
)
def test_gate_flags_are_fixed(
    tmp_path: Path, tid: str, change: dict[str, Any], fragment: str
) -> None:
    tasks = valid_tasks()
    task(tasks, tid)["gate"].update(change)
    assert_error(errors_for(tmp_path, tasks), fragment)


@pytest.mark.parametrize(
    ("gate_id", "kind"), [("T003", "stage-0.5-go"), ("T009", "first-live-charge")]
)
def test_nothing_depends_on_page_go_or_first_charge(
    tmp_path: Path, gate_id: str, kind: str
) -> None:
    tasks = valid_tasks()
    task(tasks, "T006")["deps"] = ["T005", gate_id]
    if gate_id == "T009":
        task(tasks, "T006")["deps"] = ["T005"]
        tasks.append(
            make_task(
                "T010",
                "after-charge",
                stage="7",
                phase=2,
                deps=["T009"],
                cites=["src.md:3"],
                type="docs",
            )
        )
    assert_error(errors_for(tmp_path, tasks), f"nothing may depend on gate {kind}")


def test_only_gate_c_depends_on_gate_b(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T009")["deps"] = ["T008", "T007"]
    assert_error(errors_for(tmp_path, tasks), "only gate-c may depend on gate-b")


def test_real_texts_gate_follows_the_twilio_sandbox(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004")["owns"] = ["stand_ins/fake/"]
    task(tasks, "T004")["review_areas"] = ["harness"]
    assert_error(
        errors_for(tmp_path, tasks), "must come after the task owning app/adapters/src/twilio_sms.rs"
    )


def test_kit_outputs_must_match(tmp_path: Path) -> None:
    errors = errors_for(tmp_path, valid_tasks(), kit_outputs=["runner/", "app/"])
    assert_error(errors, "kit_outputs must be exactly")


def test_kit_file_entries_match_exactly(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T004")["reads"] = ["Cargo.lock", "Cargo.lock.old", "tests/contract/x.py"]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "reads Cargo.lock.old")
    assert not any("reads Cargo.lock," in e or "tests/contract/x.py" in e for e in errors)


# --- P-1 checks (D-485) -------------------------------------------------------------------


def test_p1_limits_bind_build_tasks(tmp_path: Path) -> None:
    assert (vt.MAX_OWNS, vt.MAX_FILES, vt.MAX_LINES, vt.MAX_REQUIREMENTS) == (10, 10, 600, 12)
    tasks = valid_tasks()
    reqs = [f"TST-{i:03d}" for i in range(1, 14)]
    task(tasks, "T001").update(requirements=reqs, budget={"files": 2, "lines": 600, "context_pct": 10})
    assert_error(errors_for(tmp_path, tasks), "requirements 13 over the limit of 12")


def test_harness_keeps_the_d119_size_limits(tmp_path: Path) -> None:
    tasks = valid_tasks()
    harness = task(tasks, "T004")
    harness["owns"] = harness["owns"] + [f"stand_ins/other{i}/" for i in range(6)]
    harness["budget"] = {"files": 11, "lines": 700, "context_pct": 25}
    errors = errors_for(tmp_path, tasks)
    assert not [e for e in errors if "T004" in e and "over the limit" in e]
    harness["budget"]["context_pct"] = vt.MAX_HARNESS_CONTEXT_PCT + 1
    assert_error(errors_for(tmp_path, tasks), "budget context_pct 51 over the limit of 50")


def test_context_limits_are_set_where_the_measured_tasks_sit(tmp_path: Path) -> None:
    """D-1036: 45 for build, docs and audit, 50 for harness, 65 for cleanup; each limit itself passes."""
    assert (vt.MAX_CONTEXT_PCT, vt.MAX_HARNESS_CONTEXT_PCT, vt.MAX_CLEANUP_CONTEXT_PCT) == (45, 50, 65)
    tasks = valid_tasks()
    build, harness = task(tasks, "T001"), task(tasks, "T004")
    build["budget"]["context_pct"], harness["budget"]["context_pct"] = 45, 50
    assert not [e for e in errors_for(tmp_path, tasks) if "context_pct" in e]
    build["type"] = "cleanup"
    build["budget"]["context_pct"] = 65
    assert not [e for e in errors_for(tmp_path, tasks) if "context_pct" in e]
    build["budget"]["context_pct"] = 66
    assert_error(errors_for(tmp_path, tasks), "budget context_pct 66 over the limit of 65")
    build["type"] = "docs"
    assert_error(errors_for(tmp_path, tasks), "budget context_pct 66 over the limit of 45")


def test_finished_twin_harness_tasks_keep_their_measured_budget() -> None:
    """D-1036: only T005 to T007, already done, may store up to 75%; any other harness task stops at 50."""
    assert vt.CONTEXT_LIMIT_HISTORY == {"T005", "T006", "T007"}
    for tid in ("T005", "T006", "T007"):
        assert vt.context_limit(make_task(tid, "twin", type="harness")) == vt.MAX_HISTORY_CONTEXT_PCT == 75
    assert vt.context_limit(make_task("T074", "twin", type="harness")) == 50


def test_v1_build_and_harness_need_tests_or_acceptance(tmp_path: Path) -> None:
    tasks = valid_tasks()
    del task(tasks, "T004")["acceptance"]
    assert_error(errors_for(tmp_path, tasks), "T004 (fake-sandbox): a harness task with no tests needs an 'acceptance' line")
    task(tasks, "T004")["acceptance"] = 7
    assert_error(errors_for(tmp_path, tasks), "'acceptance' must be str")


def test_v5_goal_must_not_repeat_the_title(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["goal"] = task(tasks, "T001")["title"]
    assert_error(errors_for(tmp_path, tasks), "goal repeats the title")


def test_v6_mutant_rules_owner_needs_opus_reviewer(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["owns"] = ["app/quotes/src/rules.rs", "tests/unit/fake/test_rules.py"]
    assert_error(errors_for(tmp_path, tasks), "owns app/quotes/src/rules.rs, a cargo-mutants target")
    task(tasks, "T001")["reviewer_model"] = OPUS
    assert not [e for e in errors_for(tmp_path, tasks) if "cargo-mutants" in e]


def test_v7_task_reads_the_spec_file_of_each_requirement(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001")["reads"] = []
    assert_error(errors_for(tmp_path, tasks), "names TST-001 but does not read docs/spec/area.md")


MUTANT_DESIGN = (
    "## 4. Deliberate-break tests\n\n| Rule | Named mutants | Killed by |\n|---|---|---|\n"
    "| **Fake rule** (fee) | flip the edge; drop the check (killed by TD-TST-02, owned by T001) | TD-TST-01 |\n"
    "## 5. Journeys\n"
)


def mutant_repo(tmp_path: Path, deps: list[str]) -> list[str]:
    tasks = valid_tasks()
    mutants = make_task(
        "T010", "fake-mutants", type="harness", deps=deps, model=OPUS, reviewer_model=OPUS,
        review_areas=["harness"], owns=["tests/mutation/mutants.toml"], cites=["src.md:3"],
        title="Named mutants: the fake rule", acceptance="Each mutant is killed.",
    )
    tasks.insert(1, mutants)
    task(tasks, "T002")["deps"] = ["T001", "T010"]
    root = write_repo(tmp_path, tasks)
    design = (root / "docs" / "test-design.md").read_text(encoding="utf-8")
    design += "| TD-TST-02 | B | U | TST-002 | fake |\n" + MUTANT_DESIGN
    (root / "docs" / "test-design.md").write_text(design, encoding="utf-8")
    task(tasks, "T001")["tests"].append("TD-TST-02")
    (root / "kit" / "tasks.json").write_text(
        json.dumps({**json.loads((root / "kit" / "tasks.json").read_text()), "tasks": tasks}), encoding="utf-8"
    )
    return list(vt.run(root).errors)


def test_v3_killed_by_tests_must_be_owned_by_an_ancestor(tmp_path: Path) -> None:
    errors = mutant_repo(tmp_path, [])
    assert_error(errors, "TD-TST-01 kills a named mutant of T010 but no ancestor of T010 owns it")
    assert_error(errors, "TD-TST-02 kills a named mutant of T001 but no ancestor of T001 owns it")


def test_v3_passes_when_the_killer_comes_first(tmp_path: Path) -> None:
    errors = mutant_repo(tmp_path, ["T001"])
    assert not [e for e in errors if "kills a named mutant of T010" in e]
    assert vt.expand_tds("TD-CON-01 to TD-CON-03, TD-JOB-05") == ["TD-CON-01", "TD-CON-02", "TD-CON-03", "TD-JOB-05"]


def test_v4_coverage_gate_precedes_money_privacy_and_licensing_code(tmp_path: Path) -> None:
    tasks = valid_tasks()
    task(tasks, "T001").update(
        owns=["app/payments/src/rules.rs", "tests/unit/fake/test_rules.py"], model=OPUS,
        reviewer_model=OPUS, review_areas=["money"],
    )
    assert_error(errors_for(tmp_path, tasks), "no harness task owning tests/coverage_floor.json comes first")
    floor = make_task(
        "T010", "fake-coverage", type="harness", model=OPUS, reviewer_model=OPUS, review_areas=["harness"],
        owns=["tests/coverage_floor.json"], cites=["src.md:3"], acceptance="The floor file exists.",
    )
    tasks.insert(0, floor)
    task(tasks, "T001")["deps"] = ["T010"]
    assert not [e for e in errors_for(tmp_path, tasks) if "coverage_floor" in e]


def test_v4_twin_gate_and_harness_must_exist_and_come_first(tmp_path: Path) -> None:
    tasks = valid_tasks()
    harness = task(tasks, "T004")
    harness["owns"] = ["stand_ins/fake/", "app/adapters/src/twilio_sms.rs", "twin/app.py"]
    errors = errors_for(tmp_path, tasks)
    assert_error(errors, "changes twin/ before the task owning twin/gate.py")
    assert_error(errors, "no task owns .github/workflows/twin.yml")
    assert_error(errors, "no harness task owns .github/workflows/a11y.yml and tests/e2e/playwright.config.ts")
    harness["owns"] = harness["owns"] + ["twin/gate.py"]
    assert not [e for e in errors_for(tmp_path, tasks) if "changes twin/" in e]


def test_v9_migrations_rise_in_serial_order(tmp_path: Path) -> None:
    tasks = valid_tasks()
    first = make_task(
        "T010", "fake-early", owns=["migrations/0050_fake_early.sql"], requirements=["TST-001"],
        reads=["docs/spec/area.md"],
    )
    tasks.insert(0, first)
    task(tasks, "T001")["owns"] = ["app/fake/rules.py", "tests/unit/fake/test_rules.py", "migrations/0040_fake_late.sql"]
    task(tasks, "T001")["deps"] = ["T010"]
    task(tasks, "T001")["budget"] = {"files": 3, "lines": 200, "context_pct": 10}
    assert_error(errors_for(tmp_path, tasks), "migration migrations/0040_fake_late.sql is numbered at or below 0050 (T010)")
    task(tasks, "T001")["owns"][2] = "migrations/0060_fake_late.sql"
    assert not [e for e in errors_for(tmp_path, tasks) if "migration" in e]


def test_v11_a_rows_case_needs_a_builder_that_runs_first_or_shares_the_row(tmp_path: Path) -> None:
    """A task listing a row whose covered requirement is owned only by later tasks that do not list the row fails (D-491)."""
    tasks = valid_tasks()
    early = make_task(
        "T010",
        "fake-early",
        owns=["app/fake/early.rs", "tests/unit/fake/test_early.py"],
        budget={"files": 2, "lines": 100, "context_pct": 5},
        requirements=["TST-001"],
        tests=["TD-TST-01"],
        reads=["docs/spec/area.md"],
    )
    task(tasks, "T001")["deps"] = ["T010"]
    task(tasks, "T001")["requirements"] = ["TST-002"]
    task(tasks, "T001")["tests"] = ["J1", "J2", "J3", "J4"]
    tasks.insert(0, early)
    root = write_repo(tmp_path, tasks)
    errors = list(vt.run(root).errors)
    assert_error(errors, "TD-TST-01 covers TST-002, owned only by ['T001'], which run after this task and do not list the row")
    task(tasks, "T001")["tests"] = ["TD-TST-01", "J1", "J2", "J3", "J4"]
    root = write_repo(tmp_path, tasks)
    assert not [e for e in vt.run(root).errors if "V11" in e]


def test_v12_a_file_under_a_crate_needs_the_manifest_first(tmp_path: Path) -> None:
    """A task owning `app/<crate>/<file>` before the crate's `Cargo.toml` owner runs fails (D-493)."""
    tasks = valid_tasks()
    early = make_task(
        "T010",
        "fake-early",
        owns=["app/adapters/data/schema.json", "tests/unit/fake/test_schema.py"],
        budget={"files": 2, "lines": 100, "context_pct": 5},
        model=OPUS,
        reviewer_model=OPUS,
        review_areas=["security"],
        requirements=["TST-001"],
        tests=["TD-TST-01"],
        reads=["docs/spec/area.md"],
    )
    task(tasks, "T001")["deps"] = ["T010"]
    tasks.insert(0, early)
    root = write_repo(tmp_path, tasks)
    errors = list(vt.run(root).errors)
    assert_error(errors, "owns app/adapters/data/schema.json before ['T004'] create app/adapters/Cargo.toml")
    early["deps"] = ["T004"]
    task(tasks, "T001")["deps"] = []
    root = write_repo(tmp_path, tasks)
    assert not [e for e in vt.run(root).errors if "V12" in e]


def test_four_digit_decision_reference_resolves(tmp_path: Path) -> None:
    """D-930: a task may name D-1000 once the log has it; D-99 is never a decision id."""
    assert vt.DECISION_ID.match("D-1000") and not vt.DECISION_ID.match("D-99")
    tasks = valid_tasks()
    task(tasks, "T004")["decisions"] = ["D-1000"]
    root = write_repo(tmp_path, tasks)
    with (root / "docs" / "decisions.md").open("a", encoding="utf-8") as f:
        f.write("- **D-1000** Four digits.\n")
    assert not [e for e in vt.run(root).errors if "D-1000" in e]
