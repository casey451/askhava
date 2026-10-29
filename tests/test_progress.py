"""Tests for scripts/progress.py (progress from verified requirements, docs/governance.md section 6, D-479)."""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import progress as pg  # noqa: E402

SPEC = (
    "Prefix: `FEE`\n\n"
    "- **FEE-001** The fee is $25. Source: `BUILD_BRIEF.md:1`.\n"
    "- **FEE-002** Two tasks share this. Source: `BUILD_BRIEF.md:1`.\n"
    "- **FEE-003** Withdrawn by D-001. Source: `BUILD_BRIEF.md:1`.\n"
)
TEST_DESIGN = (
    "| ID | K | L | Requirements | Case |\n|---|---|---|---|---|\n"
    "| TD-FEE-01 | B | U | FEE-001 | tier edge |\n"
    "| TD-FEE-02 | B | U | FEE-002 | shared |\n"
)
TASKS = {
    "tasks": [
        {"id": "T001", "type": "build", "requirements": ["FEE-001"], "tests": ["TD-FEE-01"]},
        {"id": "T002", "type": "build", "requirements": ["FEE-002"], "tests": ["TD-FEE-02"]},
        {"id": "T003", "type": "build", "requirements": ["FEE-002"], "tests": []},
        {"id": "T127", "type": "gate", "requirements": [], "tests": []},
    ]
}


def repo(tmp_path: Path) -> Path:
    (tmp_path / "docs/spec").mkdir(parents=True)
    (tmp_path / "kit").mkdir()
    (tmp_path / "BUILD_BRIEF.md").write_text("line one\n", encoding="utf-8")
    (tmp_path / "docs/decisions.md").write_text("- **D-001** x\n", encoding="utf-8")
    (tmp_path / "docs/spec/fees.md").write_text(SPEC, encoding="utf-8")
    (tmp_path / "docs/test-design.md").write_text(TEST_DESIGN, encoding="utf-8")
    (tmp_path / "kit/tasks.json").write_text(json.dumps(TASKS), encoding="utf-8")
    return tmp_path


def state(*done: str) -> dict:
    return {"tasks": [{"id": t, "status": "DONE", "merged_at": "2026-11-01T00:00:00Z"} for t in done]}


GREEN = {"conclusion": "success", "ran_at": "2026-11-02T00:00:00Z"}


def by_id(rows):
    return {r.req_id: r for r in rows}


def test_real_repository_computes_and_counts_only_live_requirements() -> None:
    rows = pg.compute(REPO_ROOT, None)
    assert all(not r.verified for r in rows)
    assert all(r.state == "unstarted" for r in rows)
    assert len(rows) >= 636  # the live count grows with every plan change


def test_withdrawn_is_excluded_and_nothing_done_is_unstarted(tmp_path: Path) -> None:
    rows = by_id(pg.compute(repo(tmp_path), None))
    assert set(rows) == {"FEE-001", "FEE-002"}
    assert rows["FEE-001"].state == "unstarted"


def test_verified_needs_tasks_tests_and_nightly(tmp_path: Path) -> None:
    root = repo(tmp_path)
    rows = by_id(pg.compute(root, state("T001"), None))
    assert rows["FEE-001"].state == "partial"  # no nightly yet
    rows = by_id(pg.compute(root, state("T001"), {"conclusion": "failure", "ran_at": "2026-11-02T00:00:00Z"}))
    assert rows["FEE-001"].state == "partial"
    rows = by_id(pg.compute(root, state("T001"), {"conclusion": "success", "ran_at": "2026-10-31T00:00:00Z"}))
    assert rows["FEE-001"].state == "partial"  # nightly older than the merge
    rows = by_id(pg.compute(root, state("T001"), GREEN))
    assert rows["FEE-001"].verified


def test_split_requirement_is_partial_until_every_task_is_done(tmp_path: Path) -> None:
    root = repo(tmp_path)
    rows = by_id(pg.compute(root, state("T002"), GREEN))
    assert rows["FEE-002"].state == "partial"
    rows = by_id(pg.compute(root, state("T002", "T003"), GREEN))
    assert rows["FEE-002"].verified


def test_summary_and_markdown(tmp_path: Path) -> None:
    root = repo(tmp_path)
    graph = TASKS["tasks"]
    st = state("T001")
    rows = pg.compute(root, st, GREEN)
    summary = pg.summarize(rows, graph, {t["id"]: t for t in st["tasks"]})
    assert summary["requirements_total"] == 2 and summary["requirements_verified"] == 1
    assert summary["tasks_total"] == 3 and summary["tasks_done"] == 1  # gates are not counted
    text = pg.markdown(summary)
    assert "Requirements verified:** 1 of 2" in text and "| fees | 1 | 0 | 1 | 2 |" in text


def test_cli_runs_on_the_real_repository(capsys) -> None:
    assert pg.main(["--root", str(REPO_ROOT), "--state", "/nonexistent/state.json"]) == 0
    out = capsys.readouterr().out
    assert "0 verified" in out and "throughput only" in out
