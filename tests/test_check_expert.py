"""The expert layer check passes on the repo and fails on a broken file."""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

from scripts import check_expert

ROOT = Path(__file__).resolve().parents[1]


def test_repo_expert_layer_is_clean() -> None:
    items, summary = check_expert.run(ROOT)
    assert items == [], "\n".join(items)
    assert "Expert files:" in summary


@pytest.fixture()
def copy(tmp_path: Path) -> Path:
    for rel in ("docs/catalog", "docs/ai_smell.yaml", "scripts"):
        src = ROOT / rel
        dst = tmp_path / rel
        if src.is_dir():
            shutil.copytree(src, dst)
        else:
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(src, dst)
    return tmp_path


def test_cause_word_in_follow_up_fails(copy: Path) -> None:
    path = copy / "docs/catalog/expert/plumbing_repair.yaml"
    text = path.read_text(encoding="utf-8")
    text = text.replace(
        'text_en: "Has the water reached the floor, a wall or a ceiling?"',
        'text_en: "It\'s probably the supply line, has it reached the floor?"',
    )
    path.write_text(text, encoding="utf-8")
    items, _ = check_expert.run(copy)
    assert any("cause word" in item for item in items)


def test_unknown_photo_fails(copy: Path) -> None:
    path = copy / "docs/catalog/expert/plumbing_repair.yaml"
    text = path.read_text(encoding="utf-8").replace("photos_priority: [p1_leak_spot, p3_pipe_material, p4_damage]", "photos_priority: [p9_nothing]")
    path.write_text(text, encoding="utf-8")
    items, _ = check_expert.run(copy)
    assert any("unknown photo 'p9_nothing'" in item for item in items)


def test_missing_category_file_fails(copy: Path) -> None:
    (copy / "docs/catalog/expert/plumbing_repair.yaml").unlink()
    items, _ = check_expert.run(copy)
    assert any("plumbing_repair.yaml: missing" in item for item in items)
