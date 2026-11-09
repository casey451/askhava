"""TD-OPS-32 (T283, D-1867): every file the Rust code compiles in is covered by `Ci`'s Rust path rules.

`scripts/check_include_paths.py` over temporary trees (fake paths only) and over the real repository.
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "check_include_paths.py"
_spec = importlib.util.spec_from_file_location("check_include_paths", SCRIPT)
cip = importlib.util.module_from_spec(_spec)  # type: ignore[arg-type]  # reason: the path exists
_spec.loader.exec_module(cip)  # type: ignore[union-attr]  # reason: a file loader

RULE = r"^(app/|docs/legal/|docs/catalog/|schemas/|\.github/workflows/ci\.yml)"
NARROW = r"^(app/|docs/legal/|\.github/workflows/ci\.yml)"
CI = """jobs:
  checks:
    steps:
      - name: What this change touches
        run: |
          echo "$files" | grep -Eq '{checks}' && rust=true
          echo "$files" | grep -Eq '^(package\\.json)' && npm=true
  coverage:
    steps:
      - name: Does this change touch Rust?
        run: |
          git diff --name-only "$base" HEAD \\
            | grep -Eq '{coverage}' && rust=true
"""


def _tree(tmp_path: Path, source: str, files: tuple[str, ...] = (), coverage: str = RULE) -> Path:
    """A fake repository: one crate app/fake with `source` as src/lib.rs, the rules, and `files` present."""
    (tmp_path / ".github" / "workflows").mkdir(parents=True)
    (tmp_path / ".github" / "workflows" / "ci.yml").write_text(CI.format(checks=RULE, coverage=coverage))
    (tmp_path / "scripts").mkdir()
    (tmp_path / "scripts" / "prehandin.sh").write_text(f"RUST_PATHS='{RULE}'\nNPM_PATHS='^(package\\.json)'\n")
    (tmp_path / "app" / "fake" / "src").mkdir(parents=True)
    (tmp_path / "app" / "fake" / "Cargo.toml").write_text('[package]\nname = "fake"\n')
    (tmp_path / "app" / "fake" / "src" / "lib.rs").write_text(source)
    for name in files:
        (tmp_path / name).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / name).write_text("fake content\n")
    return tmp_path


def _problems(root: Path) -> list[str]:
    return cip.check(root)[0]


@pytest.mark.parametrize("form", ["include_str!", "include_bytes!", "include!"])
def test_td_ops_32_an_included_file_under_a_covered_folder_passes(tmp_path: Path, form: str) -> None:
    root = _tree(tmp_path, f'//! Fake crate.\nconst T: &str = {form}("../../../docs/legal/terms.md");\n',
                 ("docs/legal/terms.md",))
    assert _problems(root) == []


def test_td_ops_32_a_manifest_dir_path_is_found_and_resolved_against_the_crate(tmp_path: Path) -> None:
    source = 'fn f() -> &\'static str {\n    concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas", "/s.json")\n}\n'
    root = _tree(tmp_path, source, ("schemas/s.json",))
    assert _problems(root) == []
    (root / "app" / "fake" / "src" / "lib.rs").write_text(source.replace("schemas", "docs/other"))
    (root / "docs" / "other").mkdir(parents=True)
    (root / "docs" / "other" / "s.json").write_text("{}\n")
    problems = _problems(root)
    assert problems and all(p.startswith("app/fake/src/lib.rs:2: concat!") for p in problems), problems
    assert "docs/other/s.json: not matched by the checks rule" in problems[0]


def test_td_ops_32_a_file_under_an_uncovered_folder_fails_naming_the_line(tmp_path: Path) -> None:
    root = _tree(tmp_path, '//! Fake.\n\nconst X: &str = include_str!("../../../docs/other/x.md");\n',
                 ("docs/other/x.md",))
    problems = _problems(root)
    assert len(problems) == 3, problems
    assert all(p.startswith("app/fake/src/lib.rs:3: include_str! -> docs/other/x.md: not matched") for p in problems)
    assert subprocess.run([sys.executable, str(SCRIPT), "--root", str(root)], capture_output=True, text=True,
                          check=False).returncode == 1


def test_td_ops_32_a_dot_dot_path_leaving_a_covered_folder_fails(tmp_path: Path) -> None:
    root = _tree(tmp_path, 'const X: &str = include_str!("../../../docs/legal/../other/x.md");\n',
                 ("docs/legal/terms.md", "docs/other/x.md"))
    problems = _problems(root)
    assert problems and "-> docs/other/x.md: not matched by the checks rule" in problems[0], problems


def test_td_ops_32_covered_by_checks_but_not_by_coverage_fails(tmp_path: Path) -> None:
    root = _tree(tmp_path, 'const X: &str = include_str!("../../../docs/catalog/c.yaml");\n',
                 ("docs/catalog/c.yaml",), coverage=NARROW)
    problems = _problems(root)
    assert problems == ["app/fake/src/lib.rs:1: include_str! -> docs/catalog/c.yaml: "
                        "not matched by the coverage rule (.github/workflows/ci.yml)"]


def test_td_ops_32_a_missing_file_or_one_outside_the_repository_fails(tmp_path: Path) -> None:
    root = _tree(tmp_path, 'const X: &str = include_str!("../../../docs/legal/gone.md");\n'
                           'const Y: &str = include_str!("../../../../outside.md");\n')
    problems = _problems(root)
    assert problems[0] == "app/fake/src/lib.rs:1: include_str! -> docs/legal/gone.md: no such file"
    assert problems[1].startswith("app/fake/src/lib.rs:2: include_str! -> ") and "outside the repository" in problems[1]


def test_td_ops_32_a_path_it_cannot_resolve_fails_naming_the_line(tmp_path: Path) -> None:
    source = ('const X: &str = include_str!(concat!("../", "x.md"));\n'
              'fn f(p: &str) -> String { format!("{}{p}", concat!(env!("CARGO_MANIFEST_DIR"), P)) }\n')
    problems = _problems(_tree(tmp_path, source))
    assert problems == [
        "app/fake/src/lib.rs:1: include_str! with a path that cannot be resolved (not a string literal)",
        "app/fake/src/lib.rs:2: concat!(env!(\"CARGO_MANIFEST_DIR\"), ...) with a path that cannot be resolved "
        "(not a string literal)",
    ]


def test_td_ops_32_macros_named_in_comments_or_strings_are_not_calls(tmp_path: Path) -> None:
    source = ('//! Compiled in with `include_str!("../nothing.md")`.\n'
              '/* include!("../gone.rs") */\n'
              'const S: &str = "include_bytes!(\\"../gone.bin\\")";\n'
              "const C: char = '\"';\n"
              'const R: &str = r#"concat!(env!("CARGO_MANIFEST_DIR"), "/gone")"#;\n'
              'const B: &[u8] = br#"a " b"#;\n'
              'const P: &[u8] = br"\\";\n'
              'const X: &str = include_str!("../../../docs/other/x.md");\n')
    problems = _problems(_tree(tmp_path, source, ("docs/other/x.md",)))
    assert len(problems) == 3, problems
    assert all(p.startswith("app/fake/src/lib.rs:8: include_str! -> docs/other/x.md: not matched") for p in problems)


def test_td_ops_32_the_repository_root_is_a_note_not_a_silent_skip(tmp_path: Path) -> None:
    root = _tree(tmp_path, 'const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");\n')
    problems, notes = cip.check(root)
    assert problems == [] and notes == [
        'app/fake/src/lib.rs:1: concat!(env!("CARGO_MANIFEST_DIR"), ...) names the repository root '
        "(a run-time base, not a file)"]


def test_td_ops_32_a_workflow_without_a_rust_rule_is_red(tmp_path: Path) -> None:
    root = _tree(tmp_path, "")
    no_rule = "jobs:\n  checks:\n    steps: []\n  coverage:\n    steps: []\n"
    (root / ".github" / "workflows" / "ci.yml").write_text(no_rule)
    assert cip.main(["--root", str(root)]) == 1


def test_td_ops_32_the_rules_are_read_from_the_workflow_and_the_gate() -> None:
    rules = cip.read_rules(ROOT)
    assert list(rules) == ["checks rule (.github/workflows/ci.yml)", "coverage rule (.github/workflows/ci.yml)",
                           "RUST_PATHS (scripts/prehandin.sh)"]
    checks, _, gate = rules.values()
    assert gate == checks, "RUST_PATHS equals the Checks rule"
    for path in ("docs/legal/homeowner-terms.md", "docs/catalog/categories.yaml", "schemas/intake_reading.json",
                 "docs/architecture.md"):
        assert all(cip.re.search(rule, path) for rule in rules.values()), path
    for path in ("docs/decisions.md", "docs/other/x.md", "docs/copy/other.md"):
        assert not any(cip.re.search(rule, path) for rule in rules.values()), f"never all of docs/: {path}"


def test_td_ops_32_the_real_repository_passes() -> None:
    done = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True, text=True, check=False)
    assert done.returncode == 0, done.stdout
    problems, _ = cip.check(ROOT)
    assert problems == []
