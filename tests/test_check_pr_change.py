"""Tests for scripts/check_pr_change.py (per-change governance rules, docs/governance.md, D-479)."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_plan_lock as pl  # noqa: E402
import check_pr_change as cc  # noqa: E402

TASKS = {
    "tasks": [
        {"id": "T001", "type": "docs", "owns": ["docs/sources.md"], "deps": []},
        {"id": "T009", "type": "harness", "owns": ["runner/x.py"], "deps": []},
    ],
    "protected_paths": ["runner/"],
    "shared_paths": ["docs/decisions.md", "kit/plan.lock"],
}
DECISIONS = "# Decisions\n\n- **D-001** Question: a. Choice: b.\n"
SPEC = "- **FEE-001** The fee is $25. Source: BUILD_BRIEF.md:1\n- **FEE-002** Old rule. Source: BUILD_BRIEF.md:1\n"


def git(root: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True, text=True).stdout


def repo(tmp_path: Path) -> Path:
    root = tmp_path
    (root / "docs/spec").mkdir(parents=True)
    (root / "kit").mkdir()
    (root / "runner").mkdir()
    (root / "BUILD_BRIEF.md").write_text("brief\n", encoding="utf-8")
    (root / "docs/sources.md").write_text("sources\n", encoding="utf-8")
    (root / "docs/decisions.md").write_text(DECISIONS, encoding="utf-8")
    (root / "docs/spec/fees.md").write_text(SPEC, encoding="utf-8")
    (root / "docs/architecture.md").write_text("## 3. Modules, what each owns, and its tables\nt\n## 7. Free\nf\n", encoding="utf-8")
    (root / "kit/tasks.json").write_text(json.dumps(TASKS), encoding="utf-8")
    (root / "runner/x.py").write_text("x = 1\n", encoding="utf-8")
    pl.write_lock(root)
    git(root, "init", "-q", "-b", "main")
    git(root, "config", "user.email", "t@example.com")
    git(root, "config", "user.name", "t")
    git(root, "add", "-A")
    git(root, "commit", "-qm", "base")
    git(root, "checkout", "-qb", "work")
    return root


def commit(root: Path, trailer: str = "") -> None:
    git(root, "add", "-A")
    git(root, "commit", "-qm", "change" + (f"\n\nPlan-Change: {trailer}" if trailer else ""))


def run(root: Path, branch: str = "claude/plan") -> int:
    return cc.main(["--root", str(root), "--base", "main", "--branch", branch])


def add_decision(root: Path, text: str = "- **D-002** Question: q. Choice: c.\n") -> None:
    with (root / "docs/decisions.md").open("a", encoding="utf-8") as f:
        f.write(text)


def test_no_change_passes(tmp_path: Path) -> None:
    assert run(repo(tmp_path)) == 0


def test_task_branch_outside_owned_paths_fails(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "docs/sources.md").write_text("changed\n", encoding="utf-8")
    (root / "runner/x.py").write_text("x = 2\n", encoding="utf-8")
    commit(root)
    assert run(root, "claude/task/T001-fix") == 1
    err = capsys.readouterr().err
    assert "runner/x.py: protected path" in err
    assert "docs/sources.md" not in err.split("plan lock")[0] or "not owned" not in err


def test_task_branch_inside_owned_paths_passes_with_lock_and_decision(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "docs/sources.md").write_text("changed\n", encoding="utf-8")
    pl.write_lock(root)  # docs/sources.md is a locked file: the lock and a decision are still required
    add_decision(root)
    commit(root)
    assert run(root, "claude/task/T001-fix") == 0


def test_locked_file_change_needs_lock_and_decision(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "BUILD_BRIEF.md").write_text("brief changed\n", encoding="utf-8")
    commit(root)
    assert run(root) == 1
    err = capsys.readouterr().err
    assert "was not rewritten" in err and "no new docs/decisions.md entry" in err
    pl.write_lock(root)
    add_decision(root)
    commit(root)
    assert run(root) == 1  # a plan change must name its decision in a trailer
    assert "Plan-Change" in capsys.readouterr().err
    commit_empty(root, "D-002")
    assert run(root) == 0


def commit_empty(root: Path, trailer: str) -> None:
    git(root, "commit", "-q", "--allow-empty", "-m", f"plan change\n\nPlan-Change: {trailer}")


def test_locked_architecture_section_change_needs_decision(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "docs/architecture.md").write_text("## 3. Modules, what each owns, and its tables\nnew\n## 7. Free\nf\n", encoding="utf-8")
    pl.write_lock(root)
    commit(root)
    assert run(root) == 1
    assert "no new docs/decisions.md entry" in capsys.readouterr().err


def test_unlocked_architecture_section_change_is_free(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "docs/architecture.md").write_text("## 3. Modules, what each owns, and its tables\nt\n## 7. Free\nchanged\n", encoding="utf-8")
    commit(root)
    assert run(root) == 0


def test_decisions_log_is_append_only(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "docs/decisions.md").write_text("# Decisions\n\n- **D-001** Question: a. Choice: edited.\n", encoding="utf-8")
    commit(root)
    assert run(root) == 1
    assert "append-only" in capsys.readouterr().err


def test_requirement_change_needs_decision_naming_it(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "docs/spec/fees.md").write_text(SPEC.replace("$25", "$30"), encoding="utf-8")
    pl.write_lock(root)
    add_decision(root, "- **D-002** Question: fee. Choice: something else.\n")
    commit(root)
    assert run(root) == 1
    assert "FEE-001 changed in docs/spec but no new decision entry names it" in capsys.readouterr().err
    add_decision(root, "- **D-003** Question: FEE-001. Choice: $30.\n")
    commit(root, "D-003")
    assert run(root) == 0
    commit_empty(root, "D-099")
    assert run(root) == 1  # the trailer must name a new entry


def test_removed_requirement_id_fails(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "docs/spec/fees.md").write_text(SPEC.splitlines()[0] + "\n", encoding="utf-8")
    pl.write_lock(root)
    add_decision(root, "- **D-002** Question: FEE-002. Choice: withdrawn.\n")
    commit(root)
    assert run(root) == 1
    assert "FEE-002 was removed" in capsys.readouterr().err


def test_stale_lock_fails(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "BUILD_BRIEF.md").write_text("brief changed\n", encoding="utf-8")
    add_decision(root)
    commit(root)
    assert run(root) == 1
    assert "plan lock" in capsys.readouterr().err


def test_decisions_entries_keep_their_order(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    add_decision(root, "- **D-002** Question: q. Choice: c.\n")
    commit(root)
    assert run(root) == 0
    text = (root / "docs/decisions.md").read_text(encoding="utf-8")
    swapped = text.replace("- **D-001** Question: a. Choice: b.\n", "").rstrip("\n") + "\n- **D-001** Question: a. Choice: b.\n"
    (root / "docs/decisions.md").write_text(swapped, encoding="utf-8")
    commit(root)
    assert run(root) == 1
    assert "append-only" in capsys.readouterr().err


def test_renamed_protected_file_shows_as_deleted(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    git(root, "mv", "runner/x.py", "docs/x.py")
    commit(root)
    assert run(root, "claude/task/T001-fix") == 1
    err = capsys.readouterr().err
    assert "runner/x.py: protected path" in err


def test_lock_rewrite_by_non_harness_task_without_locked_ownership_fails(tmp_path: Path) -> None:
    root = repo(tmp_path)
    errs = cc.lock_rewrite_check(root, "claude/task/T001-fix", ["kit/plan.lock", "docs/decisions.md"])
    assert errs and "owns no locked file" in errs[0]
    assert cc.lock_rewrite_check(root, "claude/task/T001-fix", ["kit/plan.lock", "docs/sources.md"]) == []
    assert cc.lock_rewrite_check(root, "claude/task/T009-skel", ["kit/plan.lock", "BUILD_BRIEF.md"]) == []


def test_decision_may_name_requirements_as_a_range() -> None:
    assert cc.ids_named("new SEC-035 to SEC-037 and PRIV-024") == {"SEC-035", "SEC-036", "SEC-037", "PRIV-024"}
    assert "SEC-036" in cc.ids_named("SEC-035 to 037")


def test_task_branch_cannot_grant_itself_paths(tmp_path: Path, capsys) -> None:
    """D-484: ownership is read from the base task graph, never from the branch's own copy."""
    root = repo(tmp_path)
    graph = json.loads((root / "kit/tasks.json").read_text(encoding="utf-8"))
    graph["tasks"][0]["owns"].append("runner/x.py")
    graph["tasks"][0]["type"] = "harness"
    (root / "kit/tasks.json").write_text(json.dumps(graph), encoding="utf-8")
    (root / "runner/x.py").write_text("x = 2\n", encoding="utf-8")
    pl.write_lock(root)
    add_decision(root)
    commit(root)
    assert run(root, "claude/task/T001-fix") == 1
    assert "runner/x.py: protected path" in capsys.readouterr().err


