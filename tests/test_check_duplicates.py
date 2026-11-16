"""Tests for scripts/check_duplicates.py (the stage cleanups' duplicate-function scan, D-3870)."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import check_duplicates as cd  # noqa: E402

FOUR = """fn {name}(a: u32) -> u32 {{
    let b = a + 1;
    let c = b * 2;
    let d = c - 3;
    d / 4
}}
"""

THREE = """fn {name}(a: u32) -> u32 {{
    let b = a + 1;
    let c = b * 2;
    c - 3
}}
"""


def write(root: Path, rel: str, text: str) -> None:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def places(root: Path) -> list[list[tuple[str, int, str]]]:
    return [sorted(group) for group in cd.groups(root)]


def test_four_line_duplicate_is_found(tmp_path: Path) -> None:
    write(tmp_path, "app/one/src/a.rs", "//! A.\n\n" + FOUR.format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", FOUR.format(name="beta"))
    assert places(tmp_path) == [[("app/one/src/a.rs", 3, "alpha"), ("app/two/src/b.rs", 1, "beta")]]


def test_three_line_duplicate_is_ignored(tmp_path: Path) -> None:
    write(tmp_path, "app/one/src/a.rs", THREE.format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", THREE.format(name="beta"))
    assert places(tmp_path) == []


def test_comments_and_whitespace_are_ignored(tmp_path: Path) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.format(name="alpha"))
    noisy = """fn beta(a: u32) -> u32 {
        // a comment line that is not counted
        let b = a+1;   /* inline */
        let  c =  b * 2;

        let d = c
            - 3;
        d / 4
    }
    """
    write(tmp_path, "app/two/src/b.rs", noisy)
    assert [[name for _, _, name in group] for group in places(tmp_path)] == [["alpha", "beta"]]


def test_comment_lines_do_not_count_toward_four(tmp_path: Path) -> None:
    body = THREE.replace("let b", "// note\n    let b")
    write(tmp_path, "app/one/src/a.rs", body.format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", body.format(name="beta"))
    assert places(tmp_path) == []


def test_string_contents_differ(tmp_path: Path) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.replace("d / 4", 'd.max("x".len() as u32)').format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", FOUR.replace("d / 4", 'd.max("y".len() as u32)').format(name="beta"))
    assert places(tmp_path) == []


@pytest.mark.parametrize(
    "rel",
    ["app/one/src/tests/b.rs", "app/one/tests/b.rs", "app/one/target/b.rs"],
)
def test_test_directories_are_ignored(tmp_path: Path, rel: str) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.format(name="alpha"))
    write(tmp_path, rel, FOUR.format(name="beta"))
    assert places(tmp_path) == []


@pytest.mark.parametrize(
    "wrap",
    [
        "#[cfg(test)]\nmod tests {{\n    use super::*;\n" + FOUR + "}}\n",
        "#[test]\n" + FOUR,
        "#[tokio::test(flavor = \"multi_thread\")]\nasync " + FOUR,
        "#![cfg(test)]\n" + FOUR,
    ],
)
def test_test_code_is_ignored(tmp_path: Path, wrap: str) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", wrap.format(name="beta"))
    assert places(tmp_path) == []


@pytest.mark.parametrize(
    "item",
    [
        "#[cfg(test)]\nconst X: usize = 1 << 4;\n",
        "#[cfg(test)]\nconst Y: bool = 1 < 2;\n",
        "#[cfg(test)]\nfn helper<T: Into<u8>>(t: T) -> Vec<u8> {\n    vec![t.into()]\n}\n",
        "#[cfg(test)]\nconst Z: [u8; 2] = [0; 2];\n",
        "#[cfg(test)]\n#[path = \"x_tests.rs\"]\nmod tests;\n",
    ],
)
def test_test_item_before_production_duplicate_keeps_it(tmp_path: Path, item: str) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", item + FOUR.format(name="beta"))
    assert [[name for _, _, name in group] for group in places(tmp_path)] == [["alpha", "beta"]]


def test_kept_group_is_printed_as_kept(tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.format(name="alpha"))
    write(tmp_path, "app/two/src/b.rs", FOUR.format(name="beta"))
    keep = (frozenset({"app/one/src/a.rs alpha", "app/two/src/b.rs beta"}), "only look alike"),
    monkeypatch.setattr(cd, "KEEP", keep)
    assert cd.main(["--root", str(tmp_path), "--strict"]) == 0
    out = capsys.readouterr().out
    assert "kept (only look alike): app/one/src/a.rs:1 alpha, app/two/src/b.rs:1 beta" in out
    assert "duplicates: 1 group(s), 1 kept, 0 finding(s)" in out


def test_strict_exit_codes(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    write(tmp_path, "app/one/src/a.rs", FOUR.format(name="alpha"))
    assert cd.main(["--root", str(tmp_path), "--strict"]) == 0
    write(tmp_path, "app/two/src/b.rs", FOUR.format(name="beta"))
    assert cd.main(["--root", str(tmp_path)]) == 0
    assert cd.main(["--root", str(tmp_path), "--strict"]) == 1
    out = capsys.readouterr().out
    assert "duplicate: app/one/src/a.rs:1 alpha, app/two/src/b.rs:1 beta" in out
    assert "duplicates: 1 group(s), 0 kept, 1 finding(s)" in out


def test_no_app_directory_passes(tmp_path: Path) -> None:
    assert cd.main(["--root", str(tmp_path), "--strict"]) == 0


def test_real_repository_runs_report_only() -> None:
    assert cd.main(["--root", str(REPO_ROOT)]) == 0
