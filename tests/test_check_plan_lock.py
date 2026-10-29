"""Tests for scripts/check_plan_lock.py (the blueprint fingerprint, docs/governance.md, D-479)."""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_plan_lock as pl  # noqa: E402

ARCH = """# Arch
## 1. Shape
one
## 3. Modules, what each owns, and its tables
table
## 4. Dependency rules
rules
## 5. Shared kernel (`core`)
kernel
## 6. Job lifecycle state machine
machine
## 7. Adapters and stand-ins
free text
"""


def repo(tmp_path: Path) -> Path:
    (tmp_path / "docs/spec").mkdir(parents=True)
    (tmp_path / "kit").mkdir()
    (tmp_path / "scripts").mkdir()
    (tmp_path / "BUILD_BRIEF.md").write_text("brief\n", encoding="utf-8")
    (tmp_path / "docs/spec/fees.md").write_text("- **FEE-001** x. Source: a.md:1\n", encoding="utf-8")
    (tmp_path / "docs/architecture.md").write_text(ARCH, encoding="utf-8")
    (tmp_path / "scripts/x.py").write_text("print(1)\n", encoding="utf-8")
    pl.write_lock(tmp_path)
    return tmp_path


def test_real_repository_lock_matches() -> None:
    assert pl.differences(REPO_ROOT) == []


def test_lock_covers_locked_files_and_sections(tmp_path: Path) -> None:
    root = repo(tmp_path)
    items = pl.read_lock(root)
    assert "BUILD_BRIEF.md" in items and "docs/spec/fees.md" in items and "scripts/x.py" in items
    assert "docs/architecture.md#3" in items and "docs/architecture.md#6" in items
    assert "docs/architecture.md#7" not in items


def test_change_to_locked_file_is_detected(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "BUILD_BRIEF.md").write_text("brief changed\n", encoding="utf-8")
    assert pl.differences(root) == ["BUILD_BRIEF.md: changed"]
    assert pl.main(["--root", str(root)]) == 1


def test_change_to_locked_section_only_is_detected(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "docs/architecture.md").write_text(ARCH.replace("rules\n", "new rules\n"), encoding="utf-8")
    assert pl.differences(root) == ["docs/architecture.md#4: changed"]


def test_change_outside_locked_sections_passes(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "docs/architecture.md").write_text(ARCH.replace("free text", "other text"), encoding="utf-8")
    assert pl.differences(root) == []
    assert pl.main(["--root", str(root)]) == 0


def test_new_locked_file_and_missing_file(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "docs/spec/calls.md").write_text("- **CALL-001** y. Source: a.md:1\n", encoding="utf-8")
    (root / "scripts/x.py").unlink()
    diffs = pl.differences(root)
    assert "docs/spec/calls.md: not in the lock (new locked file)" in diffs
    assert "scripts/x.py: in the lock but missing from the tree" in diffs


def test_write_rewrites_lock(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "BUILD_BRIEF.md").write_text("brief changed\n", encoding="utf-8")
    assert pl.main(["--root", str(root), "--write"]) == 0
    assert pl.differences(root) == []
    payload = json.loads((root / "kit/plan.lock").read_text(encoding="utf-8"))
    assert payload["schema"] == 1 and payload["items"]
