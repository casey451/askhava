"""Tests for scripts/check_crates.py (approved crates only, D-479)."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_crates as ck  # noqa: E402


def repo(tmp_path: Path) -> Path:
    (tmp_path / "kit").mkdir()
    shutil.copy(REPO_ROOT / "kit/allowed_crates.txt", tmp_path / "kit/allowed_crates.txt")
    return tmp_path


def test_real_repository_passes() -> None:
    assert ck.run(REPO_ROOT) == []


def test_no_manifest_passes(tmp_path: Path) -> None:
    assert ck.run(repo(tmp_path)) == []


def test_listed_crates_pass_and_unlisted_fail(tmp_path: Path) -> None:
    root = repo(tmp_path)
    (root / "Cargo.toml").write_text('[workspace]\nmembers = ["app/*"]\n[workspace.dependencies]\naxum = "=0.8.9"\nserde = "1"\n', encoding="utf-8")
    (root / "app/jobs").mkdir(parents=True)
    (root / "app/jobs/Cargo.toml").write_text('[package]\nname = "jobs"\n[dependencies]\naxum.workspace = true\nleftpad = "1"\n[dev-dependencies]\nproptest = "1"\n', encoding="utf-8")
    problems = ck.run(root)
    assert len(problems) == 1 and "leftpad" in problems[0]
