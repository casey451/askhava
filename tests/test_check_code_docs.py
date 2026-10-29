"""Tests for scripts/check_code_docs.py (the real repository plus planted gaps, D-473)."""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_code_docs as ccd  # noqa: E402

GOOD_DOC = "\n\n".join(f"{h}\n\ntext" for h in ccd.HEADINGS) + "\n"


def fake_repo(tmp_path: Path) -> Path:
    (tmp_path / "docs/spec").mkdir(parents=True)
    (tmp_path / "docs/spec/fees.md").write_text("- **FEE-001** Fee tiers.\n- **FEE-039** Ongoing.\n", encoding="utf-8")
    (tmp_path / "docs/architecture.md").write_text("| `app/payments` | fees |\n", encoding="utf-8")
    crate = tmp_path / "app/payments"
    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
    (crate / "CLAUDE.md").write_text(GOOD_DOC, encoding="utf-8")
    (crate / "src/lib.rs").write_text("//! Fees (FEE-001, FEE-039).\npub fn x() {}\n", encoding="utf-8")
    return tmp_path


def test_real_repository_passes() -> None:
    assert ccd.run(REPO_ROOT) == []


def test_good_fake_repo_passes(tmp_path: Path) -> None:
    assert ccd.run(fake_repo(tmp_path)) == []


def test_missing_crate_doc_fails(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "app/payments/CLAUDE.md").unlink()
    assert any("CLAUDE.md: missing" in p for p in ccd.run(root))


def test_headings_out_of_order_fail(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "app/payments/CLAUDE.md").write_text("## Never break\n\n## Purpose\n", encoding="utf-8")
    assert any("headings must be" in p for p in ccd.run(root))


def test_long_crate_doc_fails(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "app/payments/CLAUDE.md").write_text(GOOD_DOC + "x\n" * 130, encoding="utf-8")
    assert any("limit 120" in p for p in ccd.run(root))


def test_crate_not_in_architecture_fails(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "docs/architecture.md").write_text("nothing\n", encoding="utf-8")
    assert any("not named in docs/architecture.md" in p for p in ccd.run(root))


def test_rust_file_without_module_comment_fails(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "app/payments/src/tiers.rs").write_text("pub fn y() {}\n", encoding="utf-8")
    assert any("tiers.rs: must start with a //!" in p for p in ccd.run(root))


def test_migration_without_comment_fails(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "migrations").mkdir()
    (root / "migrations/0001_x.sql").write_text("CREATE TABLE t ();\n", encoding="utf-8")
    assert any("0001_x.sql" in p for p in ccd.run(root))


def test_unknown_requirement_id_fails(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "app/payments/src/lib.rs").write_text("//! Fees (FEE-999).\n", encoding="utf-8")
    assert any("FEE-999" in p for p in ccd.run(root))


def test_test_design_and_decision_ids_are_not_requirements(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "app/payments/src/lib.rs").write_text("//! TD-FEE-01 and D-473 (FEE-001).\n", encoding="utf-8")
    assert ccd.run(root) == []


def test_finished_task_must_name_its_requirements(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "kit").mkdir()
    task = {"id": "T058", "type": "build", "owns": ["app/payments/src/lib.rs"], "requirements": ["FEE-001", "FEE-043"]}
    (root / "kit/tasks.json").write_text(json.dumps({"tasks": [task]}), encoding="utf-8")
    assert ccd.run(root) == ["T058: FEE-043 is named in none of its owned files"]


def test_unfinished_task_is_not_traced_yet(tmp_path: Path) -> None:
    root = fake_repo(tmp_path)
    (root / "kit").mkdir()
    task = {"id": "T059", "type": "build", "owns": ["app/payments/src/stripe.rs"], "requirements": ["FEE-001"]}
    (root / "kit/tasks.json").write_text(json.dumps({"tasks": [task]}), encoding="utf-8")
    assert ccd.run(root) == []


def test_main_reports_problems(tmp_path: Path, capsys) -> None:  # type: ignore[no-untyped-def]
    root = fake_repo(tmp_path)
    (root / "app/payments/CLAUDE.md").unlink()
    assert ccd.main(["--root", str(root)]) == 1
    assert "1 problem(s)" in capsys.readouterr().out
