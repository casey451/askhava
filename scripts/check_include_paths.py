"""Every file the Rust code compiles in must make `Ci` build and test Rust when it changes (TD-OPS-32, D-1867).

`Ci` runs the Rust, coverage and end-to-end jobs only when the diff matches a path rule: the Checks rule and the
Coverage rule in .github/workflows/ci.yml and their copy `RUST_PATHS` in scripts/prehandin.sh. A file the app
pulls in at compile time (the Terms, the catalog, the AI schema) changes what the binary does, so a change to it
alone must match all three, or it merges with Checks, Coverage and E2e green having run no Rust.

Finds, in app/**/*.rs and tests/{integration,security,recovery,fixtures}/**/*.rs (comments and strings skipped):
- every `include_str!`, `include_bytes!` and `include!` path, resolved against the source file's folder;
- every `concat!(env!("CARGO_MANIFEST_DIR"), ...)` path, resolved against the crate's folder (the nearest
  folder above the source file holding a Cargo.toml).

Fails (exit 1), naming the source line and the resolved file, when a resolved path:
- is not matched by the Checks rule, the Coverage rule and RUST_PATHS (each read from its file, one source);
- does not exist, or lies outside the repository;
- cannot be resolved (an argument that is not a plain string literal, a path built at run time).
A path that resolves to the repository root itself is the workspace base for run-time paths (a working
folder for the stand-ins, `format!("{root}/...")`), not a compiled-in file: it is printed as a note, never
skipped silently (D-1867). A folder is matched as `folder/`.

Usage: python3 scripts/check_include_paths.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

import yaml

RUST_DIRS = ("app", "tests/integration", "tests/security", "tests/recovery", "tests/fixtures")
CI_FILE = ".github/workflows/ci.yml"
GATE_FILE = "scripts/prehandin.sh"
RULE_JOBS = ("checks", "coverage")
RUST_GREP = re.compile(r"grep -Eq '([^']+)'\s*&&\s*rust=true")
GATE_RULE = re.compile(r"^RUST_PATHS='([^']+)'", re.MULTILINE)
LITERAL = r'"([^"\\]*)"'
INCLUDE = re.compile(r"\b(include_str|include_bytes|include)!\s*\(")
INCLUDE_ARG = re.compile(r"\s*" + LITERAL + r"\s*,?\s*\)")
CONCAT_ENV = re.compile(r"\bconcat!\s*\(\s*env!\s*\(")
MANIFEST_ARG = re.compile(r'\s*"CARGO_MANIFEST_DIR"\s*\)\s*')
CONCAT_REST = re.compile(r"((?:,\s*" + LITERAL + r"\s*)*),?\s*\)")
SKIPPED = re.compile(r"//[^\n]*|/\*.*?\*/|\br(#*)\".*?\"\1|\"(?:[^\"\\]|\\.)*\"|'(?:[^'\\\n]|\\[^']*)'", re.DOTALL)


def read_rules(root: Path) -> dict[str, str]:
    """The three Rust path rules by name, read from ci.yml and prehandin.sh; a missing rule raises ValueError."""
    ci = yaml.safe_load((root / CI_FILE).read_text(encoding="utf-8"))
    rules: dict[str, str] = {}
    for job in RULE_JOBS:
        runs = [str(step.get("run", "")) for step in ci["jobs"][job]["steps"]]
        found = [m.group(1) for run in runs for m in RUST_GREP.finditer(run)]
        if len(found) != 1:
            raise ValueError(f"{CI_FILE}: job '{job}' has {len(found)} Rust path rules, expected 1")
        rules[f"{job} rule ({CI_FILE})"] = found[0]
    gate = GATE_RULE.search((root / GATE_FILE).read_text(encoding="utf-8"))
    if gate is None:
        raise ValueError(f"{GATE_FILE}: no RUST_PATHS rule")
    rules[f"RUST_PATHS ({GATE_FILE})"] = gate.group(1)
    return rules


def crate_dir(source: Path, root: Path) -> Path:
    """The nearest folder above `source` holding a Cargo.toml (what CARGO_MANIFEST_DIR names); else the root."""
    for folder in source.parents:
        if (folder / "Cargo.toml").is_file() or folder == root:
            return folder
    return root


def code_only(text: str) -> str:
    """`text` with every comment and string or char literal blanked (newlines kept), so a macro name inside
    one (`"include_str!(..)"` in an assertion, a `//` line) is never taken for a call."""
    return SKIPPED.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)


def find_paths(source: Path, root: Path) -> list[tuple[int, str, str | None]]:
    """(line, form, joined path or None when it cannot be resolved) for each compiled-in path in one file."""
    text = source.read_text(encoding="utf-8")
    code = code_only(text)
    found: list[tuple[int, str, str | None]] = []
    for match in INCLUDE.finditer(code):
        arg = INCLUDE_ARG.match(text, match.end())
        joined = os.path.join(source.parent, arg.group(1)) if arg else None
        found.append((text.count("\n", 0, match.start()) + 1, match.group(1) + "!", joined))
    for match in CONCAT_ENV.finditer(code):
        manifest = MANIFEST_ARG.match(text, match.end())
        if manifest is None:
            continue
        rest = CONCAT_REST.match(text, manifest.end())
        parts = re.findall(LITERAL, rest.group(1)) if rest else []
        joined = str(crate_dir(source, root)) + "".join(parts) if rest else None
        found.append((text.count("\n", 0, match.start()) + 1, "concat!(env!(\"CARGO_MANIFEST_DIR\"), ...)", joined))
    return found


def judge(joined: str, root: Path, rules: dict[str, str]) -> tuple[str, list[str]]:
    """The resolved repository path and its problems (empty when every rule matches it)."""
    resolved = Path(os.path.normpath(joined))
    try:
        rel = resolved.relative_to(root).as_posix()
    except ValueError:
        return str(resolved), ["outside the repository"]
    if not resolved.exists():
        return rel, ["no such file"]
    if rel == ".":
        return rel, []
    shown = rel + "/" if resolved.is_dir() else rel
    return shown, [f"not matched by the {name}" for name, rule in rules.items() if not re.search(rule, shown)]


def check(root: Path) -> tuple[list[str], list[str]]:
    """(problems, notes) over every Rust source file under RUST_DIRS."""
    root = root.resolve()
    rules = read_rules(root)
    problems: list[str] = []
    notes: list[str] = []
    sources = sorted(p for d in RUST_DIRS if (root / d).is_dir() for p in (root / d).rglob("*.rs"))
    for source in sources:
        where = source.relative_to(root).as_posix()
        for line, form, joined in find_paths(source, root):
            if joined is None:
                problems.append(f"{where}:{line}: {form} with a path that cannot be resolved (not a string literal)")
                continue
            shown, issues = judge(joined, root, rules)
            if shown == ".":
                notes.append(f"{where}:{line}: {form} names the repository root (a run-time base, not a file)")
            problems.extend(f"{where}:{line}: {form} -> {shown}: {issue}" for issue in issues)
    return problems, notes


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args(argv)
    try:
        problems, notes = check(args.root)
    except (OSError, KeyError, TypeError, ValueError, yaml.YAMLError) as err:
        print(f"check_include_paths: cannot read the path rules: {err}")
        return 1
    for note in notes:
        print(f"note: {note}")
    for problem in problems:
        print(f"ERROR {problem}")
    if problems:
        print(f"check_include_paths: {len(problems)} problem(s); add the folder to the three rules (D-1867)")
        return 1
    print("check_include_paths: every compiled-in path is covered by the Checks, Coverage and RUST_PATHS rules")
    return 0


if __name__ == "__main__":
    sys.exit(main())