def test_deleting_a_locked_file_needs_a_decision(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "BUILD_BRIEF.md").unlink()
    pl.write_lock(root)
    commit(root)
    assert run(root) == 1
    assert "no new docs/decisions.md entry" in capsys.readouterr().err


def test_trailer_must_name_a_new_entry_exactly(tmp_path: Path, capsys) -> None:
    root = repo(tmp_path)
    (root / "BUILD_BRIEF.md").write_text("brief changed\n", encoding="utf-8")
    pl.write_lock(root)
    add_decision(root, "- **D-002** Question: like D-001 before it. Choice: c.\n")
    commit(root, "D-001")
    assert run(root) == 1
    assert "not a new entry" in capsys.readouterr().err


def test_a_d_number_used_twice_fails_with_a_clear_message(tmp_path: Path, capsys) -> None:
    """D-530: two pieces of work that took one number; the change that lands second fails."""
    root = repo(tmp_path)
    add_decision(root, "- **D-002** Question: one lane. Choice: c.\n- **D-002** Question: other lane. Choice: d.\n")
    commit(root)
    assert run(root) == 1
    err = capsys.readouterr().err
    assert "docs/decisions.md has D-002 on 2 entries; renumber the newer one" in err
    assert cc.decisions_unique(DECISIONS + "- **D-001** Question: again. Choice: e.\n") != []


