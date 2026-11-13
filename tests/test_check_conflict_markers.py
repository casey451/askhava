"""Tests for scripts/check_conflict_markers.py (no conflict markers in tracked files, D-3307)."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_conflict_markers as cm  # noqa: E402

OPEN = "<" * 7
MID = "=" * 7
CLOSE = ">" * 7


def repo(tmp_path: Path, files: dict[str, bytes]) -> Path:
    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
    for name, data in files.items():
        (tmp_path / name).write_bytes(data)
    subprocess.run(["git", "add", "-A"], cwd=tmp_path, check=True)
    return tmp_path


def test_all_three_markers_fail_with_file_and_line(tmp_path: Path) -> None:
    text = f"a\n{OPEN} HEAD\nx\n{MID}\ny\n{CLOSE} branch\n"
    root = repo(tmp_path, {"notes.md": text.encode()})
    assert cm.scan(root) == [f"notes.md:{n}: git conflict marker" for n in (2, 4, 6)]


def test_clean_tree_passes(tmp_path: Path) -> None:
    root = repo(tmp_path, {"a.md": b"Title\n-----\ntext <<< and === inline\n"})
    assert cm.scan(root) == []


def test_binary_file_is_skipped(tmp_path: Path) -> None:
    root = repo(tmp_path, {"b.bin": b"\0\n" + MID.encode() + b"\n"})
    assert cm.scan(root) == []


def test_real_repository_passes() -> None:
    assert cm.scan(REPO_ROOT) == []