def test_gaps_in_decision_numbers_pass(tmp_path: Path) -> None:
    """Reserved blocks leave unused numbers behind; a gap is never an error (D-530)."""
    root = repo(tmp_path)
    add_decision(root, "- **D-004** Question: q. Choice: c.\n- **D-009** Question: r. Choice: d.\n")
    commit(root)
    assert run(root) == 0
    assert cc.decisions_unique(None) == []


def test_two_branches_appending_decisions_merge_cleanly_locally(tmp_path: Path) -> None:
    """The repository's .gitattributes (`docs/decisions.md merge=union`) lets a local merge keep both appends."""
    root = repo(tmp_path)
    (root / ".gitattributes").write_text((REPO_ROOT / ".gitattributes").read_text(encoding="utf-8"), encoding="utf-8")
    commit(root)
    git(root, "checkout", "-qb", "other")
    add_decision(root, "- **D-007** Question: other. Choice: c.\n")
    commit(root)
    git(root, "checkout", "-q", "work")
    add_decision(root, "- **D-002** Question: work. Choice: c.\n")
    commit(root)
    git(root, "merge", "-q", "--no-edit", "other")
    text = (root / "docs/decisions.md").read_text(encoding="utf-8")
    assert "- **D-002** Question: work." in text and "- **D-007** Question: other." in text and "<<<<<<<" not in text


def test_a_stale_branch_whose_number_main_took_meanwhile_fails(tmp_path: Path, capsys) -> None:
    """D-530: the branch appended D-002 on an old base; main has since merged its own D-002."""
    root = repo(tmp_path)
    add_decision(root, "- **D-002** Question: branch. Choice: c.\n")
    commit(root)
    git(root, "checkout", "-q", "main")
    add_decision(root, "- **D-002** Question: landed first. Choice: d.\n")
    commit(root)
    git(root, "checkout", "-q", "work")
    assert run(root) == 1
    assert "D-002 is new on this branch but main already has a D-002 entry" in capsys.readouterr().err
    (root / "docs/decisions.md").write_text(DECISIONS + "- **D-003** Question: branch. Choice: c.\n", encoding="utf-8")
    commit(root)
    assert run(root) == 0
    assert cc.decisions_not_taken_on_main(DECISIONS, DECISIONS, DECISIONS) == []
